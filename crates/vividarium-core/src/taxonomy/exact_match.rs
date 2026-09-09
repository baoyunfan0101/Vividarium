use rusqlite::{Connection, params};

use super::{TaxonRank, TaxonomyNameType};
use crate::CoreResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExactTaxonomyNameMatch {
    pub(crate) taxon_id: i64,
    pub(crate) name_id: i64,
    pub(crate) name_type: TaxonomyNameType,
    pub(crate) name: String,
}

pub(crate) fn match_exact_taxonomy_name(
    connection: &Connection,
    name: &str,
    rank: TaxonRank,
    name_type: TaxonomyNameType,
) -> CoreResult<Vec<ExactTaxonomyNameMatch>> {
    let mut statement = connection.prepare(
        r#"
            SELECT taxa.taxon_id, taxon_names.name_id, taxon_names.name_type,
                   taxon_names.name
            FROM taxon_names
            JOIN taxa USING (taxon_id)
            WHERE taxon_names.name = ? COLLATE BINARY
              AND taxon_names.name_type = ?
              AND taxa.rank = ?
            ORDER BY taxa.taxon_id, taxon_names.name_id
            "#,
    )?;
    statement
        .query_map(params![name, name_type.code(), rank.code()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .map(|row| {
            let (taxon_id, name_id, name_type, name) = row?;
            Ok(ExactTaxonomyNameMatch {
                taxon_id,
                name_id,
                name_type: TaxonomyNameType::from_code(name_type)?,
                name,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;

    #[test]
    fn matches_exact_name_rank_and_single_name_type() {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open_test(directory.path().join("metadata.db")).unwrap();
        let connection = database.connect_taxonomy_metadata_context().unwrap();
        connection
            .execute_batch(
                r#"
                PRAGMA foreign_keys = OFF;
                INSERT INTO taxa (taxon_id, rank) VALUES
                    (1, 4),
                    (2, 5),
                    (3, 5),
                    (4, 5);
                INSERT INTO taxa (taxon_id, parent_taxon_id, rank)
                    VALUES (5, 99, 5);
                INSERT INTO taxon_names (taxon_id, name_type, name) VALUES
                    (1, 1, 'Exact Name'),
                    (2, 1, 'Exact Name'),
                    (3, 2, 'Exact Name'),
                    (4, 1, 'exact name'),
                    (5, 1, 'Orphan Name');
                "#,
            )
            .unwrap();

        let matches = match_exact_taxonomy_name(
            &connection,
            "Exact Name",
            TaxonRank::Species,
            TaxonomyNameType::SciName,
        )
        .unwrap();

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].taxon_id, 2);
        assert!(matches[0].name_id > 0);
        assert_eq!(matches[0].name_type, TaxonomyNameType::SciName);
        assert_eq!(matches[0].name, "Exact Name");
        assert_eq!(
            match_exact_taxonomy_name(
                &connection,
                "Exact Name",
                TaxonRank::Genus,
                TaxonomyNameType::SciName,
            )
            .unwrap()[0]
                .taxon_id,
            1
        );
        assert_eq!(
            match_exact_taxonomy_name(
                &connection,
                "Exact Name",
                TaxonRank::Species,
                TaxonomyNameType::Synonym,
            )
            .unwrap()[0]
                .taxon_id,
            3
        );
        assert!(
            match_exact_taxonomy_name(
                &connection,
                "EXACT NAME",
                TaxonRank::Species,
                TaxonomyNameType::SciName,
            )
            .unwrap()
            .is_empty()
        );

        let orphan = match_exact_taxonomy_name(
            &connection,
            "Orphan Name",
            TaxonRank::Species,
            TaxonomyNameType::SciName,
        )
        .unwrap();
        assert_eq!(orphan.len(), 1);
        assert_eq!(orphan[0].taxon_id, 5);
        assert_eq!(orphan[0].name, "Orphan Name");
    }
}
