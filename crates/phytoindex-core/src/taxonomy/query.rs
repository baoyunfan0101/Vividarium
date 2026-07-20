use std::collections::BTreeSet;

use rusqlite::{Connection, params};
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
                matches: load_name_matches(connection, taxon_id, &pattern)?,
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

fn load_name_matches(
    connection: &Connection,
    taxon_id: i64,
    pattern: &str,
) -> CoreResult<Vec<TaxonNameMatch>> {
    let mut matches = Vec::new();
    for kind in [
        TaxonomyNameKind::Scientific,
        TaxonomyNameKind::English,
        TaxonomyNameKind::Chinese,
    ] {
        let sql = format!(
            r#"
            SELECT {}, is_accepted
            FROM {}
            WHERE taxon_id = ? AND {} LIKE ? ESCAPE '\'
            ORDER BY is_accepted DESC, {}
            "#,
            kind.column(),
            kind.table(),
            kind.column(),
            kind.column()
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params![taxon_id, pattern], |row| {
            Ok(TaxonNameMatch {
                name_kind: kind,
                name: row.get(0)?,
                is_accepted: row.get::<_, i64>(1)? != 0,
            })
        })?;
        matches.extend(rows.collect::<Result<Vec<_>, _>>()?);
    }
    Ok(matches)
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}
