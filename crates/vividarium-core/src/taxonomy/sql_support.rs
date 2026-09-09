use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use base64::Engine;
use rusqlite::{Connection, ffi};

use super::sql_types::{SqlColumn, SqlValue};
use crate::{CancellationToken, CoreError, CoreResult};

pub(super) const CUSTOM_SQL_STATEMENT_TIMEOUT: Duration = Duration::from_secs(30);
pub(super) const SQL_IMPORT_STATEMENT_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Debug, Clone, Copy)]
pub(super) struct SqlStatementExecutionLimits {
    pub(super) timeout: Duration,
}

pub(super) struct SqlStatementExecutionContext<'a> {
    pub(super) cancellation: &'a CancellationToken,
    pub(super) limits: SqlStatementExecutionLimits,
    pub(super) statement_index: u64,
    pub(super) statement_total: u64,
    pub(super) workflow: &'static str,
}

pub(super) struct RawScriptStep {
    pub(super) tail_offset: usize,
    pub(super) statement: Option<RawStatementExecution>,
}

pub(super) struct RawPreparedStatement {
    pub(super) tail_offset: usize,
    pub(super) statement: Option<RawStatement>,
}

pub(super) struct RawStatementExecution {
    pub(super) columns: Vec<SqlColumn>,
    pub(super) rows: Vec<Vec<SqlValue>>,
    pub(super) returned_rows: u64,
    pub(super) truncated: bool,
    pub(super) read_only: bool,
    pub(super) affected_rows: u64,
}

pub(super) unsafe fn execute_preview_statement_guarded(
    connection: &Connection,
    sql_tail: &str,
    maximum_result_rows: usize,
    context: &SqlStatementExecutionContext<'_>,
) -> CoreResult<RawScriptStep> {
    unsafe {
        execute_one_statement_guarded(connection, sql_tail, maximum_result_rows, true, context)
    }
}

pub(super) unsafe fn execute_statement_to_completion_guarded(
    connection: &Connection,
    sql_tail: &str,
    context: &SqlStatementExecutionContext<'_>,
) -> CoreResult<RawScriptStep> {
    unsafe { execute_one_statement_guarded(connection, sql_tail, 0, false, context) }
}

unsafe fn execute_one_statement_guarded(
    connection: &Connection,
    sql_tail: &str,
    maximum_result_rows: usize,
    stop_read_only_after_preview: bool,
    context: &SqlStatementExecutionContext<'_>,
) -> CoreResult<RawScriptStep> {
    let started_at = Instant::now();
    let result = with_sql_statement_guard(connection, context, || unsafe {
        execute_one_statement_raw(
            connection,
            sql_tail,
            maximum_result_rows,
            stop_read_only_after_preview,
        )
    });
    let elapsed = started_at.elapsed();
    if let Ok(step) = &result {
        if let Some(statement) = &step.statement {
            eprintln!(
                "{} statement {}/{} completed in {} ms (read_only={}, returned_rows={}, affected_rows={})",
                context.workflow,
                context.statement_index,
                context.statement_total,
                elapsed.as_millis(),
                statement.read_only,
                statement.returned_rows,
                statement.affected_rows
            );
        }
    }
    result
}

pub(super) fn with_sql_statement_guard<T>(
    connection: &Connection,
    context: &SqlStatementExecutionContext<'_>,
    execute: impl FnOnce() -> CoreResult<T>,
) -> CoreResult<T> {
    context.cancellation.check()?;
    let started_at = Instant::now();
    let deadline = started_at + context.limits.timeout;
    let timed_out = Arc::new(AtomicBool::new(false));
    let handler_timed_out = timed_out.clone();
    let cancellation = context.cancellation.clone();
    connection.progress_handler(
        1_000,
        Some(move || {
            if cancellation.is_cancelled() {
                return true;
            }
            if Instant::now() >= deadline {
                handler_timed_out.store(true, Ordering::Release);
                return true;
            }
            false
        }),
    );
    let result = execute();
    context
        .cancellation
        .install_sqlite_progress_handler(connection);
    let elapsed = started_at.elapsed();
    if timed_out.load(Ordering::Acquire) {
        eprintln!(
            "{} statement {}/{} timed out after {} ms",
            context.workflow,
            context.statement_index,
            context.statement_total,
            elapsed.as_millis()
        );
        return Err(CoreError::InvalidArgument(format!(
            "{} statement {} of {} exceeded the {} execution limit.",
            context.workflow,
            context.statement_index,
            context.statement_total,
            format_duration(context.limits.timeout)
        )));
    }
    if context.cancellation.is_cancelled() {
        eprintln!(
            "{} statement {}/{} cancelled after {} ms",
            context.workflow,
            context.statement_index,
            context.statement_total,
            elapsed.as_millis()
        );
        return Err(CoreError::Cancelled);
    }
    result
}

fn format_duration(duration: Duration) -> String {
    if duration.as_secs() > 0 && duration.subsec_nanos() == 0 {
        format!("{} second", duration.as_secs())
    } else {
        format!("{} ms", duration.as_millis())
    }
}

pub(super) fn count_sql_statements(sql: &str) -> CoreResult<u64> {
    let mut statement_start = 0;
    let mut statement_count = 0_u64;
    for (index, character) in sql.char_indices() {
        if character != ';' {
            continue;
        }
        let statement_end = index + character.len_utf8();
        let candidate = CString::new(&sql[statement_start..statement_end])
            .map_err(|error| CoreError::InvalidArgument(format!("invalid sql: {error}")))?;
        if unsafe { ffi::sqlite3_complete(candidate.as_ptr()) } != 0 {
            statement_count += 1;
            statement_start = statement_end;
        }
    }
    if !sql[statement_start..].trim().is_empty() {
        statement_count += 1;
    }
    Ok(statement_count)
}

unsafe fn execute_one_statement_raw(
    connection: &Connection,
    sql_tail: &str,
    maximum_result_rows: usize,
    stop_read_only_after_preview: bool,
) -> CoreResult<RawScriptStep> {
    let database = unsafe { connection.handle() };
    let prepared = unsafe { prepare_statement_raw(connection, sql_tail) }?;
    let Some(statement) = prepared.statement else {
        return Ok(RawScriptStep {
            tail_offset: prepared.tail_offset,
            statement: None,
        });
    };
    let column_count = unsafe { ffi::sqlite3_column_count(statement.0) as usize };
    let columns = unsafe { statement_columns(statement.0, column_count) };
    let read_only = unsafe { ffi::sqlite3_stmt_readonly(statement.0) != 0 };
    let mut rows = Vec::new();
    let mut returned_rows = 0_u64;
    let mut truncated = false;
    loop {
        let step = unsafe { ffi::sqlite3_step(statement.0) };
        match step {
            ffi::SQLITE_ROW => {
                returned_rows += 1;
                if rows.len() < maximum_result_rows {
                    rows.push(unsafe { statement_row(statement.0, column_count) });
                } else {
                    truncated = true;
                    if read_only && stop_read_only_after_preview {
                        break;
                    }
                }
            }
            ffi::SQLITE_DONE => break,
            code => return Err(sqlite_error(database, code)),
        }
    }
    Ok(RawScriptStep {
        tail_offset: prepared.tail_offset,
        statement: Some(RawStatementExecution {
            columns,
            rows,
            returned_rows,
            truncated,
            read_only,
            affected_rows: unsafe { ffi::sqlite3_changes64(database) }.max(0) as u64,
        }),
    })
}

pub(super) unsafe fn prepare_statement_raw(
    connection: &Connection,
    sql_tail: &str,
) -> CoreResult<RawPreparedStatement> {
    let database = unsafe { connection.handle() };
    let sql_tail = CString::new(sql_tail)
        .map_err(|error| CoreError::InvalidArgument(format!("invalid sql: {error}")))?;
    let mut raw_statement = ptr::null_mut();
    let mut next_sql = ptr::null();
    let code = unsafe {
        ffi::sqlite3_prepare_v2(
            database,
            sql_tail.as_ptr(),
            -1,
            &mut raw_statement,
            &mut next_sql,
        )
    };
    if code != ffi::SQLITE_OK {
        return Err(sqlite_error(database, code));
    }
    let tail_offset = if next_sql.is_null() {
        sql_tail.as_bytes().len()
    } else {
        unsafe { next_sql.offset_from(sql_tail.as_ptr()) as usize }
    };
    if tail_offset == 0 {
        return Err(CoreError::InvalidArgument(
            "sql parser did not advance".into(),
        ));
    }
    if raw_statement.is_null() {
        return Ok(RawPreparedStatement {
            tail_offset,
            statement: None,
        });
    }
    Ok(RawPreparedStatement {
        tail_offset,
        statement: Some(RawStatement(raw_statement)),
    })
}

pub(super) unsafe fn statement_columns(
    statement: *mut ffi::sqlite3_stmt,
    column_count: usize,
) -> Vec<SqlColumn> {
    (0..column_count)
        .map(|index| {
            let index = index as i32;
            SqlColumn {
                name: unsafe { sqlite_text(ffi::sqlite3_column_name(statement, index)) }
                    .unwrap_or_default(),
                declared_type: unsafe {
                    sqlite_text(ffi::sqlite3_column_decltype(statement, index))
                },
            }
        })
        .collect()
}

pub(super) unsafe fn statement_row(
    statement: *mut ffi::sqlite3_stmt,
    column_count: usize,
) -> Vec<SqlValue> {
    (0..column_count)
        .map(|index| unsafe { statement_value(statement, index as i32) })
        .collect()
}

unsafe fn statement_value(statement: *mut ffi::sqlite3_stmt, index: i32) -> SqlValue {
    match unsafe { ffi::sqlite3_column_type(statement, index) } {
        ffi::SQLITE_NULL => SqlValue::Null,
        ffi::SQLITE_INTEGER => {
            SqlValue::Integer(unsafe { ffi::sqlite3_column_int64(statement, index) })
        }
        ffi::SQLITE_FLOAT => {
            SqlValue::Real(unsafe { ffi::sqlite3_column_double(statement, index) })
        }
        ffi::SQLITE_TEXT => {
            let pointer = unsafe { ffi::sqlite3_column_text(statement, index) };
            let length = unsafe { ffi::sqlite3_column_bytes(statement, index) }.max(0) as usize;
            let bytes = if pointer.is_null() {
                &[]
            } else {
                unsafe { std::slice::from_raw_parts(pointer, length) }
            };
            SqlValue::Text(String::from_utf8_lossy(bytes).into_owned())
        }
        ffi::SQLITE_BLOB => {
            let pointer = unsafe { ffi::sqlite3_column_blob(statement, index) };
            let length = unsafe { ffi::sqlite3_column_bytes(statement, index) }.max(0) as usize;
            let bytes = if pointer.is_null() {
                &[]
            } else {
                unsafe { std::slice::from_raw_parts(pointer.cast::<u8>(), length) }
            };
            SqlValue::Blob(base64::engine::general_purpose::STANDARD.encode(bytes))
        }
        _ => SqlValue::Null,
    }
}

unsafe fn sqlite_text(pointer: *const std::ffi::c_char) -> Option<String> {
    (!pointer.is_null()).then(|| {
        unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned()
    })
}

pub(super) struct RawStatement(pub(super) *mut ffi::sqlite3_stmt);

impl Drop for RawStatement {
    fn drop(&mut self) {
        unsafe {
            ffi::sqlite3_finalize(self.0);
        }
    }
}

pub(super) fn sqlite_error(database: *mut ffi::sqlite3, code: i32) -> CoreError {
    let message = unsafe { CStr::from_ptr(ffi::sqlite3_errmsg(database)) }
        .to_string_lossy()
        .into_owned();
    CoreError::Database(rusqlite::Error::SqliteFailure(
        ffi::Error::new(code),
        Some(message),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statement_deadline_interrupts_sqlite_execution_with_context() {
        let connection = Connection::open_in_memory().unwrap();
        let cancellation = CancellationToken::new();
        let result = unsafe {
            execute_statement_to_completion_guarded(
                &connection,
                "WITH RECURSIVE loop(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM loop) SELECT * FROM loop;",
                &SqlStatementExecutionContext {
                    cancellation: &cancellation,
                    limits: SqlStatementExecutionLimits {
                        timeout: Duration::from_millis(10),
                    },
                    statement_index: 2,
                    statement_total: 3,
                    workflow: "SQL Import",
                },
            )
        };
        let message = match result {
            Err(error) => error.to_string(),
            Ok(_) => panic!("statement should time out"),
        };
        assert!(message.contains("SQL Import statement 2 of 3"));
        assert!(message.contains("10 ms execution limit"));
    }

    #[test]
    fn cancellation_is_not_reported_as_timeout() {
        let connection = Connection::open_in_memory().unwrap();
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let result = unsafe {
            execute_statement_to_completion_guarded(
                &connection,
                "SELECT 1",
                &SqlStatementExecutionContext {
                    cancellation: &cancellation,
                    limits: SqlStatementExecutionLimits {
                        timeout: Duration::from_secs(1),
                    },
                    statement_index: 1,
                    statement_total: 1,
                    workflow: "Custom SQL",
                },
            )
        };

        assert!(matches!(result, Err(CoreError::Cancelled)));
    }

    #[test]
    fn timed_out_statement_rolls_back_earlier_transaction_mutations() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE values_table(value INTEGER); INSERT INTO values_table VALUES (1);",
            )
            .unwrap();
        let cancellation = CancellationToken::new();
        {
            let transaction = connection.transaction().unwrap();
            transaction
                .execute("UPDATE values_table SET value = 2", [])
                .unwrap();
            let result = unsafe {
                execute_statement_to_completion_guarded(
                    &transaction,
                    "WITH RECURSIVE loop(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM loop) SELECT * FROM loop;",
                    &SqlStatementExecutionContext {
                        cancellation: &cancellation,
                        limits: SqlStatementExecutionLimits {
                            timeout: Duration::from_millis(10),
                        },
                        statement_index: 2,
                        statement_total: 2,
                        workflow: "Custom SQL",
                    },
                )
            };
            assert!(result.is_err());
        }
        assert_eq!(
            connection
                .query_row("SELECT value FROM values_table", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}
