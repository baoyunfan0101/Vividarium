use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, params_from_iter};
use serde::{Deserialize, Serialize};

use super::sql_types::{SqlColumn, SqlObjectType, SqlSourceObject, SqlSourceSchema};
use crate::{CancellationToken, CoreError, CoreResult, OperationProgress, OperationProgressUnit};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum SqlDataSource {
    Csv { alias: String, path: PathBuf },
    Sqlite { alias: String, path: PathBuf },
}

pub(super) fn inspect_sql_data_source(
    source: &SqlDataSource,
    delimiter: u8,
) -> CoreResult<SqlSourceSchema> {
    validate_sources(std::slice::from_ref(source))?;
    match source {
        SqlDataSource::Csv { alias, path } => inspect_csv_source(alias, path, delimiter),
        SqlDataSource::Sqlite { alias, path } => inspect_sqlite_source(alias, path),
    }
}

pub(super) fn prepare_sources(
    connection: &mut Connection,
    sources: &[SqlDataSource],
    delimiter: u8,
) -> CoreResult<Vec<String>> {
    prepare_sources_with_progress(
        connection,
        sources,
        delimiter,
        &CancellationToken::new(),
        &mut |_| {},
    )
}

pub(super) fn prepare_sources_with_progress(
    connection: &mut Connection,
    sources: &[SqlDataSource],
    delimiter: u8,
    cancellation: &CancellationToken,
    progress: &mut (dyn FnMut(OperationProgress) + Send),
) -> CoreResult<Vec<String>> {
    validate_sources(sources)?;
    let mut attached = Vec::new();
    for source in sources {
        cancellation.check()?;
        match source {
            SqlDataSource::Csv { alias, path } => {
                load_csv_table_with_progress(
                    connection,
                    alias,
                    path,
                    delimiter,
                    cancellation,
                    progress,
                )?;
            }
            SqlDataSource::Sqlite { alias, path } => {
                attach_read_only_sqlite(connection, alias, path)?;
                attached.push(alias.clone());
            }
        }
    }
    Ok(attached)
}

pub(super) fn attach_read_only_sqlite(
    connection: &Connection,
    alias: &str,
    path: &Path,
) -> CoreResult<()> {
    let path = std::fs::canonicalize(path)?;
    let uri = sqlite_read_only_uri(&path);
    connection.execute(
        &format!("ATTACH DATABASE ? AS {}", quote_identifier(alias)),
        [uri],
    )?;
    Ok(())
}

pub(super) fn detach_sources(connection: &Connection, aliases: &[String]) -> CoreResult<()> {
    for alias in aliases.iter().rev() {
        connection.execute_batch(&format!("DETACH DATABASE {}", quote_identifier(alias)))?;
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn load_csv_table(
    connection: &mut Connection,
    alias: &str,
    path: &Path,
    delimiter: u8,
) -> CoreResult<()> {
    load_csv_table_with_progress(
        connection,
        alias,
        path,
        delimiter,
        &CancellationToken::new(),
        &mut |_| {},
    )
}

pub(super) fn load_csv_table_with_progress(
    connection: &mut Connection,
    alias: &str,
    path: &Path,
    delimiter: u8,
    cancellation: &CancellationToken,
    progress: &mut (dyn FnMut(OperationProgress) + Send),
) -> CoreResult<()> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(true)
        .flexible(false)
        .from_path(path)?;
    let columns = validated_columns(reader.headers()?.iter())?;
    let definitions = columns
        .iter()
        .map(|column| format!("{} TEXT", quote_identifier(column)))
        .collect::<Vec<_>>()
        .join(", ");
    let transaction = connection.transaction()?;
    transaction.execute_batch(&format!(
        "CREATE TEMP TABLE {} ({definitions})",
        quote_identifier(alias)
    ))?;
    let column_list = columns
        .iter()
        .map(|column| quote_identifier(column))
        .collect::<Vec<_>>()
        .join(", ");
    let placeholders = std::iter::repeat_n("?", columns.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "INSERT INTO temp.{} ({column_list}) VALUES ({placeholders})",
        quote_identifier(alias)
    );
    let mut insert = transaction.prepare(&sql)?;
    for (index, record) in reader.records().enumerate() {
        if index.is_multiple_of(1_000) {
            cancellation.check()?;
            report_source_progress(
                progress,
                "preparing_input_sources",
                Some(index as u64),
                None,
                Some(OperationProgressUnit::Items),
            );
        }
        let row_number = index + 2;
        let record = record.map_err(|error| {
            CoreError::InvalidArgument(format!("CSV row {row_number} could not be read: {error}"))
        })?;
        insert
            .execute(params_from_iter(record.iter()))
            .map_err(|error| {
                CoreError::InvalidArgument(format!(
                    "CSV row {row_number} could not be inserted: {error}"
                ))
            })?;
    }
    let loaded_rows = reader.position().record().saturating_sub(1);
    report_source_progress(
        progress,
        "preparing_input_sources",
        Some(loaded_rows),
        None,
        Some(OperationProgressUnit::Items),
    );
    drop(insert);
    transaction.commit()?;
    Ok(())
}

fn validate_sources(sources: &[SqlDataSource]) -> CoreResult<()> {
    let mut aliases = HashSet::new();
    for source in sources {
        let alias = match source {
            SqlDataSource::Csv { alias, path } | SqlDataSource::Sqlite { alias, path } => {
                if !path.is_file() {
                    return Err(CoreError::NotFound(format!(
                        "sql data source {}",
                        path.display()
                    )));
                }
                alias
            }
        };
        if !is_safe_identifier(alias) {
            return Err(CoreError::InvalidArgument(format!(
                "invalid sql data source alias: {alias}"
            )));
        }
        let normalized = alias.to_ascii_lowercase();
        if matches!(
            normalized.as_str(),
            "main"
                | "temp"
                | "base"
                | "metadata"
                | "taxonomy"
                | "taxonomy_base"
                | "active_photo_library"
        ) {
            return Err(CoreError::InvalidArgument(format!(
                "reserved sql data source alias: {alias}"
            )));
        }
        if !aliases.insert(normalized) {
            return Err(CoreError::InvalidArgument(format!(
                "duplicate sql data source alias: {alias}"
            )));
        }
    }
    Ok(())
}

fn inspect_csv_source(alias: &str, path: &Path, delimiter: u8) -> CoreResult<SqlSourceSchema> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(true)
        .flexible(false)
        .from_path(path)?;
    let columns = validated_columns(reader.headers()?.iter())?
        .into_iter()
        .map(|name| SqlColumn {
            name,
            declared_type: Some("TEXT".into()),
        })
        .collect();
    Ok(SqlSourceSchema {
        alias: alias.into(),
        objects: vec![SqlSourceObject {
            name: alias.into(),
            object_type: SqlObjectType::Table,
            columns,
        }],
    })
}

pub(super) fn inspect_sqlite_source(alias: &str, path: &Path) -> CoreResult<SqlSourceSchema> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI,
    )?;
    let mut statement = connection.prepare(
        r#"
        SELECT name, type, sql
        FROM sqlite_schema
        WHERE type IN ('table', 'view')
          AND name NOT LIKE 'sqlite_%'
        ORDER BY name
        "#,
    )?;
    let objects = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|(name, object_type, sql)| {
            let object_type = if sql.as_deref().is_some_and(|sql| {
                sql.trim_start()
                    .to_ascii_uppercase()
                    .starts_with("CREATE VIRTUAL TABLE")
            }) {
                SqlObjectType::VirtualTable
            } else if object_type == "view" {
                SqlObjectType::View
            } else {
                SqlObjectType::Table
            };
            let columns = inspect_object_columns(&connection, &name)?;
            Ok(SqlSourceObject {
                name,
                object_type,
                columns,
            })
        })
        .collect::<CoreResult<Vec<_>>>()?;
    Ok(SqlSourceSchema {
        alias: alias.into(),
        objects,
    })
}

fn inspect_object_columns(connection: &Connection, object: &str) -> CoreResult<Vec<SqlColumn>> {
    let mut statement =
        connection.prepare(&format!("PRAGMA table_xinfo({})", quote_identifier(object)))?;
    statement
        .query_map([], |row| {
            let declared_type = row.get::<_, String>(2)?;
            Ok(SqlColumn {
                name: row.get(1)?,
                declared_type: (!declared_type.is_empty()).then_some(declared_type),
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub(super) fn validated_columns<'a>(
    columns: impl Iterator<Item = &'a str>,
) -> CoreResult<Vec<String>> {
    let mut seen = BTreeSet::new();
    let mut output = Vec::new();
    for column in columns {
        if column.trim().is_empty() || column.contains('\0') {
            return Err(CoreError::InvalidArgument(format!(
                "invalid sql source column: {column}"
            )));
        }
        if !seen.insert(column.to_ascii_lowercase()) {
            return Err(CoreError::InvalidArgument(format!(
                "duplicate sql source column: {column}"
            )));
        }
        output.push(column.to_string());
    }
    if output.is_empty() {
        return Err(CoreError::InvalidArgument(
            "sql source requires at least one column".into(),
        ));
    }
    Ok(output)
}

pub(super) fn is_safe_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

pub(super) fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn sqlite_read_only_uri(path: &Path) -> String {
    let path = path.to_string_lossy();
    let encoded = path
        .bytes()
        .flat_map(|byte| match byte {
            b'%' | b'?' | b'#' => {
                let hex = b"0123456789ABCDEF";
                vec![b'%', hex[(byte >> 4) as usize], hex[(byte & 0x0f) as usize]]
            }
            _ => vec![byte],
        })
        .collect::<Vec<_>>();
    format!("file:{}?mode=ro", String::from_utf8_lossy(&encoded))
}

fn report_source_progress(
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
