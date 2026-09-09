use rusqlite::{Connection, Transaction, params};

use super::PhotoMatchedName;
use crate::CoreResult;
use crate::taxonomy::TaxonomyNameType;

pub(super) fn replace(
    transaction: &Transaction<'_>,
    photo_id: i64,
    matched_names: &[PhotoMatchedName],
) -> CoreResult<()> {
    transaction.execute(
        "DELETE FROM photo_taxon_mapping_names WHERE photo_id = ?",
        [photo_id],
    )?;
    let mut statement = transaction.prepare_cached(
        r#"
        INSERT INTO photo_taxon_mapping_names (
            photo_id, name_id, name_type, name
        ) VALUES (?, ?, ?, ?)
        "#,
    )?;
    for matched_name in matched_names {
        statement.execute(params![
            photo_id,
            matched_name.name_id,
            matched_name.name_type.code(),
            matched_name.name.as_str(),
        ])?;
    }
    Ok(())
}

pub(super) fn load(connection: &Connection, photo_id: i64) -> CoreResult<Vec<PhotoMatchedName>> {
    let mut statement = connection.prepare(
        r#"
        SELECT name_id, name_type, name
        FROM photo_taxon_mapping_names
        WHERE photo_id = ?
        ORDER BY name_type, name_id
        "#,
    )?;
    let rows = statement.query_map([photo_id], |row| {
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
