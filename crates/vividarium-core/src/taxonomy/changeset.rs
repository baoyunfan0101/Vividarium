use std::collections::BTreeSet;
use std::ffi::{CStr, c_int, c_void};
use std::io::Cursor;
use std::ptr;

use rusqlite::fallible_streaming_iterator::FallibleStreamingIterator;
use rusqlite::hooks::Action;
use rusqlite::session::{ChangesetItem, ChangesetIter, Session};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OptionalExtension, params_from_iter};

use crate::{CoreError, CoreResult};

const TAXONOMY_SESSION_TABLES: [&str; 2] = ["taxa", "taxon_names"];

#[derive(Debug)]
enum RollbackConflict {
    Row {
        table: String,
        action: String,
        conflict_type: String,
    },
    ForeignKey {
        conflict_count: Option<c_int>,
    },
}

#[derive(Default)]
struct RollbackApplyContext {
    first_conflict: Option<RollbackConflict>,
}

pub(super) fn is_taxonomy_session_table(table_name: &str) -> bool {
    TAXONOMY_SESSION_TABLES.contains(&table_name)
}

pub(super) fn start_taxonomy_session(connection: &Connection) -> CoreResult<Session<'_>> {
    let mut session = Session::new(connection)?;
    for table in TAXONOMY_SESSION_TABLES {
        session.attach(Some(table))?;
    }
    Ok(session)
}

pub(super) fn affected_taxon_ids_from_changeset(
    connection: &Connection,
    changeset_blob: &[u8],
) -> CoreResult<BTreeSet<i64>> {
    let input = &mut Cursor::new(changeset_blob) as &mut dyn std::io::Read;
    let mut changes = ChangesetIter::start_strm(&input)?;
    let mut taxon_ids = BTreeSet::new();
    let mut taxon_name_ids = BTreeSet::new();
    while let Some(item) = changes.next()? {
        let operation = item.op()?;
        match operation.table_name() {
            "taxa" => {
                collect_changeset_integers(item, operation.code(), 0, &mut taxon_ids)?;
                collect_changeset_integers(item, operation.code(), 1, &mut taxon_ids)?;
            }
            "taxon_names" => {
                if !collect_changeset_integers(item, operation.code(), 1, &mut taxon_ids)? {
                    collect_changeset_integers(item, operation.code(), 0, &mut taxon_name_ids)?;
                }
            }
            table => {
                return Err(CoreError::Consistency(format!(
                    "unexpected taxonomy changeset table: {table}"
                )));
            }
        }
    }
    drop(changes);
    for chunk in taxon_name_ids.into_iter().collect::<Vec<_>>().chunks(500) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let mut statement = connection.prepare(&format!(
            "SELECT DISTINCT taxon_id FROM taxon_names WHERE name_id IN ({placeholders})"
        ))?;
        for taxon_id in
            statement.query_map(params_from_iter(chunk.iter()), |row| row.get::<_, i64>(0))?
        {
            taxon_ids.insert(taxon_id?);
        }
    }
    Ok(taxon_ids)
}

pub(super) fn apply_inverse_taxonomy_changeset(
    connection: &Connection,
    operation_id: i64,
    changeset_blob: &[u8],
) -> CoreResult<()> {
    if changeset_blob.is_empty() {
        return Ok(());
    }
    let changeset_size = c_int::try_from(changeset_blob.len())
        .map_err(|_| CoreError::Consistency("taxonomy rollback changeset is too large".into()))?;
    let mut context = RollbackApplyContext::default();
    let flags = rusqlite::ffi::SQLITE_CHANGESETAPPLY_INVERT
        | rusqlite::ffi::SQLITE_CHANGESETAPPLY_FKNOACTION;
    let result = unsafe {
        rusqlite::ffi::sqlite3changeset_apply_v2(
            connection.handle(),
            changeset_size,
            changeset_blob.as_ptr() as *mut c_void,
            None,
            Some(capture_rollback_conflict),
            &mut context as *mut RollbackApplyContext as *mut c_void,
            ptr::null_mut(),
            ptr::null_mut(),
            flags,
        )
    };
    if result == rusqlite::ffi::SQLITE_OK {
        return Ok(());
    }
    if let Some(conflict) = context.first_conflict {
        return Err(CoreError::Consistency(match conflict {
            RollbackConflict::Row {
                table,
                action,
                conflict_type,
            } => format!(
                "Rollback conflict in {table} while restoring operation {operation_id}: {conflict_type} conflict during {action}; the current data no longer matches the state created by this operation."
            ),
            RollbackConflict::ForeignKey { conflict_count } => {
                let count = conflict_count.map_or_else(
                    || "an unknown number of unresolved relationships".into(),
                    |count| {
                        format!(
                            "{count} unresolved foreign-key relationship{}",
                            if count == 1 { "" } else { "s" }
                        )
                    },
                );
                format!(
                    "Rollback conflict while restoring operation {operation_id}: foreign-key conflict with {count}; the current taxonomy no longer matches the state created by this operation."
                )
            }
        }));
    }
    Err(CoreError::Consistency(format!(
        "Rollback failed while restoring operation {operation_id}: SQLite changeset apply returned code {result}."
    )))
}

pub(super) fn validate_foreign_key_integrity(connection: &Connection) -> CoreResult<()> {
    let violation = connection
        .query_row("PRAGMA foreign_key_check", [], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .optional()?;
    if let Some((table, row_id, parent)) = violation {
        return Err(CoreError::Consistency(format!(
            "taxonomy rollback left a foreign-key violation in {table} row {} referencing {parent}",
            row_id.map_or_else(|| "unknown".into(), |value| value.to_string())
        )));
    }
    Ok(())
}

unsafe extern "C" fn capture_rollback_conflict(
    context: *mut c_void,
    conflict_type: c_int,
    item: *mut rusqlite::ffi::sqlite3_changeset_iter,
) -> c_int {
    let context = unsafe { &mut *(context as *mut RollbackApplyContext) };
    if context.first_conflict.is_none() {
        if conflict_type == rusqlite::ffi::SQLITE_CHANGESET_FOREIGN_KEY {
            let mut conflict_count = 0;
            let result =
                unsafe { rusqlite::ffi::sqlite3changeset_fk_conflicts(item, &mut conflict_count) };
            context.first_conflict = Some(RollbackConflict::ForeignKey {
                conflict_count: (result == rusqlite::ffi::SQLITE_OK).then_some(conflict_count),
            });
            return rusqlite::ffi::SQLITE_CHANGESET_ABORT;
        }
        let mut table = ptr::null();
        let mut column_count = 0;
        let mut action = 0;
        let mut indirect = 0;
        let result = unsafe {
            rusqlite::ffi::sqlite3changeset_op(
                item,
                &mut table,
                &mut column_count,
                &mut action,
                &mut indirect,
            )
        };
        let table = if result == rusqlite::ffi::SQLITE_OK && !table.is_null() {
            unsafe { CStr::from_ptr(table) }
                .to_string_lossy()
                .into_owned()
        } else {
            "taxonomy".into()
        };
        context.first_conflict = Some(RollbackConflict::Row {
            table,
            action: changeset_action_label(action).into(),
            conflict_type: changeset_conflict_label(conflict_type).into(),
        });
    }
    rusqlite::ffi::SQLITE_CHANGESET_ABORT
}

fn changeset_action_label(action: c_int) -> &'static str {
    match action {
        rusqlite::ffi::SQLITE_INSERT => "insert",
        rusqlite::ffi::SQLITE_UPDATE => "update",
        rusqlite::ffi::SQLITE_DELETE => "delete",
        _ => "unknown action",
    }
}

fn changeset_conflict_label(conflict_type: c_int) -> &'static str {
    match conflict_type {
        rusqlite::ffi::SQLITE_CHANGESET_DATA => "data",
        rusqlite::ffi::SQLITE_CHANGESET_NOTFOUND => "not-found",
        rusqlite::ffi::SQLITE_CHANGESET_CONFLICT => "row",
        rusqlite::ffi::SQLITE_CHANGESET_CONSTRAINT => "constraint",
        rusqlite::ffi::SQLITE_CHANGESET_FOREIGN_KEY => "foreign-key",
        _ => "unknown",
    }
}

fn collect_changeset_integers(
    item: &ChangesetItem,
    action: Action,
    column: usize,
    values: &mut BTreeSet<i64>,
) -> CoreResult<bool> {
    let mut found = false;
    match action {
        Action::SQLITE_INSERT => {
            found |= collect_changeset_integer(item.new_value(column), values)?;
        }
        Action::SQLITE_DELETE => {
            found |= collect_changeset_integer(item.old_value(column), values)?;
        }
        Action::SQLITE_UPDATE => {
            found |= collect_changeset_integer(item.old_value(column), values)?;
            found |= collect_changeset_integer(item.new_value(column), values)?;
        }
        _ => {
            return Err(CoreError::Consistency(format!(
                "unexpected taxonomy changeset action: {action:?}"
            )));
        }
    }
    Ok(found)
}

fn collect_changeset_integer(
    value: rusqlite::Result<ValueRef<'_>>,
    values: &mut BTreeSet<i64>,
) -> CoreResult<bool> {
    match value {
        Ok(ValueRef::Integer(value)) => {
            values.insert(value);
            Ok(true)
        }
        Ok(ValueRef::Null) => Ok(false),
        Err(rusqlite::Error::InvalidColumnIndex(_)) => Ok(false),
        Err(error) => Err(error.into()),
        Ok(_) => Err(CoreError::Consistency(
            "taxonomy changeset identifier is not an integer".into(),
        )),
    }
}
