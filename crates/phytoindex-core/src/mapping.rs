use std::collections::{BTreeMap, BTreeSet, HashMap};

use rusqlite::types::Value as SqlValue;
use rusqlite::{OptionalExtension, Transaction, params, params_from_iter};
use serde::{Deserialize, Serialize};

use crate::db::{Database, taxon_from_row};
use crate::error::{CoreError, CoreResult};
use crate::models::{MappingMetadata, MappingNode, MappingSyncResult, Photo, Taxon};
use crate::photos;
use crate::taxonomy::{
    TaxonDisplayNames, TaxonRank, TaxonSearchResult, TaxonSummary, TaxonomyNameKind, search_taxa,
    search_taxa_with_connection,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PhotoTaxonStatus {
    Matched,
    Unmatched,
    Ambiguous,
    Processing,
    Stale,
}

impl PhotoTaxonStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Matched => "matched",
            Self::Unmatched => "unmatched",
            Self::Ambiguous => "ambiguous",
            Self::Processing => "processing",
            Self::Stale => "stale",
        }
    }

    fn from_str(value: &str) -> CoreResult<Self> {
        match value {
            "matched" => Ok(Self::Matched),
            "unmatched" => Ok(Self::Unmatched),
            "ambiguous" => Ok(Self::Ambiguous),
            "processing" => Ok(Self::Processing),
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
    pub children: Vec<PhotoTaxonUsage>,
    pub subtree_photo_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PhotoTaxonPhotoPage {
    pub items: Vec<Photo>,
    pub next_photo_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PhotoMappingRunResult {
    pub processed: usize,
    pub changed: usize,
    pub pending: i64,
}

const PHOTO_TAXON_CANDIDATE_LIMIT: usize = 500;
const PHOTO_MAPPING_BATCH_SIZE: usize = 200;

pub type MappingProgressCallback<'a> = dyn FnMut(u64, Option<u64>, &str) + Send + 'a;

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
    let processing_photo_count =
        connection.query_row("SELECT COUNT(*) FROM photo_mapping_queue", [], |row| {
            row.get(0)
        })?;
    Ok(MappingMetadata {
        mapped_photo_count: count("matched")?,
        unmatched_photo_count: count("unmatched")?,
        ambiguous_photo_count: count("ambiguous")?,
        processing_photo_count,
        mapping_taxa_count,
    })
}

pub fn get_photo_mapping(
    database: &Database,
    photo_id: i64,
) -> CoreResult<Option<PhotoTaxonMapping>> {
    let connection = database.connect()?;
    let stored = connection
        .query_row(
            "SELECT photo_id, taxon_id, status FROM photo_taxon_mapping WHERE photo_id = ?",
            [photo_id],
            mapping_from_row,
        )
        .optional()?;
    if stored.is_none()
        && !connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM photos WHERE photo_id = ?)",
            [photo_id],
            |row| row.get::<_, bool>(0),
        )?
    {
        return Ok(None);
    }
    let processing = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM photo_mapping_queue WHERE photo_id = ?)",
        [photo_id],
        |row| row.get::<_, bool>(0),
    )?;
    if processing {
        Ok(Some(PhotoTaxonMapping {
            photo_id,
            taxon_id: stored.and_then(|mapping| mapping.taxon_id),
            status: PhotoTaxonStatus::Processing,
        }))
    } else {
        Ok(stored)
    }
}

pub fn get_photo_taxon_match(database: &Database, photo_id: i64) -> CoreResult<PhotoTaxonMatch> {
    let photo = photos::get_photo(database, photo_id)?
        .ok_or_else(|| CoreError::NotFound(format!("photo {photo_id}")))?;
    let connection = database.connect()?;
    let results = search_photo_taxa(&connection, &photo.filename)?;
    let mapping = get_photo_mapping(database, photo_id)?.unwrap_or(PhotoTaxonMapping {
        photo_id,
        taxon_id: None,
        status: unresolved_status(&results),
    });
    let candidates = results.into_iter().map(photo_candidate).collect();
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
    let results = search_photo_taxa(&transaction, &photo.filename)?;
    if !results
        .iter()
        .any(|result| result.summary.taxon_id == taxon_id)
    {
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
    delete_queued_photo_ids(&transaction, &[photo_id])?;
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

pub fn get_photo_taxon_node(
    database: &Database,
    taxon_id: Option<i64>,
    show_empty: bool,
) -> CoreResult<PhotoTaxonNode> {
    let connection = database.connect()?;
    let taxon = match taxon_id {
        Some(taxon_id) => load_usage_taxon(&connection, taxon_id, show_empty)?
            .ok_or_else(|| CoreError::NotFound(format!("photo taxon node {taxon_id}")))?,
        None => {
            let children = load_usage_children(&connection, None, show_empty)?;
            let subtree_photo_count = connection.query_row(
                "SELECT COUNT(*) FROM photo_taxon_mapping WHERE status = 'matched'",
                [],
                |row| row.get(0),
            )?;
            return Ok(PhotoTaxonNode {
                taxon: None,
                children,
                subtree_photo_count,
            });
        }
    };
    let children = load_usage_children(&connection, Some(taxon.taxon_id), show_empty)?;
    let subtree_photo_count = taxon.subtree_photo_count;
    Ok(PhotoTaxonNode {
        taxon: Some(taxon),
        children,
        subtree_photo_count,
    })
}

pub fn list_photos_for_taxon(
    database: &Database,
    taxon_id: Option<i64>,
    include_descendants: bool,
    after_photo_id: Option<i64>,
    limit: usize,
) -> CoreResult<PhotoTaxonPhotoPage> {
    let connection = database.connect()?;
    let limit = limit.clamp(1, 500);
    let fetch_limit = limit + 1;
    let after_photo_id = after_photo_id.unwrap_or_default();
    let suffix = match (taxon_id, include_descendants) {
        (Some(_), true) => {
            r#"
            JOIN photo_taxon_mapping ON photo_taxon_mapping.photo_id = photos.photo_id
            JOIN (
                WITH RECURSIVE descendants(taxon_id) AS (
                    SELECT taxon_id FROM taxa WHERE taxon_id = ?1
                    UNION ALL
                    SELECT child.taxon_id
                    FROM taxa AS child
                    JOIN descendants ON child.parent_taxon_id = descendants.taxon_id
                )
                SELECT taxon_id FROM descendants
            ) AS selected_taxa ON selected_taxa.taxon_id = photo_taxon_mapping.taxon_id
            WHERE photo_taxon_mapping.status = 'matched' AND photos.photo_id > ?2
            ORDER BY photos.photo_id LIMIT ?3
        "#
        }
        (Some(_), false) => {
            r#"
            JOIN photo_taxon_mapping ON photo_taxon_mapping.photo_id = photos.photo_id
            WHERE photo_taxon_mapping.status = 'matched'
              AND photo_taxon_mapping.taxon_id = ?1 AND photos.photo_id > ?2
            ORDER BY photos.photo_id LIMIT ?3
        "#
        }
        (None, _) => {
            r#"
            JOIN photo_taxon_mapping ON photo_taxon_mapping.photo_id = photos.photo_id
            WHERE photo_taxon_mapping.status = 'matched' AND photos.photo_id > ?2
            ORDER BY photos.photo_id LIMIT ?3
        "#
        }
    };
    let sql = photo_query(suffix);
    let mut statement = connection.prepare(&sql)?;
    let rows = match taxon_id {
        Some(taxon_id) => statement.query_map(
            params![taxon_id, after_photo_id, fetch_limit as i64],
            crate::db::photo_from_row,
        )?,
        None => statement.query_map(
            params![SqlValue::Null, after_photo_id, fetch_limit as i64],
            crate::db::photo_from_row,
        )?,
    };
    let mut items = rows.collect::<Result<Vec<_>, _>>()?;
    let next_photo_id = if items.len() > limit {
        items.truncate(limit);
        items.last().map(|photo| photo.photo_id)
    } else {
        None
    };
    Ok(PhotoTaxonPhotoPage {
        items,
        next_photo_id,
    })
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
    transaction.execute("DELETE FROM photo_mapping_queue", [])?;
    remap_photo_ids(&transaction, &ids)?;
    transaction.commit()?;
    mapping_result(database, photos)
}

pub(crate) fn refresh_after_taxonomy_changes(
    database: &Database,
    taxon_ids: impl IntoIterator<Item = i64>,
) -> CoreResult<()> {
    let taxon_ids = taxon_ids.into_iter().collect::<BTreeSet<_>>();
    if taxon_ids.is_empty() {
        return Ok(());
    }
    let mut connection = database.connect()?;
    let transaction = connection.transaction()?;
    let mut photo_ids = BTreeSet::<i64>::new();
    let taxon_ids = taxon_ids.into_iter().collect::<Vec<_>>();
    {
        let selection = id_selection(
            &transaction,
            &taxon_ids,
            "taxon_id",
            "temp_mapping_taxon_ids",
        )?;
        let mut statement = transaction.prepare(&format!(
            r#"
            SELECT photo_id
            FROM photo_taxon_mapping
            WHERE {}
            "#,
            selection.predicate
        ))?;
        let rows = statement.query_map(params_from_iter(selection.values), |row| {
            row.get::<_, i64>(0)
        })?;
        for photo_id in rows {
            photo_ids.insert(photo_id?);
        }
    }
    let photo_ids = photo_ids.into_iter().collect::<Vec<_>>();
    queue_photo_ids(&transaction, &photo_ids, "taxonomy")?;
    transaction.commit()?;
    Ok(())
}

pub(crate) fn refresh_existing_mapped_taxa(database: &Database) -> CoreResult<()> {
    let connection = database.connect()?;
    let taxon_ids = connection
        .prepare(
            r#"
            SELECT DISTINCT taxon_id
            FROM photo_taxon_mapping
            WHERE taxon_id IS NOT NULL
            ORDER BY taxon_id
            "#,
        )?
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(connection);
    refresh_after_taxonomy_changes(database, taxon_ids)
}

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
    let total = connection.query_row("SELECT COUNT(*) FROM photo_mapping_queue", [], |row| {
        row.get::<_, i64>(0)
    })?;
    drop(connection);

    let mut processed = 0usize;
    let mut changed = 0usize;
    progress(0, Some(total as u64), "Matching photo names");
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
        changed += remap_photo_ids(&transaction, &photo_ids)?;
        delete_queued_photo_ids(&transaction, &photo_ids)?;
        transaction.commit()?;
        processed += photo_ids.len();
        progress(processed as u64, Some(total as u64), "Matching photo names");
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

pub(crate) fn remap_photo_ids(
    transaction: &Transaction<'_>,
    photo_ids: &[i64],
) -> CoreResult<usize> {
    if photo_ids.is_empty() {
        return Ok(0);
    }
    let photos = load_photo_names(transaction, photo_ids)?;
    let old_mappings = load_mappings(transaction, photo_ids)?;
    let mut direct_deltas = BTreeMap::<i64, i64>::new();
    let mut changed = 0usize;
    for (photo_id, filename) in photos {
        let results = search_photo_taxa(transaction, &filename)?;
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
            (None, unresolved_status(&results))
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

fn load_usage_taxon(
    connection: &rusqlite::Connection,
    taxon_id: i64,
    show_empty: bool,
) -> CoreResult<Option<PhotoTaxonUsage>> {
    connection
        .query_row(
            &format!(
                "{} WHERE taxa.taxon_id = ? AND (? OR COALESCE(photo_taxon_usage.subtree_photo_count, 0) > 0)",
                usage_taxon_select()
            ),
            params![taxon_id, show_empty],
            usage_taxon_from_row,
        )
        .optional()
        .map_err(Into::into)
}

fn load_usage_children(
    connection: &rusqlite::Connection,
    parent_taxon_id: Option<i64>,
    show_empty: bool,
) -> CoreResult<Vec<PhotoTaxonUsage>> {
    let parent_filter = if parent_taxon_id.is_some() {
        "taxa.parent_taxon_id = ?1"
    } else {
        "taxa.parent_taxon_id IS NULL AND ?1 IS NULL"
    };
    let sql = format!(
        r#"
        {} WHERE {parent_filter}
          AND (?2 OR COALESCE(photo_taxon_usage.subtree_photo_count, 0) > 0)
        ORDER BY taxa.rank, scientific_name, taxa.taxon_id
        "#,
        usage_taxon_select()
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params![parent_taxon_id, show_empty], usage_taxon_from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn usage_taxon_select() -> &'static str {
    r#"
    SELECT taxa.taxon_id, taxa.rank,
           (SELECT name FROM taxon_names
            WHERE taxon_names.taxon_id = taxa.taxon_id
              AND name_kind = 1 AND is_accepted = 1) AS scientific_name,
           (SELECT name FROM taxon_names
            WHERE taxon_names.taxon_id = taxa.taxon_id
              AND name_kind = 2 AND is_accepted = 1) AS english_name,
           (SELECT name FROM taxon_names
            WHERE taxon_names.taxon_id = taxa.taxon_id
              AND name_kind = 3 AND is_accepted = 1) AS chinese_name,
           COALESCE(photo_taxon_usage.direct_photo_count, 0) AS direct_photo_count,
           COALESCE(photo_taxon_usage.subtree_photo_count, 0) AS subtree_photo_count
    FROM taxa
    LEFT JOIN photo_taxon_usage USING (taxon_id)
    "#
}

fn usage_taxon_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PhotoTaxonUsage> {
    let rank = row.get::<_, i64>(1)?;
    Ok(PhotoTaxonUsage {
        taxon_id: row.get(0)?,
        rank: TaxonRank::from_code(rank).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Integer,
                Box::new(error),
            )
        })?,
        names: TaxonDisplayNames {
            scientific: row.get(2)?,
            english: row.get(3)?,
            chinese: row.get(4)?,
        },
        direct_photo_count: row.get(5)?,
        subtree_photo_count: row.get(6)?,
    })
}

fn photo_query(suffix: &str) -> String {
    format!(
        r#"
        SELECT photos.photo_id, photos.directory_id,
               CASE WHEN photo_directories.relative_path = '' THEN photos.filename
                    ELSE photo_directories.relative_path || '/' || photos.filename END AS relative_path,
               photos.filename, photos.file_size, photos.modified_at_ns, photos.thumbnail_path
        FROM photos
        JOIN photo_directories ON photo_directories.directory_id = photos.directory_id
        {suffix}
        "#
    )
}

fn photo_match_query(filename: &str) -> &str {
    std::path::Path::new(filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(filename)
}

fn search_photo_taxa(
    connection: &rusqlite::Connection,
    filename: &str,
) -> CoreResult<Vec<TaxonSearchResult>> {
    search_taxa_with_connection(
        connection,
        photo_match_query(filename),
        PHOTO_TAXON_CANDIDATE_LIMIT,
    )
}

fn unresolved_status(results: &[TaxonSearchResult]) -> PhotoTaxonStatus {
    if results.is_empty() {
        PhotoTaxonStatus::Unmatched
    } else {
        PhotoTaxonStatus::Ambiguous
    }
}

fn photo_candidate(result: TaxonSearchResult) -> PhotoTaxonCandidate {
    PhotoTaxonCandidate {
        accepted_names: result.summary.names.clone(),
        summary: result.summary,
        matched_names: result
            .matches
            .into_iter()
            .map(|name| PhotoMatchedName {
                name_id: name.name_id,
                name_kind: name.name_kind,
                name: name.name,
                is_accepted: name.is_accepted,
            })
            .collect(),
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
    use crate::taxonomy::{
        TaxonInputRow, TaxonNameInput, TaxonUpdateInput, TaxonUpdateOptions, apply_rows,
        execute_custom_taxonomy_sql, update_taxon,
    };
    use std::fs;

    #[test]
    fn matches_the_filename_stem_and_builds_sparse_usage() {
        let data = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("Canis lupus.jpg"), b"photo").unwrap();
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
        let mut progress = |_: u64, _: Option<u64>, _: &str| {};
        process_pending_photo_matches(&database, &mut progress).unwrap();
        let photo = photos::list_photos(&database).unwrap().remove(0);
        let matched = get_photo_taxon_match(&database, photo.photo_id).unwrap();
        assert_eq!(matched.mapping.status, PhotoTaxonStatus::Ambiguous);
        let species_id = matched
            .candidates
            .iter()
            .find(|candidate| candidate.summary.names.scientific.as_deref() == Some("Canis lupus"))
            .unwrap()
            .summary
            .taxon_id;
        let mapping = select_photo_taxon(&database, photo.photo_id, species_id).unwrap();
        assert_eq!(mapping.status, PhotoTaxonStatus::Matched);
        let node = get_by_taxon_id(&database, mapping.taxon_id).unwrap();
        assert_eq!(node.direct_photo_count, 1);
        assert_eq!(node.subtree_photo_count, 1);
        assert_eq!(get_root(&database).unwrap().children.len(), 1);
        let sparse_root = get_photo_taxon_node(&database, None, false).unwrap();
        assert_eq!(sparse_root.children.len(), 1);
        assert_eq!(sparse_root.subtree_photo_count, 1);
        let page = list_photos_for_taxon(&database, mapping.taxon_id, true, None, 20).unwrap();
        assert_eq!(page.items, vec![photo]);
        assert_eq!(page.next_photo_id, None);
        execute_custom_taxonomy_sql(
            &database,
            "UPDATE taxon_names SET name = 'Canis lycaon' WHERE name = 'Canis lupus'",
            None,
        )
        .unwrap();
        process_pending_photo_matches(&database, &mut progress).unwrap();
        let old_taxon_id = mapping.taxon_id;
        let mapping = get_photo_mapping(&database, mapping.photo_id)
            .unwrap()
            .unwrap();
        assert_eq!(mapping.status, PhotoTaxonStatus::Unmatched);
        assert_eq!(mapping.taxon_id, None);
        assert!(get_photo_taxon_node(&database, old_taxon_id, false).is_err());
    }

    #[test]
    fn accepts_a_user_choice_only_from_ambiguous_candidates() {
        let data = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("Shared name.jpg"), b"photo").unwrap();
        let database = Database::open(data.path().join("vividarium.db")).unwrap();
        let connection = database.connect().unwrap();
        for _ in 0..2 {
            connection
                .execute("INSERT INTO taxa (rank) VALUES (5)", [])
                .unwrap();
            let taxon_id = connection.last_insert_rowid();
            connection
                .execute(
                    r#"
                    INSERT INTO taxon_names (taxon_id, name_kind, name, is_accepted)
                    VALUES (?, 1, 'Shared name', 1)
                    "#,
                    [taxon_id],
                )
                .unwrap();
        }
        let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
        refresh_directory(&database, library.root_directory_id).unwrap();
        let photo = photos::list_photos(&database).unwrap().remove(0);
        assert_eq!(
            get_photo_mapping(&database, photo.photo_id)
                .unwrap()
                .unwrap()
                .status,
            PhotoTaxonStatus::Processing
        );
        let mut progress = |_: u64, _: Option<u64>, _: &str| {};
        process_pending_photo_matches(&database, &mut progress).unwrap();
        let matched = get_photo_taxon_match(&database, photo.photo_id).unwrap();
        assert_eq!(matched.mapping.status, PhotoTaxonStatus::Ambiguous);
        assert_eq!(matched.candidates.len(), 2);
        let selected_taxon_id = matched.candidates[0].summary.taxon_id;
        let selected = select_photo_taxon(&database, photo.photo_id, selected_taxon_id).unwrap();
        assert_eq!(selected.status, PhotoTaxonStatus::Matched);
        assert_eq!(selected.taxon_id, Some(selected_taxon_id));
        let error = select_photo_taxon(&database, photo.photo_id, i64::MAX).unwrap_err();
        assert!(error.to_string().contains("not a filename candidate"));
    }

    #[test]
    fn does_not_synthesize_processing_for_a_missing_photo() {
        let data = tempfile::tempdir().unwrap();
        let database = Database::open(data.path().join("vividarium.db")).unwrap();

        assert_eq!(get_photo_mapping(&database, 404).unwrap(), None);
    }

    #[test]
    fn queues_a_photo_when_its_selected_taxon_is_deleted() {
        let data = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("Felis catus.jpg"), b"photo").unwrap();
        let database = Database::open(data.path().join("vividarium.db")).unwrap();
        let connection = database.connect().unwrap();
        connection
            .execute("INSERT INTO taxa (rank) VALUES (5)", [])
            .unwrap();
        let taxon_id = connection.last_insert_rowid();
        connection
            .execute(
                r#"
                INSERT INTO taxon_names (taxon_id, name_kind, name, is_accepted)
                VALUES (?, 1, 'Felis catus', 1)
                "#,
                [taxon_id],
            )
            .unwrap();
        drop(connection);
        let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
        refresh_directory(&database, library.root_directory_id).unwrap();
        let photo = photos::list_photos(&database).unwrap().remove(0);
        let mut progress = |_: u64, _: Option<u64>, _: &str| {};
        process_pending_photo_matches(&database, &mut progress).unwrap();
        select_photo_taxon(&database, photo.photo_id, taxon_id).unwrap();

        crate::taxonomy::delete_taxon(&database, taxon_id).unwrap();

        assert_eq!(
            get_photo_mapping(&database, photo.photo_id)
                .unwrap()
                .unwrap()
                .status,
            PhotoTaxonStatus::Processing
        );
        process_pending_photo_matches(&database, &mut progress).unwrap();
        assert_eq!(
            get_photo_mapping(&database, photo.photo_id)
                .unwrap()
                .unwrap()
                .status,
            PhotoTaxonStatus::Unmatched
        );
    }

    #[test]
    fn taxonomy_update_queues_only_affected_photos() {
        let data = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("Canis lupus.jpg"), b"photo").unwrap();
        fs::write(root.path().join("Felis catus.jpg"), b"photo").unwrap();
        fs::write(root.path().join("domestic cat.jpg"), b"photo").unwrap();
        let database = Database::open(data.path().join("vividarium.db")).unwrap();
        let connection = database.connect().unwrap();
        connection
            .execute("INSERT INTO taxa (rank) VALUES (5)", [])
            .unwrap();
        let canis_taxon_id = connection.last_insert_rowid();
        connection
            .execute(
                r#"
                INSERT INTO taxon_names (taxon_id, name_kind, name, is_accepted)
                VALUES (?, 1, 'Canis lupus', 1)
                "#,
                [canis_taxon_id],
            )
            .unwrap();
        connection
            .execute("INSERT INTO taxa (rank) VALUES (5)", [])
            .unwrap();
        let felis_taxon_id = connection.last_insert_rowid();
        connection
            .execute(
                r#"
                INSERT INTO taxon_names (taxon_id, name_kind, name, is_accepted)
                VALUES (?, 1, 'Felis catus', 1)
                "#,
                [felis_taxon_id],
            )
            .unwrap();
        drop(connection);
        let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
        refresh_directory(&database, library.root_directory_id).unwrap();
        let mut progress = |_: u64, _: Option<u64>, _: &str| {};
        process_pending_photo_matches(&database, &mut progress).unwrap();
        let photos = photos::list_photos(&database).unwrap();
        let canis_photo = photos
            .iter()
            .find(|photo| photo.filename == "Canis lupus.jpg")
            .unwrap();
        let felis_photo = photos
            .iter()
            .find(|photo| photo.filename == "Felis catus.jpg")
            .unwrap();
        let domestic_cat_photo = photos
            .iter()
            .find(|photo| photo.filename == "domestic cat.jpg")
            .unwrap();
        select_photo_taxon(&database, canis_photo.photo_id, canis_taxon_id).unwrap();
        select_photo_taxon(&database, felis_photo.photo_id, felis_taxon_id).unwrap();

        update_taxon(
            &database,
            TaxonUpdateInput {
                taxon_id: felis_taxon_id,
                geological_range: None,
                scientific: None,
                english: Some(TaxonNameInput {
                    name: "domestic cat".into(),
                    is_accepted: Some(true),
                    ..Default::default()
                }),
                chinese: None,
            },
            TaxonUpdateOptions {
                allow_new_names: true,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(
            get_photo_mapping(&database, felis_photo.photo_id)
                .unwrap()
                .unwrap()
                .status,
            PhotoTaxonStatus::Processing
        );
        assert_eq!(
            get_photo_mapping(&database, canis_photo.photo_id)
                .unwrap()
                .unwrap()
                .status,
            PhotoTaxonStatus::Matched
        );
        assert_eq!(
            get_photo_mapping(&database, domestic_cat_photo.photo_id)
                .unwrap()
                .unwrap()
                .status,
            PhotoTaxonStatus::Unmatched
        );
        assert_eq!(get_metadata(&database).unwrap().processing_photo_count, 1);
    }

    #[test]
    fn batches_usage_deltas_for_shared_ancestors() {
        let data = tempfile::tempdir().unwrap();
        let database = Database::open(data.path().join("vividarium.db")).unwrap();
        let mut connection = database.connect().unwrap();
        connection
            .execute("INSERT INTO taxa (rank) VALUES (1)", [])
            .unwrap();
        let root_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO taxa (parent_taxon_id, rank) VALUES (?, 2)",
                [root_id],
            )
            .unwrap();
        let first_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO taxa (parent_taxon_id, rank) VALUES (?, 2)",
                [root_id],
            )
            .unwrap();
        let second_id = connection.last_insert_rowid();
        let transaction = connection.transaction().unwrap();
        let deltas = BTreeMap::from([(first_id, 1), (second_id, 1)]);

        apply_usage_deltas(&transaction, &deltas).unwrap();

        assert_eq!(
            transaction
                .query_row(
                    "SELECT subtree_photo_count FROM photo_taxon_usage WHERE taxon_id = ?",
                    [root_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            2
        );
        assert_eq!(
            transaction
                .query_row(
                    "SELECT SUM(direct_photo_count) FROM photo_taxon_usage WHERE taxon_id IN (?, ?)",
                    params![first_id, second_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            2
        );
    }

    #[test]
    fn large_id_sets_use_a_temporary_table() {
        let data = tempfile::tempdir().unwrap();
        let database = Database::open(data.path().join("vividarium.db")).unwrap();
        let mut connection = database.connect().unwrap();
        let transaction = connection.transaction().unwrap();
        let ids = (1..=501).collect::<Vec<_>>();
        let selection =
            id_selection(&transaction, &ids, "photo_id", "temp_mapping_photo_ids").unwrap();
        assert!(selection.values.is_empty());
        assert_eq!(
            transaction
                .query_row("SELECT COUNT(*) FROM temp_mapping_photo_ids", [], |row| row
                    .get::<_, i64>(0),)
                .unwrap(),
            501
        );
        assert!(selection.predicate.contains("temp_mapping_photo_ids"));
    }
}
