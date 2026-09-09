//! Operation summaries, audit rows, and cursor-page contracts.
//!
//! Domain-specific list, export, and rollback functions are exposed by
//! [`crate::photos`] and [`crate::taxonomy`].

use std::io::Write;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OptionalExtension, Transaction, params, params_from_iter};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::taxonomy::TaxonInputRow;
use crate::{CoreError, CoreResult};

const AUDIT_COLUMNS: [&str; 9] = [
    "operation_id",
    "sequence",
    "entity_type",
    "entity_id",
    "action",
    "before_json",
    "after_json",
    "succeeded",
    "message",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperationSummary {
    pub operation_id: i64,
    pub kind: String,
    pub source: String,
    pub applied_at: String,
    pub total_items: usize,
    pub succeeded_items: usize,
    pub failed_items: usize,
    pub rollbackable: bool,
    pub has_formatted_input: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OperationAuditRow {
    pub operation_id: i64,
    pub sequence: usize,
    pub entity_type: String,
    pub entity_id: Option<String>,
    pub action: String,
    pub before_json: Option<Value>,
    pub after_json: Option<Value>,
    pub succeeded: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OperationPage<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OperationInput {
    CustomSql { sql: String },
    FormattedUpdate { rows: Vec<TaxonInputRow> },
    TaxonomyAction { action: String, input: Value },
}

#[derive(Debug, Clone)]
pub(crate) struct NewOperation<'a> {
    pub kind: &'a str,
    pub source: &'a str,
    pub total_items: usize,
    pub succeeded_items: usize,
    pub failed_items: usize,
    pub rollbackable: bool,
    pub has_formatted_input: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct NewAuditRow<'a> {
    pub sequence: usize,
    pub entity_type: &'a str,
    pub entity_id: Option<String>,
    pub action: &'a str,
    pub before_json: Option<Value>,
    pub after_json: Option<Value>,
    pub succeeded: bool,
    pub message: &'a str,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum OperationCursor {
    Summaries { operation_id: i64 },
    Audit { operation_id: i64, sequence: usize },
}

pub(crate) fn insert_operation(
    transaction: &Transaction<'_>,
    operation: NewOperation<'_>,
) -> CoreResult<i64> {
    if operation.succeeded_items + operation.failed_items != operation.total_items {
        return Err(CoreError::Consistency(
            "operation item totals do not balance".into(),
        ));
    }
    transaction.execute(
        r#"
        INSERT INTO operations (
            kind, source, total_items, succeeded_items, failed_items,
            rollbackable, has_formatted_input
        ) VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
        params![
            operation.kind,
            operation.source,
            operation.total_items as i64,
            operation.succeeded_items as i64,
            operation.failed_items as i64,
            operation.rollbackable,
            operation.has_formatted_input
        ],
    )?;
    Ok(transaction.last_insert_rowid())
}

pub(crate) fn update_operation_counts(
    transaction: &Transaction<'_>,
    operation_id: i64,
    total_items: usize,
    succeeded_items: usize,
    failed_items: usize,
) -> CoreResult<()> {
    if succeeded_items + failed_items != total_items {
        return Err(CoreError::Consistency(
            "operation item totals do not balance".into(),
        ));
    }
    let updated = transaction.execute(
        r#"
        UPDATE operations
        SET total_items = ?, succeeded_items = ?, failed_items = ?
        WHERE operation_id = ?
        "#,
        params![
            total_items as i64,
            succeeded_items as i64,
            failed_items as i64,
            operation_id
        ],
    )?;
    if updated != 1 {
        return Err(CoreError::NotFound(format!("operation {operation_id}")));
    }
    Ok(())
}

pub(crate) fn insert_audit_row(
    transaction: &Transaction<'_>,
    operation_id: i64,
    row: NewAuditRow<'_>,
) -> CoreResult<()> {
    let before_json = row
        .before_json
        .map(|value| serde_json::to_string(&value))
        .transpose()
        .map_err(invalid_json)?;
    let after_json = row
        .after_json
        .map(|value| serde_json::to_string(&value))
        .transpose()
        .map_err(invalid_json)?;
    transaction.execute(
        r#"
        INSERT INTO operation_audit_rows (
            operation_id, sequence, entity_type, entity_id, action,
            before_json, after_json, succeeded, message
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
        params![
            operation_id,
            row.sequence as i64,
            row.entity_type,
            row.entity_id,
            row.action,
            before_json,
            after_json,
            row.succeeded,
            row.message
        ],
    )?;
    Ok(())
}

pub(crate) fn insert_operation_input(
    transaction: &Transaction<'_>,
    operation_id: i64,
    input: &OperationInput,
) -> CoreResult<()> {
    transaction.execute(
        "INSERT INTO operation_inputs (operation_id, input_json) VALUES (?, ?)",
        params![
            operation_id,
            serde_json::to_string(input).map_err(invalid_operation_input)?
        ],
    )?;
    Ok(())
}

pub(crate) fn get_operation_input(
    connection: &Connection,
    operation_id: i64,
) -> CoreResult<Option<OperationInput>> {
    let operation = get_operation(connection, operation_id)?
        .ok_or_else(|| CoreError::NotFound(format!("operation {operation_id}")))?;
    if operation.source == "formatted_update" && operation.has_formatted_input {
        let mut statement = connection.prepare(
            r#"
            SELECT input_json
            FROM operation_formatted_inputs
            WHERE operation_id = ?
            ORDER BY sequence
            "#,
        )?;
        let rows = statement
            .query_map([operation_id], |row| row.get::<_, String>(0))?
            .map(|row| {
                let json = row?;
                serde_json::from_str::<TaxonInputRow>(&json).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        json.len(),
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(Some(OperationInput::FormattedUpdate { rows }));
    }
    let input_json = connection
        .query_row(
            "SELECT input_json FROM operation_inputs WHERE operation_id = ?",
            [operation_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    input_json
        .map(|json| {
            serde_json::from_str(&json).map_err(|error| {
                CoreError::Consistency(format!("invalid operation input: {error}"))
            })
        })
        .transpose()
}

pub(crate) fn list_operations(
    connection: &Connection,
    cursor: Option<&str>,
    limit: usize,
) -> CoreResult<OperationPage<OperationSummary>> {
    let before_id = match decode_cursor(cursor)? {
        None => None,
        Some(OperationCursor::Summaries { operation_id }) => Some(operation_id),
        Some(_) => return Err(invalid_cursor()),
    };
    let limit = limit.clamp(1, 500);
    let mut items = if let Some(before_id) = before_id {
        let mut statement = connection.prepare(
            r#"
            SELECT operation_id, kind, source, applied_at,
                   total_items, succeeded_items, failed_items,
                   rollbackable, has_formatted_input
            FROM operations
            WHERE operation_id < ?
            ORDER BY operation_id DESC
            LIMIT ?
            "#,
        )?;
        statement
            .query_map(params![before_id, (limit + 1) as i64], summary_from_row)?
            .collect::<Result<Vec<_>, _>>()?
    } else {
        let mut statement = connection.prepare(
            r#"
            SELECT operation_id, kind, source, applied_at,
                   total_items, succeeded_items, failed_items,
                   rollbackable, has_formatted_input
            FROM operations
            ORDER BY operation_id DESC
            LIMIT ?
            "#,
        )?;
        statement
            .query_map([(limit + 1) as i64], summary_from_row)?
            .collect::<Result<Vec<_>, _>>()?
    };
    let has_more = items.len() > limit;
    items.truncate(limit);
    let next_cursor = has_more
        .then(|| {
            items.last().map(|item| {
                encode_cursor(&OperationCursor::Summaries {
                    operation_id: item.operation_id,
                })
            })
        })
        .flatten()
        .transpose()?;
    Ok(OperationPage { items, next_cursor })
}

pub(crate) fn get_operation(
    connection: &Connection,
    operation_id: i64,
) -> CoreResult<Option<OperationSummary>> {
    connection
        .query_row(
            r#"
            SELECT operation_id, kind, source, applied_at,
                   total_items, succeeded_items, failed_items,
                   rollbackable, has_formatted_input
            FROM operations
            WHERE operation_id = ?
            "#,
            [operation_id],
            summary_from_row,
        )
        .optional()
        .map_err(Into::into)
}

pub(crate) fn list_operation_audit(
    connection: &Connection,
    operation_id: i64,
    cursor: Option<&str>,
    limit: usize,
) -> CoreResult<OperationPage<OperationAuditRow>> {
    ensure_operation_exists(connection, operation_id)?;
    let after_sequence = match decode_cursor(cursor)? {
        None => 0,
        Some(OperationCursor::Audit {
            operation_id: cursor_operation_id,
            sequence,
        }) if cursor_operation_id == operation_id => sequence,
        Some(_) => return Err(invalid_cursor()),
    };
    let limit = limit.clamp(1, 500);
    let mut statement = connection.prepare(
        r#"
        SELECT operation_id, sequence, entity_type, entity_id, action,
               before_json, after_json, succeeded, message
        FROM operation_audit_rows
        WHERE operation_id = ? AND sequence > ?
        ORDER BY sequence
        LIMIT ?
        "#,
    )?;
    let mut items = statement
        .query_map(
            params![operation_id, after_sequence as i64, (limit + 1) as i64],
            audit_from_row,
        )?
        .map(|row| row.and_then(parse_audit_json))
        .collect::<Result<Vec<_>, _>>()?;
    let has_more = items.len() > limit;
    items.truncate(limit);
    let next_cursor = has_more
        .then(|| {
            items.last().map(|item| {
                encode_cursor(&OperationCursor::Audit {
                    operation_id,
                    sequence: item.sequence,
                })
            })
        })
        .flatten()
        .transpose()?;
    Ok(OperationPage { items, next_cursor })
}

pub(crate) fn write_operation_audit<W: Write>(
    connection: &Connection,
    operation_ids: Option<&[i64]>,
    delimiter: u8,
    writer: W,
) -> CoreResult<()> {
    if let Some(operation_ids) = operation_ids {
        validate_operation_ids(connection, operation_ids)?;
    }
    let mut csv = csv::WriterBuilder::new()
        .delimiter(delimiter)
        .from_writer(writer);
    csv.write_record(AUDIT_COLUMNS)?;
    let (filter, values) = operation_filter(operation_ids);
    let mut statement = connection.prepare(&format!(
        r#"
        SELECT operation_id, sequence, entity_type, entity_id, action,
               before_json, after_json, succeeded, message
        FROM operation_audit_rows
        {filter}
        ORDER BY operation_id, sequence
        "#
    ))?;
    let mut rows = statement.query(params_from_iter(values))?;
    while let Some(row) = rows.next()? {
        csv.write_record([
            row.get::<_, i64>(0)?.to_string(),
            row.get::<_, i64>(1)?.to_string(),
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?.unwrap_or_default(),
            row.get::<_, Option<String>>(6)?.unwrap_or_default(),
            row.get::<_, bool>(7)?.to_string(),
            row.get::<_, String>(8)?,
        ])?;
    }
    csv.flush()?;
    Ok(())
}

pub(crate) fn delete_operation(transaction: &Transaction<'_>, operation_id: i64) -> CoreResult<()> {
    let deleted = transaction.execute(
        "DELETE FROM operations WHERE operation_id = ?",
        [operation_id],
    )?;
    if deleted != 1 {
        return Err(CoreError::NotFound(format!("operation {operation_id}")));
    }
    Ok(())
}

fn summary_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<OperationSummary> {
    Ok(OperationSummary {
        operation_id: row.get(0)?,
        kind: row.get(1)?,
        source: row.get(2)?,
        applied_at: row.get(3)?,
        total_items: row.get::<_, i64>(4)? as usize,
        succeeded_items: row.get::<_, i64>(5)? as usize,
        failed_items: row.get::<_, i64>(6)? as usize,
        rollbackable: row.get(7)?,
        has_formatted_input: row.get(8)?,
    })
}

type StoredAuditRow = (
    i64,
    i64,
    String,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    bool,
    String,
);

fn audit_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredAuditRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
    ))
}

fn parse_audit_json(row: StoredAuditRow) -> rusqlite::Result<OperationAuditRow> {
    let (operation_id, sequence, entity_type, entity_id, action, before, after, succeeded, message) =
        row;
    Ok(OperationAuditRow {
        operation_id,
        sequence: sequence as usize,
        entity_type,
        entity_id,
        action,
        before_json: parse_optional_json(before)?,
        after_json: parse_optional_json(after)?,
        succeeded,
        message,
    })
}

fn parse_optional_json(value: Option<String>) -> rusqlite::Result<Option<Value>> {
    value
        .map(|value| {
            serde_json::from_str(&value).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    value.len(),
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })
        .transpose()
}

fn validate_operation_ids(connection: &Connection, operation_ids: &[i64]) -> CoreResult<()> {
    if operation_ids.is_empty() {
        return Err(CoreError::InvalidArgument(
            "at least one operation id is required".into(),
        ));
    }
    if operation_ids.iter().any(|value| *value <= 0) {
        return Err(CoreError::InvalidArgument(
            "operation ids must be positive".into(),
        ));
    }
    let unique = operation_ids
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let placeholders = std::iter::repeat_n("?", unique.len())
        .collect::<Vec<_>>()
        .join(",");
    let count = connection.query_row(
        &format!("SELECT COUNT(*) FROM operations WHERE operation_id IN ({placeholders})"),
        params_from_iter(unique.iter()),
        |row| row.get::<_, usize>(0),
    )?;
    if count != unique.len() {
        return Err(CoreError::NotFound(
            "one or more operations do not exist".into(),
        ));
    }
    Ok(())
}

fn ensure_operation_exists(connection: &Connection, operation_id: i64) -> CoreResult<()> {
    if get_operation(connection, operation_id)?.is_none() {
        return Err(CoreError::NotFound(format!("operation {operation_id}")));
    }
    Ok(())
}

fn operation_filter(operation_ids: Option<&[i64]>) -> (String, Vec<SqlValue>) {
    let Some(operation_ids) = operation_ids else {
        return (String::new(), Vec::new());
    };
    let unique = operation_ids
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let placeholders = std::iter::repeat_n("?", unique.len())
        .collect::<Vec<_>>()
        .join(",");
    (
        format!("WHERE operation_id IN ({placeholders})"),
        unique.into_iter().map(SqlValue::Integer).collect(),
    )
}

fn encode_cursor(cursor: &OperationCursor) -> CoreResult<String> {
    let json = serde_json::to_vec(cursor).map_err(|error| {
        CoreError::InvalidArgument(format!("invalid operation cursor: {error}"))
    })?;
    Ok(URL_SAFE_NO_PAD.encode(json))
}

fn decode_cursor(cursor: Option<&str>) -> CoreResult<Option<OperationCursor>> {
    let Some(cursor) = cursor.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let bytes = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| invalid_cursor())?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| invalid_cursor())
}

fn invalid_cursor() -> CoreError {
    CoreError::InvalidArgument("invalid operation cursor".into())
}

fn invalid_json(error: serde_json::Error) -> CoreError {
    CoreError::InvalidArgument(format!("invalid operation audit JSON: {error}"))
}

fn invalid_operation_input(error: serde_json::Error) -> CoreError {
    CoreError::InvalidArgument(format!("invalid operation input: {error}"))
}

#[cfg(test)]
#[path = "operations/tests.rs"]
mod tests;
