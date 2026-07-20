use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, params_from_iter, types::Value as SqlValue};
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
    pub taxon_id: i64,
    pub rank: TaxonRank,
    pub parent_taxon_id: Option<i64>,
    pub geological_range: Option<String>,
    pub names: TaxonNamesDetail,
    pub identifiers: Vec<TaxonIdentifierDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonDetailNode {
    pub summary: TaxonSummary,
    pub detail: TaxonDetail,
    pub children: Vec<TaxonSummary>,
}

pub fn get_taxon_summary(database: &Database, taxon_id: i64) -> CoreResult<Option<TaxonSummary>> {
    let connection = database.connect()?;
    load_taxon_summary(&connection, taxon_id)
}

pub fn get_taxon_detail(database: &Database, taxon_id: i64) -> CoreResult<Option<TaxonDetail>> {
    let connection = database.connect()?;
    load_taxon_detail(&connection, taxon_id)
}

pub fn get_taxon_detail_node(
    database: &Database,
    taxon_id: i64,
) -> CoreResult<Option<TaxonDetailNode>> {
    let connection = database.connect()?;
    load_taxon_detail_node(&connection, taxon_id)
}

pub(super) fn load_taxon_summary(
    connection: &Connection,
    taxon_id: i64,
) -> CoreResult<Option<TaxonSummary>> {
    Ok(load_taxon_summaries(connection, &[taxon_id])?.pop())
}

pub(super) fn load_taxon_summaries(
    connection: &Connection,
    taxon_ids: &[i64],
) -> CoreResult<Vec<TaxonSummary>> {
    if taxon_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut seen = HashSet::new();
    let unique_ids = taxon_ids
        .iter()
        .copied()
        .filter(|taxon_id| seen.insert(*taxon_id))
        .collect::<Vec<_>>();
    let values_clause = unique_ids
        .iter()
        .map(|_| "(?, ?)")
        .collect::<Vec<_>>()
        .join(", ");
    let mut params = Vec::with_capacity(unique_ids.len() * 2);
    for (index, taxon_id) in unique_ids.iter().enumerate() {
        params.push(SqlValue::Integer(*taxon_id));
        params.push(SqlValue::Integer(index as i64));
    }
    let sql = format!(
        r#"
        WITH RECURSIVE
        input(taxon_id, sort_order) AS (VALUES {values_clause}),
        lineage(root_taxon_id, sort_order, taxon_id, parent_taxon_id, rank, scientific, english, chinese, depth, path) AS (
            SELECT
                input.taxon_id,
                input.sort_order,
                taxa.taxon_id,
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
                ),
                0,
                ',' || taxa.taxon_id || ','
            FROM input
            JOIN taxa ON taxa.taxon_id = input.taxon_id
            UNION ALL
            SELECT
                lineage.root_taxon_id,
                lineage.sort_order,
                parent.taxon_id,
                parent.parent_taxon_id,
                parent.rank,
                COALESCE(
                    (SELECT scientific_name FROM scientific
                     WHERE scientific.taxon_id = parent.taxon_id AND is_accepted = 1),
                    (SELECT scientific_name FROM scientific
                     WHERE scientific.taxon_id = parent.taxon_id
                     ORDER BY scientific_name LIMIT 1)
                ),
                COALESCE(
                    (SELECT english_name FROM english
                     WHERE english.taxon_id = parent.taxon_id AND is_accepted = 1),
                    (SELECT english_name FROM english
                     WHERE english.taxon_id = parent.taxon_id
                     ORDER BY english_name LIMIT 1)
                ),
                COALESCE(
                    (SELECT chinese_name FROM chinese
                     WHERE chinese.taxon_id = parent.taxon_id AND is_accepted = 1),
                    (SELECT chinese_name FROM chinese
                     WHERE chinese.taxon_id = parent.taxon_id
                     ORDER BY chinese_name LIMIT 1)
                ),
                lineage.depth + 1,
                lineage.path || parent.taxon_id || ','
            FROM lineage
            JOIN taxa AS parent ON parent.taxon_id = lineage.parent_taxon_id
            WHERE instr(lineage.path, ',' || parent.taxon_id || ',') = 0
        )
        SELECT root_taxon_id, taxon_id, parent_taxon_id, rank, scientific, english, chinese, depth
        FROM lineage
        ORDER BY sort_order, depth DESC
        "#
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(params), |row| {
        Ok(LineageRow {
            root_taxon_id: row.get(0)?,
            taxon_id: row.get(1)?,
            parent_taxon_id: row.get(2)?,
            rank: row.get(3)?,
            names: TaxonDisplayNames {
                scientific: row.get(4)?,
                english: row.get(5)?,
                chinese: row.get(6)?,
            },
            depth: row.get(7)?,
        })
    })?;
    let mut rows_by_root: HashMap<i64, Vec<LineageRow>> = HashMap::new();
    for row in rows {
        let row = row?;
        rows_by_root.entry(row.root_taxon_id).or_default().push(row);
    }
    let mut summaries_by_id = HashMap::new();
    for taxon_id in &unique_ids {
        let Some(mut lineage) = rows_by_root.remove(taxon_id) else {
            continue;
        };
        lineage.sort_by(|left, right| right.depth.cmp(&left.depth));
        if lineage
            .first()
            .and_then(|ancestor| ancestor.parent_taxon_id)
            .is_some()
        {
            return Err(CoreError::InvalidArgument(format!(
                "taxon {taxon_id} has missing parent or parent cycle"
            )));
        }
        let current_index = lineage
            .iter()
            .position(|row| row.taxon_id == *taxon_id)
            .ok_or_else(|| {
                CoreError::InvalidArgument(format!("taxon {taxon_id} has no lineage entry"))
            })?;
        let current = lineage.remove(current_index);
        let breadcrumb = lineage
            .into_iter()
            .map(|row| {
                Ok(TaxonBreadcrumbItem {
                    taxon_id: row.taxon_id,
                    rank: parse_rank(&row.rank)?,
                    names: row.names,
                })
            })
            .collect::<CoreResult<Vec<_>>>()?;
        summaries_by_id.insert(
            *taxon_id,
            TaxonSummary {
                taxon_id: current.taxon_id,
                rank: parse_rank(&current.rank)?,
                breadcrumb,
                names: current.names,
            },
        );
    }
    Ok(taxon_ids
        .iter()
        .filter_map(|taxon_id| summaries_by_id.get(taxon_id).cloned())
        .collect::<Vec<_>>())
}

#[derive(Debug)]
struct LineageRow {
    root_taxon_id: i64,
    taxon_id: i64,
    parent_taxon_id: Option<i64>,
    rank: String,
    names: TaxonDisplayNames,
    depth: i64,
}

pub(super) fn load_taxon_detail(
    connection: &Connection,
    taxon_id: i64,
) -> CoreResult<Option<TaxonDetail>> {
    let detail_row = connection
        .query_row(
            "SELECT rank, parent_taxon_id, geological_range FROM taxa WHERE taxon_id = ?",
            [taxon_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((rank, parent_taxon_id, geological_range)) = detail_row else {
        return Ok(None);
    };
    load_taxon_detail_for_existing(
        connection,
        taxon_id,
        parse_rank(&rank)?,
        parent_taxon_id,
        geological_range,
    )
    .map(Some)
}

fn load_taxon_detail_for_existing(
    connection: &Connection,
    taxon_id: i64,
    rank: TaxonRank,
    parent_taxon_id: Option<i64>,
    geological_range: Option<String>,
) -> CoreResult<TaxonDetail> {
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
    Ok(TaxonDetail {
        taxon_id,
        rank,
        parent_taxon_id,
        geological_range,
        names: TaxonNamesDetail {
            scientific: load_names(connection, taxon_id, TaxonomyNameKind::Scientific)?,
            english: load_names(connection, taxon_id, TaxonomyNameKind::English)?,
            chinese: load_names(connection, taxon_id, TaxonomyNameKind::Chinese)?,
        },
        identifiers,
    })
}

fn load_taxon_base(
    connection: &Connection,
    taxon_id: i64,
) -> CoreResult<Option<(TaxonRank, Option<i64>, Option<String>)>> {
    connection
        .query_row(
            "SELECT rank, parent_taxon_id, geological_range FROM taxa WHERE taxon_id = ?",
            [taxon_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?
        .map(|(rank, parent_taxon_id, geological_range)| {
            Ok((parse_rank(&rank)?, parent_taxon_id, geological_range))
        })
        .transpose()
}

pub(super) fn load_taxon_detail_node(
    connection: &Connection,
    taxon_id: i64,
) -> CoreResult<Option<TaxonDetailNode>> {
    let Some((rank, parent_taxon_id, geological_range)) = load_taxon_base(connection, taxon_id)?
    else {
        return Ok(None);
    };
    let child_ids = load_child_taxon_ids(connection, taxon_id)?;
    let summary_ids = std::iter::once(taxon_id)
        .chain(child_ids.iter().copied())
        .collect::<Vec<_>>();
    let mut summaries = load_taxon_summaries(connection, &summary_ids)?;
    if summaries.len() != summary_ids.len() {
        return Err(CoreError::InvalidArgument(format!(
            "taxon {taxon_id} children changed while loading summaries"
        )));
    }
    let summary = summaries
        .first()
        .cloned()
        .ok_or_else(|| CoreError::InvalidArgument(format!("taxon {taxon_id} no longer exists")))?;
    let children = summaries.split_off(1);
    Ok(Some(TaxonDetailNode {
        summary,
        detail: load_taxon_detail_for_existing(
            connection,
            taxon_id,
            rank,
            parent_taxon_id,
            geological_range,
        )?,
        children,
    }))
}

fn load_child_taxon_ids(connection: &Connection, taxon_id: i64) -> CoreResult<Vec<i64>> {
    let mut statement = connection.prepare(
        r#"
        SELECT taxon_id
        FROM taxa
        WHERE parent_taxon_id = ?
        ORDER BY rank, taxon_id
        "#,
    )?;
    let rows = statement.query_map([taxon_id], |row| row.get::<_, i64>(0))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
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
