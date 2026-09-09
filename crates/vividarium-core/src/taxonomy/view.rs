use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::{
    TaxonRank, TaxonomyNameType,
    page::{
        TaxonomyCursor, TaxonomyPage, decode_cursor, encode_cursor, invalid_cursor, page_limit,
    },
};
use crate::{CoreError, CoreResult, Database};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonDisplayNames {
    pub sci_name: Option<String>,
    pub zh_name: Option<String>,
    pub en_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonBreadcrumbItem {
    pub taxon_id: i64,
    pub rank: TaxonRank,
    pub names: TaxonDisplayNames,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonSummary {
    pub taxon_id: i64,
    pub rank: TaxonRank,
    pub breadcrumb: Vec<TaxonBreadcrumbItem>,
    pub names: TaxonDisplayNames,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonDisplayItem {
    pub taxon_id: i64,
    pub rank: TaxonRank,
    pub names: TaxonDisplayNames,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonDisplaySummary {
    pub current_rank: TaxonRank,
    pub items: Vec<TaxonDisplayItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonChild {
    pub taxon_id: i64,
    pub rank: TaxonRank,
    pub names: TaxonDisplayNames,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonNameDetail {
    pub name_id: i64,
    pub name: String,
    pub authority_year: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonNamesDetail {
    pub sci_name: Option<TaxonNameDetail>,
    pub synonyms: Vec<TaxonNameDetail>,
    pub zh_name: Option<TaxonNameDetail>,
    pub zh_aliases: Vec<TaxonNameDetail>,
    pub en_name: Option<TaxonNameDetail>,
    pub en_aliases: Vec<TaxonNameDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonDetail {
    pub taxon_id: i64,
    pub rank: TaxonRank,
    pub parent_taxon_id: Option<i64>,
    pub breadcrumb: Vec<TaxonBreadcrumbItem>,
    pub geological_range: Option<String>,
    pub names: TaxonNamesDetail,
}

pub fn get_taxon_summary(database: &Database, taxon_id: i64) -> CoreResult<Option<TaxonSummary>> {
    load_taxon_summary(&database.connect_taxonomy_metadata_context()?, taxon_id)
}

pub fn get_taxon_display_summary(
    database: &Database,
    taxon_id: i64,
) -> CoreResult<Option<TaxonDisplaySummary>> {
    load_taxon_display_summary(&database.connect_taxonomy_metadata_context()?, taxon_id)
}

pub fn get_taxon_detail(database: &Database, taxon_id: i64) -> CoreResult<Option<TaxonDetail>> {
    load_taxon_detail(&database.connect_taxonomy_metadata_context()?, taxon_id)
}

pub fn list_taxon_children(
    database: &Database,
    taxon_id: i64,
    cursor: Option<&str>,
    limit: usize,
) -> CoreResult<TaxonomyPage<TaxonChild>> {
    load_taxon_children(
        &database.connect_taxonomy_metadata_context()?,
        taxon_id,
        cursor,
        limit,
    )
}

pub(super) fn load_taxon_summary(
    connection: &Connection,
    taxon_id: i64,
) -> CoreResult<Option<TaxonSummary>> {
    let Some((rank, parent_taxon_id)) = load_taxon_base(connection, taxon_id)? else {
        return Ok(None);
    };
    let names = load_display_names(connection, taxon_id)?;
    let breadcrumb = load_breadcrumb(connection, taxon_id, parent_taxon_id)?;
    Ok(Some(TaxonSummary {
        taxon_id,
        rank,
        breadcrumb,
        names,
    }))
}

pub(crate) fn load_taxon_display_summary(
    connection: &Connection,
    taxon_id: i64,
) -> CoreResult<Option<TaxonDisplaySummary>> {
    let mut statement = connection.prepare(
        r#"
        WITH RECURSIVE lineage(taxon_id, rank, parent_taxon_id) AS (
            SELECT taxon_id, rank, parent_taxon_id
            FROM taxa
            WHERE taxon_id = ?1
            UNION
            SELECT parent.taxon_id, parent.rank, parent.parent_taxon_id
            FROM taxa parent
            JOIN lineage child ON child.parent_taxon_id = parent.taxon_id
        ),
        current AS (
            SELECT rank FROM lineage WHERE taxon_id = ?1
        )
        SELECT
            lineage.taxon_id,
            lineage.rank,
            current.rank,
            MAX(CASE WHEN taxon_names.name_type = 1 THEN taxon_names.name END),
            MAX(CASE WHEN taxon_names.name_type = 3 THEN taxon_names.name END),
            MAX(CASE WHEN taxon_names.name_type = 5 THEN taxon_names.name END)
        FROM lineage
        CROSS JOIN current
        LEFT JOIN taxon_names
          ON taxon_names.taxon_id = lineage.taxon_id
         AND taxon_names.name_type IN (1, 3, 5)
        WHERE lineage.taxon_id = ?1
           OR (current.rank >= 3 AND lineage.rank >= 3)
        GROUP BY lineage.taxon_id, lineage.rank, current.rank
        ORDER BY lineage.rank, lineage.taxon_id
        "#,
    )?;
    let rows = statement.query_map([taxon_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    })?;
    let mut current_rank = None;
    let mut items = Vec::new();
    for row in rows {
        let (item_taxon_id, rank, current, sci_name, zh_name, en_name) = row?;
        current_rank = Some(TaxonRank::from_code(current)?);
        items.push(TaxonDisplayItem {
            taxon_id: item_taxon_id,
            rank: TaxonRank::from_code(rank)?,
            names: TaxonDisplayNames {
                sci_name,
                zh_name,
                en_name,
            },
        });
    }
    Ok(current_rank.map(|current_rank| TaxonDisplaySummary {
        current_rank,
        items,
    }))
}

fn load_breadcrumb(
    connection: &Connection,
    taxon_id: i64,
    parent_taxon_id: Option<i64>,
) -> CoreResult<Vec<TaxonBreadcrumbItem>> {
    let mut breadcrumb = Vec::new();
    let mut current = parent_taxon_id;
    let mut seen = HashSet::from([taxon_id]);
    while let Some(parent_id) = current {
        if !seen.insert(parent_id) {
            return Err(CoreError::InvalidArgument(
                "taxonomy hierarchy contains a cycle".into(),
            ));
        }
        let Some((parent_rank, next_parent)) = load_taxon_base(connection, parent_id)? else {
            return Err(CoreError::InvalidArgument(format!(
                "taxon {taxon_id} references missing parent {parent_id}"
            )));
        };
        breadcrumb.push(TaxonBreadcrumbItem {
            taxon_id: parent_id,
            rank: parent_rank,
            names: load_display_names(connection, parent_id)?,
        });
        current = next_parent;
    }
    breadcrumb.reverse();
    Ok(breadcrumb)
}

pub(crate) fn load_taxon_summaries(
    connection: &Connection,
    taxon_ids: &[i64],
) -> CoreResult<Vec<TaxonSummary>> {
    taxon_ids
        .iter()
        .map(|taxon_id| {
            load_taxon_summary(connection, *taxon_id)?.ok_or_else(|| {
                CoreError::InvalidArgument(format!("taxon {taxon_id} no longer exists"))
            })
        })
        .collect()
}

fn load_taxon_detail(connection: &Connection, taxon_id: i64) -> CoreResult<Option<TaxonDetail>> {
    let base = connection
        .query_row(
            "SELECT rank, parent_taxon_id, geological_range FROM taxa WHERE taxon_id = ?",
            [taxon_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((rank, parent_taxon_id, geological_range)) = base else {
        return Ok(None);
    };
    Ok(Some(TaxonDetail {
        taxon_id,
        rank: TaxonRank::from_code(rank)?,
        parent_taxon_id,
        breadcrumb: load_breadcrumb(connection, taxon_id, parent_taxon_id)?,
        geological_range,
        names: load_name_details(connection, taxon_id)?,
    }))
}

fn load_taxon_children(
    connection: &Connection,
    parent_taxon_id: i64,
    cursor: Option<&str>,
    limit: usize,
) -> CoreResult<TaxonomyPage<TaxonChild>> {
    let cursor = match decode_cursor(cursor)? {
        Some(TaxonomyCursor::TaxonChildren {
            parent_taxon_id: cursor_parent,
            rank,
            taxon_id,
        }) if cursor_parent == parent_taxon_id => Some((rank, taxon_id)),
        Some(_) => return Err(invalid_cursor()),
        None => None,
    };
    let limit = page_limit(limit);
    let mut rows = match cursor {
        Some((rank, taxon_id)) => {
            let mut statement = connection.prepare(
                r#"
                SELECT taxon_id, rank
                FROM taxa
                WHERE parent_taxon_id = ?
                  AND (rank > ? OR (rank = ? AND taxon_id > ?))
                ORDER BY rank, taxon_id
                LIMIT ?
                "#,
            )?;
            statement
                .query_map(
                    params![
                        parent_taxon_id,
                        rank.code(),
                        rank.code(),
                        taxon_id,
                        (limit + 1) as i64
                    ],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )?
                .collect::<Result<Vec<_>, _>>()?
        }
        None => {
            let mut statement = connection.prepare(
                r#"
                SELECT taxon_id, rank
                FROM taxa
                WHERE parent_taxon_id = ?
                ORDER BY rank, taxon_id
                LIMIT ?
                "#,
            )?;
            statement
                .query_map(params![parent_taxon_id, (limit + 1) as i64], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?
        }
    };
    let has_more = rows.len() > limit;
    rows.truncate(limit);
    let mut items = Vec::with_capacity(rows.len());
    for (taxon_id, rank) in rows {
        items.push(TaxonChild {
            taxon_id,
            rank: TaxonRank::from_code(rank)?,
            names: load_display_names(connection, taxon_id)?,
        });
    }
    let next_cursor = if has_more {
        items
            .last()
            .map(|item| {
                encode_cursor(&TaxonomyCursor::TaxonChildren {
                    parent_taxon_id,
                    rank: item.rank,
                    taxon_id: item.taxon_id,
                })
            })
            .transpose()?
    } else {
        None
    };
    Ok(TaxonomyPage { items, next_cursor })
}

fn load_taxon_base(
    connection: &Connection,
    taxon_id: i64,
) -> CoreResult<Option<(TaxonRank, Option<i64>)>> {
    let value = connection
        .query_row(
            "SELECT rank, parent_taxon_id FROM taxa WHERE taxon_id = ?",
            [taxon_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?)),
        )
        .optional()?;
    value
        .map(|(rank, parent)| Ok((TaxonRank::from_code(rank)?, parent)))
        .transpose()
}

fn load_display_names(connection: &Connection, taxon_id: i64) -> CoreResult<TaxonDisplayNames> {
    let mut names = TaxonDisplayNames::default();
    let mut statement = connection.prepare(
        r#"
        SELECT name_type, name
        FROM taxon_names
        WHERE taxon_id = ? AND name_type IN (1, 3, 5)
        "#,
    )?;
    let rows = statement.query_map([taxon_id], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (name_type, name) = row?;
        match TaxonomyNameType::from_code(name_type)? {
            TaxonomyNameType::SciName => names.sci_name = Some(name),
            TaxonomyNameType::ZhName => names.zh_name = Some(name),
            TaxonomyNameType::EnName => names.en_name = Some(name),
            _ => {}
        }
    }
    Ok(names)
}

fn load_name_details(connection: &Connection, taxon_id: i64) -> CoreResult<TaxonNamesDetail> {
    let mut result = TaxonNamesDetail::default();
    let mut statement = connection.prepare(
        r#"
        SELECT name_id, name_type, name, authority_year, source
        FROM taxon_names
        WHERE taxon_id = ?
        ORDER BY name_type, name
        "#,
    )?;
    let rows = statement.query_map([taxon_id], |row| {
        Ok((
            row.get::<_, i64>(1)?,
            TaxonNameDetail {
                name_id: row.get(0)?,
                name: row.get(2)?,
                authority_year: row.get(3)?,
                source: row.get(4)?,
            },
        ))
    })?;
    for row in rows {
        let (name_type, detail) = row?;
        match TaxonomyNameType::from_code(name_type)? {
            TaxonomyNameType::SciName => result.sci_name = Some(detail),
            TaxonomyNameType::Synonym => result.synonyms.push(detail),
            TaxonomyNameType::ZhName => result.zh_name = Some(detail),
            TaxonomyNameType::ZhAlias => result.zh_aliases.push(detail),
            TaxonomyNameType::EnName => result.en_name = Some(detail),
            TaxonomyNameType::EnAlias => result.en_aliases.push(detail),
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> (tempfile::TempDir, Database) {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open_test(directory.path().join("test.db")).unwrap();
        database
            .connect_taxonomy_metadata_context()
            .unwrap()
            .execute_batch(
                r#"
                INSERT INTO taxa (
                    taxon_id, parent_taxon_id, rank, geological_range
                ) VALUES
                    (1, NULL, 1, NULL),
                    (2, 1, 3, NULL),
                    (3, 2, 4, NULL),
                    (4, 3, 5, 'Pleistocene-present'),
                    (5, 3, 5, NULL);
                INSERT INTO taxon_names (
                    name_id, taxon_id, name_type, name, authority_year, source
                ) VALUES
                    (1, 1, 1, 'Animalia', NULL, 'Catalogue A'),
                    (2, 2, 1, 'Canidae', NULL, 'Catalogue A'),
                    (3, 3, 1, 'Canis', NULL, 'Catalogue A'),
                    (4, 4, 1, 'Canis lupus', 'Linnaeus, 1758', 'Catalogue A'),
                    (5, 4, 2, 'Canis lycaon', 'Schreber, 1775', 'Catalogue B'),
                    (6, 4, 3, 'Gray wolf', NULL, 'Catalogue C'),
                    (7, 4, 4, 'Wolf alias', NULL, 'Catalogue D'),
                    (8, 4, 5, 'Wolf', NULL, 'Catalogue E'),
                    (9, 4, 6, 'Grey wolf', NULL, 'Catalogue F'),
                    (10, 5, 1, 'Canis latrans', 'Say, 1823', 'Catalogue A');
                "#,
            )
            .unwrap();
        (directory, database)
    }

    #[test]
    fn detail_contains_ancestor_breadcrumb_and_all_name_groups() {
        let (_directory, database) = database();

        let detail = get_taxon_detail(&database, 4).unwrap().unwrap();

        assert_eq!(detail.taxon_id, 4);
        assert_eq!(detail.rank, TaxonRank::Species);
        assert_eq!(detail.parent_taxon_id, Some(3));
        assert_eq!(
            detail.geological_range.as_deref(),
            Some("Pleistocene-present")
        );
        assert_eq!(
            detail
                .breadcrumb
                .iter()
                .map(|item| (item.taxon_id, item.names.sci_name.as_deref()))
                .collect::<Vec<_>>(),
            vec![
                (1, Some("Animalia")),
                (2, Some("Canidae")),
                (3, Some("Canis")),
            ]
        );

        let scientific_name = detail.names.sci_name.unwrap();
        assert_eq!(scientific_name.name_id, 4);
        assert_eq!(scientific_name.name, "Canis lupus");
        assert_eq!(
            scientific_name.authority_year.as_deref(),
            Some("Linnaeus, 1758")
        );
        assert_eq!(scientific_name.source.as_deref(), Some("Catalogue A"));
        assert_eq!(detail.names.synonyms[0].name_id, 5);
        assert_eq!(detail.names.zh_name.unwrap().name_id, 6);
        assert_eq!(detail.names.zh_aliases[0].name_id, 7);
        assert_eq!(detail.names.en_name.unwrap().name_id, 8);
        assert_eq!(detail.names.en_aliases[0].name_id, 9);
    }

    #[test]
    fn display_summary_returns_only_family_through_the_current_rank() {
        let (_directory, database) = database();

        for (taxon_id, expected) in [
            (
                4,
                vec![
                    (2, TaxonRank::Family),
                    (3, TaxonRank::Genus),
                    (4, TaxonRank::Species),
                ],
            ),
            (3, vec![(2, TaxonRank::Family), (3, TaxonRank::Genus)]),
            (2, vec![(2, TaxonRank::Family)]),
            (1, vec![(1, TaxonRank::Kingdom)]),
        ] {
            let summary = get_taxon_display_summary(&database, taxon_id)
                .unwrap()
                .unwrap();
            assert_eq!(
                summary
                    .items
                    .iter()
                    .map(|item| (item.taxon_id, item.rank))
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!(summary.current_rank, summary.items.last().unwrap().rank);
        }
        assert!(get_taxon_display_summary(&database, 404).unwrap().is_none());
    }

    #[test]
    fn children_are_loaded_with_an_independent_cursor_page() {
        let (_directory, database) = database();

        let first = list_taxon_children(&database, 3, None, 1).unwrap();
        assert_eq!(first.items.len(), 1);
        assert_eq!(first.items[0].taxon_id, 4);
        assert_eq!(first.items[0].rank, TaxonRank::Species);
        assert!(first.next_cursor.is_some());

        let second = list_taxon_children(&database, 3, first.next_cursor.as_deref(), 1).unwrap();
        assert_eq!(second.items.len(), 1);
        assert_eq!(second.items[0].taxon_id, 5);
        assert!(second.next_cursor.is_none());

        assert!(list_taxon_children(&database, 2, first.next_cursor.as_deref(), 1).is_err());
    }
}
