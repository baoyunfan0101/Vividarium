use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{TaxonRank, TaxonomyNameKind, parse_rank};
use crate::{CoreError, CoreResult, Database};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonDisplayNames {
    pub scientific: Option<String>,
    pub english: Option<String>,
    pub chinese: Option<String>,
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
pub struct TaxonNameDetail {
    pub name: String,
    pub is_accepted: bool,
    pub authority_year: Option<String>,
    pub category: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonNamesDetail {
    pub scientific: Vec<TaxonNameDetail>,
    pub english: Vec<TaxonNameDetail>,
    pub chinese: Vec<TaxonNameDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonIdentifierDetail {
    pub source: String,
    pub external_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonDetail {
    pub summary: TaxonSummary,
    pub parent_taxon_id: Option<i64>,
    pub geological_range: Option<String>,
    pub names: TaxonNamesDetail,
    pub identifiers: Vec<TaxonIdentifierDetail>,
}

pub fn get_taxon_summary(database: &Database, taxon_id: i64) -> CoreResult<Option<TaxonSummary>> {
    let connection = database.connect()?;
    load_taxon_summary(&connection, taxon_id)
}

pub fn get_taxon_detail(database: &Database, taxon_id: i64) -> CoreResult<Option<TaxonDetail>> {
    let connection = database.connect()?;
    load_taxon_detail(&connection, taxon_id)
}

pub(super) fn load_taxon_summary(
    connection: &Connection,
    taxon_id: i64,
) -> CoreResult<Option<TaxonSummary>> {
    let mut current_id = Some(taxon_id);
    let mut visited = HashSet::new();
    let mut lineage = Vec::new();
    while let Some(id) = current_id {
        if !visited.insert(id) {
            return Err(CoreError::InvalidArgument(format!(
                "taxon parent cycle detected at {id}"
            )));
        }
        let row = connection
            .query_row(
                r#"
                SELECT
                    taxa.parent_taxon_id,
                    taxa.rank,
                    COALESCE(
                        (SELECT scientific_name FROM scientific
                         WHERE scientific.taxon_id = taxa.taxon_id AND is_accepted = 1),
                        (SELECT scientific_name FROM scientific
                         WHERE scientific.taxon_id = taxa.taxon_id
                         ORDER BY scientific_name LIMIT 1)
                    ),
                    COALESCE(
                        (SELECT english_name FROM english
                         WHERE english.taxon_id = taxa.taxon_id AND is_accepted = 1),
                        (SELECT english_name FROM english
                         WHERE english.taxon_id = taxa.taxon_id
                         ORDER BY english_name LIMIT 1)
                    ),
                    COALESCE(
                        (SELECT chinese_name FROM chinese
                         WHERE chinese.taxon_id = taxa.taxon_id AND is_accepted = 1),
                        (SELECT chinese_name FROM chinese
                         WHERE chinese.taxon_id = taxa.taxon_id
                         ORDER BY chinese_name LIMIT 1)
                    )
                FROM taxa
                WHERE taxa.taxon_id = ?
                "#,
                [id],
                |row| {
                    Ok((
                        row.get::<_, Option<i64>>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )
            .optional()?;
        let Some((parent_taxon_id, rank, scientific, english, chinese)) = row else {
            if id == taxon_id {
                return Ok(None);
            }
            return Err(CoreError::InvalidArgument(format!(
                "taxon {taxon_id} has missing parent {id}"
            )));
        };
        lineage.push(TaxonBreadcrumbItem {
            taxon_id: id,
            rank: parse_rank(&rank)?,
            names: TaxonDisplayNames {
                scientific,
                english,
                chinese,
            },
        });
        current_id = parent_taxon_id;
    }
    lineage.reverse();
    let current = lineage.pop().ok_or_else(|| {
        CoreError::InvalidArgument(format!("taxon {taxon_id} has no lineage entry"))
    })?;
    Ok(Some(TaxonSummary {
        taxon_id: current.taxon_id,
        rank: current.rank,
        breadcrumb: lineage,
        names: current.names,
    }))
}

fn load_taxon_detail(connection: &Connection, taxon_id: i64) -> CoreResult<Option<TaxonDetail>> {
    let Some(summary) = load_taxon_summary(connection, taxon_id)? else {
        return Ok(None);
    };
    let (parent_taxon_id, geological_range) = connection.query_row(
        "SELECT parent_taxon_id, geological_range FROM taxa WHERE taxon_id = ?",
        [taxon_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let mut identifier_statement = connection.prepare(
        r#"
        SELECT source, external_id
        FROM taxon_identifiers
        WHERE taxon_id = ?
        ORDER BY source, external_id
        "#,
    )?;
    let identifiers = identifier_statement
        .query_map([taxon_id], |row| {
            Ok(TaxonIdentifierDetail {
                source: row.get(0)?,
                external_id: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(TaxonDetail {
        summary,
        parent_taxon_id,
        geological_range,
        names: TaxonNamesDetail {
            scientific: load_names(connection, taxon_id, TaxonomyNameKind::Scientific)?,
            english: load_names(connection, taxon_id, TaxonomyNameKind::English)?,
            chinese: load_names(connection, taxon_id, TaxonomyNameKind::Chinese)?,
        },
        identifiers,
    }))
}

fn load_names(
    connection: &Connection,
    taxon_id: i64,
    kind: TaxonomyNameKind,
) -> CoreResult<Vec<TaxonNameDetail>> {
    let sql = format!(
        "SELECT {}, is_accepted, authority_year, category, source FROM {} WHERE taxon_id = ? ORDER BY is_accepted DESC, {}",
        kind.column(),
        kind.table(),
        kind.column()
    );
    let mut statement = connection.prepare(&sql)?;
    let names = statement.query_map([taxon_id], |row| {
        Ok(TaxonNameDetail {
            name: row.get(0)?,
            is_accepted: row.get::<_, i64>(1)? != 0,
            authority_year: row.get(2)?,
            category: row.get(3)?,
            source: row.get(4)?,
        })
    })?;
    Ok(names.collect::<Result<Vec<_>, _>>()?)
}
