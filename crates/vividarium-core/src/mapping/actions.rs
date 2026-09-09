use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use super::{
    PhotoMappingDetail, PhotoMappingSummary, PhotoTaxonStatus, apply_usage_deltas, candidates,
    delete_queued_photo_ids, mapping_from_row, matched_names, remap_photo_ids,
};
use crate::models::MappingMetadata;
use crate::taxonomy::{TaxonDisplaySummary, load_taxon_display_summary};
use crate::{CoreError, CoreResult, Database};

pub fn get_metadata(database: &Database) -> CoreResult<MappingMetadata> {
    let connection = database.connect()?;
    let count = |status: &str| -> CoreResult<i64> {
        Ok(connection.query_row(
            r#"
            SELECT COUNT(*)
            FROM photo_taxon_mapping
            WHERE status = ?
              AND NOT EXISTS (
                  SELECT 1
                  FROM photo_mapping_queue
                  WHERE photo_mapping_queue.photo_id =
                        photo_taxon_mapping.photo_id
              )
            "#,
            [status],
            |row| row.get(0),
        )?)
    };
    let mapping_taxa_count = connection.query_row(
        "SELECT COUNT(*) FROM current_photo_taxon_usage WHERE subtree_photo_count > 0",
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

pub fn get_photo_mapping(database: &Database, photo_id: i64) -> CoreResult<PhotoMappingSummary> {
    let connection = database.connect()?;
    get_photo_mapping_from_connection(&connection, photo_id)
}

fn get_photo_mapping_from_connection(
    connection: &Connection,
    photo_id: i64,
) -> CoreResult<PhotoMappingSummary> {
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
        return Err(CoreError::NotFound(format!("photo {photo_id}")));
    }
    let processing = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM photo_mapping_queue WHERE photo_id = ?)",
        [photo_id],
        |row| row.get::<_, bool>(0),
    )?;
    if processing {
        Ok(PhotoMappingSummary {
            photo_id,
            taxon_id: None,
            status: PhotoTaxonStatus::Processing,
        })
    } else {
        stored.ok_or_else(|| {
            CoreError::Consistency(format!(
                "photo {photo_id} has neither a mapping nor a mapping queue entry"
            ))
        })
    }
}

pub fn get_photo_taxon_display_summary(
    database: &Database,
    photo_id: i64,
) -> CoreResult<Option<TaxonDisplaySummary>> {
    let connection = database.connect()?;
    let taxon_id = connection
        .query_row(
            r#"
            SELECT photo_taxon_mapping.taxon_id
            FROM photo_taxon_mapping
            WHERE photo_taxon_mapping.photo_id = ?
              AND photo_taxon_mapping.status = 'matched'
              AND photo_taxon_mapping.taxon_id IS NOT NULL
              AND NOT EXISTS (
                  SELECT 1 FROM photo_mapping_queue
                  WHERE photo_mapping_queue.photo_id = photo_taxon_mapping.photo_id
              )
            "#,
            [photo_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    taxon_id
        .map(|taxon_id| load_taxon_display_summary(&connection, taxon_id))
        .transpose()
        .map(Option::flatten)
}

pub fn get_photo_mapping_detail(
    database: &Database,
    photo_id: i64,
) -> CoreResult<PhotoMappingDetail> {
    let connection = database.connect()?;
    let mapping = get_photo_mapping_from_connection(&connection, photo_id)?;
    let matched_names = if mapping.status == PhotoTaxonStatus::Matched {
        matched_names::load(&connection, photo_id)?
    } else {
        Vec::new()
    };
    let candidates = if mapping.status == PhotoTaxonStatus::Ambiguous {
        candidates::load(&connection, photo_id)?
    } else {
        Vec::new()
    };
    Ok(PhotoMappingDetail {
        mapping,
        matched_names,
        candidates,
    })
}

pub fn set_photo_mapping(
    database: &Database,
    photo_id: i64,
    taxon_id: i64,
) -> CoreResult<PhotoMappingSummary> {
    let mut connection = database.connect()?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    ensure_photo_exists(&transaction, photo_id)?;
    ensure_taxon_exists(&transaction, taxon_id)?;
    let mapping = replace_photo_mapping(&transaction, photo_id, Some(taxon_id))?;
    transaction.commit()?;
    Ok(mapping)
}

pub fn clear_photo_mapping(database: &Database, photo_id: i64) -> CoreResult<PhotoMappingSummary> {
    let mut connection = database.connect()?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    ensure_photo_exists(&transaction, photo_id)?;
    let mapping = replace_photo_mapping(&transaction, photo_id, None)?;
    transaction.commit()?;
    Ok(mapping)
}

pub fn remap_photo(database: &Database, photo_id: i64) -> CoreResult<PhotoMappingSummary> {
    let mut connection = database.connect()?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    ensure_photo_exists(&transaction, photo_id)?;
    remap_photo_ids(&transaction, &[photo_id])?;
    delete_queued_photo_ids(&transaction, &[photo_id])?;
    transaction.commit()?;
    get_photo_mapping(database, photo_id)
}

fn ensure_photo_exists(connection: &rusqlite::Connection, photo_id: i64) -> CoreResult<()> {
    let exists = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM photos WHERE photo_id = ?)",
        [photo_id],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Err(CoreError::NotFound(format!("photo {photo_id}")));
    }
    Ok(())
}

fn ensure_taxon_exists(connection: &rusqlite::Connection, taxon_id: i64) -> CoreResult<()> {
    let exists = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM taxa WHERE taxon_id = ?)",
        [taxon_id],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Err(CoreError::NotFound(format!("taxon {taxon_id}")));
    }
    Ok(())
}

fn replace_photo_mapping(
    transaction: &Transaction<'_>,
    photo_id: i64,
    taxon_id: Option<i64>,
) -> CoreResult<PhotoMappingSummary> {
    let selected_matched_names = taxon_id
        .map(|taxon_id| candidates::load_matched_names(transaction, photo_id, taxon_id))
        .transpose()?
        .unwrap_or_default();
    let old_taxon_id = transaction
        .query_row(
            r#"
            SELECT taxon_id
            FROM photo_taxon_mapping
            WHERE photo_id = ? AND status = 'matched'
            "#,
            [photo_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let status = if taxon_id.is_some() {
        PhotoTaxonStatus::Matched
    } else {
        PhotoTaxonStatus::Unmatched
    };
    transaction.execute(
        r#"
        INSERT INTO photo_taxon_mapping (photo_id, taxon_id, status)
        VALUES (?, ?, ?)
        ON CONFLICT(photo_id) DO UPDATE SET
            taxon_id = excluded.taxon_id,
            status = excluded.status
        "#,
        params![photo_id, taxon_id, status.as_str()],
    )?;
    matched_names::replace(transaction, photo_id, &selected_matched_names)?;
    candidates::clear(transaction, photo_id)?;
    if old_taxon_id != taxon_id {
        let mut deltas = BTreeMap::new();
        if let Some(old_taxon_id) = old_taxon_id {
            deltas.insert(old_taxon_id, -1);
        }
        if let Some(taxon_id) = taxon_id {
            *deltas.entry(taxon_id).or_default() += 1;
        }
        apply_usage_deltas(transaction, &deltas)?;
    }
    delete_queued_photo_ids(transaction, &[photo_id])?;
    Ok(PhotoMappingSummary {
        photo_id,
        taxon_id,
        status,
    })
}
