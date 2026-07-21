use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, params_from_iter, types::Value as SqlValue};
use serde::{Deserialize, Serialize};

use super::{
    TaxonomyNameKind, page::page_limit, view::load_taxon_details, view::load_taxon_summaries,
};
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
    let Some(query) = normalize_search_query(query) else {
        return Ok(Vec::new());
    };
    let connection = database.connect()?;
    search_taxa_with_connection(&connection, &query, limit)
}

fn search_taxa_with_connection(
    connection: &Connection,
    query: &str,
    limit: usize,
) -> CoreResult<Vec<TaxonSearchResult>> {
    let limit = page_limit(limit);
    let search = SearchQuery::new(query);
    let ids = search_taxon_ids(connection, &search, limit)?;
    let summaries = load_taxon_summaries(connection, &ids)?;
    let details = load_taxon_details(connection, &ids)?;
    let matches_by_id = load_name_matches_for_taxa(connection, &ids, &search)?;
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
        .collect::<CoreResult<Vec<_>>>()
}

#[derive(Debug, Clone)]
struct SearchQuery {
    normalized: String,
    prefix_upper: String,
    word_prefix_match: Option<String>,
    contains_match: Option<String>,
    word_prefix_like_pattern: Option<String>,
    contains_like_pattern: Option<String>,
}

impl SearchQuery {
    fn new(query: &str) -> Self {
        let normalized = query.to_ascii_lowercase();
        let char_count = query.chars().count();
        Self {
            prefix_upper: format!("{normalized}\u{10ffff}"),
            word_prefix_match: (char_count >= 2).then(|| quoted_fts_match(&format!(" {query}"))),
            contains_match: (char_count >= 3).then(|| quoted_fts_match(query)),
            word_prefix_like_pattern: (char_count >= 2)
                .then(|| format!("% {}%", escape_like(query))),
            contains_like_pattern: (char_count >= 3).then(|| format!("%{}%", escape_like(query))),
            normalized,
        }
    }
}

fn search_taxon_ids(
    connection: &Connection,
    search: &SearchQuery,
    limit: usize,
) -> CoreResult<Vec<i64>> {
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    append_exact_matches(connection, search, limit, &mut ids, &mut seen)?;
    if ids.len() >= limit {
        return Ok(ids);
    }
    append_full_prefix_matches(connection, search, limit, &mut ids, &mut seen)?;
    if ids.len() >= limit {
        return Ok(ids);
    }
    if let Some(query) = search.word_prefix_match.as_ref() {
        append_fts_matches(connection, query, limit, &mut ids, &mut seen)?;
    }
    if ids.len() >= limit {
        return Ok(ids);
    }
    if let Some(query) = search.contains_match.as_ref() {
        append_fts_matches(connection, query, limit, &mut ids, &mut seen)?;
    }
    Ok(ids)
}

fn append_exact_matches(
    connection: &Connection,
    search: &SearchQuery,
    limit: usize,
    ids: &mut Vec<i64>,
    seen: &mut HashSet<i64>,
) -> CoreResult<()> {
    let remaining = limit.saturating_sub(ids.len());
    if remaining == 0 {
        return Ok(());
    }
    let sql = r#"
        SELECT taxon_id
        FROM taxon_names
        WHERE name_search = ?
        GROUP BY taxon_id
        ORDER BY MIN(CASE WHEN is_accepted = 1 THEN 0 ELSE 1 END), MIN(name_kind), taxon_id
        LIMIT ?
        "#;
    append_query_ids(
        connection,
        sql,
        vec![
            SqlValue::Text(search.normalized.clone()),
            SqlValue::Integer(remaining as i64),
        ],
        ids,
        seen,
    )
}

fn append_full_prefix_matches(
    connection: &Connection,
    search: &SearchQuery,
    limit: usize,
    ids: &mut Vec<i64>,
    seen: &mut HashSet<i64>,
) -> CoreResult<()> {
    let remaining = limit.saturating_sub(ids.len());
    if remaining == 0 {
        return Ok(());
    }
    let (exclusion_sql, mut values) = exclusion_clause("taxon_names", seen);
    let sql = format!(
        r#"
        SELECT taxon_id
        FROM taxon_names
        WHERE name_search >= ?
          AND name_search < ?
          AND name_search != ?
          {exclusion_sql}
        GROUP BY taxon_id
        ORDER BY MIN(name_search), MIN(CASE WHEN is_accepted = 1 THEN 0 ELSE 1 END), MIN(name_kind), taxon_id
        LIMIT ?
        "#
    );
    let mut params = vec![
        SqlValue::Text(search.normalized.clone()),
        SqlValue::Text(search.prefix_upper.clone()),
        SqlValue::Text(search.normalized.clone()),
    ];
    params.append(&mut values);
    params.push(SqlValue::Integer(remaining as i64));
    append_query_ids(connection, &sql, params, ids, seen)
}

fn append_fts_matches(
    connection: &Connection,
    query: &str,
    limit: usize,
    ids: &mut Vec<i64>,
    seen: &mut HashSet<i64>,
) -> CoreResult<()> {
    let remaining = limit.saturating_sub(ids.len());
    if remaining == 0 {
        return Ok(());
    }
    let (exclusion_sql, mut values) = exclusion_clause("taxon_names", seen);
    let sql = format!(
        r#"
        SELECT taxon_names.taxon_id
        FROM taxon_names_fts
        JOIN taxon_names ON taxon_names.name_id = taxon_names_fts.rowid
        WHERE taxon_names_fts MATCH ?
          {exclusion_sql}
        GROUP BY taxon_names.taxon_id
        ORDER BY MIN(taxon_names.name_search),
                 MIN(CASE WHEN taxon_names.is_accepted = 1 THEN 0 ELSE 1 END),
                 MIN(taxon_names.name_kind),
                 taxon_names.taxon_id
        LIMIT ?
        "#
    );
    let mut params = vec![SqlValue::Text(query.to_string())];
    params.append(&mut values);
    params.push(SqlValue::Integer(remaining as i64));
    append_query_ids(connection, &sql, params, ids, seen)
}

fn append_query_ids(
    connection: &Connection,
    sql: &str,
    params: Vec<SqlValue>,
    ids: &mut Vec<i64>,
    seen: &mut HashSet<i64>,
) -> CoreResult<()> {
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map(params_from_iter(params), |row| row.get::<_, i64>(0))?;
    for row in rows {
        let taxon_id = row?;
        if seen.insert(taxon_id) {
            ids.push(taxon_id);
        }
    }
    Ok(())
}

fn exclusion_clause(table_name: &str, seen: &HashSet<i64>) -> (String, Vec<SqlValue>) {
    if seen.is_empty() {
        return (String::new(), Vec::new());
    }
    let placeholders = vec!["?"; seen.len()].join(", ");
    let mut values = seen.iter().copied().collect::<Vec<_>>();
    values.sort_unstable();
    let values = values.into_iter().map(SqlValue::Integer).collect();
    (
        format!("AND {table_name}.taxon_id NOT IN ({placeholders})"),
        values,
    )
}

fn load_name_matches_for_taxa(
    connection: &Connection,
    taxon_ids: &[i64],
    search: &SearchQuery,
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
    let mut query_params = Vec::with_capacity(taxon_ids.len() * 2 + 5);
    for (index, taxon_id) in taxon_ids.iter().enumerate() {
        query_params.push(SqlValue::Integer(*taxon_id));
        query_params.push(SqlValue::Integer(index as i64));
    }
    query_params.push(SqlValue::Text(search.normalized.clone()));
    query_params.push(SqlValue::Text(search.prefix_upper.clone()));
    let mut conditions =
        vec!["(taxon_names.name_search >= ? AND taxon_names.name_search < ?)".to_string()];
    if let Some(pattern) = search.word_prefix_like_pattern.as_ref() {
        conditions.push("taxon_names.name LIKE ? ESCAPE '\\'".to_string());
        query_params.push(SqlValue::Text(pattern.clone()));
    }
    if let Some(pattern) = search.contains_like_pattern.as_ref() {
        conditions.push("taxon_names.name LIKE ? ESCAPE '\\'".to_string());
        query_params.push(SqlValue::Text(pattern.clone()));
    }
    let conditions = conditions.join(" OR ");
    let sql = format!(
        r#"
        WITH input(taxon_id, sort_order) AS (VALUES {values_clause})
        SELECT input.taxon_id, taxon_names.name_kind, taxon_names.name, taxon_names.is_accepted
        FROM input
        JOIN taxon_names ON taxon_names.taxon_id = input.taxon_id
        WHERE {conditions}
        ORDER BY input.sort_order, taxon_names.name_kind, taxon_names.is_accepted DESC, taxon_names.name
        "#
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(query_params), |row| {
        let name_kind = TaxonomyNameKind::from_code(row.get::<_, i64>(1)?).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Integer,
                error.to_string().into(),
            )
        })?;
        Ok((
            row.get::<_, i64>(0)?,
            TaxonNameMatch {
                name_kind,
                name: row.get(2)?,
                is_accepted: row.get::<_, i64>(3)? != 0,
            },
        ))
    })?;
    for row in rows {
        let (taxon_id, name_match) = row?;
        matches_by_id.entry(taxon_id).or_default().push(name_match);
    }
    Ok(matches_by_id)
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn normalize_search_query(value: &str) -> Option<String> {
    normalize_whitespace(value)
}

fn normalize_whitespace(value: &str) -> Option<String> {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    (!value.is_empty()).then_some(value)
}

fn quoted_fts_match(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}
