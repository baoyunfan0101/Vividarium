use chrono::Local;
use rusqlite::{OptionalExtension, Transaction, params};

use crate::db::{Database, taxon_from_row};
use crate::error::{CoreError, CoreResult};
use crate::models::{MappingMetadata, MappingNode, MappingSyncResult, Photo, Taxon};
use crate::photos::{self, ProgressCallback};

const SPECIAL_UNMAPPED_TAXON_ID: i64 = 0;

pub fn get_metadata(database: &Database) -> CoreResult<MappingMetadata> {
    let connection = database.connect()?;
    let values = connection
        .query_row(
            r#"
            SELECT last_synced_at, photos_last_synced_at, taxa_last_synced_at
            FROM photos_taxa_mapping_metadata LIMIT 1
            "#,
            [],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?
        .unwrap_or((None, None, None));
    let mapped_photo_count = connection.query_row(
        "SELECT COUNT(*) FROM photos_taxa_mapping WHERE taxon_id != ?",
        [SPECIAL_UNMAPPED_TAXON_ID],
        |row| row.get(0),
    )?;
    let mapping_taxa_count =
        connection.query_row("SELECT COUNT(*) FROM photos_taxa_mapping_taxa", [], |row| {
            row.get(0)
        })?;
    Ok(MappingMetadata {
        last_synced_at: values.0,
        photos_last_synced_at: values.1,
        taxa_last_synced_at: values.2,
        mapped_photo_count,
        mapping_taxa_count,
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
                "SELECT * FROM photos_taxa_mapping_taxa WHERE taxon_id = ?",
                [id],
                taxon_from_row,
            )
            .optional()?,
        None => None,
    };
    let photo_ids = match taxon_id {
        Some(id) => {
            let mut statement = connection.prepare(
                "SELECT photo_id FROM photos_taxa_mapping WHERE taxon_id = ? ORDER BY photo_id",
            )?;
            let rows = statement.query_map([id], |row| row.get::<_, i64>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        }
        None => Vec::new(),
    };
    let children = if let Some(id) = taxon_id {
        let mut statement = connection.prepare(
            "SELECT * FROM photos_taxa_mapping_taxa WHERE parent_id = ? ORDER BY rank, name, taxon_id",
        )?;
        let rows = statement.query_map([id], taxon_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()?
    } else {
        let mut statement = connection.prepare(
            "SELECT * FROM photos_taxa_mapping_taxa WHERE parent_id IS NULL AND rank = 'ordo' ORDER BY name, taxon_id",
        )?;
        let rows = statement.query_map([], taxon_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    Ok(MappingNode {
        taxon,
        photo_ids,
        children,
    })
}

pub fn get_by_binomial(database: &Database, name: &str) -> CoreResult<MappingNode> {
    let connection = database.connect()?;
    let id = connection
        .query_row(
            "SELECT taxon_id FROM photos_taxa_mapping_taxa WHERE binomial_name = ? ORDER BY taxon_id",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    drop(connection);
    match id {
        Some(id) => get_by_taxon_id(database, Some(id)),
        None => Ok(empty_node()),
    }
}

pub fn get_by_name(database: &Database, name: &str) -> CoreResult<MappingNode> {
    let connection = database.connect()?;
    let id = connection
        .query_row(
            "SELECT taxon_id FROM photos_taxa_mapping_taxa WHERE name = ? ORDER BY taxon_id",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    drop(connection);
    match id {
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
    let field = match mode {
        "name" => "name",
        "binomial" => "binomial_name",
        value => {
            return Err(CoreError::InvalidArgument(format!(
                "invalid suggestion mode: {value}"
            )));
        }
    };
    let escaped = query
        .trim()
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    if escaped.is_empty() {
        return Ok(Vec::new());
    }
    let connection = database.connect()?;
    let sql = format!(
        r#"
        SELECT * FROM photos_taxa_mapping_taxa
        WHERE {field} IS NOT NULL AND {field} LIKE ? ESCAPE '\'
        ORDER BY CASE WHEN {field} LIKE ? ESCAPE '\' THEN 0 ELSE 1 END,
                 rank, {field}, taxon_id
        LIMIT ?
        "#,
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        params![format!("%{escaped}%"), format!("{escaped}%"), limit as i64],
        taxon_from_row,
    )?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn update_mapping(
    database: &Database,
    progress: &mut ProgressCallback<'_>,
) -> CoreResult<MappingSyncResult> {
    let photos = photos::list_changed_photos(database)?;
    sync_photos(database, &photos, false, progress)
}

pub fn rebuild_mapping(
    database: &Database,
    progress: &mut ProgressCallback<'_>,
) -> CoreResult<MappingSyncResult> {
    let photos = photos::list_photos(database)?;
    sync_photos(database, &photos, true, progress)
}

fn sync_photos(
    database: &Database,
    photos: &[Photo],
    rebuild: bool,
    progress: &mut ProgressCallback<'_>,
) -> CoreResult<MappingSyncResult> {
    progress(0, Some(photos.len() as u64), "Mapping photos");
    let mut connection = database.connect()?;
    let transaction = connection.transaction()?;
    let orphan_mappings_deleted = if rebuild {
        transaction.execute("DELETE FROM photos_taxa_mapping", [])?;
        transaction.execute("DELETE FROM photos_taxa_mapping_taxa", [])?;
        0
    } else {
        transaction.execute(
            r#"
            DELETE FROM photos_taxa_mapping
            WHERE NOT EXISTS (
                SELECT 1 FROM photos
                WHERE photos.photo_id = photos_taxa_mapping.photo_id
            )
            "#,
            [],
        )?
    };
    let mut unmapped_photos = Vec::new();
    for (index, photo) in photos.iter().enumerate() {
        let taxon_id = taxon_id_for_photo(&transaction, photo)?;
        if taxon_id == SPECIAL_UNMAPPED_TAXON_ID {
            unmapped_photos.push(photo.clone());
        }
        transaction.execute(
            r#"
            INSERT INTO photos_taxa_mapping (photo_id, taxon_id) VALUES (?, ?)
            ON CONFLICT(photo_id) DO UPDATE SET taxon_id = excluded.taxon_id
            "#,
            params![photo.photo_id, taxon_id],
        )?;
        progress(
            (index + 1) as u64,
            Some(photos.len() as u64),
            "Mapping photos",
        );
    }
    let photos_last_synced_at = transaction.query_row(
        "SELECT MAX(last_synced_at) FROM photos_metadata",
        [],
        |row| row.get::<_, Option<String>>(0),
    )?;
    let taxa_last_synced_at = transaction
        .query_row(
            "SELECT last_synced_at FROM taxa_metadata LIMIT 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    let now = Local::now().format("%Y-%m-%d %H:%M:%S%.6f").to_string();
    transaction.execute("DELETE FROM photos_taxa_mapping_metadata", [])?;
    transaction.execute(
        r#"
        INSERT INTO photos_taxa_mapping_metadata (
            last_synced_at, photos_last_synced_at, taxa_last_synced_at
        ) VALUES (?, ?, ?)
        "#,
        params![now, photos_last_synced_at, taxa_last_synced_at],
    )?;
    transaction.commit()?;
    Ok(MappingSyncResult {
        processed: photos.len(),
        mapped: photos.len(),
        unmapped: unmapped_photos.len(),
        unmapped_photos,
        orphan_mappings_deleted,
    })
}

fn taxon_id_for_photo(transaction: &Transaction<'_>, photo: &Photo) -> CoreResult<i64> {
    let Some(binomial_name) = photo.binomial_name.as_deref() else {
        return Ok(SPECIAL_UNMAPPED_TAXON_ID);
    };
    if let Some(id) = transaction
        .query_row(
            "SELECT taxon_id FROM photos_taxa_mapping_taxa WHERE binomial_name = ? ORDER BY taxon_id",
            [binomial_name],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    let Some(source_id) = transaction
        .query_row(
            "SELECT taxon_id FROM taxa WHERE binomial_name = ? ORDER BY taxon_id",
            [binomial_name],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
    else {
        return Ok(SPECIAL_UNMAPPED_TAXON_ID);
    };
    let mut lineage = Vec::new();
    let mut current_id = Some(source_id);
    while let Some(id) = current_id {
        let taxon = transaction.query_row(
            "SELECT * FROM taxa WHERE taxon_id = ?",
            [id],
            taxon_from_row,
        )?;
        current_id = taxon.parent_id;
        lineage.push(taxon);
    }
    lineage.reverse();
    for taxon in lineage {
        transaction.execute(
            r#"
            INSERT INTO photos_taxa_mapping_taxa (
                taxon_id, rank, name, parent_id, binomial_name
            ) VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(taxon_id) DO UPDATE SET rank = excluded.rank,
                name = excluded.name, parent_id = excluded.parent_id,
                binomial_name = excluded.binomial_name
            "#,
            params![
                taxon.taxon_id,
                taxon.rank,
                taxon.name,
                taxon.parent_id,
                taxon.binomial_name
            ],
        )?;
    }
    Ok(source_id)
}

fn empty_node() -> MappingNode {
    MappingNode {
        taxon: None,
        photo_ids: Vec::new(),
        children: Vec::new(),
    }
}
