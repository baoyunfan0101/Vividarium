use std::collections::{BTreeSet, HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, params};

use super::types::{TaxonRank, TaxonomyNameType};
use crate::models::{OperationProgress, OperationProgressUnit};
use crate::naming::normalize_taxonomy_name;
use crate::{CancellationToken, CoreError, CoreResult};

pub(super) fn normalize_name(value: Option<&str>) -> Option<String> {
    value.and_then(normalize_taxonomy_name)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TaxonomyValidationIssue {
    pub code: &'static str,
    pub message: String,
    pub taxon_id: Option<i64>,
    pub related_taxon_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TaxonomyValidationOptions {
    pub check_parent_structure: bool,
    pub check_scientific_name_count: bool,
    pub check_localized_accepted_name_count: bool,
    pub check_localized_alias_dependencies: bool,
    pub check_duplicate_name_family: bool,
    pub check_orphan_names: bool,
    pub require_normalized_names: bool,
}

impl TaxonomyValidationOptions {
    pub const fn full() -> Self {
        Self {
            check_parent_structure: true,
            check_scientific_name_count: true,
            check_localized_accepted_name_count: true,
            check_localized_alias_dependencies: true,
            check_duplicate_name_family: true,
            check_orphan_names: true,
            require_normalized_names: true,
        }
    }

    pub const fn sql_import_staging() -> Self {
        Self {
            check_parent_structure: true,
            check_scientific_name_count: true,
            check_localized_accepted_name_count: true,
            check_localized_alias_dependencies: true,
            check_duplicate_name_family: false,
            check_orphan_names: true,
            require_normalized_names: false,
        }
    }
}

pub(super) fn visit_taxonomy_validation_issues(
    connection: &Connection,
    require_normalized_names: bool,
    visit: impl FnMut(TaxonomyValidationIssue) -> bool,
) -> CoreResult<()> {
    let mut options = TaxonomyValidationOptions::full();
    options.require_normalized_names = require_normalized_names;
    visit_taxonomy_validation_issues_with_progress(connection, options, |_| {}, visit)
}

#[cfg(test)]
pub(super) fn visit_taxonomy_validation_issues_with_options(
    connection: &Connection,
    options: TaxonomyValidationOptions,
    visit: impl FnMut(TaxonomyValidationIssue) -> bool,
) -> CoreResult<()> {
    visit_taxonomy_validation_issues_with_progress(connection, options, |_| {}, visit)
}

pub(super) fn visit_taxonomy_validation_issues_with_progress(
    connection: &Connection,
    options: TaxonomyValidationOptions,
    progress: impl FnMut(OperationProgress),
    visit: impl FnMut(TaxonomyValidationIssue) -> bool,
) -> CoreResult<()> {
    visit_taxonomy_validation_issues_with_progress_internal(
        connection, options, progress, None, visit,
    )
}

pub(super) fn visit_taxonomy_validation_issues_with_progress_and_cancellation(
    connection: &Connection,
    options: TaxonomyValidationOptions,
    progress: impl FnMut(OperationProgress),
    cancellation: &CancellationToken,
    visit: impl FnMut(TaxonomyValidationIssue) -> bool,
) -> CoreResult<()> {
    visit_taxonomy_validation_issues_with_progress_internal(
        connection,
        options,
        progress,
        Some(cancellation),
        visit,
    )
}

fn visit_taxonomy_validation_issues_with_progress_internal(
    connection: &Connection,
    options: TaxonomyValidationOptions,
    mut progress: impl FnMut(OperationProgress),
    cancellation: Option<&CancellationToken>,
    mut visit: impl FnMut(TaxonomyValidationIssue) -> bool,
) -> CoreResult<()> {
    if let Some(cancellation) = cancellation {
        cancellation.check()?;
    }
    progress(validation_progress(
        "loading_taxonomy_structure",
        None,
        None,
        None,
    ));
    let taxa = connection
        .prepare("SELECT taxon_id, parent_taxon_id, rank FROM taxa ORDER BY taxon_id")?
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let by_id = taxa
        .iter()
        .map(|(taxon_id, parent_taxon_id, rank)| (*taxon_id, (*parent_taxon_id, *rank)))
        .collect::<HashMap<_, _>>();
    let total_taxa = taxa.len() as u64;
    progress(validation_progress(
        "loading_taxonomy_structure",
        Some(total_taxa),
        Some(total_taxa),
        Some(OperationProgressUnit::Taxa),
    ));
    if options.check_parent_structure {
        progress(validation_progress(
            "checking_parent_cycles",
            Some(0),
            Some(total_taxa),
            Some(OperationProgressUnit::Taxa),
        ));
        let cycle_taxa = cycle_taxon_ids_with_progress_and_cancellation(
            &by_id,
            |current, total| {
                progress(validation_progress(
                    "checking_parent_cycles",
                    Some(current),
                    Some(total),
                    Some(OperationProgressUnit::Taxa),
                ));
            },
            cancellation,
        )?;
        progress(validation_progress(
            "checking_parent_relationships",
            Some(0),
            Some(total_taxa),
            Some(OperationProgressUnit::Taxa),
        ));
        for (index, (taxon_id, parent_taxon_id, rank)) in taxa.iter().enumerate() {
            if (index + 1).is_multiple_of(10_000) {
                if let Some(cancellation) = cancellation {
                    cancellation.check()?;
                }
                progress(validation_progress(
                    "checking_parent_relationships",
                    Some((index + 1) as u64),
                    Some(total_taxa),
                    Some(OperationProgressUnit::Taxa),
                ));
            }
            if cycle_taxa.contains(&taxon_id) {
                if !visit(TaxonomyValidationIssue {
                    code: "parent_cycle",
                    message: format!("Taxon {taxon_id} belongs to a cyclic parent relationship."),
                    taxon_id: Some(*taxon_id),
                    related_taxon_id: *parent_taxon_id,
                }) {
                    return Ok(());
                }
                continue;
            }
            if *rank == TaxonRank::Kingdom.code() {
                if parent_taxon_id.is_some()
                    && !visit(TaxonomyValidationIssue {
                        code: "kingdom_has_parent",
                        message: format!("Kingdom taxon {taxon_id} must be a root taxon."),
                        taxon_id: Some(*taxon_id),
                        related_taxon_id: *parent_taxon_id,
                    })
                {
                    return Ok(());
                }
                continue;
            }
            let Some(parent_taxon_id) = parent_taxon_id else {
                if !visit(TaxonomyValidationIssue {
                    code: "missing_parent",
                    message: format!("Taxon {taxon_id} must have a parent taxon."),
                    taxon_id: Some(*taxon_id),
                    related_taxon_id: None,
                }) {
                    return Ok(());
                }
                continue;
            };
            let Some((_, parent_rank)) = by_id.get(parent_taxon_id) else {
                if !visit(TaxonomyValidationIssue {
                    code: "parent_not_found",
                    message: format!(
                        "Taxon {taxon_id} references missing parent taxon {parent_taxon_id}."
                    ),
                    taxon_id: Some(*taxon_id),
                    related_taxon_id: Some(*parent_taxon_id),
                }) {
                    return Ok(());
                }
                continue;
            };
            if *parent_rank >= *rank
                && !visit(TaxonomyValidationIssue {
                    code: "invalid_parent_rank",
                    message: format!("Taxon {taxon_id} must have a parent with a higher rank."),
                    taxon_id: Some(*taxon_id),
                    related_taxon_id: Some(*parent_taxon_id),
                })
            {
                return Ok(());
            }
        }
        progress(validation_progress(
            "checking_parent_relationships",
            Some(total_taxa),
            Some(total_taxa),
            Some(OperationProgressUnit::Taxa),
        ));
    }
    if options.check_scientific_name_count {
        progress(validation_progress(
            "checking_scientific_names",
            None,
            None,
            None,
        ));
        let mut invalid_sci_names = connection.prepare(
            r#"
            SELECT taxa.taxon_id
            FROM taxa
            LEFT JOIN taxon_names
              ON taxon_names.taxon_id = taxa.taxon_id
             AND taxon_names.name_type = 1
            GROUP BY taxa.taxon_id
            HAVING COUNT(taxon_names.name_id) != 1
            ORDER BY taxa.taxon_id
            "#,
        )?;
        for row in invalid_sci_names.query_map([], |row| row.get::<_, i64>(0))? {
            let taxon_id = row?;
            if !visit(TaxonomyValidationIssue {
                code: "invalid_sci_name_count",
                message: format!("Taxon {taxon_id} must have exactly one scientific name."),
                taxon_id: Some(taxon_id),
                related_taxon_id: None,
            }) {
                return Ok(());
            }
        }
    }
    if options.check_localized_accepted_name_count || options.check_localized_alias_dependencies {
        progress(validation_progress(
            "checking_localized_names",
            None,
            None,
            None,
        ));
    }
    if options.check_localized_accepted_name_count {
        let mut duplicate_accepted_names = connection.prepare(
            r#"
            SELECT taxon_id, name_type
            FROM taxon_names
            WHERE name_type IN (?, ?)
            GROUP BY taxon_id, name_type
            HAVING COUNT(name_id) > 1
            ORDER BY taxon_id, name_type
            "#,
        )?;
        for row in duplicate_accepted_names.query_map(
            params![
                TaxonomyNameType::ZhName.code(),
                TaxonomyNameType::EnName.code()
            ],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )? {
            let (taxon_id, name_type) = row?;
            let (code, language) = if name_type == TaxonomyNameType::ZhName.code() {
                ("invalid_zh_name_count", "Chinese")
            } else {
                ("invalid_en_name_count", "English")
            };
            if !visit(TaxonomyValidationIssue {
                code,
                message: format!(
                    "Taxon {taxon_id} must have at most one {language} accepted name."
                ),
                taxon_id: Some(taxon_id),
                related_taxon_id: None,
            }) {
                return Ok(());
            }
        }
    }
    if options.check_localized_alias_dependencies {
        let localized_name_types = [
            (
                TaxonomyNameType::ZhAlias,
                TaxonomyNameType::ZhName,
                "zh_alias_without_accepted_name",
                "Chinese",
            ),
            (
                TaxonomyNameType::EnAlias,
                TaxonomyNameType::EnName,
                "en_alias_without_accepted_name",
                "English",
            ),
        ];
        let mut aliases_without_accepted_name = connection.prepare(
            r#"
            SELECT DISTINCT alias.taxon_id
            FROM taxon_names AS alias
            WHERE alias.name_type = ?
              AND NOT EXISTS (
                SELECT 1
                FROM taxon_names AS accepted
                WHERE accepted.taxon_id = alias.taxon_id
                  AND accepted.name_type = ?
              )
            ORDER BY alias.taxon_id
            "#,
        )?;
        for (alias_type, accepted_type, code, language) in localized_name_types {
            for row in aliases_without_accepted_name
                .query_map(params![alias_type.code(), accepted_type.code()], |row| {
                    row.get::<_, i64>(0)
                })?
            {
                let taxon_id = row?;
                if !visit(TaxonomyValidationIssue {
                    code,
                    message: format!(
                        "Taxon {taxon_id} has {language} aliases but no {language} accepted name."
                    ),
                    taxon_id: Some(taxon_id),
                    related_taxon_id: None,
                }) {
                    return Ok(());
                }
            }
        }
    }
    if options.check_duplicate_name_family {
        progress(validation_progress(
            "checking_duplicate_names",
            None,
            None,
            None,
        ));
        let mut duplicate_family_names = connection.prepare(
            r#"
            SELECT taxon_id, (name_type + 1) / 2 AS name_family, name
            FROM taxon_names
            GROUP BY taxon_id, name_family, name
            HAVING COUNT(name_id) > 1
            ORDER BY taxon_id, name_family, name
            "#,
        )?;
        for row in duplicate_family_names.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })? {
            let (taxon_id, name_family, name) = row?;
            let family = match name_family {
                1 => "scientific",
                2 => "Chinese",
                3 => "English",
                _ => "invalid",
            };
            if !visit(TaxonomyValidationIssue {
                code: "duplicate_name_family",
                message: format!("Taxon {taxon_id} contains duplicate {family} name '{name}'."),
                taxon_id: Some(taxon_id),
                related_taxon_id: None,
            }) {
                return Ok(());
            }
        }
    }
    if options.check_orphan_names {
        progress(validation_progress(
            "checking_orphan_names",
            None,
            None,
            None,
        ));
        let mut orphan_name_taxa = connection.prepare(
            r#"
            SELECT DISTINCT taxon_names.taxon_id
            FROM taxon_names
            LEFT JOIN taxa ON taxa.taxon_id = taxon_names.taxon_id
            WHERE taxa.taxon_id IS NULL
            ORDER BY taxon_names.taxon_id
            "#,
        )?;
        for row in orphan_name_taxa.query_map([], |row| row.get::<_, i64>(0))? {
            let taxon_id = row?;
            if !visit(TaxonomyValidationIssue {
                code: "name_taxon_not_found",
                message: format!("Taxon names reference missing taxon {taxon_id}."),
                taxon_id: Some(taxon_id),
                related_taxon_id: None,
            }) {
                return Ok(());
            }
        }
    }
    if options.require_normalized_names {
        progress(validation_progress(
            "checking_normalized_names",
            None,
            None,
            None,
        ));
        let mut statement =
            connection.prepare("SELECT name_id, taxon_id, name FROM taxon_names")?;
        for row in statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })? {
            let (name_id, taxon_id, name) = row?;
            if normalize_name(Some(&name)).as_deref() != Some(name.as_str())
                && !visit(TaxonomyValidationIssue {
                    code: "name_not_normalized",
                    message: format!("Taxon name {name_id} is not normalized."),
                    taxon_id: Some(taxon_id),
                    related_taxon_id: None,
                })
            {
                return Ok(());
            }
        }
    }
    Ok(())
}

fn validation_progress(
    stage: &str,
    current: Option<u64>,
    total: Option<u64>,
    unit: Option<OperationProgressUnit>,
) -> OperationProgress {
    OperationProgress {
        stage: stage.into(),
        current,
        total,
        unit,
    }
}

#[cfg(test)]
pub(super) fn cycle_taxon_ids(by_id: &HashMap<i64, (Option<i64>, i64)>) -> HashSet<i64> {
    cycle_taxon_ids_with_progress(by_id, |_, _| {})
}

#[cfg(test)]
fn cycle_taxon_ids_with_progress(
    by_id: &HashMap<i64, (Option<i64>, i64)>,
    progress: impl FnMut(u64, u64),
) -> HashSet<i64> {
    cycle_taxon_ids_with_progress_and_cancellation(by_id, progress, None)
        .expect("cycle detection without cancellation cannot fail")
}

fn cycle_taxon_ids_with_progress_and_cancellation(
    by_id: &HashMap<i64, (Option<i64>, i64)>,
    progress: impl FnMut(u64, u64),
    cancellation: Option<&CancellationToken>,
) -> CoreResult<HashSet<i64>> {
    cycle_taxon_ids_with_progress_cancellation_and_traversal_hook(
        by_id,
        progress,
        cancellation,
        |_| {},
    )
}

pub(super) fn cycle_taxon_ids_with_progress_cancellation_and_traversal_hook(
    by_id: &HashMap<i64, (Option<i64>, i64)>,
    mut progress: impl FnMut(u64, u64),
    cancellation: Option<&CancellationToken>,
    mut traversal_hook: impl FnMut(usize),
) -> CoreResult<HashSet<i64>> {
    if let Some(cancellation) = cancellation {
        cancellation.check()?;
    }
    let mut states = HashMap::<i64, VisitState>::new();
    let mut cycle_taxa = HashSet::new();
    let total = by_id.len() as u64;
    let mut completed = 0_u64;
    for &origin_taxon_id in by_id.keys() {
        if states.get(&origin_taxon_id) == Some(&VisitState::Done) {
            continue;
        }
        let mut path = Vec::new();
        let mut path_positions = HashMap::new();
        let mut current_taxon_id = Some(origin_taxon_id);
        let mut traversed = 0_usize;
        while let Some(taxon_id) = current_taxon_id {
            traversed += 1;
            traversal_hook(traversed);
            if traversed.is_multiple_of(1_000) {
                if let Some(cancellation) = cancellation {
                    cancellation.check()?;
                }
            }
            if let Some(&position) = path_positions.get(&taxon_id) {
                cycle_taxa.extend(path[position..].iter().copied());
                break;
            }
            if states.get(&taxon_id) == Some(&VisitState::Done) || !by_id.contains_key(&taxon_id) {
                break;
            }
            states.insert(taxon_id, VisitState::Visiting);
            path_positions.insert(taxon_id, path.len());
            path.push(taxon_id);
            current_taxon_id = by_id[&taxon_id].0;
        }
        for taxon_id in path {
            states.insert(taxon_id, VisitState::Done);
            completed += 1;
            if completed.is_multiple_of(10_000) {
                if let Some(cancellation) = cancellation {
                    cancellation.check()?;
                }
                progress(completed, total);
            }
        }
    }
    progress(total, total);
    Ok(cycle_taxa)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Visiting,
    Done,
}

pub(super) fn validate_taxonomy(connection: &Connection) -> CoreResult<()> {
    let mut first_issue = None;
    visit_taxonomy_validation_issues(connection, true, |issue| {
        first_issue = Some(issue);
        false
    })?;
    if let Some(issue) = first_issue {
        return Err(CoreError::InvalidArgument(issue.message));
    }
    Ok(())
}

pub(super) fn validate_taxonomy_changes_with_cancellation(
    connection: &Connection,
    validation_scope: &BTreeSet<i64>,
    cancellation: &CancellationToken,
) -> CoreResult<()> {
    cancellation.check()?;
    connection.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS vividarium_validation_taxa(taxon_id INTEGER PRIMARY KEY); DELETE FROM vividarium_validation_taxa;",
    )?;
    {
        let mut insert = connection
            .prepare("INSERT OR IGNORE INTO vividarium_validation_taxa(taxon_id) VALUES (?)")?;
        for (index, taxon_id) in validation_scope.iter().enumerate() {
            if index.is_multiple_of(1_000) {
                cancellation.check()?;
            }
            insert.execute([taxon_id])?;
        }
    }

    let taxa = connection
        .prepare(
            r#"
            SELECT taxa.taxon_id, taxa.parent_taxon_id, taxa.rank
            FROM taxa
            JOIN vividarium_validation_taxa USING (taxon_id)
            "#,
        )?
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let by_id = taxa
        .iter()
        .map(|(taxon_id, parent_taxon_id, rank)| (*taxon_id, (*parent_taxon_id, *rank)))
        .collect::<HashMap<_, _>>();
    let cycles =
        cycle_taxon_ids_with_progress_and_cancellation(&by_id, |_, _| {}, Some(cancellation))?;
    if let Some(taxon_id) = cycles.iter().next() {
        return Err(CoreError::InvalidArgument(format!(
            "Taxon {taxon_id} belongs to a cyclic parent relationship."
        )));
    }
    for (index, (taxon_id, parent_taxon_id, rank)) in taxa.iter().enumerate() {
        if index.is_multiple_of(1_000) {
            cancellation.check()?;
        }
        if *rank == TaxonRank::Kingdom.code() {
            if parent_taxon_id.is_some() {
                return Err(CoreError::InvalidArgument(format!(
                    "Kingdom taxon {taxon_id} must be a root taxon."
                )));
            }
            continue;
        }
        let Some(parent_taxon_id) = parent_taxon_id else {
            return Err(CoreError::InvalidArgument(format!(
                "Taxon {taxon_id} must have a parent taxon."
            )));
        };
        let Some((_, parent_rank)) = by_id.get(parent_taxon_id) else {
            return Err(CoreError::InvalidArgument(format!(
                "Taxon {taxon_id} references missing parent taxon {parent_taxon_id}."
            )));
        };
        if *parent_rank >= *rank {
            return Err(CoreError::InvalidArgument(format!(
                "Taxon {taxon_id} must have a parent with a higher rank."
            )));
        }
    }

    let invalid_name = connection
        .query_row(
            r#"
            SELECT issue FROM (
                SELECT 'Taxon ' || taxa.taxon_id || ' must have exactly one scientific name.' AS issue
                FROM taxa
                JOIN vividarium_validation_taxa AS changed USING (taxon_id)
                LEFT JOIN taxon_names ON taxon_names.taxon_id = taxa.taxon_id AND taxon_names.name_type = 1
                GROUP BY taxa.taxon_id HAVING COUNT(taxon_names.name_id) != 1
                UNION ALL
                SELECT 'Taxon ' || names.taxon_id || ' must have at most one localized accepted name.'
                FROM taxon_names AS names
                JOIN vividarium_validation_taxa AS changed USING (taxon_id)
                WHERE names.name_type IN (3, 5)
                GROUP BY names.taxon_id, names.name_type HAVING COUNT(names.name_id) > 1
                UNION ALL
                SELECT 'Taxon ' || alias.taxon_id || ' has aliases but no accepted localized name.'
                FROM taxon_names AS alias
                JOIN vividarium_validation_taxa AS changed USING (taxon_id)
                WHERE (alias.name_type = 4 AND NOT EXISTS (
                    SELECT 1 FROM taxon_names accepted WHERE accepted.taxon_id = alias.taxon_id AND accepted.name_type = 3
                )) OR (alias.name_type = 6 AND NOT EXISTS (
                    SELECT 1 FROM taxon_names accepted WHERE accepted.taxon_id = alias.taxon_id AND accepted.name_type = 5
                ))
                UNION ALL
                SELECT 'Taxon ' || names.taxon_id || ' contains duplicate names in one name family.'
                FROM taxon_names AS names
                JOIN vividarium_validation_taxa AS changed USING (taxon_id)
                GROUP BY names.taxon_id, (names.name_type + 1) / 2, names.name
                HAVING COUNT(names.name_id) > 1
                UNION ALL
                SELECT 'Taxon names reference missing taxon ' || names.taxon_id || '.'
                FROM taxon_names AS names
                JOIN vividarium_validation_taxa AS changed USING (taxon_id)
                LEFT JOIN taxa USING (taxon_id)
                WHERE taxa.taxon_id IS NULL
            ) LIMIT 1
            "#,
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(message) = invalid_name {
        return Err(CoreError::InvalidArgument(message));
    }
    let mut names = connection.prepare(
        r#"
        SELECT names.name_id, names.name
        FROM taxon_names AS names
        JOIN vividarium_validation_taxa AS changed USING (taxon_id)
        "#,
    )?;
    for (index, row) in names
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .enumerate()
    {
        if index.is_multiple_of(1_000) {
            cancellation.check()?;
        }
        let (name_id, name) = row?;
        if normalize_name(Some(&name)).as_deref() != Some(name.as_str()) {
            return Err(CoreError::InvalidArgument(format!(
                "Taxon name {name_id} is not normalized."
            )));
        }
    }
    connection.execute_batch("DELETE FROM vividarium_validation_taxa")?;
    Ok(())
}

pub(super) fn taxonomy_validation_scope_with_cancellation(
    connection: &Connection,
    affected_taxon_ids: &BTreeSet<i64>,
    limit: usize,
    cancellation: &CancellationToken,
) -> CoreResult<TaxonomyValidationScope> {
    cancellation.check()?;
    if affected_taxon_ids.len() > limit {
        return Ok(TaxonomyValidationScope::Full);
    }
    let query_limit = limit.checked_add(1).ok_or_else(|| {
        CoreError::InvalidArgument("incremental validation limit is too large".into())
    })?;
    let query_limit = i64::try_from(query_limit).map_err(|_| {
        CoreError::InvalidArgument("incremental validation limit is too large".into())
    })?;
    connection.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS vividarium_validation_taxa(taxon_id INTEGER PRIMARY KEY); DELETE FROM vividarium_validation_taxa;",
    )?;
    {
        let mut insert = connection
            .prepare("INSERT OR IGNORE INTO vividarium_validation_taxa(taxon_id) VALUES (?)")?;
        for (index, taxon_id) in affected_taxon_ids.iter().enumerate() {
            if index.is_multiple_of(1_000) {
                cancellation.check()?;
            }
            insert.execute([taxon_id])?;
        }
    }
    let mut statement = connection.prepare(
        r#"
        WITH RECURSIVE relevant(taxon_id) AS (
            SELECT taxon_id FROM vividarium_validation_taxa
            UNION
            SELECT taxa.taxon_id
            FROM taxa
            JOIN vividarium_validation_taxa AS changed
              ON changed.taxon_id = taxa.parent_taxon_id
            UNION
            SELECT taxa.parent_taxon_id
            FROM taxa JOIN relevant ON taxa.taxon_id = relevant.taxon_id
            WHERE taxa.parent_taxon_id IS NOT NULL
        )
        SELECT taxon_id FROM relevant
        LIMIT ?
        "#,
    )?;
    let mut scope = BTreeSet::new();
    let mut exceeded_limit = false;
    for (index, taxon_id) in statement
        .query_map([query_limit], |row| row.get::<_, i64>(0))?
        .enumerate()
    {
        if index.is_multiple_of(1_000) {
            cancellation.check()?;
        }
        scope.insert(taxon_id?);
        if scope.len() > limit {
            exceeded_limit = true;
            break;
        }
    }
    drop(statement);
    connection.execute_batch("DELETE FROM vividarium_validation_taxa")?;
    if exceeded_limit {
        Ok(TaxonomyValidationScope::Full)
    } else {
        Ok(TaxonomyValidationScope::Incremental(scope))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TaxonomyValidationScope {
    Incremental(BTreeSet<i64>),
    Full,
}

pub(super) fn validate_taxonomy_with_progress_and_cancellation(
    connection: &Connection,
    progress: impl FnMut(OperationProgress),
    cancellation: &CancellationToken,
) -> CoreResult<()> {
    validate_taxonomy_with_progress_internal(connection, progress, Some(cancellation))
}

fn validate_taxonomy_with_progress_internal(
    connection: &Connection,
    progress: impl FnMut(OperationProgress),
    cancellation: Option<&CancellationToken>,
) -> CoreResult<()> {
    let mut first_issue = None;
    visit_taxonomy_validation_issues_with_progress_internal(
        connection,
        TaxonomyValidationOptions::full(),
        progress,
        cancellation,
        |issue| {
            first_issue = Some(issue);
            false
        },
    )?;
    if let Some(issue) = first_issue {
        return Err(CoreError::InvalidArgument(issue.message));
    }
    Ok(())
}

#[cfg(test)]
#[path = "validation/tests.rs"]
mod tests;
