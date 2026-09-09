use super::*;
use crate::taxonomy::{AddSqlInputRequest, RemoveSqlInputRequest, SqlInputKind};
use crate::taxonomy::{TaxonInputRow, apply_rows, get_taxon_detail, list_operations};

const SIMPLE_IMPORT_SQL: &str = r#"
ATTACH DATABASE 'vividarium_sql_import.db' AS sql_import;
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;
CREATE TABLE sql_import.taxa (
    taxon_id INTEGER PRIMARY KEY,
    parent_taxon_id INTEGER,
    rank INTEGER NOT NULL,
    geological_range TEXT,
    CHECK (rank BETWEEN 1 AND 5),
    FOREIGN KEY (parent_taxon_id)
        REFERENCES taxa(taxon_id) ON DELETE RESTRICT
);
CREATE TABLE sql_import.taxon_names (
    name_id INTEGER PRIMARY KEY,
    taxon_id INTEGER NOT NULL,
    name_type INTEGER NOT NULL,
    name TEXT NOT NULL,
    normalized_name TEXT,
    authority_year TEXT,
    source TEXT,
    CHECK (name_type BETWEEN 1 AND 6),
    CHECK (length(trim(name)) > 0),
    FOREIGN KEY (taxon_id)
        REFERENCES taxa(taxon_id) ON DELETE CASCADE
);
CREATE UNIQUE INDEX sql_import.idx_taxon_names_scientific_family_name
    ON taxon_names(taxon_id, name) WHERE name_type IN (1, 2);
CREATE UNIQUE INDEX sql_import.idx_taxon_names_chinese_family_name
    ON taxon_names(taxon_id, name) WHERE name_type IN (3, 4);
CREATE UNIQUE INDEX sql_import.idx_taxon_names_english_family_name
    ON taxon_names(taxon_id, name) WHERE name_type IN (5, 6);
CREATE INDEX sql_import.idx_taxon_names_taxon_type
    ON taxon_names(taxon_id, name_type);
INSERT INTO sql_import.taxa
SELECT CAST(taxon_id AS INTEGER), NULL, CAST(rank AS INTEGER), geological_range
FROM source_taxa;
INSERT INTO sql_import.taxon_names
SELECT 1, CAST(taxon_id AS INTEGER), 1, name, NULL, NULL, 'test'
FROM source_taxa;
COMMIT;
DETACH DATABASE sql_import;
"#;

fn add_simple_input(directory: &tempfile::TempDir, database: &Database) {
    let csv_path = directory.path().join("simple-source.csv");
    fs::write(
        &csv_path,
        "taxon_id,rank,name,geological_range\n101,1,Animalia,Recent\n",
    )
    .unwrap();
    add_sql_import_input(
        database,
        &AddSqlInputRequest {
            kind: SqlInputKind::Csv,
            alias: "source_taxa".into(),
            path: csv_path,
        },
    )
    .unwrap();
}

fn execute_simple(database: &Database) -> SqlImportExecutionResult {
    execute_sql_import_sql(
        database,
        &ValidateSqlImportRequest {
            sql: SIMPLE_IMPORT_SQL.into(),
        },
    )
    .unwrap()
}

#[test]
fn validate_executes_sql_and_builds_the_candidate_in_one_request() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    add_simple_input(&directory, &database);

    let result = validate_sql_import(
        &database,
        &ValidateSqlImportRequest {
            sql: SIMPLE_IMPORT_SQL.into(),
        },
    )
    .unwrap();

    assert!(result.execution.statements_executed > 0);
    assert!(result.execution.script_saved);
    assert!(result.can_apply);
    assert!(result.validation.valid);
    assert!(result.validation.can_apply);
    assert!(result.warnings.is_empty());
    let workspace = workspace(&database).unwrap();
    assert!(workspace.join(CANDIDATE_DATABASE).is_file());
    assert!(workspace.join(VALIDATION_STATE).is_file());
}

#[test]
fn sql_import_staging_schemas_follow_the_staging_database() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    assert!(
        list_sql_import_staging_schemas(&database)
            .unwrap()
            .is_empty()
    );
    add_simple_input(&directory, &database);

    execute_simple(&database);
    let schemas = list_sql_import_staging_schemas(&database).unwrap();

    assert_eq!(schemas.len(), 1);
    assert_eq!(schemas[0].alias, "sql_import");
    assert!(
        schemas[0]
            .objects
            .iter()
            .any(|object| object.name == "taxa")
    );
    assert!(
        schemas[0]
            .objects
            .iter()
            .any(|object| object.name == "taxon_names")
    );
}

#[test]
fn staging_schema_indexes_scientific_name_validation_by_taxon_and_type() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    add_simple_input(&directory, &database);
    execute_simple(&database);
    let connection =
        Connection::open(workspace(&database).unwrap().join(STAGING_DATABASE)).unwrap();
    let columns = connection
        .prepare("PRAGMA index_info('idx_taxon_names_taxon_type')")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(2))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(columns, ["taxon_id", "name_type"]);
    let plan = connection
        .prepare(
            r#"
            EXPLAIN QUERY PLAN
            SELECT taxa.taxon_id
            FROM taxa
            LEFT JOIN taxon_names
              ON taxon_names.taxon_id = taxa.taxon_id
             AND taxon_names.name_type = 1
            GROUP BY taxa.taxon_id
            HAVING COUNT(taxon_names.name_id) != 1
            ORDER BY taxa.taxon_id
            "#,
        )
        .unwrap()
        .query_map([], |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(
        plan.iter()
            .any(|detail| detail.contains("idx_taxon_names_taxon_type"))
    );
    assert!(
        plan.iter()
            .all(|detail| !detail.to_ascii_uppercase().contains("AUTOMATIC"))
    );
}

#[test]
fn staging_access_index_preserves_name_family_uniqueness() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    add_simple_input(&directory, &database);
    execute_simple(&database);
    let connection =
        Connection::open(workspace(&database).unwrap().join(STAGING_DATABASE)).unwrap();
    assert!(
        connection
            .execute(
                "INSERT INTO taxon_names (taxon_id, name_type, name) VALUES (101, 2, 'Animalia')",
                [],
            )
            .is_err()
    );
    connection
        .execute(
            "INSERT INTO taxon_names (taxon_id, name_type, name) VALUES (101, 3, 'Animal')",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO taxon_names (taxon_id, name_type, name) VALUES (101, 5, 'Animal')",
            [],
        )
        .unwrap();
    assert!(
        connection
            .execute(
                "INSERT INTO taxon_names (taxon_id, name_type, name) VALUES (101, 4, 'Animal')",
                [],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "INSERT INTO taxon_names (taxon_id, name_type, name) VALUES (101, 6, 'Animal')",
                [],
            )
            .is_err()
    );
}

#[test]
fn sql_import_database_schemas_expose_the_current_taxonomy() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();

    let schemas = list_sql_import_database_schemas(&database).unwrap();

    assert_eq!(schemas.len(), 1);
    assert_eq!(schemas[0].alias, "taxonomy");
    assert!(
        schemas[0]
            .objects
            .iter()
            .any(|object| object.name == "taxa")
    );
    assert!(
        schemas[0]
            .objects
            .iter()
            .any(|object| object.name == "taxon_names")
    );
}

#[test]
fn sql_import_can_read_but_not_mutate_the_current_taxonomy() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    apply_rows(
        &database,
        &[TaxonInputRow {
            kingdom: Some("Existing kingdom".into()),
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();
    let sql = r#"
ATTACH DATABASE 'vividarium_sql_import.db' AS sql_import;
CREATE TABLE sql_import.existing_taxa AS
SELECT taxon_id, rank FROM taxonomy.taxa;
DETACH DATABASE sql_import;
"#;

    execute_sql_import_sql(&database, &ValidateSqlImportRequest { sql: sql.into() }).unwrap();
    let staging = workspace(&database).unwrap().join(STAGING_DATABASE);
    assert_eq!(
        Connection::open(staging)
            .unwrap()
            .query_row("SELECT COUNT(*) FROM existing_taxa", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );

    let error = execute_sql_import_sql(
        &database,
        &ValidateSqlImportRequest {
            sql: "DELETE FROM taxonomy.taxa;".into(),
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("not authorized"));
}

#[test]
fn sql_import_uses_the_configured_csv_delimiter() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    crate::general::update_general_settings(
        &database,
        &crate::general::GeneralSettings {
            csv_delimiter: ";".into(),
            ..crate::general::GeneralSettings::default()
        },
    )
    .unwrap();
    let csv_path = directory.path().join("semicolon-source.csv");
    fs::write(
        &csv_path,
        "taxon_id;rank;name;geological_range\n101;1;Animalia;Recent\n",
    )
    .unwrap();
    add_sql_import_input(
        &database,
        &AddSqlInputRequest {
            kind: SqlInputKind::Csv,
            alias: "source_taxa".into(),
            path: csv_path,
        },
    )
    .unwrap();
    let result = validate_sql_import(
        &database,
        &ValidateSqlImportRequest {
            sql: SIMPLE_IMPORT_SQL.into(),
        },
    )
    .unwrap();
    assert!(result.validation.valid);
    assert_eq!(result.validation.taxa_count, 1);
}

#[test]
fn validate_reports_real_stages_and_sql_statement_progress() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    add_simple_input(&directory, &database);
    let mut progress = Vec::new();

    let result = validate_sql_import_with_progress(
        &database,
        &ValidateSqlImportRequest {
            sql: SIMPLE_IMPORT_SQL.into(),
        },
        &mut |event| progress.push(event),
    )
    .unwrap();

    assert!(result.can_apply);
    let stages = progress
        .iter()
        .map(|event| event.stage.as_str())
        .collect::<Vec<_>>();
    let expected = [
        PREPARING_INPUT_SOURCES,
        EXECUTING_SQL,
        FINALIZING_STAGING_DATABASE,
        FINGERPRINTING_STAGING,
        CHECKING_STAGING_DATABASE,
        INSPECTING_STAGING_SCHEMA,
        NORMALIZING_NAMES,
        VALIDATING_STAGING_TAXONOMY,
        "loading_taxonomy_structure",
        "checking_parent_cycles",
        "checking_parent_relationships",
        "checking_scientific_names",
        "checking_localized_names",
        "checking_orphan_names",
        BUILDING_CANDIDATE_TAXA,
        BUILDING_CANDIDATE_NAMES,
        VALIDATING_CANDIDATE_DATABASE,
        "checking_duplicate_names",
        "checking_normalized_names",
        READY_TO_APPLY,
    ];
    let mut previous = 0;
    for stage in expected {
        let index = stages[previous..]
            .iter()
            .position(|candidate| *candidate == stage)
            .map(|index| index + previous)
            .unwrap_or_else(|| panic!("missing progress stage {stage}: {stages:?}"));
        previous = index;
    }
    let sql_events = progress
        .iter()
        .filter(|event| event.stage == EXECUTING_SQL)
        .collect::<Vec<_>>();
    assert!(!sql_events.is_empty());
    assert_eq!(sql_events[0].current, Some(1));
    assert_eq!(
        sql_events[0].total,
        Some(result.execution.statements_executed as u64)
    );
    assert_eq!(sql_events[0].unit, Some(OperationProgressUnit::Statements));
    assert_eq!(sql_events.last().unwrap().current, sql_events[0].total);
    for stage in ["checking_parent_cycles", "checking_parent_relationships"] {
        let events = progress
            .iter()
            .filter(|event| event.stage == stage)
            .collect::<Vec<_>>();
        let mut start = 0;
        for index in 1..=events.len() {
            if index == events.len() || events[index - 1].current > events[index].current {
                let run = &events[start..index];
                assert!(
                    run.windows(2)
                        .all(|events| events[0].current <= events[1].current)
                );
                let final_event = run.last().unwrap();
                assert_eq!(final_event.current, final_event.total);
                assert_eq!(final_event.unit, Some(OperationProgressUnit::Taxa));
                start = index;
            }
        }
    }
    assert!(
        progress
            .iter()
            .filter(|event| {
                matches!(
                    event.stage.as_str(),
                    "checking_scientific_names"
                        | "checking_localized_names"
                        | "checking_orphan_names"
                )
            })
            .all(|event| event.current.is_none() && event.total.is_none() && event.unit.is_none())
    );
    for stage in ["checking_duplicate_names", "checking_normalized_names"] {
        assert_eq!(
            progress.iter().filter(|event| event.stage == stage).count(),
            1,
            "candidate validation stage {stage} should run once"
        );
    }
    let fingerprint_events = progress
        .iter()
        .filter(|event| event.stage == FINGERPRINTING_STAGING)
        .collect::<Vec<_>>();
    assert!(!fingerprint_events.is_empty());
    assert_eq!(fingerprint_events[0].current, Some(0));
    assert!(
        fingerprint_events
            .iter()
            .all(|event| event.unit == Some(OperationProgressUnit::Bytes))
    );
    assert!(fingerprint_events.windows(2).all(|events| {
        events[0].current.unwrap() <= events[1].current.unwrap()
            && events[0].total == events[1].total
    }));
    let fingerprint_final = fingerprint_events.last().unwrap();
    assert_eq!(fingerprint_final.current, fingerprint_final.total);
    let normalization_events = progress
        .iter()
        .filter(|event| event.stage == NORMALIZING_NAMES)
        .collect::<Vec<_>>();
    assert!(normalization_events.windows(2).all(|events| {
        events[0].current.unwrap() <= events[1].current.unwrap()
            && events[0].total == events[1].total
    }));
    assert!(progress.iter().any(|event| {
        event.stage == NORMALIZING_NAMES && event.current == event.total && event.total == Some(1)
    }));
    assert!(progress.iter().any(|event| {
        event.stage == BUILDING_CANDIDATE_TAXA
            && event.current.is_none()
            && event.total.is_none()
            && event.unit == Some(OperationProgressUnit::Taxa)
    }));
    assert!(progress.iter().any(|event| {
        event.stage == BUILDING_CANDIDATE_TAXA
            && event.current == event.total
            && event.total == Some(1)
    }));
    assert!(progress.iter().any(|event| {
        event.stage == BUILDING_CANDIDATE_NAMES
            && event.current == event.total
            && event.total == Some(1)
    }));
}

#[test]
fn sql_statement_count_uses_sqlite_statement_boundaries() {
    assert_eq!(count_sql_statements("SELECT ';'; SELECT 2;").unwrap(), 2);
    assert_eq!(count_sql_statements("SELECT 1").unwrap(), 1);
}

#[test]
fn sql_statement_progress_reports_each_statement_before_execution() {
    for (sql, expected) in [("SELECT 1;", vec![1]), ("SELECT 1; SELECT 2;", vec![1, 2])] {
        let connection = Connection::open_in_memory().unwrap();
        let mut progress = Vec::new();
        let messages = execute_sql_import_script(
            &connection,
            sql,
            "unused.db",
            &mut |event| progress.push(event),
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(messages.len(), expected.len());
        assert_eq!(
            progress
                .iter()
                .map(|event| event.current.unwrap())
                .collect::<Vec<_>>(),
            expected
        );
        assert!(progress.iter().all(|event| {
            event.total == Some(messages.len() as u64)
                && event.unit == Some(OperationProgressUnit::Statements)
        }));
    }
}

#[test]
fn validate_stops_after_sql_execution_failure() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();

    let mut progress = Vec::new();
    let error = validate_sql_import_with_progress(
        &database,
        &ValidateSqlImportRequest {
            sql: "SELECT * FROM missing_source;".into(),
        },
        &mut |event| progress.push(event),
    )
    .unwrap_err();

    assert!(error.to_string().contains("missing_source"));
    let workspace = workspace(&database).unwrap();
    assert!(!workspace.join(CANDIDATE_DATABASE).exists());
    assert!(!workspace.join(VALIDATION_STATE).exists());
    assert!(
        progress
            .iter()
            .all(|event| event.stage != FINALIZING_STAGING_DATABASE)
    );
}

#[test]
fn persistent_inputs_and_successful_sql_survive_apply_and_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let metadata_path = directory.path().join("metadata.db");
    let database = Database::open(&metadata_path).unwrap();
    apply_rows(
        &database,
        &[TaxonInputRow {
            kingdom: Some("Old kingdom".into()),
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();
    let old_identity = database.taxonomy_identity().unwrap();
    add_simple_input(&directory, &database);

    let execution = execute_simple(&database);
    assert!(execution.statements_executed > 0);
    assert_eq!(execution.messages.len(), execution.statements_executed);
    assert!(execution.script_saved);
    assert!(execution.warnings.is_empty());
    let validation = validate_sql_import_candidate(&database).unwrap();
    assert!(validation.can_apply, "{:?}", validation.errors);
    let mut progress = Vec::new();
    let result = apply_sql_import_with_progress_and_cancellation(
        &database,
        &mut |event| {
            if event.stage == APPLYING_SQL_IMPORT {
                assert_eq!(database.taxonomy_identity().unwrap(), old_identity);
            }
            progress.push(event);
        },
        &CancellationToken::new(),
    )
    .unwrap();

    assert_eq!(result.metadata.taxa_count, 1);
    assert!(result.warnings.is_empty());
    assert_ne!(database.taxonomy_identity().unwrap(), old_identity);
    let stages = progress
        .iter()
        .map(|event| event.stage.as_str())
        .collect::<Vec<_>>();
    let validating = stages
        .iter()
        .position(|stage| *stage == VALIDATING_SQL_IMPORT_CANDIDATE)
        .unwrap();
    let fingerprinting = stages
        .iter()
        .position(|stage| *stage == FINGERPRINTING_STAGING)
        .unwrap();
    let applying = stages
        .iter()
        .position(|stage| *stage == APPLYING_SQL_IMPORT)
        .unwrap();
    assert!(validating < fingerprinting && fingerprinting < applying);
    let fingerprint = progress
        .iter()
        .filter(|event| event.stage == FINGERPRINTING_STAGING)
        .collect::<Vec<_>>();
    assert_eq!(fingerprint.first().unwrap().current, Some(0));
    assert_eq!(
        fingerprint.last().unwrap().current,
        fingerprint.last().unwrap().total
    );
    assert_eq!(
        get_taxon_detail(&database, 101)
            .unwrap()
            .unwrap()
            .names
            .sci_name
            .unwrap()
            .name,
        "Animalia"
    );
    assert!(
        list_operations(&database, None, 10)
            .unwrap()
            .items
            .is_empty()
    );
    assert_eq!(list_sql_import_inputs(&database).unwrap().len(), 1);
    assert_eq!(get_sql_import_sql(&database).unwrap(), SIMPLE_IMPORT_SQL);
    let workspace = workspace(&database).unwrap();
    assert!(!workspace.join(STAGING_DATABASE).exists());
    assert!(!workspace.join(CANDIDATE_DATABASE).exists());
    assert!(!workspace.join(VALIDATION_STATE).exists());
    drop(database);

    let database = Database::open(metadata_path).unwrap();
    assert_eq!(list_sql_import_inputs(&database).unwrap().len(), 1);
    assert_eq!(get_sql_import_sql(&database).unwrap(), SIMPLE_IMPORT_SQL);
}

#[test]
fn execution_success_is_saved_even_when_validation_fails() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    add_simple_input(&directory, &database);
    let invalid_sql =
        SIMPLE_IMPORT_SQL.replace("COMMIT;", "DELETE FROM sql_import.taxon_names;\nCOMMIT;");

    execute_sql_import_sql(
        &database,
        &ValidateSqlImportRequest {
            sql: invalid_sql.clone(),
        },
    )
    .unwrap();
    let validation = validate_sql_import_candidate(&database).unwrap();

    assert!(!validation.valid);
    assert!(!validation.can_apply);
    assert_eq!(validation.errors[0].code, "invalid_sci_name_count");
    assert_eq!(validation.errors[0].taxon_id, Some(101));
    assert!(apply_sql_import(&database).is_err());
    assert_eq!(get_sql_import_sql(&database).unwrap(), invalid_sql);
}

#[test]
fn taxonomy_validation_failure_is_a_structured_result() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    add_simple_input(&directory, &database);
    let invalid_sql = SIMPLE_IMPORT_SQL.replace(
        "COMMIT;",
        r#"
INSERT INTO sql_import.taxa (taxon_id, parent_taxon_id, rank)
VALUES (202, 101, 1);
INSERT INTO sql_import.taxon_names (name_id, taxon_id, name_type, name)
VALUES (2, 202, 1, 'Second kingdom');
COMMIT;"#,
    );

    let mut progress = Vec::new();
    let result = validate_sql_import_with_progress(
        &database,
        &ValidateSqlImportRequest { sql: invalid_sql },
        &mut |event| progress.push(event),
    )
    .unwrap();

    assert!(!result.validation.valid);
    assert!(!result.validation.can_apply);
    assert!(!result.can_apply);
    assert_eq!(result.validation.total_error_count, 1);
    assert_eq!(result.validation.errors.len(), 1);
    assert_eq!(result.validation.errors[0].code, "kingdom_has_parent");
    assert_eq!(result.validation.errors[0].taxon_id, Some(202));
    assert_eq!(result.validation.errors[0].related_taxon_id, Some(101));
    assert_eq!(
        result.validation.errors[0].message,
        "Kingdom taxon 202 must be a root taxon."
    );
    assert!(
        progress
            .iter()
            .any(|event| event.stage == VALIDATING_STAGING_TAXONOMY)
    );
    assert_eq!(progress.last().unwrap().stage, VALIDATION_FAILED);
}

#[test]
fn exact_family_name_conflicts_are_reported_once() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    add_simple_input(&directory, &database);
    let invalid_sql = SIMPLE_IMPORT_SQL.replace(
        "COMMIT;",
        r#"
DROP INDEX sql_import.idx_taxon_names_scientific_family_name;
INSERT INTO sql_import.taxon_names (name_id, taxon_id, name_type, name)
VALUES (2, 101, 2, 'Animalia');
COMMIT;"#,
    );

    let result =
        validate_sql_import(&database, &ValidateSqlImportRequest { sql: invalid_sql }).unwrap();

    assert!(!result.validation.valid);
    assert_eq!(result.validation.total_error_count, 1);
    assert_eq!(result.validation.errors.len(), 1);
    assert_eq!(result.validation.errors[0].code, "duplicate_canonical_name");
    assert_eq!(result.validation.errors[0].taxon_id, Some(101));
}

#[test]
fn normalized_name_conflicts_are_validation_results() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    add_simple_input(&directory, &database);
    let invalid_sql = SIMPLE_IMPORT_SQL.replace(
        "COMMIT;",
        r#"
INSERT INTO sql_import.taxon_names (name_id, taxon_id, name_type, name)
VALUES (2, 101, 2, 'Animalia  old'),
       (3, 101, 2, 'Animalia old');
COMMIT;"#,
    );

    let result =
        validate_sql_import(&database, &ValidateSqlImportRequest { sql: invalid_sql }).unwrap();

    assert!(!result.validation.valid);
    assert_eq!(result.validation.total_error_count, 1);
    assert_eq!(result.validation.errors.len(), 1);
    assert_eq!(result.validation.errors[0].code, "duplicate_canonical_name");
    assert_eq!(result.validation.errors[0].taxon_id, Some(101));
}

#[test]
fn accepted_and_alias_canonical_name_conflicts_are_validation_results() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    add_simple_input(&directory, &database);
    let invalid_sql = SIMPLE_IMPORT_SQL.replace(
        "COMMIT;",
        r#"
INSERT INTO sql_import.taxon_names (name_id, taxon_id, name_type, name)
VALUES (2, 101, 2, 'Animalia ');
COMMIT;"#,
    );

    let result =
        validate_sql_import(&database, &ValidateSqlImportRequest { sql: invalid_sql }).unwrap();

    assert!(!result.validation.valid);
    assert_eq!(result.validation.total_error_count, 1);
    assert_eq!(result.validation.errors.len(), 1);
    assert_eq!(result.validation.errors[0].code, "duplicate_canonical_name");
    assert_eq!(result.validation.errors[0].taxon_id, Some(101));
}

#[test]
fn failed_execution_does_not_replace_saved_sql() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    add_simple_input(&directory, &database);
    execute_simple(&database);

    let error = execute_sql_import_sql(
        &database,
        &ValidateSqlImportRequest {
            sql: "SELECT FROM".into(),
        },
    )
    .unwrap_err();

    assert!(!error.to_string().is_empty());
    assert_eq!(get_sql_import_sql(&database).unwrap(), SIMPLE_IMPORT_SQL);
}

#[test]
fn failed_execution_restores_existing_staging_and_validation() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    add_simple_input(&directory, &database);
    execute_simple(&database);
    assert!(validate_sql_import_candidate(&database).unwrap().can_apply);

    execute_sql_import_sql(
        &database,
        &ValidateSqlImportRequest {
            sql: "SELECT FROM".into(),
        },
    )
    .unwrap_err();

    assert!(validate_sql_import_candidate(&database).unwrap().can_apply);
    assert_eq!(get_sql_import_sql(&database).unwrap(), SIMPLE_IMPORT_SQL);
}

#[test]
fn sql_import_timeout_restores_all_previous_artifacts() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    add_simple_input(&directory, &database);
    execute_simple(&database);
    assert!(validate_sql_import_candidate(&database).unwrap().can_apply);
    let workspace = workspace(&database).unwrap();
    let artifacts = [STAGING_DATABASE, CANDIDATE_DATABASE, VALIDATION_STATE]
        .map(|filename| (filename, fs::read(workspace.join(filename)).unwrap()));
    let mut progress = Vec::new();
    let request = ValidateSqlImportRequest {
        sql: r#"
            ATTACH DATABASE 'vividarium_sql_import.db' AS sql_import;
            WITH RECURSIVE loop(value) AS (
                SELECT 1
                UNION ALL
                SELECT value + 1 FROM loop
            )
            SELECT COUNT(*) FROM loop;
        "#
        .into(),
    };

    let error = execute_sql_import_sql_in_workspace_with_timeout(
        &database,
        &request,
        &workspace,
        &mut |event| progress.push(event),
        &CancellationToken::new(),
        Duration::from_millis(10),
    )
    .unwrap_err();

    assert!(error.to_string().contains("SQL Import statement 2 of 2"));
    assert!(error.to_string().contains("10 ms execution limit"));
    assert!(
        progress
            .iter()
            .all(|event| event.stage != FINALIZING_STAGING_DATABASE)
    );
    for (filename, expected) in artifacts {
        assert_eq!(fs::read(workspace.join(filename)).unwrap(), expected);
    }
    assert!(validate_sql_import_candidate(&database).unwrap().can_apply);
    assert_eq!(get_sql_import_sql(&database).unwrap(), SIMPLE_IMPORT_SQL);
}

#[test]
fn committed_sql_import_sql_reports_script_save_failure_as_warning() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    add_simple_input(&directory, &database);
    database
        .connect_metadata()
        .unwrap()
        .execute("DROP TABLE app_metadata", [])
        .unwrap();

    let result = execute_simple(&database);

    assert!(!result.script_saved);
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("script could not be saved"));
    assert!(
        workspace(&database)
            .unwrap()
            .join(STAGING_DATABASE)
            .is_file()
    );
}

#[test]
fn adding_input_returns_cleanup_warning_after_registry_commit() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    let workspace = workspace(&database).unwrap();
    fs::create_dir(workspace.join(CANDIDATE_BUILD_DATABASE)).unwrap();
    let csv_path = directory.path().join("source.csv");
    fs::write(&csv_path, "taxon_id,name\n1,Animalia\n").unwrap();

    let result = add_sql_import_input(
        &database,
        &AddSqlInputRequest {
            kind: SqlInputKind::Csv,
            alias: "source_taxa".into(),
            path: csv_path,
        },
    )
    .unwrap();

    assert_eq!(result.inputs.len(), 1);
    assert_eq!(result.input.alias, "source_taxa");
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("queued for retry"));
    let deferred = workspace.join(format!(".invalidated-{CANDIDATE_BUILD_DATABASE}"));
    fs::remove_dir(deferred).unwrap();
}

#[test]
fn source_removal_invalidates_staging_validation_and_candidate() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    add_simple_input(&directory, &database);
    execute_simple(&database);
    assert!(validate_sql_import_candidate(&database).unwrap().can_apply);
    let workspace = workspace(&database).unwrap();
    assert!(workspace.join(STAGING_DATABASE).exists());
    assert!(workspace.join(CANDIDATE_DATABASE).exists());

    let result = remove_sql_import_input(
        &database,
        &RemoveSqlInputRequest {
            alias: "source_taxa".into(),
        },
    )
    .unwrap();

    assert!(result.inputs.is_empty());
    assert!(!workspace.join(STAGING_DATABASE).exists());
    assert!(!workspace.join(CANDIDATE_DATABASE).exists());
    assert!(!workspace.join(VALIDATION_STATE).exists());
}

#[test]
fn removing_input_returns_cleanup_warning_after_registry_commit() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    add_simple_input(&directory, &database);
    let stored_path = database
        .connect_metadata()
        .unwrap()
        .query_row(
            "SELECT stored_path FROM sql_inputs WHERE scope = 2 AND alias = 'source_taxa'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap();
    fs::remove_file(&stored_path).unwrap();
    fs::create_dir(&stored_path).unwrap();

    let result = remove_sql_import_input(
        &database,
        &RemoveSqlInputRequest {
            alias: "source_taxa".into(),
        },
    )
    .unwrap();

    assert!(result.inputs.is_empty());
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("queued for retry"));
    fs::remove_dir(stored_path).unwrap();
}

#[test]
fn source_removal_rejects_busy_workspace_and_missing_alias() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    add_simple_input(&directory, &database);
    let workspace_mutex = workspace_mutex(&database).unwrap();
    let guard = lock_workspace(&workspace_mutex).unwrap();
    let busy = remove_sql_import_input(
        &database,
        &RemoveSqlInputRequest {
            alias: "source_taxa".into(),
        },
    )
    .unwrap_err();
    assert!(busy.to_string().contains("workspace is busy"));
    drop(guard);
    assert_eq!(list_sql_import_inputs(&database).unwrap().len(), 1);

    let missing = remove_sql_import_input(
        &database,
        &RemoveSqlInputRequest {
            alias: "missing".into(),
        },
    )
    .unwrap_err();
    assert!(missing.to_string().contains("SQL input missing"));
    assert_eq!(list_sql_import_inputs(&database).unwrap().len(), 1);
}

#[test]
fn built_in_sql_reads_a_named_sqlite_input() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    let source_path = directory.path().join("source.db");
    let source = Connection::open(&source_path).unwrap();
    source
        .execute_batch(
            r#"
            CREATE TABLE taxa (
                id INTEGER PRIMARY KEY,
                parent INTEGER,
                category INTEGER,
                rank INTEGER,
                scientific_name TEXT,
                authority_year TEXT,
                geological_range TEXT,
                english_name TEXT
            );
            CREATE TABLE synonyms (
                parent INTEGER,
                category INTEGER,
                synonym TEXT,
                authority_year TEXT,
                PRIMARY KEY (parent, synonym, authority_year)
            );
            CREATE TABLE chinese (
                id INTEGER,
                is_accepted INTEGER,
                chinese_name TEXT,
                source TEXT,
                PRIMARY KEY (id, chinese_name)
            );
            INSERT INTO taxa VALUES
                (10, NULL, 0, 60, 'Animalia', NULL, 'Recent', 'Animals'),
                (11, 10, 0, 601, 'Fallback species', NULL, 'Recent', NULL);
            INSERT INTO synonyms VALUES
                (10, 0, 'Animalia', 'self authority'),
                (10, 0, 'Metazoa', '1758'),
                (10, 0, 'Metazoa', '1900');
            INSERT INTO chinese VALUES
                (10, 1, 'Animals zh', 'test'),
                (11, 1, '   ', 'ignored'),
                (11, 0, 'Fallback alias B', 'test'),
                (11, 0, 'Fallback alias A', 'test');
            "#,
        )
        .unwrap();
    drop(source);
    add_sql_import_input(
        &database,
        &AddSqlInputRequest {
            kind: SqlInputKind::Sqlite,
            alias: "biolib".into(),
            path: source_path,
        },
    )
    .unwrap();

    execute_sql_import_sql(
        &database,
        &ValidateSqlImportRequest {
            sql: get_sql_import_sql(&database).unwrap(),
        },
    )
    .unwrap();
    let staging = Connection::open(workspace(&database).unwrap().join(STAGING_DATABASE)).unwrap();
    let animalia_names = staging
        .prepare(
            r#"
            SELECT name_type, name, authority_year, source
            FROM taxon_names
            WHERE taxon_id = 10
            ORDER BY name_type, name
            "#,
        )
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        animalia_names,
        vec![
            (1, "Animalia".into(), None, Some("biolib".into())),
            (
                2,
                "Metazoa".into(),
                Some("1900".into()),
                Some("biolib".into())
            ),
            (3, "Animals zh".into(), None, Some("test".into())),
            (5, "Animals".into(), None, Some("biolib".into())),
        ]
    );
    let fallback_names = staging
        .prepare(
            "SELECT name_type, name FROM taxon_names WHERE taxon_id = 11 ORDER BY name_type, name",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        fallback_names,
        vec![
            (1, "Fallback species".into()),
            (3, "Fallback alias A".into()),
            (4, "Fallback alias B".into()),
        ]
    );
    let validation = validate_sql_import_candidate(&database).unwrap();
    assert!(validation.can_apply, "{:?}", validation.errors);
    assert_eq!(validation.taxa_count, 2);
}

#[test]
fn sql_import_rejects_unregistered_attachments() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    let error = execute_sql_import_sql(
        &database,
        &ValidateSqlImportRequest {
            sql: "ATTACH DATABASE 'other.db' AS other".into(),
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("not authorized"));
}

#[test]
fn external_staging_change_invalidates_apply() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    add_simple_input(&directory, &database);
    execute_simple(&database);
    assert!(validate_sql_import_candidate(&database).unwrap().can_apply);
    let workspace = workspace(&database).unwrap();
    Connection::open(workspace.join(STAGING_DATABASE))
        .unwrap()
        .execute(
            "UPDATE taxon_names SET source = 'changed' WHERE name_id = 1",
            [],
        )
        .unwrap();

    let error = apply_sql_import(&database).unwrap_err();
    assert!(error.to_string().contains("fingerprint is stale"));
    assert!(!workspace.join(CANDIDATE_DATABASE).exists());
}

#[test]
fn apply_returns_cleanup_warning_after_taxonomy_replacement() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    add_simple_input(&directory, &database);
    execute_simple(&database);
    assert!(validate_sql_import_candidate(&database).unwrap().can_apply);
    let workspace = workspace(&database).unwrap();
    fs::create_dir(workspace.join(CANDIDATE_BUILD_DATABASE)).unwrap();

    let result = apply_sql_import(&database).unwrap();

    assert_eq!(result.metadata.taxa_count, 1);
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("queued for retry"));
    fs::remove_dir(workspace.join(CANDIDATE_BUILD_DATABASE)).unwrap();
}
