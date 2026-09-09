//! Photo-to-taxon matching, mapping state, and taxonomy-based photo browsing.
//!
//! This module is the public mapping facade. Candidate persistence, query
//! construction, and usage accounting remain private implementation details.

use std::collections::{BTreeMap, HashMap};

use rusqlite::types::Value as SqlValue;
use rusqlite::{Transaction, params, params_from_iter};
use serde::{Deserialize, Serialize};

use crate::db::Database;
use crate::error::{CoreError, CoreResult};
use crate::models::{OperationProgress, OperationProgressUnit, Photo};
use crate::naming::PhotoFilenameParser;
use crate::taxonomy::{
    TaxonDisplayNames, TaxonRank, TaxonSummary, TaxonomyNameType, load_taxon_summaries,
    match_exact_taxonomy_name,
};

mod actions;
mod candidates;
mod matched_names;
mod name_match;
mod navigation;
mod status;
mod tree;

pub use actions::{
    clear_photo_mapping, get_metadata, get_photo_mapping, get_photo_mapping_detail,
    get_photo_taxon_display_summary, remap_photo, set_photo_mapping,
};
pub use name_match::{
    PhotoNameField, PhotoNameMatchSettings, get_photo_name_match_settings,
    set_photo_name_match_settings,
};
pub use navigation::{list_taxon_photos, search_photo_taxa, suggest_photo_taxa};
pub use status::{list_photos_by_mapping_status, search_photos_by_mapping_status};
pub use tree::{browse_photo_taxon, get_photo_taxon_counts, get_photo_taxon_node};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PhotoTaxonStatus {
    Matched,
    Unmatched,
    Ambiguous,
    Processing,
}

impl PhotoTaxonStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Matched => "matched",
            Self::Unmatched => "unmatched",
            Self::Ambiguous => "ambiguous",
            Self::Processing => "processing",
        }
    }

    fn from_str(value: &str) -> CoreResult<Self> {
        match value {
            "matched" => Ok(Self::Matched),
            "unmatched" => Ok(Self::Unmatched),
            "ambiguous" => Ok(Self::Ambiguous),
            "processing" => Ok(Self::Processing),
            _ => Err(CoreError::InvalidArgument(format!(
                "invalid photo taxon status: {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PhotoMappingSummary {
    pub photo_id: i64,
    pub taxon_id: Option<i64>,
    pub status: PhotoTaxonStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PhotoMappingDetail {
    pub mapping: PhotoMappingSummary,
    pub matched_names: Vec<PhotoMatchedName>,
    pub candidates: Vec<PhotoTaxonCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PhotoMatchedName {
    pub name_id: i64,
    pub name_type: TaxonomyNameType,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PhotoTaxonCandidate {
    pub summary: TaxonSummary,
    pub matched_names: Vec<PhotoMatchedName>,
    pub accepted_names: TaxonDisplayNames,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PhotoTaxonUsage {
    pub taxon_id: i64,
    pub rank: TaxonRank,
    pub names: TaxonDisplayNames,
    pub direct_photo_count: i64,
    pub subtree_photo_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PhotoTaxonNode {
    pub taxon: Option<PhotoTaxonUsage>,
    pub subtree_photo_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PhotoTaxonEntryCounts {
    pub taxon_count: i64,
    pub photo_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PhotoTaxonItem {
    Taxon { taxon: PhotoTaxonUsage },
    Photo { photo: Photo },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PhotoMappingListStatus {
    Matched,
    Unmatched,
    Ambiguous,
    Processing,
}

impl PhotoMappingListStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Matched => "matched",
            Self::Unmatched => "unmatched",
            Self::Ambiguous => "ambiguous",
            Self::Processing => "processing",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PhotoMappingListItem {
    pub photo: Photo,
    pub mapping: PhotoMappingSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PhotoMappingRunResult {
    pub processed: usize,
    pub changed: usize,
    pub pending: i64,
}

const PHOTO_TAXON_CANDIDATE_LIMIT: usize = 500;
const PHOTO_MAPPING_BATCH_SIZE: usize = 200;

pub type MappingProgressCallback<'a> = dyn FnMut(OperationProgress) + Send + 'a;

pub(crate) fn queue_photo_ids(
    transaction: &Transaction<'_>,
    photo_ids: &[i64],
    reason: &str,
) -> CoreResult<()> {
    if photo_ids.is_empty() {
        return Ok(());
    }
    let mut statement = transaction.prepare_cached(
        r#"
        INSERT INTO photo_mapping_queue (photo_id, reason)
        VALUES (?, ?)
        ON CONFLICT(photo_id) DO UPDATE SET reason = excluded.reason
        "#,
    )?;
    for photo_id in photo_ids {
        statement.execute(params![photo_id, reason])?;
    }
    Ok(())
}

pub fn process_pending_photo_matches(
    database: &Database,
    progress: &mut MappingProgressCallback<'_>,
) -> CoreResult<PhotoMappingRunResult> {
    let connection = database.connect()?;
    let filename_parser = PhotoFilenameParser::load(&connection)?;
    let match_settings = name_match::load(&connection)?;
    let total = connection.query_row("SELECT COUNT(*) FROM photo_mapping_queue", [], |row| {
        row.get::<_, i64>(0)
    })?;
    drop(connection);

    let mut processed = 0usize;
    let mut changed = 0usize;
    progress(OperationProgress {
        stage: "mapping_photos".into(),
        current: Some(0),
        total: Some(total as u64),
        unit: Some(OperationProgressUnit::Photos),
    });
    loop {
        let mut connection = database.connect()?;
        let transaction = connection.transaction()?;
        let photo_ids = {
            let mut statement = transaction.prepare(
                r#"
                SELECT photo_id
                FROM photo_mapping_queue
                ORDER BY photo_id
                LIMIT ?
                "#,
            )?;
            statement
                .query_map([PHOTO_MAPPING_BATCH_SIZE as i64], |row| {
                    row.get::<_, i64>(0)
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        if photo_ids.is_empty() {
            break;
        }
        changed +=
            remap_photo_ids_with(&transaction, &photo_ids, &filename_parser, &match_settings)?;
        delete_queued_photo_ids(&transaction, &photo_ids)?;
        transaction.commit()?;
        processed += photo_ids.len();
        progress(OperationProgress {
            stage: "mapping_photos".into(),
            current: Some(processed as u64),
            total: Some(total as u64),
            unit: Some(OperationProgressUnit::Photos),
        });
        std::thread::yield_now();
    }
    let connection = database.connect()?;
    let queued = connection.query_row("SELECT COUNT(*) FROM photo_mapping_queue", [], |row| {
        row.get::<_, i64>(0)
    })?;
    Ok(PhotoMappingRunResult {
        processed,
        changed,
        pending: queued,
    })
}

pub fn has_pending_photo_matches(database: &Database) -> CoreResult<bool> {
    let connection = database.connect()?;
    connection
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM photo_mapping_queue)",
            [],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

pub(crate) fn remap_photo_ids(
    transaction: &Transaction<'_>,
    photo_ids: &[i64],
) -> CoreResult<usize> {
    if photo_ids.is_empty() {
        return Ok(0);
    }
    let filename_parser = PhotoFilenameParser::load(transaction)?;
    let match_settings = name_match::load(transaction)?;
    remap_photo_ids_with(transaction, photo_ids, &filename_parser, &match_settings)
}

fn remap_photo_ids_with(
    transaction: &Transaction<'_>,
    photo_ids: &[i64],
    filename_parser: &PhotoFilenameParser,
    match_settings: &PhotoNameMatchSettings,
) -> CoreResult<usize> {
    if photo_ids.is_empty() {
        return Ok(0);
    }
    let photos = load_photo_names(transaction, photo_ids)?;
    let old_mappings = load_mappings(transaction, photo_ids)?;
    let mut direct_deltas = BTreeMap::<i64, i64>::new();
    let mut changed = 0usize;
    for (photo_id, filename) in photos {
        let results =
            match_photo_taxa_with(transaction, filename_parser, match_settings, &filename)?;
        let old_mapping = old_mappings.get(&photo_id).copied();
        let old_taxon_id = old_mapping.and_then(|(taxon_id, status)| {
            (status == PhotoTaxonStatus::Matched)
                .then_some(taxon_id)
                .flatten()
        });
        let (new_taxon_id, new_status) = if old_taxon_id.is_some_and(|taxon_id| {
            results
                .iter()
                .any(|result| result.summary.taxon_id == taxon_id)
        }) {
            (old_taxon_id, PhotoTaxonStatus::Matched)
        } else {
            (resolved_taxon_id(&results), resolved_status(&results))
        };
        if old_taxon_id != new_taxon_id {
            if let Some(taxon_id) = old_taxon_id {
                *direct_deltas.entry(taxon_id).or_default() -= 1;
            }
            if let Some(taxon_id) = new_taxon_id {
                *direct_deltas.entry(taxon_id).or_default() += 1;
            }
        }
        if old_mapping != Some((new_taxon_id, new_status)) {
            transaction.execute(
                r#"
                INSERT INTO photo_taxon_mapping (photo_id, taxon_id, status)
                VALUES (?, ?, ?)
                ON CONFLICT(photo_id) DO UPDATE SET
                    taxon_id = excluded.taxon_id,
                    status = excluded.status
                "#,
                params![photo_id, new_taxon_id, new_status.as_str()],
            )?;
            changed += 1;
        }
        let stable_matched_names: &[PhotoMatchedName] = if new_status == PhotoTaxonStatus::Matched {
            let taxon_id = new_taxon_id.ok_or_else(|| {
                CoreError::Consistency(format!("matched photo {photo_id} has no resolved taxon"))
            })?;
            &results
                .iter()
                .find(|result| result.summary.taxon_id == taxon_id)
                .ok_or_else(|| {
                    CoreError::Consistency(format!(
                        "matched photo {photo_id} has no candidate for taxon {taxon_id}"
                    ))
                })?
                .matched_names
        } else {
            &[]
        };
        matched_names::replace(transaction, photo_id, stable_matched_names)?;
        if new_status == PhotoTaxonStatus::Ambiguous {
            candidates::replace(transaction, photo_id, &results)?;
        } else {
            candidates::clear(transaction, photo_id)?;
        }
    }
    apply_usage_deltas(transaction, &direct_deltas)?;
    Ok(changed)
}

pub(crate) fn remove_photo_mappings(
    transaction: &Transaction<'_>,
    photo_ids: &[i64],
) -> CoreResult<()> {
    if photo_ids.is_empty() {
        return Ok(());
    }
    let old_taxa = load_mapped_taxa(transaction, photo_ids)?;
    let mut direct_deltas = BTreeMap::<i64, i64>::new();
    for taxon_id in old_taxa.into_values() {
        *direct_deltas.entry(taxon_id).or_default() -= 1;
    }
    let selection = id_selection(transaction, photo_ids, "photo_id", "temp_mapping_photo_ids")?;
    transaction.execute(
        &format!(
            "DELETE FROM photo_taxon_mapping WHERE {}",
            selection.predicate
        ),
        params_from_iter(selection.values),
    )?;
    apply_usage_deltas(transaction, &direct_deltas)
}

pub(crate) fn remove_directory_mappings(
    transaction: &Transaction<'_>,
    directory_ids: &[i64],
) -> CoreResult<()> {
    if directory_ids.is_empty() {
        return Ok(());
    }
    let selection = id_selection(
        transaction,
        directory_ids,
        "directory_id",
        "temp_mapping_directory_ids",
    )?;
    let mut statement = transaction.prepare(&format!(
        r#"
        WITH RECURSIVE descendants(directory_id) AS (
            SELECT directory_id FROM photo_directories WHERE {}
            UNION ALL
            SELECT child.directory_id
            FROM photo_directories AS child
            JOIN descendants ON child.parent_directory_id = descendants.directory_id
        )
        SELECT photos.photo_id
        FROM photos
        JOIN descendants USING (directory_id)
        "#,
        selection.predicate
    ))?;
    let rows = statement.query_map(params_from_iter(selection.values), |row| {
        row.get::<_, i64>(0)
    })?;
    let photo_ids = rows.collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    remove_photo_mappings(transaction, &photo_ids)
}

fn match_photo_taxa_with(
    connection: &rusqlite::Connection,
    filename_parser: &PhotoFilenameParser,
    settings: &PhotoNameMatchSettings,
    filename: &str,
) -> CoreResult<Vec<PhotoTaxonCandidate>> {
    let parsed = filename_parser.parse(filename)?;
    for field in &settings.priority {
        let Some(name) = field.value(&parsed.info) else {
            continue;
        };
        let candidates = find_photo_name_candidates(connection, *field, name)?;
        if !candidates.is_empty() {
            return Ok(candidates);
        }
    }
    Ok(Vec::new())
}

fn find_photo_name_candidates(
    connection: &rusqlite::Connection,
    field: PhotoNameField,
    name: &str,
) -> CoreResult<Vec<PhotoTaxonCandidate>> {
    let mut matches =
        match_exact_taxonomy_name(connection, name, field.rank(), field.accepted_name_type())?;
    if matches.is_empty() {
        matches =
            match_exact_taxonomy_name(connection, name, field.rank(), field.alias_name_type())?;
    }
    let matches = matches
        .into_iter()
        .take(PHOTO_TAXON_CANDIDATE_LIMIT)
        .collect::<Vec<_>>();
    let taxon_ids = matches
        .iter()
        .map(|matched| matched.taxon_id)
        .collect::<Vec<_>>();
    let summaries = load_taxon_summaries(connection, &taxon_ids)?;
    Ok(matches
        .into_iter()
        .zip(summaries)
        .map(|(matched, summary)| PhotoTaxonCandidate {
            accepted_names: summary.names.clone(),
            matched_names: vec![PhotoMatchedName {
                name_id: matched.name_id,
                name_type: matched.name_type,
                name: matched.name,
            }],
            summary,
        })
        .collect())
}

fn resolved_taxon_id(results: &[PhotoTaxonCandidate]) -> Option<i64> {
    match results {
        [candidate] => Some(candidate.summary.taxon_id),
        _ => None,
    }
}

fn resolved_status(results: &[PhotoTaxonCandidate]) -> PhotoTaxonStatus {
    match results.len() {
        0 => PhotoTaxonStatus::Unmatched,
        1 => PhotoTaxonStatus::Matched,
        _ => PhotoTaxonStatus::Ambiguous,
    }
}

fn load_photo_names(
    transaction: &Transaction<'_>,
    photo_ids: &[i64],
) -> CoreResult<Vec<(i64, String)>> {
    let selection = id_selection(transaction, photo_ids, "photo_id", "temp_mapping_photo_ids")?;
    let mut statement = transaction.prepare(&format!(
        "SELECT photo_id, filename FROM photos WHERE {} ORDER BY photo_id",
        selection.predicate
    ))?;
    let rows = statement.query_map(params_from_iter(selection.values), |row| {
        Ok((row.get(0)?, row.get(1)?))
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn load_mapped_taxa(
    transaction: &Transaction<'_>,
    photo_ids: &[i64],
) -> CoreResult<HashMap<i64, i64>> {
    let selection = id_selection(transaction, photo_ids, "photo_id", "temp_mapping_photo_ids")?;
    let mut statement = transaction.prepare(&format!(
        r#"
        SELECT photo_id, taxon_id
        FROM photo_taxon_mapping
        WHERE status = 'matched' AND {}
        "#,
        selection.predicate
    ))?;
    let rows = statement.query_map(params_from_iter(selection.values), |row| {
        Ok((row.get(0)?, row.get(1)?))
    })?;
    Ok(rows.collect::<Result<HashMap<_, _>, _>>()?)
}

fn load_mappings(
    transaction: &Transaction<'_>,
    photo_ids: &[i64],
) -> CoreResult<HashMap<i64, (Option<i64>, PhotoTaxonStatus)>> {
    let selection = id_selection(transaction, photo_ids, "photo_id", "temp_mapping_photo_ids")?;
    let mut statement = transaction.prepare(&format!(
        r#"
        SELECT photo_id, taxon_id, status
        FROM photo_taxon_mapping
        WHERE {}
        "#,
        selection.predicate
    ))?;
    let rows = statement.query_map(params_from_iter(selection.values), |row| {
        let status = row.get::<_, String>(2)?;
        let status = PhotoTaxonStatus::from_str(&status).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
        Ok((row.get::<_, i64>(0)?, (row.get(1)?, status)))
    })?;
    Ok(rows.collect::<Result<HashMap<_, _>, _>>()?)
}

fn apply_usage_deltas(
    transaction: &Transaction<'_>,
    direct_deltas: &BTreeMap<i64, i64>,
) -> CoreResult<()> {
    transaction.execute_batch(
        r#"
        CREATE TEMP TABLE IF NOT EXISTS temp_photo_taxon_deltas (
            taxon_id INTEGER PRIMARY KEY,
            delta INTEGER NOT NULL
        ) WITHOUT ROWID;
        DELETE FROM temp_photo_taxon_deltas;
        "#,
    )?;
    {
        let mut statement = transaction.prepare_cached(
            "INSERT INTO temp_photo_taxon_deltas (taxon_id, delta) VALUES (?, ?)",
        )?;
        for (&taxon_id, &delta) in direct_deltas {
            if delta != 0 {
                statement.execute(params![taxon_id, delta])?;
            }
        }
    }
    transaction.execute_batch(
        r#"
        CREATE TEMP TABLE IF NOT EXISTS temp_photo_usage_deltas (
            taxon_id INTEGER PRIMARY KEY,
            direct_delta INTEGER NOT NULL,
            subtree_delta INTEGER NOT NULL
        ) WITHOUT ROWID;
        DELETE FROM temp_photo_usage_deltas;

        WITH RECURSIVE lineage(taxon_id, parent_taxon_id, delta) AS (
            SELECT taxa.taxon_id, taxa.parent_taxon_id, seeds.delta
            FROM temp_photo_taxon_deltas AS seeds
            JOIN taxa ON taxa.taxon_id = seeds.taxon_id
            UNION ALL
            SELECT parent.taxon_id, parent.parent_taxon_id, child.delta
            FROM lineage AS child
            JOIN taxa AS parent ON parent.taxon_id = child.parent_taxon_id
        ),
        subtree_deltas AS (
            SELECT taxon_id, SUM(delta) AS delta
            FROM lineage
            GROUP BY taxon_id
        ),
        affected_taxa AS (
            SELECT taxon_id FROM temp_photo_taxon_deltas
            UNION
            SELECT taxon_id FROM subtree_deltas
        )
        INSERT INTO temp_photo_usage_deltas (
            taxon_id, direct_delta, subtree_delta
        )
        SELECT affected_taxa.taxon_id,
               COALESCE(direct.delta, 0),
               COALESCE(subtree.delta, 0)
        FROM affected_taxa
        LEFT JOIN temp_photo_taxon_deltas AS direct USING (taxon_id)
        LEFT JOIN subtree_deltas AS subtree USING (taxon_id)
        WHERE TRUE;

        UPDATE photo_taxon_usage
        SET direct_photo_count = direct_photo_count + (
                SELECT direct_delta
                FROM temp_photo_usage_deltas AS delta
                WHERE delta.taxon_id = photo_taxon_usage.taxon_id
            ),
            subtree_photo_count = subtree_photo_count + (
                SELECT subtree_delta
                FROM temp_photo_usage_deltas AS delta
                WHERE delta.taxon_id = photo_taxon_usage.taxon_id
            )
        WHERE taxon_id IN (SELECT taxon_id FROM temp_photo_usage_deltas);

        INSERT INTO photo_taxon_usage (
            taxon_id, direct_photo_count, subtree_photo_count
        )
        SELECT delta.taxon_id, delta.direct_delta, delta.subtree_delta
        FROM temp_photo_usage_deltas AS delta
        LEFT JOIN photo_taxon_usage AS usage USING (taxon_id)
        WHERE usage.taxon_id IS NULL;
        "#,
    )?;
    transaction.execute(
        "DELETE FROM photo_taxon_usage WHERE direct_photo_count = 0 AND subtree_photo_count = 0",
        [],
    )?;
    Ok(())
}

fn mapping_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PhotoMappingSummary> {
    let status = row.get::<_, String>(2)?;
    Ok(PhotoMappingSummary {
        photo_id: row.get(0)?,
        taxon_id: row.get(1)?,
        status: PhotoTaxonStatus::from_str(&status).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
    })
}

struct IdSelection {
    predicate: String,
    values: Vec<SqlValue>,
}

fn id_selection(
    transaction: &Transaction<'_>,
    ids: &[i64],
    column: &str,
    temp_table: &str,
) -> CoreResult<IdSelection> {
    const INLINE_ID_LIMIT: usize = 500;
    if ids.len() <= INLINE_ID_LIMIT {
        let placeholders = std::iter::repeat_n("?", ids.len())
            .collect::<Vec<_>>()
            .join(",");
        return Ok(IdSelection {
            predicate: format!("{column} IN ({placeholders})"),
            values: ids.iter().copied().map(SqlValue::Integer).collect(),
        });
    }
    transaction.execute_batch(&format!(
        r#"
        CREATE TEMP TABLE IF NOT EXISTS {temp_table} (
            value INTEGER PRIMARY KEY
        ) WITHOUT ROWID;
        DELETE FROM {temp_table};
        "#
    ))?;
    let mut statement =
        transaction.prepare_cached(&format!("INSERT INTO {temp_table} (value) VALUES (?)"))?;
    for id in ids {
        statement.execute([id])?;
    }
    Ok(IdSelection {
        predicate: format!("{column} IN (SELECT value FROM {temp_table})"),
        values: Vec::new(),
    })
}

fn delete_queued_photo_ids(transaction: &Transaction<'_>, photo_ids: &[i64]) -> CoreResult<()> {
    let selection = id_selection(transaction, photo_ids, "photo_id", "temp_mapping_photo_ids")?;
    transaction.execute(
        &format!(
            "DELETE FROM photo_mapping_queue WHERE {}",
            selection.predicate
        ),
        params_from_iter(selection.values),
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
