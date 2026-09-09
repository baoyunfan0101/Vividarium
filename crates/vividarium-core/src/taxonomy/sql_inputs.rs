use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::backup::Backup;
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::cleanup;
use super::sql_sources::{SqlDataSource, inspect_sql_data_source, is_safe_identifier};
use super::sql_types::SqlSourceSchema;
use crate::{CoreError, CoreResult, Database};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SqlInputKind {
    Sqlite,
    Csv,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AddSqlInputRequest {
    pub kind: SqlInputKind,
    pub alias: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoveSqlInputRequest {
    pub alias: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistentSqlInput {
    pub kind: SqlInputKind,
    pub alias: String,
    pub original_path: PathBuf,
    pub available: bool,
    pub schema: SqlSourceSchema,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AddSqlInputResult {
    pub input: PersistentSqlInput,
    pub inputs: Vec<PersistentSqlInput>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoveSqlInputResult {
    pub inputs: Vec<PersistentSqlInput>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SqlInputScope {
    CustomSql,
    SqlImport,
}

impl SqlInputScope {
    const fn code(self) -> i64 {
        match self {
            Self::CustomSql => 1,
            Self::SqlImport => 2,
        }
    }

    const fn directory(self) -> &'static str {
        match self {
            Self::CustomSql => "custom-sql",
            Self::SqlImport => "sql-import",
        }
    }
}

pub(crate) fn list_inputs(
    database: &Database,
    scope: SqlInputScope,
) -> CoreResult<Vec<PersistentSqlInput>> {
    let connection = database.connect_metadata()?;
    list_inputs_on_connection(&connection, scope)
}

fn list_inputs_on_connection(
    connection: &Connection,
    scope: SqlInputScope,
) -> CoreResult<Vec<PersistentSqlInput>> {
    let mut statement = connection.prepare(
        r#"
        SELECT alias, source_type, original_path, stored_path, schema_json
        FROM sql_inputs
        WHERE scope = ?
        ORDER BY alias COLLATE NOCASE
        "#,
    )?;
    statement
        .query_map([scope.code()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|(alias, kind, original_path, stored_path, schema_json)| {
            let kind = kind_from_code(kind)?;
            let schema = serde_json::from_str(&schema_json).map_err(|error| {
                CoreError::Consistency(format!("invalid SQL input schema: {error}"))
            })?;
            Ok(PersistentSqlInput {
                kind,
                alias,
                original_path: PathBuf::from(original_path),
                available: Path::new(&stored_path).is_file(),
                schema,
            })
        })
        .collect()
}

pub(crate) fn stored_sources(
    database: &Database,
    scope: SqlInputScope,
) -> CoreResult<Vec<SqlDataSource>> {
    let connection = database.connect_metadata()?;
    let mut statement = connection.prepare(
        r#"
        SELECT alias, source_type, stored_path
        FROM sql_inputs
        WHERE scope = ?
        ORDER BY alias COLLATE NOCASE
        "#,
    )?;
    statement
        .query_map([scope.code()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|(alias, kind, path)| {
            let path = PathBuf::from(path);
            if !path.is_file() {
                return Err(CoreError::NotFound(format!("stored SQL input {alias}")));
            }
            match kind_from_code(kind)? {
                SqlInputKind::Csv => Ok(SqlDataSource::Csv { alias, path }),
                SqlInputKind::Sqlite => Ok(SqlDataSource::Sqlite { alias, path }),
            }
        })
        .collect()
}

pub(crate) fn add_input(
    database: &Database,
    scope: SqlInputScope,
    request: &AddSqlInputRequest,
) -> CoreResult<AddSqlInputResult> {
    let warnings = cleanup::retry_pending(database);
    validate_alias(&request.alias)?;
    if !request.path.is_file() {
        return Err(CoreError::NotFound(format!(
            "SQL input {}",
            request.path.display()
        )));
    }
    let source = data_source(request.kind, request.alias.clone(), request.path.clone());
    let delimiter = crate::general::get_csv_delimiter_byte(database)?;
    inspect_sql_data_source(&source, delimiter)?;
    let directory = input_directory(database, scope);
    fs::create_dir_all(&directory)?;
    let extension = match request.kind {
        SqlInputKind::Csv => "csv",
        SqlInputKind::Sqlite => "db",
    };
    let stored_path = directory.join(format!("{}.{}", Uuid::new_v4(), extension));
    let mut file_guard = InputFileGuard::new(stored_path.clone());
    let added = (|| {
        copy_input(request.kind, &request.path, &stored_path)?;
        let stored_source = data_source(request.kind, request.alias.clone(), stored_path.clone());
        let schema = inspect_sql_data_source(&stored_source, delimiter)?;
        let schema_json = serde_json::to_string(&schema).map_err(|error| {
            CoreError::Consistency(format!("could not serialize SQL input schema: {error}"))
        })?;
        let mut connection = database.connect_metadata()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            r#"
            INSERT INTO sql_inputs (
                scope, alias, source_type, original_path, stored_path, schema_json
            ) VALUES (?, ?, ?, ?, ?, ?)
            "#,
            params![
                scope.code(),
                request.alias,
                kind_code(request.kind),
                request.path.to_string_lossy(),
                stored_path.to_string_lossy(),
                schema_json,
            ],
        )?;
        let inputs = list_inputs_on_connection(&transaction, scope)?;
        transaction.commit()?;
        Ok((
            PersistentSqlInput {
                kind: request.kind,
                alias: request.alias.clone(),
                original_path: request.path.clone(),
                available: true,
                schema,
            },
            inputs,
        ))
    })();
    match added {
        Ok((input, inputs)) => {
            file_guard.disarm();
            Ok(AddSqlInputResult {
                input,
                inputs,
                warnings,
            })
        }
        Err(error) => Err(file_guard.cleanup_error(database, error)),
    }
}

pub(crate) fn remove_input(
    database: &Database,
    scope: SqlInputScope,
    request: &RemoveSqlInputRequest,
) -> CoreResult<RemoveSqlInputResult> {
    let mut warnings = cleanup::retry_pending(database);
    let mut connection = database.connect_metadata()?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let stored_path = transaction
        .query_row(
            "SELECT stored_path FROM sql_inputs WHERE scope = ? AND alias = ?",
            params![scope.code(), request.alias],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| CoreError::NotFound(format!("SQL input {}", request.alias)))?;
    transaction.execute(
        "DELETE FROM sql_inputs WHERE scope = ? AND alias = ?",
        params![scope.code(), request.alias],
    )?;
    let inputs = list_inputs_on_connection(&transaction, scope)?;
    transaction.commit()?;
    if let Some(warning) =
        cleanup::remove_or_defer(database, Path::new(&stored_path), "stored SQL input")
    {
        warnings.push(warning);
    }
    Ok(RemoveSqlInputResult { inputs, warnings })
}

struct InputFileGuard {
    path: PathBuf,
    armed: bool,
}

impl InputFileGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }

    fn cleanup_error(mut self, database: &Database, error: CoreError) -> CoreError {
        self.armed = false;
        match cleanup::remove_or_defer(database, &self.path, "unregistered SQL input") {
            Some(warning) => CoreError::Consistency(format!("{error}; {warning}")),
            None => error,
        }
    }
}

impl Drop for InputFileGuard {
    fn drop(&mut self) {
        if self.armed && self.path.exists() {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn input_directory(database: &Database, scope: SqlInputScope) -> PathBuf {
    database
        .metadata_path()
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("sql-inputs")
        .join(scope.directory())
}

fn validate_alias(alias: &str) -> CoreResult<()> {
    if !is_safe_identifier(alias) {
        return Err(CoreError::InvalidArgument(format!(
            "invalid SQL input alias: {alias}"
        )));
    }
    if matches!(
        alias.to_ascii_lowercase().as_str(),
        "main" | "temp" | "sql_import" | "metadata" | "taxonomy" | "active_photo_library"
    ) {
        return Err(CoreError::InvalidArgument(format!(
            "reserved SQL input alias: {alias}"
        )));
    }
    Ok(())
}

fn data_source(kind: SqlInputKind, alias: String, path: PathBuf) -> SqlDataSource {
    match kind {
        SqlInputKind::Csv => SqlDataSource::Csv { alias, path },
        SqlInputKind::Sqlite => SqlDataSource::Sqlite { alias, path },
    }
}

fn copy_input(kind: SqlInputKind, source_path: &Path, stored_path: &Path) -> CoreResult<()> {
    match kind {
        SqlInputKind::Csv => {
            fs::copy(source_path, stored_path)?;
        }
        SqlInputKind::Sqlite => {
            let source = Connection::open_with_flags(
                source_path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            let mut destination = Connection::open(stored_path)?;
            let backup = Backup::new(&source, &mut destination)?;
            backup.run_to_completion(256, Duration::from_millis(10), None)?;
        }
    }
    Ok(())
}

const fn kind_code(kind: SqlInputKind) -> i64 {
    match kind {
        SqlInputKind::Sqlite => 1,
        SqlInputKind::Csv => 2,
    }
}

fn kind_from_code(code: i64) -> CoreResult<SqlInputKind> {
    match code {
        1 => Ok(SqlInputKind::Sqlite),
        2 => Ok(SqlInputKind::Csv),
        _ => Err(CoreError::Consistency(format!(
            "invalid SQL input kind: {code}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_persist_across_database_reopen_and_remove_authoritatively() {
        let directory = tempfile::tempdir().unwrap();
        let metadata_path = directory.path().join("metadata.db");
        let csv_path = directory.path().join("taxa.csv");
        fs::write(&csv_path, "taxon_id,name\n1,Animalia\n").unwrap();
        let database = Database::open(&metadata_path).unwrap();
        add_input(
            &database,
            SqlInputScope::CustomSql,
            &AddSqlInputRequest {
                kind: SqlInputKind::Csv,
                alias: "taxa_csv".into(),
                path: csv_path,
            },
        )
        .unwrap();
        drop(database);

        let database = Database::open(&metadata_path).unwrap();
        let inputs = list_inputs(&database, SqlInputScope::CustomSql).unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].alias, "taxa_csv");
        assert!(inputs[0].available);
        let result = remove_input(
            &database,
            SqlInputScope::CustomSql,
            &RemoveSqlInputRequest {
                alias: "taxa_csv".into(),
            },
        )
        .unwrap();
        assert!(result.inputs.is_empty());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn registry_failures_remove_new_csv_and_sqlite_files() {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(directory.path().join("metadata.db")).unwrap();
        let csv_path = directory.path().join("taxa.csv");
        fs::write(&csv_path, "taxon_id,name\n1,Animalia\n").unwrap();
        let sqlite_path = directory.path().join("taxa.db");
        Connection::open(&sqlite_path)
            .unwrap()
            .execute("CREATE TABLE taxa (taxon_id INTEGER)", [])
            .unwrap();

        let first = add_input(
            &database,
            SqlInputScope::CustomSql,
            &AddSqlInputRequest {
                kind: SqlInputKind::Csv,
                alias: "source".into(),
                path: csv_path.clone(),
            },
        )
        .unwrap();
        assert_eq!(first.inputs.len(), 1);
        assert!(
            add_input(
                &database,
                SqlInputScope::CustomSql,
                &AddSqlInputRequest {
                    kind: SqlInputKind::Sqlite,
                    alias: "source".into(),
                    path: sqlite_path.clone(),
                },
            )
            .is_err()
        );
        assert_eq!(
            fs::read_dir(input_directory(&database, SqlInputScope::CustomSql))
                .unwrap()
                .count(),
            1
        );

        add_input(
            &database,
            SqlInputScope::SqlImport,
            &AddSqlInputRequest {
                kind: SqlInputKind::Sqlite,
                alias: "source".into(),
                path: sqlite_path,
            },
        )
        .unwrap();
        assert!(
            add_input(
                &database,
                SqlInputScope::SqlImport,
                &AddSqlInputRequest {
                    kind: SqlInputKind::Csv,
                    alias: "source".into(),
                    path: csv_path,
                },
            )
            .is_err()
        );
        assert_eq!(
            fs::read_dir(input_directory(&database, SqlInputScope::SqlImport))
                .unwrap()
                .count(),
            1
        );
    }
}
