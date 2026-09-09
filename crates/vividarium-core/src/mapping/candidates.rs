use std::collections::BTreeMap;

use rusqlite::{Connection, Transaction, params};

use super::{PhotoMatchedName, PhotoTaxonCandidate};
use crate::taxonomy::{TaxonomyNameType, load_taxon_summaries};
use crate::{CoreError, CoreResult};

pub(super) fn replace(
    transaction: &Transaction<'_>,
    photo_id: i64,
    candidates: &[PhotoTaxonCandidate],
) -> CoreResult<()> {
    clear(transaction, photo_id)?;
    if candidates.len() < 2 {
        return Err(CoreError::Consistency(format!(
            "ambiguous photo {photo_id} has fewer than two candidates"
        )));
    }
    let mut insert_candidate = transaction
        .prepare_cached("INSERT INTO photo_taxon_candidates (photo_id, taxon_id) VALUES (?, ?)")?;
    let mut insert_name = transaction.prepare_cached(
        r#"
        INSERT INTO photo_taxon_candidate_names (
            photo_id, taxon_id, name_id, name_type, name
        ) VALUES (?, ?, ?, ?, ?)
        "#,
    )?;
    for candidate in candidates {
        let taxon_id = candidate.summary.taxon_id;
        insert_candidate.execute(params![photo_id, taxon_id])?;
        if candidate.matched_names.is_empty() {
            return Err(CoreError::Consistency(format!(
                "candidate taxon {taxon_id} for photo {photo_id} has no matched names"
            )));
        }
        for matched_name in &candidate.matched_names {
            insert_name.execute(params![
                photo_id,
                taxon_id,
                matched_name.name_id,
                matched_name.name_type.code(),
                matched_name.name.as_str()
            ])?;
        }
    }
    Ok(())
}

pub(super) fn clear(transaction: &Transaction<'_>, photo_id: i64) -> CoreResult<()> {
    transaction.execute(
        "DELETE FROM photo_taxon_candidates WHERE photo_id = ?",
        [photo_id],
    )?;
    Ok(())
}

pub(super) fn load_matched_names(
    connection: &Connection,
    photo_id: i64,
    taxon_id: i64,
) -> CoreResult<Vec<PhotoMatchedName>> {
    let mut statement = connection.prepare(
        r#"
        SELECT name_id, name_type, name
        FROM photo_taxon_candidate_names
        WHERE photo_id = ? AND taxon_id = ?
        ORDER BY name_type, name_id
        "#,
    )?;
    let rows = statement.query_map(params![photo_id, taxon_id], |row| {
        let name_type = TaxonomyNameType::from_code(row.get(1)?).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Integer,
                Box::new(error),
            )
        })?;
        Ok(PhotoMatchedName {
            name_id: row.get(0)?,
            name_type,
            name: row.get(2)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub(super) fn load(connection: &Connection, photo_id: i64) -> CoreResult<Vec<PhotoTaxonCandidate>> {
    let taxon_ids = {
        let mut statement = connection.prepare(
            r#"
            SELECT taxon_id
            FROM photo_taxon_candidates
            WHERE photo_id = ?
            ORDER BY taxon_id
            "#,
        )?;
        statement
            .query_map([photo_id], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    if taxon_ids.len() < 2 {
        return Err(CoreError::Consistency(format!(
            "ambiguous photo {photo_id} has fewer than two persisted candidates"
        )));
    }
    let mut matched_names = taxon_ids
        .iter()
        .copied()
        .map(|taxon_id| (taxon_id, Vec::new()))
        .collect::<BTreeMap<_, _>>();
    let mut statement = connection.prepare(
        r#"
        SELECT taxon_id, name_id, name_type, name
        FROM photo_taxon_candidate_names
        WHERE photo_id = ?
        ORDER BY taxon_id, name_type, name_id
        "#,
    )?;
    let rows = statement.query_map([photo_id], |row| {
        let name_type = TaxonomyNameType::from_code(row.get(2)?).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Integer,
                Box::new(error),
            )
        })?;
        Ok((
            row.get::<_, i64>(0)?,
            PhotoMatchedName {
                name_id: row.get(1)?,
                name_type,
                name: row.get(3)?,
            },
        ))
    })?;
    for row in rows {
        let (taxon_id, matched_name) = row?;
        matched_names
            .get_mut(&taxon_id)
            .ok_or_else(|| {
                CoreError::Consistency(format!(
                    "photo {photo_id} has a matched name for unknown candidate {taxon_id}"
                ))
            })?
            .push(matched_name);
    }
    if let Some(taxon_id) = matched_names
        .iter()
        .find_map(|(taxon_id, names)| names.is_empty().then_some(*taxon_id))
    {
        return Err(CoreError::Consistency(format!(
            "candidate taxon {taxon_id} for photo {photo_id} has no persisted matched names"
        )));
    }
    let summaries = load_taxon_summaries(connection, &taxon_ids)?;
    Ok(summaries
        .into_iter()
        .map(|summary| PhotoTaxonCandidate {
            accepted_names: summary.names.clone(),
            matched_names: matched_names.remove(&summary.taxon_id).unwrap_or_default(),
            summary,
        })
        .collect())
}
