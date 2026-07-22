use std::collections::{BTreeMap, BTreeSet, HashMap};

use aho_corasick::AhoCorasick;
use rusqlite::types::Value as SqlValue;
use rusqlite::{OptionalExtension, Transaction, params, params_from_iter};
use serde::{Deserialize, Serialize};

use crate::db::{Database, taxon_from_row};
use crate::error::{CoreError, CoreResult};
use crate::models::{MappingMetadata, MappingNode, MappingSyncResult, Photo, Taxon};
use crate::photos;
use crate::taxonomy::{
    TaxonDisplayNames, TaxonSummary, TaxonomyNameKind, get_taxon_summary, search_taxa,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PhotoTaxonStatus {
    Matched,
    Unmatched,
    Ambiguous,
    Stale,
}

impl PhotoTaxonStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Matched => "matched",
            Self::Unmatched => "unmatched",
            Self::Ambiguous => "ambiguous",
            Self::Stale => "stale",
        }
    }

    fn from_str(value: &str) -> CoreResult<Self> {
        match value {
            "matched" => Ok(Self::Matched),
            "unmatched" => Ok(Self::Unmatched),
            "ambiguous" => Ok(Self::Ambiguous),
            "stale" => Ok(Self::Stale),
            _ => Err(CoreError::InvalidArgument(format!(
                "invalid photo taxon status: {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PhotoTaxonMapping {
    pub photo_id: i64,
    pub taxon_id: Option<i64>,
    pub status: PhotoTaxonStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PhotoMatchedName {
    pub name_id: i64,
    pub name_kind: TaxonomyNameKind,
    pub name: String,
    pub is_accepted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PhotoTaxonCandidate {
    pub summary: TaxonSummary,
    pub matched_names: Vec<PhotoMatchedName>,
    pub accepted_names: TaxonDisplayNames,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PhotoTaxonMatch {
    pub mapping: PhotoTaxonMapping,
    pub candidates: Vec<PhotoTaxonCandidate>,
}

#[derive(Debug, Clone)]
struct TaxonNameRecord {
    name_id: i64,
    taxon_id: i64,
    name_kind: TaxonomyNameKind,
    name: String,
    is_accepted: bool,
}

struct TaxonNameMatcher {
    automaton: Option<AhoCorasick>,
    patterns: Vec<Vec<TaxonNameRecord>>,
}

#[derive(Debug)]
struct MatchResult {
    taxon_id: Option<i64>,
    status: PhotoTaxonStatus,
    records_by_taxon: BTreeMap<i64, Vec<TaxonNameRecord>>,
}

pub fn get_metadata(database: &Database) -> CoreResult<MappingMetadata> {
    let connection = database.connect()?;
    let count = |status: &str| -> CoreResult<i64> {
        Ok(connection.query_row(
            "SELECT COUNT(*) FROM photo_taxon_mapping WHERE status = ?",
            [status],
            |row| row.get(0),
        )?)
    };
    let mapping_taxa_count = connection.query_row(
        "SELECT COUNT(*) FROM photo_taxon_usage WHERE subtree_photo_count > 0",
        [],
        |row| row.get(0),
    )?;
    Ok(MappingMetadata {
        mapped_photo_count: count("matched")?,
        unmatched_photo_count: count("unmatched")?,
        ambiguous_photo_count: count("ambiguous")?,
        mapping_taxa_count,
    })
}

pub fn get_photo_mapping(
    database: &Database,
    photo_id: i64,
) -> CoreResult<Option<PhotoTaxonMapping>> {
    let connection = database.connect()?;
    connection
        .query_row(
            "SELECT photo_id, taxon_id, status FROM photo_taxon_mapping WHERE photo_id = ?",
            [photo_id],
            mapping_from_row,
        )
        .optional()
        .map_err(Into::into)
}

pub fn get_photo_taxon_match(database: &Database, photo_id: i64) -> CoreResult<PhotoTaxonMatch> {
    let photo = photos::get_photo(database, photo_id)?
        .ok_or_else(|| CoreError::NotFound(format!("photo {photo_id}")))?;
    let connection = database.connect()?;
    let matcher = TaxonNameMatcher::load(&connection)?;
    let result = matcher.match_filename(&photo.filename);
    let mapping = get_photo_mapping(database, photo_id)?.unwrap_or(PhotoTaxonMapping {
        photo_id,
        taxon_id: result.taxon_id,
        status: result.status,
    });
    let mut candidates = Vec::new();
    for (taxon_id, records) in result.records_by_taxon {
        let Some(summary) = get_taxon_summary(database, taxon_id)? else {
            continue;
        };
        candidates.push(PhotoTaxonCandidate {
            accepted_names: summary.names.clone(),
            summary,
            matched_names: records
                .into_iter()
                .map(|record| PhotoMatchedName {
                    name_id: record.name_id,
                    name_kind: record.name_kind,
                    name: record.name,
                    is_accepted: record.is_accepted,
                })
                .collect(),
        });
    }
    Ok(PhotoTaxonMatch {
        mapping,
        candidates,
    })
}

pub fn select_photo_taxon(
    database: &Database,
    photo_id: i64,
    taxon_id: i64,
) -> CoreResult<PhotoTaxonMapping> {
    let photo = photos::get_photo(database, photo_id)?
        .ok_or_else(|| CoreError::NotFound(format!("photo {photo_id}")))?;
    let mut connection = database.connect()?;
    let transaction = connection.transaction()?;
    let matcher = TaxonNameMatcher::load(&transaction)?;
    let result = matcher.match_filename(&photo.filename);
    if !result.records_by_taxon.contains_key(&taxon_id) {
        return Err(CoreError::InvalidArgument(format!(
            "taxon {taxon_id} is not a filename candidate for photo {photo_id}"
        )));
    }
    let old_taxon_id = transaction
        .query_row(
            r#"
            SELECT taxon_id FROM photo_taxon_mapping
            WHERE photo_id = ? AND status = 'matched'
            "#,
            [photo_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    transaction.execute(
        r#"
        INSERT INTO photo_taxon_mapping (photo_id, taxon_id, status)
        VALUES (?, ?, 'matched')
        ON CONFLICT(photo_id) DO UPDATE SET taxon_id = excluded.taxon_id, status = 'matched'
        "#,
        params![photo_id, taxon_id],
    )?;
    if old_taxon_id != Some(taxon_id) {
        let mut deltas = BTreeMap::new();
        if let Some(old_taxon_id) = old_taxon_id {
            deltas.insert(old_taxon_id, -1);
        }
        *deltas.entry(taxon_id).or_default() += 1;
        apply_usage_deltas(&transaction, &deltas)?;
    }
    transaction.commit()?;
    Ok(PhotoTaxonMapping {
        photo_id,
        taxon_id: Some(taxon_id),
        status: PhotoTaxonStatus::Matched,
    })
}

pub fn get_root(database: &Database) -> CoreResult<MappingNode> {
    get_by_taxon_id(database, None)
}

pub fn get_by_taxon_id(database: &Database, taxon_id: Option<i64>) -> CoreResult<MappingNode> {
    let connection = database.connect()?;
    let taxon = match taxon_id {
        Some(id) => connection
            .query_row(
                "SELECT * FROM taxa_display WHERE taxon_id = ?",
                [id],
                taxon_from_row,
            )
            .optional()?,
        None => None,
    };
    let photo_ids = match taxon_id {
        Some(id) => {
            let mut statement = connection.prepare(
                r#"
                SELECT photo_id FROM photo_taxon_mapping
                WHERE taxon_id = ? AND status = 'matched'
                ORDER BY photo_id
                "#,
            )?;
            let rows = statement.query_map([id], |row| row.get::<_, i64>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        }
        None => Vec::new(),
    };
    let mut statement = match taxon_id {
        Some(_) => connection.prepare(
            r#"
            SELECT taxa_display.*
            FROM taxa_display
            JOIN photo_taxon_usage USING (taxon_id)
            WHERE taxa_display.parent_id = ?
              AND photo_taxon_usage.subtree_photo_count > 0
            ORDER BY taxa_display.rank, taxa_display.name, taxa_display.taxon_id
            "#,
        )?,
        None => connection.prepare(
            r#"
            SELECT taxa_display.*
            FROM taxa_display
            JOIN photo_taxon_usage USING (taxon_id)
            WHERE taxa_display.parent_id IS NULL
              AND photo_taxon_usage.subtree_photo_count > 0
            ORDER BY taxa_display.name, taxa_display.taxon_id
            "#,
        )?,
    };
    let rows = match taxon_id {
        Some(id) => statement.query_map([id], taxon_from_row)?,
        None => statement.query_map([], taxon_from_row)?,
    };
    let children = rows.collect::<Result<Vec<_>, _>>()?;
    let (direct_photo_count, subtree_photo_count) = match taxon_id {
        Some(id) => connection
            .query_row(
                r#"
                SELECT direct_photo_count, subtree_photo_count
                FROM photo_taxon_usage WHERE taxon_id = ?
                "#,
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .unwrap_or((0, 0)),
        None => {
            let total = connection.query_row(
                "SELECT COUNT(*) FROM photo_taxon_mapping WHERE status = 'matched'",
                [],
                |row| row.get(0),
            )?;
            (0, total)
        }
    };
    Ok(MappingNode {
        taxon,
        photo_ids,
        children,
        direct_photo_count,
        subtree_photo_count,
    })
}

pub fn get_by_binomial(database: &Database, name: &str) -> CoreResult<MappingNode> {
    get_by_taxonomy_name(database, name, Some(TaxonomyNameKind::Scientific))
}

pub fn get_by_name(database: &Database, name: &str) -> CoreResult<MappingNode> {
    get_by_taxonomy_name(database, name, None)
}

fn get_by_taxonomy_name(
    database: &Database,
    name: &str,
    kind: Option<TaxonomyNameKind>,
) -> CoreResult<MappingNode> {
    let connection = database.connect()?;
    let taxon_id = match kind {
        Some(kind) => connection
            .query_row(
                r#"
                SELECT taxon_id FROM taxon_names
                WHERE name_kind = ? AND name = ?
                ORDER BY is_accepted DESC, taxon_id LIMIT 1
                "#,
                params![kind.code(), name],
                |row| row.get::<_, i64>(0),
            )
            .optional()?,
        None => connection
            .query_row(
                r#"
                SELECT taxon_id FROM taxon_names
                WHERE name = ?
                ORDER BY is_accepted DESC, name_kind, taxon_id LIMIT 1
                "#,
                [name],
                |row| row.get::<_, i64>(0),
            )
            .optional()?,
    };
    drop(connection);
    match taxon_id {
        Some(id) => get_by_taxon_id(database, Some(id)),
        None => Ok(empty_node()),
    }
}

pub fn suggest(
    database: &Database,
    query: &str,
    mode: &str,
    limit: usize,
) -> CoreResult<Vec<Taxon>> {
    let results = search_taxa(database, query, limit)?;
    let values = results
        .into_iter()
        .filter(|result| {
            mode != "binomial"
                || result
                    .matches
                    .iter()
                    .any(|value| value.name_kind == TaxonomyNameKind::Scientific)
        })
        .map(|result| Taxon {
            taxon_id: result.summary.taxon_id,
            rank: format!("{:?}", result.summary.rank).to_ascii_lowercase(),
            name: result
                .summary
                .names
                .chinese
                .or(result.summary.names.english)
                .or(result.summary.names.scientific.clone())
                .unwrap_or_default(),
            parent_id: result.summary.breadcrumb.last().map(|value| value.taxon_id),
            binomial_name: result.summary.names.scientific,
        })
        .collect();
    Ok(values)
}

pub fn rebuild_mapping(database: &Database) -> CoreResult<MappingSyncResult> {
    let photos = photos::list_photos(database)?;
    let ids = photos
        .iter()
        .map(|photo| photo.photo_id)
        .collect::<Vec<_>>();
    let mut connection = database.connect()?;
    let transaction = connection.transaction()?;
    transaction.execute("DELETE FROM photo_taxon_usage", [])?;
    transaction.execute("DELETE FROM photo_taxon_mapping", [])?;
    remap_photo_ids(&transaction, &ids)?;
    transaction.commit()?;
    mapping_result(database, photos)
}

pub(crate) fn refresh_after_taxonomy_change(database: &Database) -> CoreResult<()> {
    rebuild_mapping(database).map(|_| ())
}

pub(crate) fn remap_photo_ids(transaction: &Transaction<'_>, photo_ids: &[i64]) -> CoreResult<()> {
    if photo_ids.is_empty() {
        return Ok(());
    }
    let matcher = TaxonNameMatcher::load(transaction)?;
    let photos = load_photo_names(transaction, photo_ids)?;
    let old_taxa = load_mapped_taxa(transaction, photo_ids)?;
    let mut direct_deltas = BTreeMap::<i64, i64>::new();
    for (photo_id, filename) in photos {
        let result = matcher.match_filename(&filename);
        let old_taxon_id = old_taxa.get(&photo_id).copied();
        if old_taxon_id != result.taxon_id {
            if let Some(taxon_id) = old_taxon_id {
                *direct_deltas.entry(taxon_id).or_default() -= 1;
            }
            if let Some(taxon_id) = result.taxon_id {
                *direct_deltas.entry(taxon_id).or_default() += 1;
            }
        }
        transaction.execute(
            r#"
            INSERT INTO photo_taxon_mapping (photo_id, taxon_id, status)
            VALUES (?, ?, ?)
            ON CONFLICT(photo_id) DO UPDATE SET
                taxon_id = excluded.taxon_id,
                status = excluded.status
            "#,
            params![photo_id, result.taxon_id, result.status.as_str()],
        )?;
    }
    apply_usage_deltas(transaction, &direct_deltas)
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
    let (placeholders, values) = input_values(photo_ids);
    transaction.execute(
        &format!("DELETE FROM photo_taxon_mapping WHERE photo_id IN ({placeholders})"),
        params_from_iter(values),
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
    let (placeholders, values) = input_values(directory_ids);
    let mut statement = transaction.prepare(&format!(
        r#"
        WITH RECURSIVE descendants(directory_id) AS (
            SELECT directory_id FROM photo_directories WHERE directory_id IN ({placeholders})
            UNION ALL
            SELECT child.directory_id
            FROM photo_directories AS child
            JOIN descendants ON child.parent_directory_id = descendants.directory_id
        )
        SELECT photos.photo_id
        FROM photos
        JOIN descendants USING (directory_id)
        "#
    ))?;
    let rows = statement.query_map(params_from_iter(values), |row| row.get::<_, i64>(0))?;
    let photo_ids = rows.collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    remove_photo_mappings(transaction, &photo_ids)
}

fn mapping_result(database: &Database, photos: Vec<Photo>) -> CoreResult<MappingSyncResult> {
    let connection = database.connect()?;
    let mapped = connection.query_row(
        "SELECT COUNT(*) FROM photo_taxon_mapping WHERE status = 'matched'",
        [],
        |row| row.get::<_, i64>(0),
    )? as usize;
    let ambiguous = connection.query_row(
        "SELECT COUNT(*) FROM photo_taxon_mapping WHERE status = 'ambiguous'",
        [],
        |row| row.get::<_, i64>(0),
    )? as usize;
    let unmapped_ids = {
        let mut statement = connection.prepare(
            "SELECT photo_id FROM photo_taxon_mapping WHERE status = 'unmatched' ORDER BY photo_id",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, i64>(0))?;
        rows.collect::<Result<BTreeSet<_>, _>>()?
    };
    let unmapped_photos = photos
        .iter()
        .filter(|photo| unmapped_ids.contains(&photo.photo_id))
        .cloned()
        .collect::<Vec<_>>();
    Ok(MappingSyncResult {
        processed: photos.len(),
        mapped,
        unmapped: unmapped_photos.len(),
        ambiguous,
        unmapped_photos,
        orphan_mappings_deleted: 0,
    })
}

impl TaxonNameMatcher {
    fn load(connection: &rusqlite::Connection) -> CoreResult<Self> {
        let mut statement = connection.prepare(
            r#"
            SELECT name_id, taxon_id, name_kind, name, is_accepted
            FROM taxon_names
            ORDER BY name_id
            "#,
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)? != 0,
            ))
        })?;
        let mut grouped = BTreeMap::<String, Vec<TaxonNameRecord>>::new();
        for row in rows {
            let (name_id, taxon_id, name_kind, name, is_accepted) = row?;
            let normalized = normalize_match_text(&name);
            if normalized.is_empty() {
                continue;
            }
            grouped
                .entry(format!(" {normalized} "))
                .or_default()
                .push(TaxonNameRecord {
                    name_id,
                    taxon_id,
                    name_kind: TaxonomyNameKind::from_code(name_kind)?,
                    name,
                    is_accepted,
                });
        }
        let keys = grouped.keys().cloned().collect::<Vec<_>>();
        let patterns = grouped.into_values().collect::<Vec<_>>();
        let automaton = (!keys.is_empty())
            .then(|| AhoCorasick::new(&keys))
            .transpose()
            .map_err(|error| CoreError::InvalidArgument(error.to_string()))?;
        Ok(Self {
            automaton,
            patterns,
        })
    }

    fn match_filename(&self, filename: &str) -> MatchResult {
        let stem = std::path::Path::new(filename)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(filename);
        let haystack = format!(" {} ", normalize_match_text(stem));
        let mut matched_patterns = Vec::new();
        let mut longest = 0;
        if let Some(automaton) = &self.automaton {
            for value in automaton.find_overlapping_iter(&haystack) {
                let length = value.len();
                match length.cmp(&longest) {
                    std::cmp::Ordering::Greater => {
                        longest = length;
                        matched_patterns.clear();
                        matched_patterns.push(value.pattern().as_usize());
                    }
                    std::cmp::Ordering::Equal => {
                        matched_patterns.push(value.pattern().as_usize());
                    }
                    std::cmp::Ordering::Less => {}
                }
            }
        }
        matched_patterns.sort_unstable();
        matched_patterns.dedup();
        let mut records_by_taxon = BTreeMap::<i64, Vec<TaxonNameRecord>>::new();
        for pattern in matched_patterns {
            for record in &self.patterns[pattern] {
                records_by_taxon
                    .entry(record.taxon_id)
                    .or_default()
                    .push(record.clone());
            }
        }
        match records_by_taxon.len() {
            0 => MatchResult {
                taxon_id: None,
                status: PhotoTaxonStatus::Unmatched,
                records_by_taxon,
            },
            1 => MatchResult {
                taxon_id: records_by_taxon.keys().next().copied(),
                status: PhotoTaxonStatus::Matched,
                records_by_taxon,
            },
            _ => MatchResult {
                taxon_id: None,
                status: PhotoTaxonStatus::Ambiguous,
                records_by_taxon,
            },
        }
    }
}

fn normalize_match_text(value: &str) -> String {
    let mut output = String::new();
    let mut separator = true;
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() {
            output.push(character);
            separator = false;
        } else if !separator {
            output.push(' ');
            separator = true;
        }
    }
    output.trim().into()
}

fn load_photo_names(
    transaction: &Transaction<'_>,
    photo_ids: &[i64],
) -> CoreResult<Vec<(i64, String)>> {
    let (placeholders, values) = input_values(photo_ids);
    let mut statement = transaction.prepare(&format!(
        "SELECT photo_id, filename FROM photos WHERE photo_id IN ({placeholders}) ORDER BY photo_id"
    ))?;
    let rows = statement.query_map(params_from_iter(values), |row| {
        Ok((row.get(0)?, row.get(1)?))
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn load_mapped_taxa(
    transaction: &Transaction<'_>,
    photo_ids: &[i64],
) -> CoreResult<HashMap<i64, i64>> {
    let (placeholders, values) = input_values(photo_ids);
    let mut statement = transaction.prepare(&format!(
        r#"
        SELECT photo_id, taxon_id
        FROM photo_taxon_mapping
        WHERE status = 'matched' AND photo_id IN ({placeholders})
        "#
    ))?;
    let rows = statement.query_map(params_from_iter(values), |row| {
        Ok((row.get(0)?, row.get(1)?))
    })?;
    Ok(rows.collect::<Result<HashMap<_, _>, _>>()?)
}

fn apply_usage_deltas(
    transaction: &Transaction<'_>,
    direct_deltas: &BTreeMap<i64, i64>,
) -> CoreResult<()> {
    let mut subtree_deltas = BTreeMap::<i64, i64>::new();
    for (&taxon_id, &delta) in direct_deltas {
        if delta == 0 {
            continue;
        }
        let mut statement = transaction.prepare(
            r#"
            WITH RECURSIVE lineage(taxon_id, parent_taxon_id) AS (
                SELECT taxon_id, parent_taxon_id FROM taxa WHERE taxon_id = ?
                UNION ALL
                SELECT parent.taxon_id, parent.parent_taxon_id
                FROM taxa AS parent
                JOIN lineage AS child ON child.parent_taxon_id = parent.taxon_id
            )
            SELECT taxon_id FROM lineage
            "#,
        )?;
        let rows = statement.query_map([taxon_id], |row| row.get::<_, i64>(0))?;
        for ancestor_id in rows {
            *subtree_deltas.entry(ancestor_id?).or_default() += delta;
        }
    }
    let taxon_ids = direct_deltas
        .keys()
        .chain(subtree_deltas.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    for taxon_id in taxon_ids {
        let direct_delta = direct_deltas.get(&taxon_id).copied().unwrap_or_default();
        let subtree_delta = subtree_deltas.get(&taxon_id).copied().unwrap_or_default();
        transaction.execute(
            r#"
            INSERT INTO photo_taxon_usage (taxon_id, direct_photo_count, subtree_photo_count)
            VALUES (?, ?, ?)
            ON CONFLICT(taxon_id) DO UPDATE SET
                direct_photo_count = direct_photo_count + excluded.direct_photo_count,
                subtree_photo_count = subtree_photo_count + excluded.subtree_photo_count
            "#,
            params![taxon_id, direct_delta, subtree_delta],
        )?;
    }
    transaction.execute(
        "DELETE FROM photo_taxon_usage WHERE direct_photo_count = 0 AND subtree_photo_count = 0",
        [],
    )?;
    Ok(())
}

fn mapping_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PhotoTaxonMapping> {
    let status = row.get::<_, String>(2)?;
    Ok(PhotoTaxonMapping {
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

fn input_values(ids: &[i64]) -> (String, Vec<SqlValue>) {
    (
        std::iter::repeat_n("?", ids.len())
            .collect::<Vec<_>>()
            .join(","),
        ids.iter().copied().map(SqlValue::Integer).collect(),
    )
}

fn empty_node() -> MappingNode {
    MappingNode {
        taxon: None,
        photo_ids: Vec::new(),
        children: Vec::new(),
        direct_photo_count: 0,
        subtree_photo_count: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::photos::{open_library, refresh_directory};
    use crate::taxonomy::{TaxonInputRow, TaxonUpdateOptions, apply_rows};
    use std::fs;

    #[test]
    fn maps_the_longest_filename_name_and_builds_sparse_usage() {
        let data = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("Canis_lupus_001.jpg"), b"photo").unwrap();
        let database = Database::open(data.path().join("vividarium.db")).unwrap();
        let rows = [
            TaxonInputRow {
                kingdom: Some("Animalia".into()),
                ..Default::default()
            },
            TaxonInputRow {
                kingdom: Some("Animalia".into()),
                order: Some("Carnivora".into()),
                ..Default::default()
            },
            TaxonInputRow {
                kingdom: Some("Animalia".into()),
                order: Some("Carnivora".into()),
                family: Some("Canidae".into()),
                ..Default::default()
            },
            TaxonInputRow {
                kingdom: Some("Animalia".into()),
                order: Some("Carnivora".into()),
                family: Some("Canidae".into()),
                genus: Some("Canis".into()),
                ..Default::default()
            },
            TaxonInputRow {
                kingdom: Some("Animalia".into()),
                order: Some("Carnivora".into()),
                family: Some("Canidae".into()),
                genus: Some("Canis".into()),
                species: Some("Canis lupus".into()),
                ..Default::default()
            },
        ];
        apply_rows(
            &database,
            &rows,
            TaxonUpdateOptions {
                allow_new_names: true,
                allow_new_taxa: true,
                ..Default::default()
            },
        )
        .unwrap();
        let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
        refresh_directory(&database, library.root_directory_id).unwrap();
        let photo = photos::list_photos(&database).unwrap().remove(0);
        let mapping = get_photo_mapping(&database, photo.photo_id)
            .unwrap()
            .unwrap();
        assert_eq!(mapping.status, PhotoTaxonStatus::Matched);
        let node = get_by_taxon_id(&database, mapping.taxon_id).unwrap();
        assert_eq!(node.direct_photo_count, 1);
        assert_eq!(node.subtree_photo_count, 1);
        assert_eq!(get_root(&database).unwrap().children.len(), 1);
    }
}
