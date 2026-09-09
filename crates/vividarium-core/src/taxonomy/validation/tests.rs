use std::collections::{HashMap, HashSet};

use rusqlite::params;
use tempfile::TempDir;

use super::*;
use crate::taxonomy::TaxonomyNameType;
use crate::{CancellationToken, CoreError, CoreResult, Database};

fn database() -> (TempDir, Database) {
    let directory = TempDir::new().unwrap();
    let database = Database::open(directory.path().join("test.db")).unwrap();
    (directory, database)
}

fn validate_parentage(rows: &[(i64, Option<i64>, i64)]) -> CoreResult<()> {
    let (_directory, database) = database();
    let connection = database.connect_taxonomy_metadata_context()?;
    connection.execute_batch("PRAGMA foreign_keys = OFF")?;
    for (taxon_id, parent_taxon_id, rank) in rows {
        connection.execute(
            "INSERT INTO taxa (taxon_id, parent_taxon_id, rank) VALUES (?, ?, ?)",
            params![taxon_id, parent_taxon_id, rank],
        )?;
        connection.execute(
            "INSERT INTO taxon_names (taxon_id, name_type, name) VALUES (?, 1, ?)",
            params![taxon_id, format!("Taxon {taxon_id}")],
        )?;
    }
    validate_taxonomy(&connection)
}

#[test]
fn parentage_validation_accepts_the_complete_five_rank_tree() {
    validate_parentage(&[
        (1, None, 1),
        (2, Some(1), 2),
        (3, Some(2), 3),
        (4, Some(3), 4),
        (5, Some(4), 5),
    ])
    .unwrap();
}

#[test]
fn parentage_validation_accepts_skipped_ranks() {
    validate_parentage(&[
        (1, None, 1),
        (2, Some(1), 2),
        (3, Some(1), 3),
        (4, Some(1), 4),
        (5, Some(2), 5),
        (6, Some(3), 5),
    ])
    .unwrap();
}

#[test]
fn parentage_validation_rejects_equal_parent_and_child_ranks() {
    let error = validate_parentage(&[(1, None, 1), (2, Some(1), 2), (3, Some(2), 2)]).unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid argument: Taxon 3 must have a parent with a higher rank."
    );
}

#[test]
fn parentage_validation_rejects_a_lower_rank_parent() {
    let error = validate_parentage(&[(1, None, 1), (2, Some(1), 5), (3, Some(2), 3)]).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Taxon 3 must have a parent with a higher rank.")
    );
}

#[test]
fn parentage_validation_rejects_a_missing_parent() {
    let error = validate_parentage(&[(1, None, 1), (2, Some(99), 2)]).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Taxon 2 references missing parent taxon 99.")
    );
}

#[test]
fn parentage_validation_rejects_a_cycle() {
    let error = validate_parentage(&[(1, Some(2), 2), (2, Some(1), 3)]).unwrap_err();
    assert!(error.to_string().contains("cyclic parent relationship"));
}

fn cycle_ids(rows: &[(i64, Option<i64>)]) -> HashSet<i64> {
    cycle_taxon_ids(
        &rows
            .iter()
            .map(|(taxon_id, parent_taxon_id)| (*taxon_id, (*parent_taxon_id, 1)))
            .collect(),
    )
}

#[test]
fn cycle_detection_handles_roots_missing_parents_and_disconnected_components() {
    assert!(cycle_ids(&[(1, None), (2, Some(1)), (3, Some(99)), (4, None)]).is_empty());
}

#[test]
fn cycle_detection_reports_only_members_of_each_cycle() {
    assert_eq!(cycle_ids(&[(1, Some(1))]), HashSet::from([1]));
    assert_eq!(
        cycle_ids(&[(1, Some(2)), (2, Some(1))]),
        HashSet::from([1, 2])
    );
    assert_eq!(
        cycle_ids(&[(1, Some(2)), (2, Some(3)), (3, Some(1)), (4, Some(2))]),
        HashSet::from([1, 2, 3])
    );
}

#[test]
fn cycle_detection_handles_a_deep_valid_lineage_once() {
    let rows = (1..=10_000)
        .map(|taxon_id| (taxon_id, (taxon_id > 1).then_some(taxon_id - 1)))
        .collect::<Vec<_>>();
    assert!(cycle_ids(&rows).is_empty());
}

#[test]
fn deep_cycle_detection_observes_cancellation_during_traversal() {
    let by_id = (1..=100_000)
        .map(|taxon_id| {
            let parent_taxon_id = if taxon_id == 100_000 { 1 } else { taxon_id + 1 };
            (taxon_id, (Some(parent_taxon_id), 1))
        })
        .collect::<HashMap<_, _>>();
    let cancellation = CancellationToken::new();
    let mut maximum_traversed = 0;
    assert!(!cancellation.is_cancelled());

    let result = cycle_taxon_ids_with_progress_cancellation_and_traversal_hook(
        &by_id,
        |_, _| {},
        Some(&cancellation),
        |traversed| {
            maximum_traversed = maximum_traversed.max(traversed);
            if traversed == 999 {
                cancellation.cancel();
            }
        },
    );

    assert!(matches!(result, Err(CoreError::Cancelled)));
    assert_eq!(maximum_traversed, 1_000);
}

#[test]
fn parentage_validation_rejects_a_parentless_non_kingdom() {
    let error = validate_parentage(&[(1, None, 1), (2, None, 3)]).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Taxon 2 must have a parent taxon.")
    );
}

#[test]
fn parentage_validation_rejects_a_kingdom_with_a_parent() {
    let error = validate_parentage(&[(1, Some(2), 1), (2, None, 1)]).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Kingdom taxon 1 must be a root taxon.")
    );
}

fn accepted_name_count_issues(name_type: TaxonomyNameType) -> Vec<TaxonomyValidationIssue> {
    let (_directory, database) = database();
    let connection = database.connect_taxonomy_metadata_context().unwrap();
    connection
        .execute_batch(&format!(
            "DROP INDEX idx_taxon_names_one_{}_name",
            match name_type {
                TaxonomyNameType::ZhName => "zh",
                TaxonomyNameType::EnName => "en",
                _ => unreachable!(),
            }
        ))
        .unwrap();
    connection
        .execute("INSERT INTO taxa (taxon_id, rank) VALUES (1, 1)", [])
        .unwrap();
    connection
        .execute(
            "INSERT INTO taxon_names (taxon_id, name_type, name) VALUES (1, 1, 'Animalia')",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO taxon_names (taxon_id, name_type, name) VALUES (1, ?, 'Accepted one'), (1, ?, 'Accepted two')",
            params![name_type.code(), name_type.code()],
        )
        .unwrap();
    let mut issues = Vec::new();
    visit_taxonomy_validation_issues(&connection, true, |issue| {
        issues.push(issue);
        true
    })
    .unwrap();
    issues
}

#[test]
fn taxonomy_validation_rejects_multiple_chinese_accepted_names() {
    let issues = accepted_name_count_issues(TaxonomyNameType::ZhName);

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].code, "invalid_zh_name_count");
    assert_eq!(issues[0].taxon_id, Some(1));
    assert_eq!(
        issues[0].message,
        "Taxon 1 must have at most one Chinese accepted name."
    );
}

#[test]
fn taxonomy_validation_rejects_multiple_english_accepted_names() {
    let issues = accepted_name_count_issues(TaxonomyNameType::EnName);

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].code, "invalid_en_name_count");
    assert_eq!(issues[0].taxon_id, Some(1));
    assert_eq!(
        issues[0].message,
        "Taxon 1 must have at most one English accepted name."
    );
}

#[test]
fn taxonomy_validation_allows_multiple_alias_names() {
    let (_directory, database) = database();
    let connection = database.connect_taxonomy_metadata_context().unwrap();
    connection
        .execute("INSERT INTO taxa (taxon_id, rank) VALUES (1, 1)", [])
        .unwrap();
    connection
        .execute_batch(
            r#"
            INSERT INTO taxon_names (taxon_id, name_type, name) VALUES
                (1, 1, 'Animalia'),
                (1, 2, 'Scientific alias one'),
                (1, 2, 'Scientific alias two'),
                (1, 3, 'Chinese accepted name'),
                (1, 4, 'Chinese alias one'),
                (1, 4, 'Chinese alias two'),
                (1, 5, 'English accepted name'),
                (1, 6, 'English alias one'),
                (1, 6, 'English alias two');
            "#,
        )
        .unwrap();

    validate_taxonomy(&connection).unwrap();
}

fn localized_alias_dependency_issues(
    alias_type: TaxonomyNameType,
    options: TaxonomyValidationOptions,
) -> Vec<TaxonomyValidationIssue> {
    let (_directory, database) = database();
    let connection = database.connect_taxonomy_metadata_context().unwrap();
    connection
        .execute_batch(
            "INSERT INTO taxa (taxon_id, rank) VALUES (1, 1);\n\
             INSERT INTO taxon_names (taxon_id, name_type, name) VALUES (1, 1, 'Animalia');",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO taxon_names (taxon_id, name_type, name) VALUES (1, ?, 'Localized alias')",
            [alias_type.code()],
        )
        .unwrap();
    let mut issues = Vec::new();
    visit_taxonomy_validation_issues_with_options(&connection, options, |issue| {
        issues.push(issue);
        true
    })
    .unwrap();
    issues
}

#[test]
fn taxonomy_validation_rejects_chinese_aliases_without_an_accepted_name() {
    let issues = localized_alias_dependency_issues(
        TaxonomyNameType::ZhAlias,
        TaxonomyValidationOptions::full(),
    );

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].code, "zh_alias_without_accepted_name");
    assert_eq!(issues[0].taxon_id, Some(1));
    assert_eq!(
        issues[0].message,
        "Taxon 1 has Chinese aliases but no Chinese accepted name."
    );
}

#[test]
fn taxonomy_validation_rejects_english_aliases_without_an_accepted_name() {
    let issues = localized_alias_dependency_issues(
        TaxonomyNameType::EnAlias,
        TaxonomyValidationOptions::full(),
    );

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].code, "en_alias_without_accepted_name");
    assert_eq!(issues[0].taxon_id, Some(1));
    assert_eq!(
        issues[0].message,
        "Taxon 1 has English aliases but no English accepted name."
    );
}

#[test]
fn staging_validation_checks_localized_alias_dependencies() {
    let issues = localized_alias_dependency_issues(
        TaxonomyNameType::ZhAlias,
        TaxonomyValidationOptions::sql_import_staging(),
    );

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].code, "zh_alias_without_accepted_name");
}

#[test]
fn taxonomy_validation_allows_empty_localized_name_families() {
    let (_directory, database) = database();
    let connection = database.connect_taxonomy_metadata_context().unwrap();
    connection
        .execute_batch(
            "INSERT INTO taxa (taxon_id, rank) VALUES (1, 1);\n\
             INSERT INTO taxon_names (taxon_id, name_type, name) VALUES (1, 1, 'Animalia');",
        )
        .unwrap();

    validate_taxonomy(&connection).unwrap();
}

#[test]
fn taxonomy_schema_enforces_name_family_uniqueness() {
    let (_directory, database) = database();
    let connection = database.connect_taxonomy_metadata_context().unwrap();
    connection
        .execute("INSERT INTO taxa (taxon_id, rank) VALUES (1, 1)", [])
        .unwrap();
    connection
        .execute(
            "INSERT INTO taxon_names (taxon_id, name_type, name) VALUES (1, 1, 'Animalia')",
            [],
        )
        .unwrap();

    assert!(
        connection
            .execute(
                "INSERT INTO taxon_names (taxon_id, name_type, name) VALUES (1, 2, 'Animalia')",
                [],
            )
            .is_err()
    );
    connection
        .execute_batch(
            r#"
            INSERT INTO taxon_names (taxon_id, name_type, name)
                VALUES (1, 2, 'animalia');
            INSERT INTO taxon_names (taxon_id, name_type, name)
                VALUES (1, 3, 'Animalia');
            "#,
        )
        .unwrap();
}

#[test]
fn taxonomy_validation_rejects_duplicate_names_within_a_family() {
    let (_directory, database) = database();
    let connection = database.connect_taxonomy_metadata_context().unwrap();
    connection
        .execute_batch(
            r#"
            DROP INDEX idx_taxon_names_scientific_family_name;
            INSERT INTO taxa (taxon_id, rank) VALUES (1, 1);
            INSERT INTO taxon_names (taxon_id, name_type, name) VALUES
                (1, 1, 'Animalia'),
                (1, 2, 'Animalia');
            "#,
        )
        .unwrap();
    let mut issues = Vec::new();

    visit_taxonomy_validation_issues(&connection, true, |issue| {
        issues.push(issue);
        true
    })
    .unwrap();

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].code, "duplicate_name_family");
    assert_eq!(issues[0].taxon_id, Some(1));
    assert_eq!(
        issues[0].message,
        "Taxon 1 contains duplicate scientific name 'Animalia'."
    );
}

#[test]
fn staging_validation_skips_duplicate_name_family_checks() {
    let (_directory, database) = database();
    let connection = database.connect_taxonomy_metadata_context().unwrap();
    connection
        .execute_batch(
            r#"
            DROP INDEX idx_taxon_names_scientific_family_name;
            INSERT INTO taxa (taxon_id, rank) VALUES (1, 1);
            INSERT INTO taxon_names (taxon_id, name_type, name) VALUES
                (1, 1, 'Animalia'),
                (1, 2, 'Animalia');
            "#,
        )
        .unwrap();
    let mut issues = Vec::new();
    visit_taxonomy_validation_issues_with_options(
        &connection,
        TaxonomyValidationOptions::sql_import_staging(),
        |issue| {
            issues.push(issue);
            true
        },
    )
    .unwrap();

    assert!(issues.is_empty());
}
