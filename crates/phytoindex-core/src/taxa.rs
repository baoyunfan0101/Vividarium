use std::path::PathBuf;

use rusqlite::OptionalExtension;

use crate::db::{Database, taxon_from_row};
use crate::error::{CoreError, CoreResult};
use crate::models::{TaxaMetadata, TaxaSyncResult, Taxon};
use crate::photos::ProgressCallback;

pub fn get_metadata(database: &Database) -> CoreResult<TaxaMetadata> {
    let connection = database.connect()?;
    let metadata = connection
        .query_row(
            r#"
            SELECT knowledge_base_path, knowledge_base_size,
                   knowledge_base_modified_at, last_synced_at
            FROM taxa_metadata LIMIT 1
            "#,
            [],
            |row| {
                Ok(TaxaMetadata {
                    knowledge_base_path: row.get(0)?,
                    knowledge_base_size: row.get(1)?,
                    knowledge_base_modified_at: row.get(2)?,
                    last_synced_at: row.get(3)?,
                    taxa_count: 0,
                })
            },
        )
        .optional()?
        .unwrap_or(TaxaMetadata {
            knowledge_base_path: None,
            knowledge_base_size: None,
            knowledge_base_modified_at: None,
            last_synced_at: None,
            taxa_count: 0,
        });
    let taxa_count = connection.query_row("SELECT COUNT(*) FROM taxa", [], |row| row.get(0))?;
    Ok(TaxaMetadata {
        taxa_count,
        ..metadata
    })
}

pub fn save_knowledge_base_path(
    database: &Database,
    knowledge_base_path: Option<&str>,
) -> CoreResult<TaxaMetadata> {
    let normalized = knowledge_base_path
        .map(expand_home)
        .map(|path| path.to_string_lossy().into_owned());
    let connection = database.connect()?;
    connection.execute("DELETE FROM taxa_metadata", [])?;
    connection.execute(
        "INSERT INTO taxa_metadata (knowledge_base_path) VALUES (?)",
        [normalized],
    )?;
    get_metadata(database)
}

pub fn get_taxon(database: &Database, taxon_id: i64) -> CoreResult<Option<Taxon>> {
    let connection = database.connect()?;
    Ok(connection
        .query_row(
            "SELECT * FROM taxa_display WHERE taxon_id = ?",
            [taxon_id],
            taxon_from_row,
        )
        .optional()?)
}

pub fn get_taxon_by_binomial(database: &Database, name: &str) -> CoreResult<Option<Taxon>> {
    let connection = database.connect()?;
    Ok(connection
        .query_row(
            "SELECT * FROM taxa_display WHERE binomial_name = ? ORDER BY taxon_id",
            [name],
            taxon_from_row,
        )
        .optional()?)
}

pub fn lineage(database: &Database, taxon_id: i64) -> CoreResult<Vec<Taxon>> {
    let mut result = Vec::new();
    let mut current_id = Some(taxon_id);
    while let Some(id) = current_id {
        let Some(taxon) = get_taxon(database, id)? else {
            break;
        };
        current_id = taxon.parent_id;
        result.push(taxon);
    }
    result.reverse();
    Ok(result)
}

pub fn update_taxa(
    _database: &Database,
    _path: Option<&str>,
    _progress: &mut ProgressCallback<'_>,
) -> CoreResult<TaxaSyncResult> {
    Err(CoreError::InvalidArgument(
        "legacy workbook import has been replaced by row-based taxonomy updates".into(),
    ))
}

pub fn rebuild_taxa(
    _database: &Database,
    _path: Option<&str>,
    _progress: &mut ProgressCallback<'_>,
) -> CoreResult<TaxaSyncResult> {
    Err(CoreError::InvalidArgument(
        "legacy workbook rebuild has been removed from the new database".into(),
    ))
}

fn expand_home(value: &str) -> PathBuf {
    if value == "~" {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(value));
    }
    if let Some(rest) = value.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(value)
}
