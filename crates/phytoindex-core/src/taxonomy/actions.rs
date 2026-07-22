use std::collections::BTreeSet;
use std::ffi::{CStr, CString};
use std::ptr;

use rusqlite::ffi;
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params, params_from_iter};
use serde::{Deserialize, Serialize};

use super::{
    ExistingTaxonUpdate, TaxonNameInput, TaxonRowOutcome, TaxonUpdateOptions, TaxonomyBatchContext,
    TaxonomyCustomSqlTempTable, TaxonomyCustomSqlTempTableMetadata, TaxonomyNameKind,
    apply_existing_taxon_update_with_log, finish_taxonomy_session, insert_operation_batch,
    insert_operation_log, is_taxonomy_session_table, normalize_name, start_taxonomy_session,
    validate_taxonomy,
};
use crate::{CoreError, CoreResult, Database};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonUpdateInput {
    pub taxon_id: i64,
    pub geological_range: Option<String>,
    pub scientific: Option<TaxonNameInput>,
    pub english: Option<TaxonNameInput>,
    pub chinese: Option<TaxonNameInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonomyUpdateActionResult {
    pub batch_id: Option<i64>,
    pub outcome: TaxonRowOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeleteTaxonNameInput {
    pub taxon_id: i64,
    pub name_kind: TaxonomyNameKind,
    pub name: String,
    pub replacement_accepted_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonomyActionResult {
    pub batch_id: i64,
    pub operation_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonomyCustomSqlResult {
    pub batch_id: Option<i64>,
    pub operation_id: Option<i64>,
    pub changeset_size: usize,
}

#[derive(Debug, Serialize)]
struct DeleteTaxonInput {
    taxon_id: i64,
}

#[derive(Debug, Serialize)]
struct CustomSqlLogInput<'a> {
    sql: &'a str,
}

pub fn delete_taxon_name(
    database: &Database,
    mut input: DeleteTaxonNameInput,
) -> CoreResult<TaxonomyActionResult> {
    input.name = normalize_name(Some(&input.name))
        .ok_or_else(|| CoreError::InvalidArgument("name is required".into()))?;
    input.replacement_accepted_name = input
        .replacement_accepted_name
        .as_deref()
        .and_then(|value| normalize_name(Some(value)));
    let mut connection = database.connect()?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    ensure_taxon_exists(&transaction, input.taxon_id)?;
    let is_accepted =
        load_name_is_accepted(&transaction, input.taxon_id, input.name_kind, &input.name)?
            .ok_or_else(|| {
                CoreError::NotFound(format!(
                    "{} name '{}' for taxon {}",
                    input.name_kind.as_str(),
                    input.name,
                    input.taxon_id
                ))
            })?;
    let remaining_names =
        count_other_names(&transaction, input.taxon_id, input.name_kind, &input.name)?;

    let mut session = start_taxonomy_session(&transaction)?;
    if is_accepted && remaining_names > 0 {
        let replacement = input.replacement_accepted_name.as_deref().ok_or_else(|| {
            CoreError::InvalidArgument(
                "deleting an accepted name requires replacement_accepted_name".into(),
            )
        })?;
        if replacement == input.name {
            return Err(CoreError::InvalidArgument(
                "replacement_accepted_name must differ from the deleted name".into(),
            ));
        }
        ensure_name_exists(&transaction, input.taxon_id, input.name_kind, replacement)?;
        promote_name(&transaction, input.taxon_id, input.name_kind, replacement)?;
    } else if input.replacement_accepted_name.as_deref().is_some() {
        return Err(CoreError::InvalidArgument(
            "replacement_accepted_name is only valid when deleting an accepted name".into(),
        ));
    }

    delete_name_record(&transaction, input.taxon_id, input.name_kind, &input.name)?;
    let changeset_blob = finish_taxonomy_session(&mut session)?;
    drop(session);
    let batch_id =
        insert_operation_batch(&transaction, &input, &TaxonomyBatchContext::QueryDeleteName)?;
    let operation_id = insert_operation_log(&transaction, batch_id, 1, &changeset_blob)?;
    transaction.commit()?;
    Ok(TaxonomyActionResult {
        batch_id,
        operation_id,
    })
}

pub fn update_taxon(
    database: &Database,
    input: TaxonUpdateInput,
    options: TaxonUpdateOptions,
) -> CoreResult<TaxonomyUpdateActionResult> {
    let mut connection = database.connect()?;
    let update = ExistingTaxonUpdate::new(
        input.geological_range.as_deref(),
        input.scientific.as_ref(),
        input.english.as_ref(),
        input.chinese.as_ref(),
    );
    let mut batch_id = None;
    let outcome = apply_existing_taxon_update_with_log(
        &mut connection,
        1,
        input.taxon_id,
        update,
        options,
        &mut batch_id,
        &input,
        &TaxonomyBatchContext::QueryUpdate { options },
    )?;
    Ok(TaxonomyUpdateActionResult { batch_id, outcome })
}

pub fn delete_taxon(database: &Database, taxon_id: i64) -> CoreResult<TaxonomyActionResult> {
    let mut connection = database.connect()?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    ensure_taxon_exists(&transaction, taxon_id)?;
    let child_count: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM taxa WHERE parent_taxon_id = ?",
        [taxon_id],
        |row| row.get(0),
    )?;
    if child_count > 0 {
        return Err(CoreError::InvalidArgument(format!(
            "taxon {taxon_id} cannot be deleted because it has child taxa"
        )));
    }
    let mapped_photo_count: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM photos_taxa_mapping WHERE taxon_id = ?",
        [taxon_id],
        |row| row.get(0),
    )?;
    if mapped_photo_count > 0 {
        return Err(CoreError::InvalidArgument(format!(
            "taxon {taxon_id} cannot be deleted because it is used by photo mappings"
        )));
    }
    let mut session = start_taxonomy_session(&transaction)?;
    transaction.execute("DELETE FROM taxa WHERE taxon_id = ?", [taxon_id])?;
    let changeset_blob = finish_taxonomy_session(&mut session)?;
    drop(session);
    let batch_id = insert_operation_batch(
        &transaction,
        &DeleteTaxonInput { taxon_id },
        &TaxonomyBatchContext::QueryDeleteTaxon,
    )?;
    let operation_id = insert_operation_log(&transaction, batch_id, 1, &changeset_blob)?;
    transaction.commit()?;
    Ok(TaxonomyActionResult {
        batch_id,
        operation_id,
    })
}

pub fn execute_custom_taxonomy_sql(
    database: &Database,
    sql: &str,
    input: Option<TaxonomyCustomSqlTempTable>,
) -> CoreResult<TaxonomyCustomSqlResult> {
    let sql = sql.trim();
    if sql.is_empty() {
        return Err(CoreError::InvalidArgument("sql is required".into()));
    }
    let mut connection = database.connect()?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let input_metadata = match input.as_ref() {
        Some(input) => Some(create_temp_input_table(&transaction, input)?),
        None => None,
    };
    authorize_custom_sql(&transaction, sql)?;
    let mut session = start_taxonomy_session(&transaction)?;
    transaction.execute_batch(sql)?;
    let mut changeset_blob = Vec::new();
    session.changeset_strm(&mut changeset_blob)?;
    let changeset_size = changeset_blob.len();
    drop(session);
    if changeset_blob.is_empty() {
        transaction.commit()?;
        return Ok(TaxonomyCustomSqlResult {
            batch_id: None,
            operation_id: None,
            changeset_size,
        });
    }
    validate_taxonomy(&transaction)?;
    let batch_id = insert_operation_batch(
        &transaction,
        &CustomSqlLogInput { sql },
        &TaxonomyBatchContext::CustomSql {
            input: input_metadata,
        },
    )?;
    let operation_id = insert_operation_log(&transaction, batch_id, 1, &changeset_blob)?;
    transaction.commit()?;
    Ok(TaxonomyCustomSqlResult {
        batch_id: Some(batch_id),
        operation_id: Some(operation_id),
        changeset_size,
    })
}

fn ensure_taxon_exists(transaction: &Transaction<'_>, taxon_id: i64) -> CoreResult<()> {
    let exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM taxa WHERE taxon_id = ?)",
        [taxon_id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(CoreError::NotFound(format!("taxon {taxon_id}")));
    }
    Ok(())
}

fn load_name_is_accepted(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    kind: TaxonomyNameKind,
    name: &str,
) -> CoreResult<Option<bool>> {
    Ok(transaction
        .query_row(
            r#"
            SELECT is_accepted
            FROM taxon_names
            WHERE taxon_id = ? AND name_kind = ? AND name = ?
            "#,
            params![taxon_id, kind.code(), name],
            |row| Ok(row.get::<_, i64>(0)? != 0),
        )
        .optional()?)
}

fn ensure_name_exists(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    kind: TaxonomyNameKind,
    name: &str,
) -> CoreResult<()> {
    let exists: bool = transaction.query_row(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM taxon_names
            WHERE taxon_id = ? AND name_kind = ? AND name = ?
        )
        "#,
        params![taxon_id, kind.code(), name],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(CoreError::NotFound(format!(
            "replacement {} name '{}' for taxon {}",
            kind.as_str(),
            name,
            taxon_id
        )));
    }
    Ok(())
}

fn count_other_names(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    kind: TaxonomyNameKind,
    name: &str,
) -> CoreResult<i64> {
    Ok(transaction.query_row(
        r#"
        SELECT COUNT(*)
        FROM taxon_names
        WHERE taxon_id = ? AND name_kind = ? AND name != ?
        "#,
        params![taxon_id, kind.code(), name],
        |row| row.get(0),
    )?)
}

fn promote_name(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    kind: TaxonomyNameKind,
    name: &str,
) -> CoreResult<()> {
    transaction.execute(
        "UPDATE taxon_names SET is_accepted = 0 WHERE taxon_id = ? AND name_kind = ?",
        params![taxon_id, kind.code()],
    )?;
    transaction.execute(
        r#"
        UPDATE taxon_names
        SET is_accepted = 1
        WHERE taxon_id = ? AND name_kind = ? AND name = ?
        "#,
        params![taxon_id, kind.code(), name],
    )?;
    Ok(())
}

fn delete_name_record(
    transaction: &Transaction<'_>,
    taxon_id: i64,
    kind: TaxonomyNameKind,
    name: &str,
) -> CoreResult<()> {
    transaction.execute(
        "DELETE FROM taxon_names WHERE taxon_id = ? AND name_kind = ? AND name = ?",
        params![taxon_id, kind.code(), name],
    )?;
    Ok(())
}

fn authorize_custom_sql(transaction: &Transaction<'_>, sql: &str) -> CoreResult<()> {
    if sql.to_ascii_lowercase().contains("taxon_names_fts") {
        return Err(CoreError::InvalidArgument(
            "custom sql cannot access taxonomy search index tables directly".into(),
        ));
    }
    transaction.authorizer(Some(custom_sql_authorizer));
    let authorize_result = prepare_custom_sql_batch(transaction, sql);
    transaction.authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
    authorize_result?;
    Ok(())
}

fn prepare_custom_sql_batch(connection: &rusqlite::Connection, sql: &str) -> CoreResult<()> {
    let database = unsafe { connection.handle() };
    let mut offset = 0;
    while offset < sql.len() {
        let sql_tail = &sql[offset..];
        let sql_tail = CString::new(sql_tail)
            .map_err(|error| CoreError::InvalidArgument(format!("invalid sql: {error}")))?;
        let mut statement = ptr::null_mut();
        let mut next_sql = ptr::null();
        let code = unsafe {
            ffi::sqlite3_prepare_v2(
                database,
                sql_tail.as_ptr(),
                -1,
                &mut statement,
                &mut next_sql,
            )
        };
        if !statement.is_null() {
            unsafe {
                ffi::sqlite3_finalize(statement);
            }
        }
        if code != ffi::SQLITE_OK {
            return Err(sqlite_error(database, code));
        }
        if next_sql.is_null() {
            break;
        }
        let tail_offset = unsafe { next_sql.offset_from(sql_tail.as_ptr()) as usize };
        if tail_offset == 0 || tail_offset >= sql_tail.as_bytes().len() {
            break;
        }
        offset += tail_offset;
    }
    Ok(())
}

fn sqlite_error(database: *mut ffi::sqlite3, code: i32) -> CoreError {
    let message = unsafe { CStr::from_ptr(ffi::sqlite3_errmsg(database)) }
        .to_string_lossy()
        .into_owned();
    CoreError::Database(rusqlite::Error::SqliteFailure(
        ffi::Error::new(code),
        Some(message),
    ))
}

fn create_temp_input_table(
    transaction: &Transaction<'_>,
    input: &TaxonomyCustomSqlTempTable,
) -> CoreResult<TaxonomyCustomSqlTempTableMetadata> {
    if input.columns.is_empty() {
        return Err(CoreError::InvalidArgument(
            "custom sql input requires at least one column".into(),
        ));
    }
    let mut seen = BTreeSet::new();
    let mut columns = Vec::with_capacity(input.columns.len());
    for column in &input.columns {
        let column = column.trim();
        if !is_safe_identifier(column) {
            return Err(CoreError::InvalidArgument(format!(
                "invalid custom sql input column: {column}"
            )));
        }
        let key = column.to_ascii_lowercase();
        if !seen.insert(key) {
            return Err(CoreError::InvalidArgument(format!(
                "duplicate custom sql input column: {column}"
            )));
        }
        columns.push(column.to_string());
    }
    for (index, row) in input.rows.iter().enumerate() {
        if row.len() != columns.len() {
            return Err(CoreError::InvalidArgument(format!(
                "custom sql input row {} has {} values but {} columns were declared",
                index + 1,
                row.len(),
                columns.len()
            )));
        }
    }
    let definitions = columns
        .iter()
        .map(|column| format!("{} TEXT", quote_identifier(column)))
        .collect::<Vec<_>>()
        .join(", ");
    transaction.execute_batch(&format!("CREATE TEMP TABLE input ({definitions})"))?;
    if !input.rows.is_empty() {
        let column_list = columns
            .iter()
            .map(|column| quote_identifier(column))
            .collect::<Vec<_>>()
            .join(", ");
        let placeholders = std::iter::repeat_n("?", columns.len())
            .collect::<Vec<_>>()
            .join(", ");
        let insert_sql = format!("INSERT INTO temp.input ({column_list}) VALUES ({placeholders})");
        let mut statement = transaction.prepare(&insert_sql)?;
        for row in &input.rows {
            statement.execute(params_from_iter(row.iter()))?;
        }
    }
    Ok(TaxonomyCustomSqlTempTableMetadata {
        columns,
        row_count: input.rows.len(),
    })
}

fn is_safe_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn custom_sql_authorizer(context: AuthContext<'_>) -> Authorization {
    match context.action {
        AuthAction::Select | AuthAction::Recursive => Authorization::Allow,
        AuthAction::Function { function_name } => {
            if function_name.eq_ignore_ascii_case("load_extension") {
                Authorization::Deny
            } else {
                Authorization::Allow
            }
        }
        AuthAction::Pragma { pragma_name, .. } => {
            if pragma_name.eq_ignore_ascii_case("data_version") {
                Authorization::Allow
            } else {
                Authorization::Deny
            }
        }
        AuthAction::Read { table_name, .. } => {
            if is_allowed_custom_sql_read(context.database_name, table_name) {
                Authorization::Allow
            } else {
                Authorization::Deny
            }
        }
        AuthAction::Insert { table_name }
        | AuthAction::Update { table_name, .. }
        | AuthAction::Delete { table_name } => {
            if is_allowed_custom_sql_write(context.accessor, table_name) {
                Authorization::Allow
            } else {
                Authorization::Deny
            }
        }
        _ => Authorization::Deny,
    }
}

fn is_allowed_custom_sql_read(database_name: Option<&str>, table_name: &str) -> bool {
    is_taxonomy_session_table(table_name)
        || table_name.starts_with("taxon_names_fts")
        || (database_name == Some("temp") && table_name == "input")
}

fn is_allowed_custom_sql_write(accessor: Option<&str>, table_name: &str) -> bool {
    is_taxonomy_session_table(table_name)
        || (accessor.is_some() && table_name.starts_with("taxon_names_fts"))
}
