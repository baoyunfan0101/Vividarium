use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use super::sql_sources::{SqlDataSource, inspect_sql_data_source};
use super::sql_types::SqlSourceSchema;
use super::sync;
use super::types::TaxonomyNameType;
use super::validation::validate_taxonomy;
use crate::db::LOCAL_TAXON_ID_FLOOR;
use crate::naming::normalize_taxonomy_name;
use crate::{CancellationToken, CoreError, CoreResult, Database, OperationProgress};

const VALIDATING_DIRECT_IMPORT_DATABASE: &str = "validating_direct_import_database";
const APPLYING_DIRECT_IMPORT: &str = "applying_direct_import";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonomyImportMetadata {
    pub source_path: String,
    pub taxa_count: i64,
    pub taxon_names_count: i64,
    pub imported_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonomyImportResult {
    pub metadata: TaxonomyImportMetadata,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectImportDatabase {
    pub source_path: String,
    pub schema: SqlSourceSchema,
}

pub fn get_taxonomy_import_metadata(
    database: &Database,
) -> CoreResult<Option<TaxonomyImportMetadata>> {
    let connection = database.connect_taxonomy_metadata_context()?;
    connection
        .query_row(
            r#"
            SELECT source_path, taxa_count, taxon_names_count, imported_at
            FROM taxonomy_base_metadata
            WHERE metadata_id = 1
            "#,
            [],
            taxonomy_import_metadata_row,
        )
        .optional()
        .map_err(Into::into)
}

pub fn inspect_direct_import_database(
    database: &Database,
    source_path: &Path,
) -> CoreResult<DirectImportDatabase> {
    let source_path = validated_direct_import_path(database, source_path)?;
    let source_path_string = source_path_string(&source_path)?;
    let schema = inspect_sql_data_source(
        &SqlDataSource::Sqlite {
            alias: "direct_import".into(),
            path: source_path,
        },
        b',',
    )?;
    Ok(DirectImportDatabase {
        source_path: source_path_string,
        schema,
    })
}

pub fn apply_direct_import(
    database: &Database,
    source_path: &Path,
) -> CoreResult<TaxonomyImportResult> {
    apply_direct_import_with_cancellation(database, source_path, &CancellationToken::new())
}

pub fn apply_direct_import_with_cancellation(
    database: &Database,
    source_path: &Path,
    cancellation: &CancellationToken,
) -> CoreResult<TaxonomyImportResult> {
    apply_direct_import_with_progress_and_cancellation(
        database,
        source_path,
        &mut |_| {},
        cancellation,
    )
}

pub fn apply_direct_import_with_progress_and_cancellation(
    database: &Database,
    source_path: &Path,
    progress: &mut (dyn FnMut(OperationProgress) + Send),
    cancellation: &CancellationToken,
) -> CoreResult<TaxonomyImportResult> {
    cancellation.check()?;
    let _guard = database.try_taxonomy_replacement()?;
    progress(OperationProgress {
        stage: VALIDATING_DIRECT_IMPORT_DATABASE.into(),
        current: None,
        total: None,
        unit: None,
    });
    let source_path =
        validated_direct_import_path_with_cancellation(database, source_path, cancellation)?;
    let source_path = source_path_string(&source_path)?;
    let mut connection = database.connect_taxonomy_metadata_context()?;
    cancellation.install_sqlite_progress_handler(&connection);
    connection.execute("ATTACH DATABASE ? AS direct_import_source", [&source_path])?;
    progress(OperationProgress {
        stage: APPLYING_DIRECT_IMPORT.into(),
        current: None,
        total: None,
        unit: None,
    });
    let result = replace_from_attached_database(&mut connection, &source_path, cancellation);
    let detach_result = connection.execute_batch("DETACH DATABASE direct_import_source");
    cancellation.normalize(match (result, detach_result) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error.into()),
    })
}

fn validated_direct_import_path(database: &Database, source_path: &Path) -> CoreResult<PathBuf> {
    validated_direct_import_path_with_cancellation(database, source_path, &CancellationToken::new())
}

fn validated_direct_import_path_with_cancellation(
    database: &Database,
    source_path: &Path,
    cancellation: &CancellationToken,
) -> CoreResult<PathBuf> {
    cancellation.check()?;
    let source_path = fs::canonicalize(source_path)?;
    let target_path = fs::canonicalize(database.taxonomy_path()?)?;
    if source_path == target_path {
        return Err(CoreError::InvalidArgument(
            "direct import database must differ from the application database".into(),
        ));
    }
    validate_direct_import_database(&source_path, cancellation)?;
    Ok(source_path)
}

fn source_path_string(source_path: &Path) -> CoreResult<String> {
    source_path
        .to_str()
        .map(str::to_string)
        .ok_or_else(|| CoreError::InvalidArgument("direct import path is not valid UTF-8".into()))
}

fn replace_from_attached_database(
    connection: &mut Connection,
    source_path: &str,
    cancellation: &CancellationToken,
) -> CoreResult<TaxonomyImportResult> {
    let transaction = connection.transaction()?;
    transaction.execute_batch(
        r#"
        DELETE FROM operations;
        DELETE FROM taxonomy_base_metadata;
        DELETE FROM taxonomy_sync_events;
        UPDATE taxa SET parent_taxon_id = NULL;
        DELETE FROM taxon_names;
        DELETE FROM taxa;
        DELETE FROM sqlite_sequence
        WHERE name IN ('operations', 'taxon_names', 'taxa');

        INSERT INTO taxa (taxon_id, parent_taxon_id, rank, geological_range)
        SELECT taxon_id, parent_taxon_id, rank, geological_range
        FROM direct_import_source.taxa
        ORDER BY rank, taxon_id;
        "#,
    )?;
    import_normalized_names(&transaction, cancellation)?;
    validate_taxonomy(&transaction)?;
    set_local_taxon_id_floor(&transaction)?;
    let taxa_count =
        transaction.query_row("SELECT COUNT(*) FROM taxa", [], |row| row.get::<_, i64>(0))?;
    let taxon_names_count =
        transaction.query_row("SELECT COUNT(*) FROM taxon_names", [], |row| {
            row.get::<_, i64>(0)
        })?;
    transaction.execute(
        r#"
        INSERT INTO taxonomy_base_metadata (
            metadata_id, source_path, taxa_count, taxon_names_count
        ) VALUES (1, ?, ?, ?)
        "#,
        params![source_path, taxa_count, taxon_names_count],
    )?;
    transaction.execute(
        "UPDATE taxonomy_identity SET taxonomy_identity = ? WHERE identity_id = 1",
        [uuid::Uuid::new_v4().to_string()],
    )?;
    sync::record_event(&transaction, None, [], true)?;
    let metadata = transaction.query_row(
        r#"
        SELECT source_path, taxa_count, taxon_names_count, imported_at
        FROM taxonomy_base_metadata
        WHERE metadata_id = 1
        "#,
        [],
        taxonomy_import_metadata_row,
    )?;
    cancellation.check()?;
    transaction.commit()?;
    Ok(TaxonomyImportResult {
        metadata,
        warnings: Vec::new(),
    })
}

fn import_normalized_names(
    transaction: &Transaction<'_>,
    cancellation: &CancellationToken,
) -> CoreResult<()> {
    let names = {
        let mut statement = transaction.prepare(
            r#"
            SELECT name_id, taxon_id, name_type, name, authority_year, source
            FROM direct_import_source.taxon_names
            ORDER BY name_id
            "#,
        )?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    let mut insert = transaction.prepare_cached(
        r#"
        INSERT INTO taxon_names (
            name_id, taxon_id, name_type, name, authority_year, source
        ) VALUES (?, ?, ?, ?, ?, ?)
        "#,
    )?;
    let mut seen_family_names = HashSet::new();
    for (name_id, taxon_id, name_type, raw_name, authority_year, source) in names {
        cancellation.check()?;
        let name = normalize_taxonomy_name(&raw_name).ok_or_else(|| {
            CoreError::InvalidArgument(format!(
                "direct import name {name_id} is empty after normalization"
            ))
        })?;
        let name_family = TaxonomyNameType::from_code(name_type)?
            .accepted_type()
            .code();
        if !seen_family_names.insert((taxon_id, name_family, name.clone())) {
            return Err(CoreError::InvalidArgument(format!(
                "direct import taxon {taxon_id} contains duplicate name '{name}' in one name family"
            )));
        }
        insert.execute(params![
            name_id,
            taxon_id,
            name_type,
            name,
            authority_year,
            source
        ])?;
    }
    Ok(())
}

fn set_local_taxon_id_floor(transaction: &Transaction<'_>) -> CoreResult<()> {
    transaction.execute(
        r#"
        INSERT INTO sqlite_sequence(name, seq)
        SELECT 'taxa', ?
        WHERE NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name = 'taxa')
        "#,
        [LOCAL_TAXON_ID_FLOOR],
    )?;
    transaction.execute(
        r#"
        UPDATE sqlite_sequence
        SET seq = max(
            seq,
            ?,
            COALESCE((SELECT MAX(taxon_id) FROM taxa), 0)
        )
        WHERE name = 'taxa'
        "#,
        [LOCAL_TAXON_ID_FLOOR],
    )?;
    Ok(())
}

fn validate_direct_import_database(
    path: &Path,
    cancellation: &CancellationToken,
) -> CoreResult<()> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    cancellation.install_sqlite_progress_handler(&connection);
    require_table_columns(
        &connection,
        "taxa",
        &["taxon_id", "parent_taxon_id", "rank", "geological_range"],
        false,
    )?;
    require_table_columns(
        &connection,
        "taxon_names",
        &[
            "name_id",
            "taxon_id",
            "name_type",
            "name",
            "normalized_name",
            "authority_year",
            "source",
        ],
        true,
    )?;
    require_column_declared_type(&connection, "taxon_names", "name_type", "INTEGER")?;
    Ok(())
}

fn require_table_columns(
    connection: &Connection,
    table: &str,
    expected: &[&str],
    include_hidden: bool,
) -> CoreResult<()> {
    let object_type = connection
        .query_row(
            "SELECT type FROM sqlite_master WHERE name = ?",
            [table],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if object_type.as_deref() != Some("table") {
        return Err(CoreError::InvalidArgument(format!(
            "direct import database is missing table {table}"
        )));
    }
    let pragma = if include_hidden {
        format!("PRAGMA table_xinfo({table})")
    } else {
        format!("PRAGMA table_info({table})")
    };
    let mut statement = connection.prepare(&pragma)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if columns != expected {
        return Err(CoreError::InvalidArgument(format!(
            "direct import table {table} has invalid columns"
        )));
    }
    Ok(())
}

fn require_column_declared_type(
    connection: &Connection,
    table: &str,
    column: &str,
    expected: &str,
) -> CoreResult<()> {
    let mut statement = connection.prepare(&format!("PRAGMA table_xinfo({table})"))?;
    let columns = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let declared_type = columns
        .iter()
        .find_map(|(name, declared_type)| (name == column).then_some(declared_type.as_str()));
    if !declared_type.is_some_and(|declared_type| declared_type.eq_ignore_ascii_case(expected)) {
        return Err(CoreError::InvalidArgument(format!(
            "direct import table {table} column {column} must use {expected}"
        )));
    }
    Ok(())
}

fn taxonomy_import_metadata_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<TaxonomyImportMetadata> {
    Ok(TaxonomyImportMetadata {
        source_path: row.get(0)?,
        taxa_count: row.get(1)?,
        taxon_names_count: row.get(2)?,
        imported_at: row.get(3)?,
    })
}

#[cfg(test)]
#[path = "direct_import/tests.rs"]
mod tests;
