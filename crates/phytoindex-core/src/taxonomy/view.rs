use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, params, params_from_iter, types::Value as SqlValue};
use serde::{Deserialize, Serialize};

use super::{
    TaxonRank, TaxonomyNameKind,
    page::{
        DEFAULT_PAGE_LIMIT, TaxonomyCursor, TaxonomyPage, decode_cursor, encode_cursor,
        invalid_cursor, page_limit,
    },
    parse_rank,
};
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
pub struct TaxonChild {
    pub taxon_id: i64,
    pub rank: TaxonRank,
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
    pub children: TaxonomyPage<TaxonChild>,
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
    get_taxon_detail_node_page(database, taxon_id, None, DEFAULT_PAGE_LIMIT)
}

pub fn get_taxon_detail_node_page(
    database: &Database,
    taxon_id: i64,
    children_cursor: Option<&str>,
    children_limit: usize,
) -> CoreResult<Option<TaxonDetailNode>> {
    let connection = database.connect()?;
    load_taxon_detail_node(&connection, taxon_id, children_cursor, children_limit)
}

pub fn list_taxon_children(
    database: &Database,
    taxon_id: i64,
    cursor: Option<&str>,
    limit: usize,
) -> CoreResult<TaxonomyPage<TaxonChild>> {
    let connection = database.connect()?;
    load_taxon_children(&connection, taxon_id, cursor, limit)
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
    let unique_ids = unique_taxon_ids(taxon_ids);
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
        SELECT root_taxon_id, taxon_id, parent_taxon_id, rank, scientific, english, chinese
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
}

pub(super) fn load_taxon_detail(
    connection: &Connection,
    taxon_id: i64,
) -> CoreResult<Option<TaxonDetail>> {
    Ok(load_taxon_details(connection, &[taxon_id])?.pop())
}

pub(super) fn load_taxon_details(
    connection: &Connection,
    taxon_ids: &[i64],
) -> CoreResult<Vec<TaxonDetail>> {
    if taxon_ids.is_empty() {
        return Ok(Vec::new());
    }
    let unique_ids = unique_taxon_ids(taxon_ids);
    let bases = load_taxon_bases(connection, &unique_ids)?;
    let scientific = load_names_for_taxa(connection, &unique_ids, TaxonomyNameKind::Scientific)?;
    let english = load_names_for_taxa(connection, &unique_ids, TaxonomyNameKind::English)?;
    let chinese = load_names_for_taxa(connection, &unique_ids, TaxonomyNameKind::Chinese)?;
    let identifiers = load_identifiers_for_taxa(connection, &unique_ids)?;
    let mut details_by_id = HashMap::new();
    for base in bases {
        let taxon_id = base.taxon_id;
        details_by_id.insert(
            taxon_id,
            TaxonDetail {
                taxon_id,
                rank: base.rank,
                parent_taxon_id: base.parent_taxon_id,
                geological_range: base.geological_range,
                names: TaxonNamesDetail {
                    scientific: scientific.get(&taxon_id).cloned().unwrap_or_default(),
                    english: english.get(&taxon_id).cloned().unwrap_or_default(),
                    chinese: chinese.get(&taxon_id).cloned().unwrap_or_default(),
                },
                identifiers: identifiers.get(&taxon_id).cloned().unwrap_or_default(),
            },
        );
    }
    Ok(taxon_ids
        .iter()
        .filter_map(|taxon_id| details_by_id.get(taxon_id).cloned())
        .collect())
}

pub(super) fn load_taxon_detail_node(
    connection: &Connection,
    taxon_id: i64,
    children_cursor: Option<&str>,
    children_limit: usize,
) -> CoreResult<Option<TaxonDetailNode>> {
    let Some(detail) = load_taxon_detail(connection, taxon_id)? else {
        return Ok(None);
    };
    let summary = load_taxon_summary(connection, taxon_id)?
        .ok_or_else(|| CoreError::InvalidArgument(format!("taxon {taxon_id} no longer exists")))?;
    Ok(Some(TaxonDetailNode {
        summary,
        detail,
        children: load_taxon_children(connection, taxon_id, children_cursor, children_limit)?,
    }))
}

fn load_taxon_children(
    connection: &Connection,
    taxon_id: i64,
    cursor: Option<&str>,
    limit: usize,
) -> CoreResult<TaxonomyPage<TaxonChild>> {
    let child_cursor = match decode_cursor(cursor)? {
        None => None,
        Some(TaxonomyCursor::TaxonChildren {
            parent_taxon_id,
            rank,
            taxon_id: cursor_taxon_id,
        }) if parent_taxon_id == taxon_id => Some((rank.as_str().to_string(), cursor_taxon_id)),
        Some(_) => return Err(invalid_cursor()),
    };
    let limit = page_limit(limit);
    let fetch_limit = limit + 1;
    let mut statement = connection.prepare(
        r#"
        SELECT
            taxa.taxon_id,
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
        WHERE parent_taxon_id = ?1
          AND (?2 IS NULL OR rank > ?2 OR (rank = ?2 AND taxon_id > ?3))
        ORDER BY rank, taxon_id
        LIMIT ?4
        "#,
    )?;
    let (cursor_rank, cursor_taxon_id) = child_cursor
        .map(|(rank, taxon_id)| (Some(rank), Some(taxon_id)))
        .unwrap_or((None, None));
    let rows = statement.query_map(
        params![taxon_id, cursor_rank, cursor_taxon_id, fetch_limit as i64],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                TaxonDisplayNames {
                    scientific: row.get(2)?,
                    english: row.get(3)?,
                    chinese: row.get(4)?,
                },
            ))
        },
    )?;
    let mut items = rows
        .map(|row| {
            let (taxon_id, rank, names) = row?;
            Ok(TaxonChild {
                taxon_id,
                rank: parse_rank(&rank)?,
                names,
            })
        })
        .collect::<CoreResult<Vec<_>>>()?;
    let next_cursor = if items.len() > limit {
        items.truncate(limit);
        items.last().map(|child| {
            encode_cursor(&TaxonomyCursor::TaxonChildren {
                parent_taxon_id: taxon_id,
                rank: child.rank,
                taxon_id: child.taxon_id,
            })
        })
    } else {
        None
    }
    .transpose()?;
    Ok(TaxonomyPage { items, next_cursor })
}

fn load_taxon_bases(connection: &Connection, taxon_ids: &[i64]) -> CoreResult<Vec<TaxonBase>> {
    if taxon_ids.is_empty() {
        return Ok(Vec::new());
    }
    let (values_clause, params) = input_values(taxon_ids);
    let sql = format!(
        r#"
        WITH input(taxon_id, sort_order) AS (VALUES {values_clause})
        SELECT taxa.taxon_id, taxa.rank, taxa.parent_taxon_id, taxa.geological_range
        FROM input
        JOIN taxa ON taxa.taxon_id = input.taxon_id
        ORDER BY input.sort_order
        "#
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(params), |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<i64>>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    })?;
    rows.map(|row| {
        let (taxon_id, rank, parent_taxon_id, geological_range) = row?;
        Ok(TaxonBase {
            taxon_id,
            rank: parse_rank(&rank)?,
            parent_taxon_id,
            geological_range,
        })
    })
    .collect()
}

fn load_names_for_taxa(
    connection: &Connection,
    taxon_ids: &[i64],
    kind: TaxonomyNameKind,
) -> CoreResult<HashMap<i64, Vec<TaxonNameDetail>>> {
    if taxon_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let (values_clause, params) = input_values(taxon_ids);
    let sql = format!(
        r#"
        WITH input(taxon_id, sort_order) AS (VALUES {values_clause})
        SELECT input.taxon_id, {}, is_accepted, authority_year, category, source
        FROM input
        JOIN {} ON {}.taxon_id = input.taxon_id
        ORDER BY input.sort_order, is_accepted DESC, {}
        "#,
        kind.column(),
        kind.table(),
        kind.table(),
        kind.column()
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(params), |row| {
        Ok((
            row.get::<_, i64>(0)?,
            TaxonNameDetail {
                name: row.get(1)?,
                is_accepted: row.get::<_, i64>(2)? != 0,
                authority_year: row.get(3)?,
                category: row.get(4)?,
                source: row.get(5)?,
            },
        ))
    })?;
    let mut names_by_id: HashMap<i64, Vec<TaxonNameDetail>> = HashMap::new();
    for row in rows {
        let (taxon_id, name) = row?;
        names_by_id.entry(taxon_id).or_default().push(name);
    }
    Ok(names_by_id)
}

fn load_identifiers_for_taxa(
    connection: &Connection,
    taxon_ids: &[i64],
) -> CoreResult<HashMap<i64, Vec<TaxonIdentifierDetail>>> {
    if taxon_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let (values_clause, params) = input_values(taxon_ids);
    let sql = format!(
        r#"
        WITH input(taxon_id, sort_order) AS (VALUES {values_clause})
        SELECT input.taxon_id, taxon_identifiers.source, taxon_identifiers.external_id
        FROM input
        JOIN taxon_identifiers ON taxon_identifiers.taxon_id = input.taxon_id
        ORDER BY input.sort_order, taxon_identifiers.source, taxon_identifiers.external_id
        "#
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(params), |row| {
        Ok((
            row.get::<_, i64>(0)?,
            TaxonIdentifierDetail {
                source: row.get(1)?,
                external_id: row.get(2)?,
            },
        ))
    })?;
    let mut identifiers_by_id: HashMap<i64, Vec<TaxonIdentifierDetail>> = HashMap::new();
    for row in rows {
        let (taxon_id, identifier) = row?;
        identifiers_by_id
            .entry(taxon_id)
            .or_default()
            .push(identifier);
    }
    Ok(identifiers_by_id)
}

fn unique_taxon_ids(taxon_ids: &[i64]) -> Vec<i64> {
    let mut seen = HashSet::new();
    taxon_ids
        .iter()
        .copied()
        .filter(|taxon_id| seen.insert(*taxon_id))
        .collect()
}

fn input_values(taxon_ids: &[i64]) -> (String, Vec<SqlValue>) {
    let values_clause = taxon_ids
        .iter()
        .map(|_| "(?, ?)")
        .collect::<Vec<_>>()
        .join(", ");
    let mut params = Vec::with_capacity(taxon_ids.len() * 2);
    for (index, taxon_id) in taxon_ids.iter().enumerate() {
        params.push(SqlValue::Integer(*taxon_id));
        params.push(SqlValue::Integer(index as i64));
    }
    (values_clause, params)
}

#[derive(Debug)]
struct TaxonBase {
    taxon_id: i64,
    rank: TaxonRank,
    parent_taxon_id: Option<i64>,
    geological_range: Option<String>,
}
