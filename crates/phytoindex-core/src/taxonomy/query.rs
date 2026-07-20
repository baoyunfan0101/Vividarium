use std::collections::{BTreeSet, HashMap};

use rusqlite::{Connection, params, params_from_iter, types::Value as SqlValue};
use serde::{Deserialize, Serialize};

use super::{TaxonomyNameKind, view::load_taxon_details, view::load_taxon_summaries};
use crate::{CoreError, CoreResult, Database};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonNameMatch {
    pub name_kind: TaxonomyNameKind,
    pub name: String,
    pub is_accepted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaxonSearchResult {
    pub summary: super::TaxonSummary,
    pub detail: super::TaxonDetail,
    pub matches: Vec<TaxonNameMatch>,
}

pub fn search_taxa(
    database: &Database,
    query: &str,
    limit: usize,
) -> CoreResult<Vec<TaxonSearchResult>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let connection = database.connect()?;
    search_taxa_with_connection(&connection, query, limit)
}

fn search_taxa_with_connection(
    connection: &Connection,
    query: &str,
    limit: usize,
) -> CoreResult<Vec<TaxonSearchResult>> {
    let pattern = format!("%{}%", escape_like(query));
    let limit = limit.clamp(1, 500);
    let mut ids = BTreeSet::new();
    for kind in [
        TaxonomyNameKind::Scientific,
        TaxonomyNameKind::English,
        TaxonomyNameKind::Chinese,
    ] {
        collect_matching_taxon_ids(connection, kind, &pattern, limit, &mut ids)?;
        if ids.len() >= limit {
            break;
        }
    }

    let ids = ids.into_iter().take(limit).collect::<Vec<_>>();
    let summaries = load_taxon_summaries(connection, &ids)?;
    let details = load_taxon_details(connection, &ids)?;
    let matches_by_id = load_name_matches_for_taxa(connection, &ids, &pattern)?;
    if summaries.len() != ids.len() || details.len() != ids.len() {
        return Err(CoreError::InvalidArgument(
            "matched taxon no longer exists".into(),
        ));
    }
    ids.into_iter()
        .zip(summaries)
        .zip(details)
        .map(|((taxon_id, summary), detail)| {
            Ok(TaxonSearchResult {
                summary,
                detail,
                matches: matches_by_id.get(&taxon_id).cloned().unwrap_or_default(),
            })
        })
        .collect()
}

fn collect_matching_taxon_ids(
    connection: &Connection,
    kind: TaxonomyNameKind,
    pattern: &str,
    limit: usize,
    ids: &mut BTreeSet<i64>,
) -> CoreResult<()> {
    let sql = format!(
        r#"
        SELECT DISTINCT taxon_id
        FROM {}
        WHERE {} LIKE ? ESCAPE '\'
        ORDER BY taxon_id
        LIMIT ?
        "#,
        kind.table(),
        kind.column()
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params![pattern, limit as i64], |row| row.get::<_, i64>(0))?;
    for row in rows {
        ids.insert(row?);
        if ids.len() >= limit {
            break;
        }
    }
    Ok(())
}

fn load_name_matches_for_taxa(
    connection: &Connection,
    taxon_ids: &[i64],
    pattern: &str,
) -> CoreResult<HashMap<i64, Vec<TaxonNameMatch>>> {
    if taxon_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let values_clause = taxon_ids
        .iter()
        .map(|_| "(?, ?)")
        .collect::<Vec<_>>()
        .join(", ");
    let mut matches_by_id: HashMap<i64, Vec<TaxonNameMatch>> = HashMap::new();
    for kind in [
        TaxonomyNameKind::Scientific,
        TaxonomyNameKind::English,
        TaxonomyNameKind::Chinese,
    ] {
        let mut query_params = Vec::with_capacity(taxon_ids.len() * 2 + 1);
        for (index, taxon_id) in taxon_ids.iter().enumerate() {
            query_params.push(SqlValue::Integer(*taxon_id));
            query_params.push(SqlValue::Integer(index as i64));
        }
        query_params.push(SqlValue::Text(pattern.to_string()));
        let sql = format!(
            r#"
            WITH input(taxon_id, sort_order) AS (VALUES {values_clause})
            SELECT input.taxon_id, {}, is_accepted
            FROM input
            JOIN {} ON {}.taxon_id = input.taxon_id
            WHERE {} LIKE ? ESCAPE '\'
            ORDER BY input.sort_order, is_accepted DESC, {}
            "#,
            kind.column(),
            kind.table(),
            kind.table(),
            kind.column(),
            kind.column()
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(query_params), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                TaxonNameMatch {
                    name_kind: kind,
                    name: row.get(1)?,
                    is_accepted: row.get::<_, i64>(2)? != 0,
                },
            ))
        })?;
        for row in rows {
            let (taxon_id, name_match) = row?;
            matches_by_id.entry(taxon_id).or_default().push(name_match);
        }
    }
    Ok(matches_by_id)
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}
