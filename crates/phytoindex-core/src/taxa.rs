use std::fs;
use std::path::{Path, PathBuf};

use calamine::{Data, Reader, open_workbook_auto};
use chrono::{DateTime, Local};
use rusqlite::{OptionalExtension, Transaction, params};

use crate::db::{Database, taxon_from_row};
use crate::error::{CoreError, CoreResult};
use crate::models::{TaxaMetadata, TaxaSyncResult, Taxon};
use crate::photos::ProgressCallback;

const PLANTS_SHEET_NAME: &str = "plants";
const RANKS: [(&str, &str, &str); 4] = [
    ("ordo", "\u{76ee}", "Ordo"),
    ("familia", "\u{79d1}", "Familia"),
    ("genus", "\u{5c5e}", "Genus"),
    ("species", "\u{79cd}", "Species"),
];

#[derive(Debug, Clone, Default)]
struct TaxaRow {
    ordo: Option<String>,
    familia: Option<String>,
    genus: Option<String>,
    species: Option<String>,
    binomial_name: Option<String>,
}

#[derive(Debug)]
struct TaxonRecord<'a> {
    rank: &'a str,
    name: &'a str,
    parent_id: Option<i64>,
    binomial_name: Option<&'a str>,
}

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
    let current = get_metadata(database)?;
    let normalized = knowledge_base_path
        .map(expand_home)
        .map(|path| path.to_string_lossy().into_owned());
    let same_path = normalized == current.knowledge_base_path;
    let mut connection = database.connect()?;
    let transaction = connection.transaction()?;
    transaction.execute("DELETE FROM taxa_metadata", [])?;
    transaction.execute(
        r#"
        INSERT INTO taxa_metadata (
            knowledge_base_path, knowledge_base_size,
            knowledge_base_modified_at, last_synced_at
        ) VALUES (?, ?, ?, ?)
        "#,
        params![
            normalized,
            same_path.then_some(current.knowledge_base_size).flatten(),
            same_path
                .then_some(current.knowledge_base_modified_at)
                .flatten(),
            current.last_synced_at
        ],
    )?;
    transaction.commit()?;
    get_metadata(database)
}

pub fn get_taxon(database: &Database, taxon_id: i64) -> CoreResult<Option<Taxon>> {
    let connection = database.connect()?;
    Ok(connection
        .query_row(
            "SELECT * FROM taxa WHERE taxon_id = ?",
            [taxon_id],
            taxon_from_row,
        )
        .optional()?)
}

pub fn get_taxon_by_binomial(database: &Database, name: &str) -> CoreResult<Option<Taxon>> {
    let connection = database.connect()?;
    Ok(connection
        .query_row(
            "SELECT * FROM taxa WHERE binomial_name = ? ORDER BY taxon_id",
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
    database: &Database,
    path: Option<&str>,
    progress: &mut ProgressCallback<'_>,
) -> CoreResult<TaxaSyncResult> {
    import_taxa(database, path, true, progress)
}

pub fn rebuild_taxa(
    database: &Database,
    path: Option<&str>,
    progress: &mut ProgressCallback<'_>,
) -> CoreResult<TaxaSyncResult> {
    import_taxa(database, path, false, progress)
}

fn import_taxa(
    database: &Database,
    path: Option<&str>,
    preserve_binomial_ids: bool,
    progress: &mut ProgressCallback<'_>,
) -> CoreResult<TaxaSyncResult> {
    let workbook_path = resolve_workbook_path(database, path)?;
    let rows = read_taxa_rows(&workbook_path)?;
    let operation = if preserve_binomial_ids {
        "Updating taxa"
    } else {
        "Rebuilding taxa"
    };
    progress(0, Some(rows.len() as u64), operation);
    let mut connection = database.connect()?;
    let transaction = connection.transaction()?;
    if !preserve_binomial_ids {
        transaction.execute("DELETE FROM taxa", [])?;
        transaction.execute("DELETE FROM sqlite_sequence WHERE name = 'taxa'", [])?;
    }
    let changed = import_rows(&transaction, &rows, preserve_binomial_ids, progress)?;
    let file_metadata = fs::metadata(&workbook_path)?;
    let modified: DateTime<Local> = file_metadata.modified()?.into();
    let modified_at = modified.format("%Y-%m-%d %H:%M:%S").to_string();
    let last_synced_at = Local::now().format("%Y-%m-%d %H:%M:%S%.6f").to_string();
    transaction.execute("DELETE FROM taxa_metadata", [])?;
    transaction.execute(
        r#"
        INSERT INTO taxa_metadata (
            knowledge_base_path, knowledge_base_size,
            knowledge_base_modified_at, last_synced_at
        ) VALUES (?, ?, ?, ?)
        "#,
        params![
            workbook_path.to_string_lossy(),
            file_metadata.len() as i64,
            modified_at,
            last_synced_at
        ],
    )?;
    let total_taxa: i64 =
        transaction.query_row("SELECT COUNT(*) FROM taxa", [], |row| row.get(0))?;
    transaction.commit()?;
    Ok(TaxaSyncResult {
        knowledge_base_path: workbook_path.to_string_lossy().into_owned(),
        knowledge_base_size: file_metadata.len() as i64,
        knowledge_base_modified_at: modified_at,
        sheet: PLANTS_SHEET_NAME.into(),
        rows_read: rows.len(),
        taxa_changed: changed,
        total_taxa: total_taxa as usize,
    })
}

fn resolve_workbook_path(database: &Database, value: Option<&str>) -> CoreResult<PathBuf> {
    let path = match value {
        Some(path) if !path.trim().is_empty() => expand_home(path),
        _ => get_metadata(database)?
            .knowledge_base_path
            .map(|path| expand_home(&path))
            .ok_or_else(|| CoreError::InvalidArgument("knowledge_base_path is required".into()))?,
    };
    if !path.is_file() {
        return Err(CoreError::NotFound(path.to_string_lossy().into_owned()));
    }
    Ok(path)
}

fn read_taxa_rows(path: &Path) -> CoreResult<Vec<TaxaRow>> {
    let mut workbook =
        open_workbook_auto(path).map_err(|error| CoreError::Workbook(error.to_string()))?;
    let range = workbook
        .worksheet_range(PLANTS_SHEET_NAME)
        .map_err(|error| CoreError::Workbook(error.to_string()))?;
    let mut rows = range.rows();
    let headers = rows
        .next()
        .ok_or_else(|| CoreError::Workbook("plants worksheet is empty".into()))?;
    let columns = detect_columns(headers)?;
    let mut result = Vec::new();
    for row in rows {
        let parsed = TaxaRow {
            ordo: cell(row, columns[0]),
            familia: cell(row, columns[1]),
            genus: cell(row, columns[2]),
            species: cell(row, columns[3]),
            binomial_name: cell(row, columns[4]),
        };
        if parsed.ordo.is_some()
            || parsed.familia.is_some()
            || parsed.genus.is_some()
            || parsed.species.is_some()
        {
            result.push(parsed);
        }
    }
    Ok(result)
}

fn detect_columns(headers: &[Data]) -> CoreResult<[usize; 5]> {
    let headers = headers.iter().map(cell_text).collect::<Vec<_>>();
    let mut result = [0; 5];
    for (index, (_, chinese, latin)) in RANKS.iter().enumerate() {
        result[index] = find_header(&headers, chinese, latin)?;
    }
    result[4] = find_header(&headers, "\u{5b66}\u{540d}", "Binomial name")?;
    Ok(result)
}

fn find_header(headers: &[String], chinese: &str, latin: &str) -> CoreResult<usize> {
    headers
        .iter()
        .position(|header| {
            header.starts_with(chinese)
                && header
                    .to_ascii_lowercase()
                    .contains(&latin.to_ascii_lowercase())
        })
        .ok_or_else(|| CoreError::Workbook(format!("missing required taxa column: {latin}")))
}

fn cell(row: &[Data], index: usize) -> Option<String> {
    row.get(index)
        .map(cell_text)
        .filter(|value| !value.is_empty())
}

fn cell_text(value: &Data) -> String {
    match value {
        Data::Empty => String::new(),
        Data::String(value) => value.trim().to_string(),
        value => value.to_string().trim().to_string(),
    }
}

fn import_rows(
    transaction: &Transaction<'_>,
    rows: &[TaxaRow],
    preserve_binomial_ids: bool,
    progress: &mut ProgressCallback<'_>,
) -> CoreResult<usize> {
    let mut parents = [None; 4];
    let mut changed = 0;
    for (index, row) in rows.iter().enumerate() {
        if let Some(name) = row.ordo.as_deref() {
            parents[0] = Some(save_taxon(
                transaction,
                TaxonRecord {
                    rank: "ordo",
                    name,
                    parent_id: None,
                    binomial_name: row.binomial_name.as_deref(),
                },
                preserve_binomial_ids,
            )?);
            parents[1..].fill(None);
            changed += 1;
        }
        if let Some(name) = row.familia.as_deref() {
            parents[1] = Some(save_taxon(
                transaction,
                TaxonRecord {
                    rank: "familia",
                    name,
                    parent_id: parents[0],
                    binomial_name: row.binomial_name.as_deref(),
                },
                preserve_binomial_ids,
            )?);
            parents[2..].fill(None);
            changed += 1;
        }
        if let Some(name) = row.genus.as_deref() {
            parents[2] = Some(save_taxon(
                transaction,
                TaxonRecord {
                    rank: "genus",
                    name,
                    parent_id: parents[1].or(parents[0]),
                    binomial_name: row.binomial_name.as_deref(),
                },
                preserve_binomial_ids,
            )?);
            parents[3] = None;
            changed += 1;
        }
        if let Some(name) = row.species.as_deref() {
            parents[3] = Some(save_taxon(
                transaction,
                TaxonRecord {
                    rank: "species",
                    name,
                    parent_id: parents[2].or(parents[1]).or(parents[0]),
                    binomial_name: row.binomial_name.as_deref(),
                },
                preserve_binomial_ids,
            )?);
            changed += 1;
        }
        progress(
            (index + 1) as u64,
            Some(rows.len() as u64),
            "Importing taxa",
        );
    }
    Ok(changed)
}

fn save_taxon(
    transaction: &Transaction<'_>,
    record: TaxonRecord<'_>,
    preserve_binomial_ids: bool,
) -> CoreResult<i64> {
    if preserve_binomial_ids && let Some(binomial_name) = record.binomial_name {
        let existing = transaction
            .query_row(
                "SELECT taxon_id FROM taxa WHERE binomial_name = ? ORDER BY taxon_id",
                [binomial_name],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        if let Some(taxon_id) = existing {
            transaction.execute(
                "UPDATE taxa SET rank = ?, name = ?, parent_id = ? WHERE taxon_id = ?",
                params![record.rank, record.name, record.parent_id, taxon_id],
            )?;
            return Ok(taxon_id);
        }
    }
    transaction.execute(
        "INSERT INTO taxa (rank, name, parent_id, binomial_name) VALUES (?, ?, ?, ?)",
        params![
            record.rank,
            record.name,
            record.parent_id,
            record.binomial_name
        ],
    )?;
    Ok(transaction.last_insert_rowid())
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
