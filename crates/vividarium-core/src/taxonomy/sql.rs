use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, TryLockError};
use std::time::Duration;

use rusqlite::ffi;
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::{Connection, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use super::changeset::{affected_taxon_ids_from_changeset, start_taxonomy_session};
#[cfg(test)]
use super::sql_inputs::SqlInputKind;
use super::sql_inputs::{
    self, AddSqlInputRequest, AddSqlInputResult, PersistentSqlInput, RemoveSqlInputRequest,
    RemoveSqlInputResult, SqlInputScope,
};
use super::sql_sources::{
    detach_sources, inspect_sqlite_source, prepare_sources, prepare_sources_with_progress,
};
use super::sql_support::{
    CUSTOM_SQL_STATEMENT_TIMEOUT, SqlStatementExecutionContext, SqlStatementExecutionLimits,
    count_sql_statements, execute_preview_statement_guarded, prepare_statement_raw, sqlite_error,
    statement_columns, statement_row, with_sql_statement_guard,
};
use super::sql_types::{SqlColumn, SqlSourceSchema, SqlStatementMessage, SqlValue};
use super::validation::{
    TaxonomyValidationScope, taxonomy_validation_scope_with_cancellation,
    validate_taxonomy_changes_with_cancellation, validate_taxonomy_with_progress_and_cancellation,
};
use crate::metadata::{self, MetadataKey};
use crate::operations::{self, NewAuditRow, NewOperation, OperationInput};
use crate::{
    CancellationToken, CoreError, CoreResult, Database, OperationProgress, OperationProgressUnit,
};

pub const DEFAULT_SQL_RESULT_ROW_LIMIT: usize = 1000;
const INCREMENTAL_VALIDATION_TAXON_LIMIT: usize = 5_000;
static CUSTOM_SQL_LOCKS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CustomTaxonomySqlRequest {
    pub sql: String,
    pub maximum_result_rows: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CustomTaxonomySqlExportRequest {
    pub sql: String,
    pub statement_index: usize,
    pub destination_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CustomSqlExecutionResult {
    pub operation_id: Option<i64>,
    pub changeset_size: usize,
    pub result_sets: Vec<SqlResultSet>,
    pub messages: Vec<SqlStatementMessage>,
    pub script_saved: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SqlResultSet {
    pub statement_index: usize,
    pub columns: Vec<SqlColumn>,
    pub rows: Vec<Vec<SqlValue>>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SqlExportResult {
    pub path: String,
    pub row_count: u64,
}

pub fn execute_custom_taxonomy_sql(
    database: &Database,
    request: &CustomTaxonomySqlRequest,
) -> CoreResult<CustomSqlExecutionResult> {
    execute_custom_taxonomy_sql_with_cancellation(database, request, &CancellationToken::new())
}

pub fn execute_custom_taxonomy_sql_with_cancellation(
    database: &Database,
    request: &CustomTaxonomySqlRequest,
    cancellation: &CancellationToken,
) -> CoreResult<CustomSqlExecutionResult> {
    execute_custom_taxonomy_sql_with_progress_and_cancellation(
        database,
        request,
        &mut |_| {},
        cancellation,
    )
}

pub fn execute_custom_taxonomy_sql_with_progress_and_cancellation(
    database: &Database,
    request: &CustomTaxonomySqlRequest,
    progress: &mut (dyn FnMut(OperationProgress) + Send),
    cancellation: &CancellationToken,
) -> CoreResult<CustomSqlExecutionResult> {
    cancellation.check()?;
    let sql_mutex = custom_sql_mutex(database)?;
    let _sql_guard = lock_custom_sql(&sql_mutex)?;
    cancellation.check()?;
    let _guard = database.try_taxonomy_mutation()?;
    let sql = require_sql(&request.sql)?;
    let maximum_result_rows = request
        .maximum_result_rows
        .unwrap_or(DEFAULT_SQL_RESULT_ROW_LIMIT)
        .min(DEFAULT_SQL_RESULT_ROW_LIMIT);
    let mut connection = database.connect_taxonomy()?;
    cancellation.install_sqlite_progress_handler(&connection);
    report_custom_sql_progress(progress, "preparing_sql_sources", None, None, None);
    let sources = sql_inputs::stored_sources(database, SqlInputScope::CustomSql)?;
    let delimiter = crate::general::get_csv_delimiter_byte(database)?;
    let attached = prepare_sources_with_progress(
        &mut connection,
        &sources,
        delimiter,
        cancellation,
        progress,
    )?;
    let execution: CoreResult<CustomSqlExecutionResult> = (|| {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut session = start_taxonomy_session(&transaction)?;
        let mut result = execute_custom_script_guarded(
            &transaction,
            sql,
            maximum_result_rows,
            custom_sql_authorizer,
            progress,
            cancellation,
            CUSTOM_SQL_STATEMENT_TIMEOUT,
        )?;
        if session.is_empty() {
            drop(session);
            cancellation.check()?;
            report_custom_sql_progress(progress, "committing_custom_sql", None, None, None);
            transaction.commit()?;
            report_custom_sql_progress(progress, "finalizing_custom_sql", None, None, None);
            return Ok(result);
        }
        report_custom_sql_progress(
            progress,
            "generating_custom_sql_changeset",
            None,
            None,
            None,
        );
        let mut changeset_blob = Vec::new();
        session.changeset_strm(&mut changeset_blob)?;
        drop(session);
        result.changeset_size = changeset_blob.len();
        let affected_taxon_ids = affected_taxon_ids_from_changeset(&transaction, &changeset_blob)?;
        let validation_scope = taxonomy_validation_scope_with_cancellation(
            &transaction,
            &affected_taxon_ids,
            INCREMENTAL_VALIDATION_TAXON_LIMIT,
            cancellation,
        )?;
        report_custom_sql_progress(progress, "validating_custom_sql_changes", None, None, None);
        match validation_scope {
            TaxonomyValidationScope::Incremental(scope) => {
                validate_taxonomy_changes_with_cancellation(&transaction, &scope, cancellation)?;
            }
            TaxonomyValidationScope::Full => {
                validate_taxonomy_with_progress_and_cancellation(
                    &transaction,
                    |_| {},
                    cancellation,
                )?;
            }
        }
        let full_remap_required = affected_taxon_ids.len() > 5000;
        report_custom_sql_progress(progress, "recording_custom_sql_operation", None, None, None);
        let operation_id = insert_custom_sql_operation(
            &transaction,
            &request.sql,
            &changeset_blob,
            &affected_taxon_ids,
        )?;
        super::sync::record_event(
            &transaction,
            Some(operation_id),
            affected_taxon_ids,
            full_remap_required,
        )?;
        cancellation.check()?;
        report_custom_sql_progress(progress, "committing_custom_sql", None, None, None);
        transaction.commit()?;
        report_custom_sql_progress(progress, "finalizing_custom_sql", None, None, None);
        result.operation_id = Some(operation_id);
        Ok(result)
    })();
    let detach = detach_sources(&connection, &attached);
    let mut result = cancellation.normalize(match (execution, detach) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), _) => Err(error),
        (Ok(mut result), Err(error)) => {
            result.warnings.push(format!(
                "custom taxonomy SQL committed, but source cleanup failed: {error}"
            ));
            Ok(result)
        }
    })?;
    cancellation.check()?;
    match database.connect_metadata().and_then(|connection| {
        metadata::set_raw(&connection, MetadataKey::CustomTaxonomySql, &request.sql)
    }) {
        Ok(()) => result.script_saved = true,
        Err(error) => result.warnings.push(format!(
            "custom taxonomy SQL committed, but the script could not be saved: {error}"
        )),
    }
    Ok(result)
}

pub fn export_custom_taxonomy_query(
    database: &Database,
    request: &CustomTaxonomySqlExportRequest,
) -> CoreResult<SqlExportResult> {
    export_custom_taxonomy_query_with_cancellation(database, request, &CancellationToken::new())
}

pub fn export_custom_taxonomy_query_with_cancellation(
    database: &Database,
    request: &CustomTaxonomySqlExportRequest,
    cancellation: &CancellationToken,
) -> CoreResult<SqlExportResult> {
    cancellation.check()?;
    let sql_mutex = custom_sql_mutex(database)?;
    let _sql_guard = lock_custom_sql(&sql_mutex)?;
    cancellation.check()?;
    let sql = require_sql(&request.sql)?;
    if !request.destination_path.is_absolute() {
        return Err(CoreError::InvalidArgument(
            "sql export destination must be an absolute path".into(),
        ));
    }
    let mut connection = database.connect_taxonomy()?;
    cancellation.install_sqlite_progress_handler(&connection);
    let sources = sql_inputs::stored_sources(database, SqlInputScope::CustomSql)?;
    let delimiter = crate::general::get_csv_delimiter_byte(database)?;
    let attached = prepare_sources(&mut connection, &sources, delimiter)?;
    let mut output_guard = PartialExportGuard::new(&request.destination_path);
    let export = export_query_statement(
        &connection,
        sql,
        request.statement_index,
        &request.destination_path,
        delimiter,
        cancellation,
        CUSTOM_SQL_STATEMENT_TIMEOUT,
        &mut output_guard,
    );
    let detach = detach_sources(&connection, &attached);
    let result = cancellation.normalize(match (export, detach) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    });
    if result.is_ok() {
        output_guard.disarm();
    }
    result
}

pub fn get_custom_taxonomy_sql(database: &Database) -> CoreResult<String> {
    Ok(metadata::get_raw(
        &database.connect_metadata()?,
        MetadataKey::CustomTaxonomySql,
    )?
    .unwrap_or_else(|| {
        "SELECT taxon_id, rank, geological_range\nFROM taxa\nORDER BY taxon_id\nLIMIT 100;".into()
    }))
}

pub fn list_custom_sql_inputs(database: &Database) -> CoreResult<Vec<PersistentSqlInput>> {
    sql_inputs::list_inputs(database, SqlInputScope::CustomSql)
}

pub fn list_custom_sql_database_schemas(database: &Database) -> CoreResult<Vec<SqlSourceSchema>> {
    Ok(vec![inspect_sqlite_source(
        "main",
        &database.taxonomy_path()?,
    )?])
}

pub fn add_custom_sql_input(
    database: &Database,
    request: &AddSqlInputRequest,
) -> CoreResult<AddSqlInputResult> {
    let sql_mutex = custom_sql_mutex(database)?;
    let _sql_guard = lock_custom_sql(&sql_mutex)?;
    sql_inputs::add_input(database, SqlInputScope::CustomSql, request)
}

pub fn remove_custom_sql_input(
    database: &Database,
    request: &RemoveSqlInputRequest,
) -> CoreResult<RemoveSqlInputResult> {
    let sql_mutex = custom_sql_mutex(database)?;
    let _sql_guard = lock_custom_sql(&sql_mutex)?;
    sql_inputs::remove_input(database, SqlInputScope::CustomSql, request)
}

fn custom_sql_mutex(database: &Database) -> CoreResult<Arc<Mutex<()>>> {
    let mut locks = CUSTOM_SQL_LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| CoreError::Consistency("Custom SQL lock registry is poisoned".into()))?;
    Ok(locks
        .entry(database.metadata_path())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone())
}

fn lock_custom_sql(mutex: &Mutex<()>) -> CoreResult<std::sync::MutexGuard<'_, ()>> {
    match mutex.try_lock() {
        Ok(guard) => Ok(guard),
        Err(TryLockError::WouldBlock) => Err(CoreError::InvalidArgument(
            "Another Custom SQL operation is already running.".into(),
        )),
        Err(TryLockError::Poisoned(_)) => Err(CoreError::Consistency(
            "Custom SQL workspace lock is poisoned".into(),
        )),
    }
}

fn execute_custom_script_guarded<F, A>(
    connection: &Connection,
    sql: &str,
    maximum_result_rows: usize,
    mut authorizer_factory: F,
    progress: &mut (dyn FnMut(OperationProgress) + Send),
    cancellation: &CancellationToken,
    statement_timeout: Duration,
) -> CoreResult<CustomSqlExecutionResult>
where
    F: FnMut() -> A,
    A: for<'a> FnMut(AuthContext<'a>) -> Authorization + Send + 'static,
{
    let mut offset = 0;
    let mut statement_index = 0_usize;
    let statement_total = count_sql_statements(sql)?;
    let mut result_sets = Vec::new();
    let mut messages = Vec::new();
    while offset < sql.len() {
        cancellation.check()?;
        let active_statement = statement_index as u64 + 1;
        report_custom_sql_progress(
            progress,
            "executing_custom_sql",
            Some(active_statement),
            Some(statement_total),
            Some(OperationProgressUnit::Statements),
        );
        connection.authorizer(Some(authorizer_factory()));
        let execution = unsafe {
            execute_preview_statement_guarded(
                connection,
                &sql[offset..],
                maximum_result_rows,
                &SqlStatementExecutionContext {
                    cancellation,
                    limits: SqlStatementExecutionLimits {
                        timeout: statement_timeout,
                    },
                    statement_index: active_statement,
                    statement_total,
                    workflow: "Custom SQL",
                },
            )
        };
        connection.authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
        let execution = execution?;
        offset += execution.tail_offset;
        let Some(execution) = execution.statement else {
            continue;
        };
        statement_index += 1;
        if !execution.columns.is_empty() {
            result_sets.push(SqlResultSet {
                statement_index,
                columns: execution.columns,
                rows: execution.rows,
                truncated: execution.truncated,
            });
        }
        let affected_rows = if execution.read_only {
            None
        } else {
            Some(execution.affected_rows)
        };
        let message = match affected_rows {
            Some(count) => format!("statement affected {count} rows"),
            None if execution.truncated => {
                format!("statement returned more than {maximum_result_rows} rows")
            }
            None => format!("statement returned {} rows", execution.returned_rows),
        };
        messages.push(SqlStatementMessage {
            statement_index,
            affected_rows,
            message,
        });
    }
    if statement_index == 0 {
        return Err(CoreError::InvalidArgument(
            "custom taxonomy SQL requires at least one executable statement".into(),
        ));
    }
    Ok(CustomSqlExecutionResult {
        operation_id: None,
        changeset_size: 0,
        result_sets,
        messages,
        script_saved: false,
        warnings: Vec::new(),
    })
}

#[cfg(test)]
fn execute_custom_script<F, A>(
    connection: &Connection,
    sql: &str,
    maximum_result_rows: usize,
    authorizer_factory: F,
) -> CoreResult<CustomSqlExecutionResult>
where
    F: FnMut() -> A,
    A: for<'a> FnMut(AuthContext<'a>) -> Authorization + Send + 'static,
{
    execute_custom_script_guarded(
        connection,
        sql,
        maximum_result_rows,
        authorizer_factory,
        &mut |_| {},
        &CancellationToken::new(),
        CUSTOM_SQL_STATEMENT_TIMEOUT,
    )
}

fn report_custom_sql_progress(
    progress: &mut (dyn FnMut(OperationProgress) + Send),
    stage: &str,
    current: Option<u64>,
    total: Option<u64>,
    unit: Option<OperationProgressUnit>,
) {
    progress(OperationProgress {
        stage: stage.into(),
        current,
        total,
        unit,
    });
}

fn export_query_statement(
    connection: &Connection,
    sql: &str,
    statement_index: usize,
    destination_path: &Path,
    delimiter: u8,
    cancellation: &CancellationToken,
    statement_timeout: Duration,
    output_guard: &mut PartialExportGuard,
) -> CoreResult<SqlExportResult> {
    let statement_total = count_sql_statements(sql)?;
    connection.authorizer(Some(custom_sql_authorizer()));
    let result = unsafe {
        export_query_statement_raw(
            connection,
            sql,
            statement_index,
            destination_path,
            delimiter,
            cancellation,
            statement_timeout,
            statement_total,
            output_guard,
        )
    };
    connection.authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
    result
}

unsafe fn export_query_statement_raw(
    connection: &Connection,
    sql: &str,
    target_statement_index: usize,
    destination_path: &Path,
    delimiter: u8,
    cancellation: &CancellationToken,
    statement_timeout: Duration,
    statement_total: u64,
    output_guard: &mut PartialExportGuard,
) -> CoreResult<SqlExportResult> {
    if target_statement_index == 0 {
        return Err(CoreError::InvalidArgument(
            "sql export statement index must be at least 1".into(),
        ));
    }
    let database = unsafe { connection.handle() };
    let mut offset = 0;
    let mut statement_index = 0;
    let statement = loop {
        if offset >= sql.len() {
            return Err(CoreError::InvalidArgument(format!(
                "sql export statement {target_statement_index} does not exist"
            )));
        }
        let prepared = unsafe { prepare_statement_raw(connection, &sql[offset..]) }?;
        offset += prepared.tail_offset;
        let Some(statement) = prepared.statement else {
            continue;
        };
        statement_index += 1;
        if statement_index == target_statement_index {
            break statement;
        }
    };
    if unsafe { ffi::sqlite3_stmt_readonly(statement.0) } == 0 {
        return Err(CoreError::InvalidArgument(
            "sql export target must be a read-only query".into(),
        ));
    }
    let column_count = unsafe { ffi::sqlite3_column_count(statement.0) as usize };
    if column_count == 0 {
        return Err(CoreError::InvalidArgument(
            "sql export query has no result columns".into(),
        ));
    }
    let file = File::create(destination_path)?;
    output_guard.arm();
    let mut writer = csv::WriterBuilder::new()
        .delimiter(delimiter)
        .from_writer(BufWriter::new(file));
    let columns = unsafe { statement_columns(statement.0, column_count) };
    writer.write_record(columns.iter().map(|column| column.name.as_str()))?;
    let mut row_count = 0_u64;
    let execution = with_sql_statement_guard(
        connection,
        &SqlStatementExecutionContext {
            cancellation,
            limits: SqlStatementExecutionLimits {
                timeout: statement_timeout,
            },
            statement_index: target_statement_index as u64,
            statement_total,
            workflow: "Custom SQL Export",
        },
        || {
            loop {
                let step = unsafe { ffi::sqlite3_step(statement.0) };
                match step {
                    ffi::SQLITE_ROW => {
                        let row = unsafe { statement_row(statement.0, column_count) };
                        writer.write_record(row.iter().map(SqlValue::csv_value))?;
                        row_count += 1;
                    }
                    ffi::SQLITE_DONE => break,
                    code => return Err(sqlite_error(database, code)),
                }
            }
            Ok(())
        },
    );
    execution?;
    writer.flush()?;
    Ok(SqlExportResult {
        path: destination_path.to_string_lossy().into_owned(),
        row_count,
    })
}

struct PartialExportGuard {
    path: PathBuf,
    armed: bool,
}

impl PartialExportGuard {
    fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            armed: false,
        }
    }

    fn arm(&mut self) {
        self.armed = true;
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for PartialExportGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn require_sql(sql: &str) -> CoreResult<&str> {
    let sql = sql.trim();
    if sql.is_empty() {
        Err(CoreError::InvalidArgument("sql is required".into()))
    } else {
        Ok(sql)
    }
}

fn custom_sql_authorizer() -> impl for<'a> FnMut(AuthContext<'a>) -> Authorization + Send + 'static
{
    let mut business_write = false;
    move |context| match context.action {
        AuthAction::Select | AuthAction::Recursive => Authorization::Allow,
        AuthAction::Function { function_name } => {
            if function_name.eq_ignore_ascii_case("load_extension") {
                Authorization::Deny
            } else {
                Authorization::Allow
            }
        }
        AuthAction::Pragma {
            pragma_name,
            pragma_value,
        } => {
            let pragma_name = pragma_name.to_ascii_lowercase();
            let read_only = matches!(
                pragma_name.as_str(),
                "foreign_key_list"
                    | "index_info"
                    | "index_list"
                    | "index_xinfo"
                    | "table_info"
                    | "table_xinfo"
            ) || (pragma_value.is_none()
                && matches!(
                    pragma_name.as_str(),
                    "data_version"
                        | "database_list"
                        | "function_list"
                        | "module_list"
                        | "table_list"
                ));
            if read_only {
                Authorization::Allow
            } else {
                Authorization::Deny
            }
        }
        AuthAction::Read { .. } => Authorization::Allow,
        AuthAction::Insert { table_name }
        | AuthAction::Update { table_name, .. }
        | AuthAction::Delete { table_name } => {
            let direct_business_write = context.database_name == Some("main")
                && context.accessor.is_none()
                && matches!(table_name, "taxa" | "taxon_names");
            let derived_write = context.database_name == Some("main")
                && (context.accessor.is_some() || business_write)
                && table_name.starts_with("taxon_names_fts");
            if direct_business_write {
                business_write = true;
                Authorization::Allow
            } else if derived_write {
                Authorization::Allow
            } else {
                Authorization::Deny
            }
        }
        _ => Authorization::Deny,
    }
}

fn insert_custom_sql_operation(
    transaction: &Transaction<'_>,
    sql: &str,
    changeset_blob: &[u8],
    affected_taxon_ids: &BTreeSet<i64>,
) -> CoreResult<i64> {
    let total_items = affected_taxon_ids.len().max(1);
    let operation_id = operations::insert_operation(
        transaction,
        NewOperation {
            kind: "taxonomy_custom_sql",
            source: "custom_sql",
            total_items,
            succeeded_items: total_items,
            failed_items: 0,
            rollbackable: true,
            has_formatted_input: false,
        },
    )?;
    operations::insert_operation_input(
        transaction,
        operation_id,
        &OperationInput::CustomSql { sql: sql.into() },
    )?;
    transaction.execute(
        r#"
        INSERT INTO operation_changesets (operation_id, changeset_blob)
        VALUES (?, ?)
        "#,
        params![operation_id, changeset_blob],
    )?;
    if affected_taxon_ids.is_empty() {
        operations::insert_audit_row(
            transaction,
            operation_id,
            NewAuditRow {
                sequence: 1,
                entity_type: "taxonomy",
                entity_id: None,
                action: "custom_sql",
                before_json: None,
                after_json: Some(serde_json::json!({
                    "changeset_size": changeset_blob.len(),
                })),
                succeeded: true,
                message: "custom SQL changed taxonomy data",
            },
        )?;
    } else {
        for (index, taxon_id) in affected_taxon_ids.iter().enumerate() {
            operations::insert_audit_row(
                transaction,
                operation_id,
                NewAuditRow {
                    sequence: index + 1,
                    entity_type: "taxon",
                    entity_id: Some(taxon_id.to_string()),
                    action: "custom_sql",
                    before_json: None,
                    after_json: Some(serde_json::json!({
                        "taxon_id": taxon_id,
                    })),
                    succeeded: true,
                    message: "custom SQL changed taxonomy data",
                },
            )?;
        }
    }
    Ok(operation_id)
}

#[cfg(test)]
#[path = "sql/tests.rs"]
mod tests;
