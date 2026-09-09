use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rusqlite::Connection;
use rusqlite::functions::FunctionFlags;

use super::super::sql_sources::{
    SqlDataSource, inspect_sql_data_source, load_csv_table, load_csv_table_with_progress,
};
use super::*;
use crate::operations::OperationInput;
use crate::taxonomy::{
    TaxonInputRow, apply_rows, get_operation_input, list_operations, rollback_operation,
};

fn database() -> (tempfile::TempDir, Database) {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    apply_rows(
        &database,
        &[TaxonInputRow {
            kingdom: Some("Animalia".into()),
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();
    (directory, database)
}

fn request(sql: &str) -> CustomTaxonomySqlRequest {
    CustomTaxonomySqlRequest {
        sql: sql.into(),
        maximum_result_rows: None,
    }
}

fn seed_mesotriton_fixture(database: &Database) {
    database
        .connect_taxonomy()
        .unwrap()
        .execute_batch(
            r#"
            INSERT INTO taxa (taxon_id, parent_taxon_id, rank) VALUES
                (101, NULL, 1),
                (102, 101, 2),
                (103, 102, 3),
                (104, 103, 4),
                (105, 104, 5);
            INSERT INTO taxon_names (name_id, taxon_id, name_type, name) VALUES
                (201, 101, 1, 'Animalia'),
                (202, 102, 1, 'Caudata'),
                (203, 103, 1, 'Salamandridae'),
                (204, 104, 1, 'Ichthyosaura'),
                (205, 104, 2, 'Mesotriton'),
                (206, 105, 1, 'Ichthyosaura alpestris'),
                (207, 105, 2, 'Mesotriton alpestris');
            "#,
        )
        .unwrap();
}

fn taxonomy_state(
    database: &Database,
) -> (Vec<(i64, Option<i64>, i64)>, Vec<(i64, i64, i64, String)>) {
    let connection = database.connect_taxonomy_metadata_context().unwrap();
    let taxa = connection
        .prepare("SELECT taxon_id, parent_taxon_id, rank FROM taxa ORDER BY taxon_id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let names = connection
        .prepare("SELECT name_id, taxon_id, name_type, name FROM taxon_names ORDER BY name_id")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    (taxa, names)
}

#[test]
fn returns_typed_results_and_only_logs_actual_mutations() {
    let (_directory, database) = database();
    let before = list_operations(&database, None, 20).unwrap().items.len();
    let query = execute_custom_taxonomy_sql(
        &database,
        &request(
            "SELECT taxon_id, rank, geological_range, CAST(NULL AS TEXT) AS missing FROM taxa",
        ),
    )
    .unwrap();
    assert_eq!(query.operation_id, None);
    assert_eq!(query.changeset_size, 0);
    assert!(query.script_saved);
    assert!(query.warnings.is_empty());
    assert_eq!(query.result_sets.len(), 1);
    assert_eq!(query.result_sets[0].rows[0][1], SqlValue::Integer(1));
    assert_eq!(query.result_sets[0].rows[0][2], SqlValue::Null);
    assert_eq!(
        list_operations(&database, None, 20).unwrap().items.len(),
        before
    );

    let mutation = execute_custom_taxonomy_sql(
        &database,
        &request("UPDATE taxa SET geological_range = 'Recent' RETURNING geological_range"),
    )
    .unwrap();
    assert!(mutation.operation_id.is_some());
    assert!(mutation.changeset_size > 0);
    assert!(mutation.script_saved);
    assert!(mutation.warnings.is_empty());
    assert_eq!(
        mutation.result_sets[0].rows,
        vec![vec![SqlValue::Text("Recent".into())]]
    );
    assert_eq!(mutation.messages[0].affected_rows, Some(1));
    assert_eq!(
        list_operations(&database, None, 20).unwrap().items.len(),
        before + 1
    );
}

#[test]
fn saves_only_successful_scripts() {
    let (_directory, database) = database();
    let initial = get_custom_taxonomy_sql(&database).unwrap();

    execute_custom_taxonomy_sql(&database, &request("SELECT 1; SELECT 2")).unwrap();
    assert_eq!(
        get_custom_taxonomy_sql(&database).unwrap(),
        "SELECT 1; SELECT 2"
    );

    execute_custom_taxonomy_sql(&database, &request("SELECT taxon_id FROM taxa")).unwrap();
    assert_eq!(
        get_custom_taxonomy_sql(&database).unwrap(),
        "SELECT taxon_id FROM taxa"
    );

    execute_custom_taxonomy_sql(&database, &request("SELECT missing FROM taxa")).unwrap_err();
    assert_eq!(
        get_custom_taxonomy_sql(&database).unwrap(),
        "SELECT taxon_id FROM taxa"
    );
    assert_ne!(get_custom_taxonomy_sql(&database).unwrap(), initial);
}

#[test]
fn custom_sql_history_preserves_the_exact_executed_script() {
    let (_directory, database) = database();
    let sql = "\n-- preserve this comment\nUPDATE taxa\nSET geological_range = 'Recent';\n";
    let result = execute_custom_taxonomy_sql(&database, &request(sql)).unwrap();
    let operation_id = result.operation_id.unwrap();

    execute_custom_taxonomy_sql(&database, &request("SELECT 2 AS later_workspace_sql")).unwrap();

    assert_eq!(
        get_operation_input(&database, operation_id).unwrap(),
        Some(OperationInput::CustomSql { sql: sql.into() })
    );
    database
        .connect_taxonomy_metadata_context()
        .unwrap()
        .execute(
            "DELETE FROM operation_inputs WHERE operation_id = ?",
            [operation_id],
        )
        .unwrap();
    assert_eq!(get_operation_input(&database, operation_id).unwrap(), None);
}

#[test]
fn complex_custom_sql_taxonomy_migration_rolls_back_exactly() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    seed_mesotriton_fixture(&database);
    let sql = r#"
-- Move Mesotriton out of Ichthyosaura and exchange the species name.
INSERT INTO taxa (parent_taxon_id, rank)
SELECT taxon_id, 4 FROM taxon_names WHERE name = 'Salamandridae';

UPDATE taxon_names
SET taxon_id = (
        SELECT taxa.taxon_id
        FROM taxa
        LEFT JOIN taxon_names AS accepted
          ON accepted.taxon_id = taxa.taxon_id AND accepted.name_type = 1
        WHERE taxa.rank = 4 AND accepted.name_id IS NULL
    ),
    name_type = 1
WHERE name_id = 205;

UPDATE taxa
SET parent_taxon_id = (SELECT taxon_id FROM taxon_names WHERE name = 'Mesotriton')
WHERE taxon_id = 105;

UPDATE taxon_names SET name_type = 2 WHERE name_id = 206;
UPDATE taxon_names SET name_type = 1 WHERE name_id = 207;
"#;
    let result = execute_custom_taxonomy_sql(&database, &request(sql)).unwrap();
    let operation_id = result.operation_id.unwrap();
    assert!(
        list_operations(&database, None, 20)
            .unwrap()
            .items
            .iter()
            .find(|operation| operation.operation_id == operation_id)
            .unwrap()
            .rollbackable
    );

    rollback_operation(&database, operation_id).unwrap();

    let connection = database.connect_taxonomy_metadata_context().unwrap();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM taxa WHERE rank = 4", [], |row| row
                .get::<_, i64>(0),)
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT parent_taxon_id FROM taxa WHERE taxon_id = 105",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        104
    );
    let names = connection
        .prepare(
            "SELECT name_id, taxon_id, name_type FROM taxon_names WHERE name_id IN (205, 206, 207) ORDER BY name_id",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(names, vec![(205, 104, 2), (206, 105, 1), (207, 105, 2)]);
    assert!(
        connection
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .query([])
            .unwrap()
            .next()
            .unwrap()
            .is_none()
    );
    assert!(
        operations::get_operation(&connection, operation_id)
            .unwrap()
            .is_none()
    );
    for table in [
        "operation_audit_rows",
        "operation_changesets",
        "operation_inputs",
    ] {
        assert_eq!(
            connection
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE operation_id = ?"),
                    [operation_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
    }
}

#[test]
fn rollback_conflict_is_diagnostic_and_atomic() {
    let (_directory, database) = database();
    let first = execute_custom_taxonomy_sql(
        &database,
        &request("UPDATE taxa SET geological_range = 'First'"),
    )
    .unwrap()
    .operation_id
    .unwrap();
    let second = execute_custom_taxonomy_sql(
        &database,
        &request("UPDATE taxa SET geological_range = 'Second'"),
    )
    .unwrap()
    .operation_id
    .unwrap();

    let error = rollback_operation(&database, first).unwrap_err();
    assert!(error.to_string().contains("Rollback conflict in taxa"));
    assert!(
        !error
            .to_string()
            .contains("Callback routine requested an abort")
    );

    let connection = database.connect_taxonomy_metadata_context().unwrap();
    assert_eq!(
        connection
            .query_row("SELECT geological_range FROM taxa", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
        "Second"
    );
    assert!(
        operations::get_operation(&connection, first)
            .unwrap()
            .is_some()
    );
    assert!(
        operations::get_operation(&connection, second)
            .unwrap()
            .is_some()
    );
    for table in [
        "operation_audit_rows",
        "operation_changesets",
        "operation_inputs",
    ] {
        assert!(
            connection
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE operation_id = ?"),
                    [first],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap()
                > 0
        );
    }
}

#[test]
fn rollback_foreign_key_conflict_is_diagnostic_and_atomic() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    seed_mesotriton_fixture(&database);
    let first = execute_custom_taxonomy_sql(
        &database,
        &request(
            r#"
INSERT INTO taxa (taxon_id, parent_taxon_id, rank) VALUES (106, 103, 4);
INSERT INTO taxon_names (name_id, taxon_id, name_type, name)
VALUES (208, 106, 1, 'Triturus');
"#,
        ),
    )
    .unwrap()
    .operation_id
    .unwrap();
    let second = execute_custom_taxonomy_sql(
        &database,
        &request(
            r#"
INSERT INTO taxa (taxon_id, parent_taxon_id, rank) VALUES (107, 106, 5);
INSERT INTO taxon_names (name_id, taxon_id, name_type, name)
VALUES (209, 107, 1, 'Triturus cristatus');
"#,
        ),
    )
    .unwrap()
    .operation_id
    .unwrap();
    let state_before_rollback = taxonomy_state(&database);

    let error = rollback_operation(&database, first).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("Rollback conflict"));
    assert!(message.contains("foreign-key"));
    assert!(message.contains(&first.to_string()));
    assert!(message.contains("1 unresolved foreign-key relationship"));
    assert!(!message.contains("Callback routine requested an abort"));

    assert_eq!(taxonomy_state(&database), state_before_rollback);
    let connection = database.connect_taxonomy_metadata_context().unwrap();
    assert!(
        operations::get_operation(&connection, first)
            .unwrap()
            .is_some()
    );
    assert!(
        operations::get_operation(&connection, second)
            .unwrap()
            .is_some()
    );
    for table in [
        "operation_audit_rows",
        "operation_changesets",
        "operation_inputs",
    ] {
        assert!(
            connection
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE operation_id = ?"),
                    [first],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap()
                > 0
        );
    }
    assert!(
        connection
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .query([])
            .unwrap()
            .next()
            .unwrap()
            .is_none()
    );
}

#[test]
fn multiple_queries_use_executable_statement_indexes() {
    let (_directory, database) = database();
    let result = execute_custom_taxonomy_sql(
        &database,
        &request(
            r#"
            -- first query
            SELECT 1 AS value;

            /* second query */
            SELECT 2 AS value;
            "#,
        ),
    )
    .unwrap();

    assert_eq!(
        result
            .result_sets
            .iter()
            .map(|result_set| result_set.statement_index)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(result.result_sets[0].rows, vec![vec![SqlValue::Integer(1)]]);
    assert_eq!(result.result_sets[1].rows, vec![vec![SqlValue::Integer(2)]]);
    assert_eq!(
        result
            .messages
            .iter()
            .map(|message| message.statement_index)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
}

#[test]
fn mixed_scripts_create_one_operation_and_keep_statement_indexes() {
    let (_directory, database) = database();
    let before = list_operations(&database, None, 20).unwrap().items.len();

    let result = execute_custom_taxonomy_sql(
        &database,
        &request(
            "UPDATE taxa SET geological_range = 'Recent'; SELECT taxon_id, geological_range FROM taxa",
        ),
    )
    .unwrap();

    assert!(result.operation_id.is_some());
    assert_eq!(result.messages[0].statement_index, 1);
    assert_eq!(result.messages[0].affected_rows, Some(1));
    assert_eq!(result.result_sets[0].statement_index, 2);
    assert_eq!(result.messages[1].affected_rows, None);
    assert_eq!(
        list_operations(&database, None, 20).unwrap().items.len(),
        before + 1
    );
}

#[test]
fn a_later_statement_failure_rolls_back_the_entire_script() {
    let (_directory, database) = database();
    let saved = get_custom_taxonomy_sql(&database).unwrap();
    let before = list_operations(&database, None, 20).unwrap().items.len();

    execute_custom_taxonomy_sql(
        &database,
        &request("UPDATE taxa SET geological_range = 'Recent'; SELECT missing_column FROM taxa"),
    )
    .unwrap_err();

    assert_eq!(
        database
            .connect_taxonomy()
            .unwrap()
            .query_row("SELECT geological_range FROM taxa", [], |row| {
                row.get::<_, Option<String>>(0)
            })
            .unwrap(),
        None
    );
    assert_eq!(
        list_operations(&database, None, 20).unwrap().items.len(),
        before
    );
    assert_eq!(get_custom_taxonomy_sql(&database).unwrap(), saved);
}

#[test]
fn custom_sql_reports_statement_and_mutation_phases() {
    let (_directory, database) = database();
    let mut progress = Vec::new();
    execute_custom_taxonomy_sql_with_progress_and_cancellation(
        &database,
        &request("UPDATE taxa SET geological_range = 'Recent'; SELECT taxon_id FROM taxa"),
        &mut |event| progress.push(event),
        &CancellationToken::new(),
    )
    .unwrap();

    let stages = progress
        .iter()
        .map(|event| event.stage.as_str())
        .collect::<Vec<_>>();
    let expected = [
        "preparing_sql_sources",
        "executing_custom_sql",
        "generating_custom_sql_changeset",
        "validating_custom_sql_changes",
        "recording_custom_sql_operation",
        "committing_custom_sql",
        "finalizing_custom_sql",
    ];
    let mut previous = 0;
    for expected in expected {
        let index = stages[previous..]
            .iter()
            .position(|stage| *stage == expected)
            .map(|index| index + previous)
            .unwrap_or_else(|| panic!("missing progress stage {expected}: {stages:?}"));
        previous = index;
    }
    let statements = progress
        .iter()
        .filter(|event| event.stage == "executing_custom_sql")
        .map(|event| (event.current, event.total))
        .collect::<Vec<_>>();
    assert_eq!(statements, vec![(Some(1), Some(2)), (Some(2), Some(2))]);
}

#[test]
fn read_only_custom_sql_skips_changeset_and_validation_progress() {
    let (_directory, database) = database();
    let mut progress = Vec::new();
    execute_custom_taxonomy_sql_with_progress_and_cancellation(
        &database,
        &request("SELECT taxon_id FROM taxa"),
        &mut |event| progress.push(event),
        &CancellationToken::new(),
    )
    .unwrap();

    let stages = progress
        .iter()
        .map(|event| event.stage.as_str())
        .collect::<Vec<_>>();
    assert!(!stages.contains(&"generating_custom_sql_changeset"));
    assert!(!stages.contains(&"validating_custom_sql_changes"));
}

#[test]
fn small_validation_scope_returns_expected_dependencies() {
    let (_directory, database) = database();
    let mut connection = database.connect_taxonomy().unwrap();
    let transaction = connection.transaction().unwrap();
    let root_id = transaction
        .query_row("SELECT taxon_id FROM taxa", [], |row| row.get::<_, i64>(0))
        .unwrap();
    transaction
        .execute(
            "INSERT INTO taxa(parent_taxon_id, rank) VALUES (?, 2)",
            [root_id],
        )
        .unwrap();
    let child_id = transaction.last_insert_rowid();
    transaction
        .execute(
            "INSERT INTO taxa(parent_taxon_id, rank) VALUES (?, 3)",
            [child_id],
        )
        .unwrap();
    let grandchild_id = transaction.last_insert_rowid();
    let scope = taxonomy_validation_scope_with_cancellation(
        &transaction,
        &BTreeSet::from([child_id]),
        10,
        &CancellationToken::new(),
    )
    .unwrap();

    assert_eq!(
        scope,
        TaxonomyValidationScope::Incremental(BTreeSet::from([root_id, child_id, grandchild_id,]))
    );
}

#[test]
fn oversized_direct_validation_set_returns_full_before_querying_dependencies() {
    let connection = Connection::open_in_memory().unwrap();

    let scope = taxonomy_validation_scope_with_cancellation(
        &connection,
        &BTreeSet::from([1, 2, 3]),
        2,
        &CancellationToken::new(),
    )
    .unwrap();

    assert_eq!(scope, TaxonomyValidationScope::Full);
}

#[test]
fn oversized_dependency_closure_returns_full_without_carrying_the_closure() {
    let (_directory, database) = database();
    let mut connection = database.connect_taxonomy().unwrap();
    let transaction = connection.transaction().unwrap();
    let root_id = transaction
        .query_row("SELECT taxon_id FROM taxa", [], |row| row.get::<_, i64>(0))
        .unwrap();
    for _ in 0..4 {
        transaction
            .execute(
                "INSERT INTO taxa(parent_taxon_id, rank) VALUES (?, 2)",
                [root_id],
            )
            .unwrap();
    }

    let scope = taxonomy_validation_scope_with_cancellation(
        &transaction,
        &BTreeSet::from([root_id]),
        3,
        &CancellationToken::new(),
    )
    .unwrap();

    assert_eq!(scope, TaxonomyValidationScope::Full);
}

#[test]
fn incremental_validation_rejects_missing_scientific_name_and_rolls_back() {
    let (_directory, database) = database();
    let error = execute_custom_taxonomy_sql(
        &database,
        &request("DELETE FROM taxon_names WHERE name_type = 1"),
    )
    .unwrap_err();

    assert!(error.to_string().contains("exactly one scientific name"));
    assert_eq!(
        database
            .connect_taxonomy()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM taxon_names WHERE name_type = 1",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
}

#[test]
fn incremental_validation_rejects_parent_cycle_and_rolls_back() {
    let (_directory, database) = database();
    let error = execute_custom_taxonomy_sql(
        &database,
        &request(
            r#"
            INSERT INTO taxa(parent_taxon_id, rank) SELECT taxon_id, 2 FROM taxa WHERE rank = 1;
            INSERT INTO taxon_names(taxon_id, name_type, name) VALUES(last_insert_rowid(), 1, 'Testales');
            UPDATE taxa SET parent_taxon_id = (SELECT taxon_id FROM taxa WHERE rank = 2) WHERE rank = 1;
            "#,
        ),
    )
    .unwrap_err();

    assert!(error.to_string().contains("cyclic parent relationship"));
    assert_eq!(
        database
            .connect_taxonomy()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM taxa", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn custom_sql_workspace_lock_fails_fast() {
    let (_directory, database) = database();
    let mutex = custom_sql_mutex(&database).unwrap();
    let _guard = mutex.lock().unwrap();

    let error = execute_custom_taxonomy_sql(&database, &request("SELECT 1")).unwrap_err();
    assert!(error.to_string().contains("already running"));
}

#[test]
fn csv_source_loading_can_be_cancelled_without_a_partial_table() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("large.csv");
    let mut contents = String::from("name,value\n");
    for index in 0..3_000 {
        contents.push_str(&format!("row-{index},{index}\n"));
    }
    std::fs::write(&path, contents).unwrap();
    let mut connection = Connection::open_in_memory().unwrap();
    let cancellation = CancellationToken::new();
    let cancel_from_progress = cancellation.clone();
    let result = load_csv_table_with_progress(
        &mut connection,
        "source",
        &path,
        b',',
        &cancellation,
        &mut |progress| {
            if progress.current.is_some_and(|current| current >= 1_000) {
                cancel_from_progress.cancel();
            }
        },
    );

    assert!(matches!(result, Err(CoreError::Cancelled)));
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_temp_master WHERE type = 'table' AND name = 'source'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
}

#[test]
fn comment_only_scripts_are_not_executable() {
    let (_directory, database) = database();

    let error =
        execute_custom_taxonomy_sql(&database, &request("-- no statement\n/* still empty */"))
            .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("at least one executable statement")
    );
}

#[test]
fn committed_sql_reports_script_save_failure_as_warning() {
    let (_directory, database) = database();
    database
        .connect_metadata()
        .unwrap()
        .execute("DROP TABLE app_metadata", [])
        .unwrap();

    let result = execute_custom_taxonomy_sql(
        &database,
        &request("UPDATE taxa SET geological_range = 'Recent'"),
    )
    .unwrap();

    let operation_id = result.operation_id.unwrap();
    assert!(!result.script_saved);
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("script could not be saved"));
    assert_eq!(
        database
            .connect_taxonomy()
            .unwrap()
            .query_row("SELECT geological_range FROM taxa LIMIT 1", [], |row| {
                row.get::<_, String>(0)
            },)
            .unwrap(),
        "Recent"
    );
    assert_eq!(
        get_operation_input(&database, operation_id).unwrap(),
        Some(OperationInput::CustomSql {
            sql: "UPDATE taxa SET geological_range = 'Recent'".into(),
        })
    );
}

#[test]
fn read_only_preview_stops_after_limit_plus_one_rows() {
    let connection = Connection::open_in_memory().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let function_calls = calls.clone();
    connection
        .create_scalar_function(
            "observe_row",
            1,
            FunctionFlags::SQLITE_UTF8,
            move |context| {
                function_calls.fetch_add(1, Ordering::Relaxed);
                context.get::<i64>(0)
            },
        )
        .unwrap();
    let result = execute_custom_script(
        &connection,
        r#"
            WITH RECURSIVE values_cte(value) AS (
                VALUES (1)
                UNION ALL
                SELECT value + 1 FROM values_cte WHERE value < 1000
            )
            SELECT observe_row(value) FROM values_cte;
            "#,
        2,
        custom_sql_authorizer,
    )
    .unwrap();

    assert_eq!(result.result_sets[0].rows.len(), 2);
    assert!(result.result_sets[0].truncated);
    assert_eq!(calls.load(Ordering::Relaxed), 3);
    assert_eq!(
        result.messages[0].message,
        "statement returned more than 2 rows"
    );
}

#[test]
fn mutation_returning_runs_to_completion_after_preview_limit() {
    let (_directory, database) = database();
    apply_rows(
        &database,
        &[
            TaxonInputRow {
                kingdom: Some("Archaea".into()),
                ..TaxonInputRow::default()
            },
            TaxonInputRow {
                kingdom: Some("Bacteria".into()),
                ..TaxonInputRow::default()
            },
        ],
    )
    .unwrap();
    let mut limited = request("UPDATE taxa SET geological_range = 'Recent' RETURNING taxon_id");
    limited.maximum_result_rows = Some(2);

    let result = execute_custom_taxonomy_sql(&database, &limited).unwrap();

    assert_eq!(result.result_sets[0].rows.len(), 2);
    assert!(result.result_sets[0].truncated);
    assert_eq!(result.messages[0].affected_rows, Some(3));
    assert_eq!(
        database
            .connect_taxonomy()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM taxa WHERE geological_range = 'Recent'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        3
    );
}

#[test]
fn csv_and_sqlite_sources_are_read_only() {
    let (directory, database) = database();
    let csv_path = directory.path().join("input.csv");
    std::fs::write(&csv_path, "name,value\nAnimalia,\n").unwrap();
    let sqlite_path = directory.path().join("source.db");
    let source = Connection::open(&sqlite_path).unwrap();
    source
        .execute_batch(
            "CREATE TABLE source_names(name TEXT); INSERT INTO source_names VALUES ('Metazoa');",
        )
        .unwrap();
    drop(source);
    add_custom_sql_input(
        &database,
        &AddSqlInputRequest {
            kind: super::SqlInputKind::Csv,
            alias: "csv_input".into(),
            path: csv_path.clone(),
        },
    )
    .unwrap();
    add_custom_sql_input(
        &database,
        &AddSqlInputRequest {
            kind: super::SqlInputKind::Sqlite,
            alias: "external".into(),
            path: sqlite_path.clone(),
        },
    )
    .unwrap();
    let result = execute_custom_taxonomy_sql(
            &database,
            &CustomTaxonomySqlRequest {
                sql: "SELECT csv_input.name, csv_input.value, external.source_names.name FROM csv_input CROSS JOIN external.source_names".into(),
                maximum_result_rows: None,
            },
        )
        .unwrap();
    assert_eq!(
        result.result_sets[0].rows[0],
        vec![
            SqlValue::Text("Animalia".into()),
            SqlValue::Text(String::new()),
            SqlValue::Text("Metazoa".into())
        ]
    );
    let error = execute_custom_taxonomy_sql(
        &database,
        &CustomTaxonomySqlRequest {
            sql: "DELETE FROM external.source_names".into(),
            maximum_result_rows: None,
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("not authorized"));
    assert_eq!(
        Connection::open(&sqlite_path)
            .unwrap()
            .query_row("SELECT COUNT(*) FROM source_names", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    let schema = inspect_sql_data_source(
        &SqlDataSource::Csv {
            alias: "csv_input".into(),
            path: csv_path,
        },
        b',',
    )
    .unwrap();
    assert_eq!(schema.objects[0].columns[0].name, "name");
}

#[test]
fn csv_load_uses_one_atomic_transaction() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("broken.csv");
    std::fs::write(&path, "name,value\nvalid,1\nbroken\n").unwrap();
    let mut connection = Connection::open_in_memory().unwrap();

    let error = load_csv_table(&mut connection, "source", &path, b',').unwrap_err();

    assert!(error.to_string().contains("CSV row 3"));
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_temp_schema WHERE name = 'source'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );

    let mut large_csv = String::from("value\n");
    for value in 0..5_000 {
        large_csv.push_str(&format!("{value}\n"));
    }
    let large_path = directory.path().join("large.csv");
    std::fs::write(&large_path, large_csv).unwrap();
    load_csv_table(&mut connection, "large_source", &large_path, b',').unwrap();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM large_source", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        5_000
    );
}

#[test]
fn custom_sql_database_schemas_include_main_taxonomy_tables() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();

    let schemas = list_custom_sql_database_schemas(&database).unwrap();

    assert_eq!(schemas.len(), 1);
    assert_eq!(schemas[0].alias, "main");
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
fn rejects_control_and_internal_write_statements() {
    let (_directory, database) = database();
    for sql in [
        "BEGIN",
        "ATTACH DATABASE 'other.db' AS other",
        "DELETE FROM operations",
        "DROP TABLE taxa",
        "PRAGMA writable_schema = ON",
    ] {
        assert!(
            execute_custom_taxonomy_sql(&database, &request(sql))
                .unwrap_err()
                .to_string()
                .contains("not authorized"),
            "{sql}"
        );
    }
}

#[test]
fn streams_one_query_to_csv() {
    let (directory, database) = database();
    let destination = directory.path().join("query.csv");
    let result = export_custom_taxonomy_query(
        &database,
        &CustomTaxonomySqlExportRequest {
            sql: "SELECT rank, name FROM taxa JOIN taxon_names USING (taxon_id)".into(),
            statement_index: 1,
            destination_path: destination.clone(),
        },
    )
    .unwrap();
    assert_eq!(result.row_count, 1);
    assert_eq!(
        std::fs::read_to_string(destination).unwrap(),
        "rank,name\n1,Animalia\n"
    );
}

#[test]
fn exports_a_complete_result_beyond_the_preview_limit() {
    let (directory, database) = database();
    let sql = r#"
        WITH RECURSIVE values_cte(value) AS (
            VALUES (1)
            UNION ALL
            SELECT value + 1 FROM values_cte WHERE value < 1500
        )
        SELECT value FROM values_cte
    "#;
    let mut preview_request = request(sql);
    preview_request.maximum_result_rows = Some(20);
    let preview = execute_custom_taxonomy_sql(&database, &preview_request).unwrap();
    assert_eq!(preview.result_sets[0].rows.len(), 20);
    assert!(preview.result_sets[0].truncated);

    let destination = directory.path().join("complete-query.csv");
    let exported = export_custom_taxonomy_query(
        &database,
        &CustomTaxonomySqlExportRequest {
            sql: sql.into(),
            statement_index: 1,
            destination_path: destination.clone(),
        },
    )
    .unwrap();

    assert_eq!(exported.row_count, 1500);
    assert_eq!(
        std::fs::read_to_string(destination)
            .unwrap()
            .lines()
            .count(),
        1501
    );
}

#[test]
fn export_query_uses_the_custom_sql_statement_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("timed-out.csv");
    let connection = Connection::open_in_memory().unwrap();
    let started_at = std::time::Instant::now();
    let mut output_guard = PartialExportGuard::new(&destination);
    let error = export_query_statement(
        &connection,
        r#"
        WITH RECURSIVE loop(value) AS (
            SELECT 1
            UNION ALL
            SELECT value + 1 FROM loop
        )
        SELECT COUNT(*) FROM loop
        "#,
        1,
        &destination,
        b',',
        &CancellationToken::new(),
        Duration::from_millis(10),
        &mut output_guard,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("Custom SQL Export statement 1 of 1")
    );
    assert!(error.to_string().contains("10 ms execution limit"));
    assert!(started_at.elapsed() < Duration::from_secs(1));
    drop(output_guard);
    assert!(!destination.exists());
}

#[test]
fn export_cancellation_remains_distinct_and_removes_partial_output() {
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("cancelled.csv");
    let connection = Connection::open_in_memory().unwrap();
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let mut output_guard = PartialExportGuard::new(&destination);

    let result = export_query_statement(
        &connection,
        "SELECT 1 AS value",
        1,
        &destination,
        b',',
        &cancellation,
        Duration::from_secs(1),
        &mut output_guard,
    );

    assert!(matches!(result, Err(CoreError::Cancelled)));
    assert!(destination.exists());
    drop(output_guard);
    assert!(!destination.exists());
}

#[test]
fn partial_export_guard_removes_output_after_post_creation_failure() {
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("partial.csv");
    let mut output_guard = PartialExportGuard::new(&destination);
    std::fs::write(&destination, "partial").unwrap();
    output_guard.arm();

    drop(output_guard);

    assert!(!destination.exists());
}

#[test]
fn exports_only_the_selected_read_only_statement() {
    let (directory, database) = database();
    let destination = directory.path().join("second-statement.csv");
    let result = export_custom_taxonomy_query(
        &database,
        &CustomTaxonomySqlExportRequest {
            sql: "UPDATE taxa SET geological_range = 'Changed'; SELECT name FROM taxon_names ORDER BY name".into(),
            statement_index: 2,
            destination_path: destination.clone(),
        },
    )
    .unwrap();

    assert_eq!(result.row_count, 1);
    assert_eq!(
        std::fs::read_to_string(destination).unwrap(),
        "name\nAnimalia\n"
    );
    assert_eq!(
        database
            .connect_taxonomy()
            .unwrap()
            .query_row("SELECT geological_range FROM taxa", [], |row| {
                row.get::<_, Option<String>>(0)
            })
            .unwrap(),
        None
    );
}

#[test]
fn empty_query_export_contains_only_the_header() {
    let (directory, database) = database();
    let destination = directory.path().join("empty-query.csv");
    let result = export_custom_taxonomy_query(
        &database,
        &CustomTaxonomySqlExportRequest {
            sql: "SELECT taxon_id, rank FROM taxa WHERE 1 = 0".into(),
            statement_index: 1,
            destination_path: destination.clone(),
        },
    )
    .unwrap();

    assert_eq!(result.row_count, 0);
    assert_eq!(
        std::fs::read_to_string(destination).unwrap(),
        "taxon_id,rank\n"
    );
}

#[test]
fn export_rejects_mutations_and_invalid_statement_indexes() {
    let (directory, database) = database();
    let mutation_destination = directory.path().join("mutation.csv");
    std::fs::write(&mutation_destination, "existing\n").unwrap();
    let mutation_error = export_custom_taxonomy_query(
        &database,
        &CustomTaxonomySqlExportRequest {
            sql: "UPDATE taxa SET geological_range = 'Changed' RETURNING taxon_id".into(),
            statement_index: 1,
            destination_path: mutation_destination.clone(),
        },
    )
    .unwrap_err();
    assert!(mutation_error.to_string().contains("read-only query"));
    assert_eq!(
        std::fs::read_to_string(mutation_destination).unwrap(),
        "existing\n"
    );

    let missing_destination = directory.path().join("missing.csv");
    let missing_error = export_custom_taxonomy_query(
        &database,
        &CustomTaxonomySqlExportRequest {
            sql: "SELECT taxon_id FROM taxa; SELECT name FROM taxon_names".into(),
            statement_index: 99,
            destination_path: missing_destination,
        },
    )
    .unwrap_err();
    assert!(
        missing_error
            .to_string()
            .contains("statement 99 does not exist")
    );

    let zero_destination = directory.path().join("zero.csv");
    let zero_error = export_custom_taxonomy_query(
        &database,
        &CustomTaxonomySqlExportRequest {
            sql: "SELECT taxon_id FROM taxa".into(),
            statement_index: 0,
            destination_path: zero_destination,
        },
    )
    .unwrap_err();
    assert!(zero_error.to_string().contains("must be at least 1"));
}

#[test]
fn configured_csv_delimiter_controls_sql_sources_and_exports() {
    let (directory, database) = database();
    crate::general::update_general_settings(
        &database,
        &crate::general::GeneralSettings {
            csv_delimiter: "|".into(),
            ..crate::general::GeneralSettings::default()
        },
    )
    .unwrap();
    let source = directory.path().join("source.csv");
    std::fs::write(&source, "name|value\nAnimalia|Recent\n").unwrap();
    add_custom_sql_input(
        &database,
        &AddSqlInputRequest {
            kind: super::SqlInputKind::Csv,
            alias: "csv_input".into(),
            path: source,
        },
    )
    .unwrap();
    let result = execute_custom_taxonomy_sql(
        &database,
        &CustomTaxonomySqlRequest {
            sql: "SELECT name, value FROM csv_input".into(),
            maximum_result_rows: None,
        },
    )
    .unwrap();
    assert_eq!(
        result.result_sets[0].rows[0],
        vec![
            SqlValue::Text("Animalia".into()),
            SqlValue::Text("Recent".into())
        ]
    );
    let destination = directory.path().join("query.csv");
    export_custom_taxonomy_query(
        &database,
        &CustomTaxonomySqlExportRequest {
            sql: "SELECT name, value FROM csv_input".into(),
            statement_index: 1,
            destination_path: destination.clone(),
        },
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(destination).unwrap(),
        "name|value\nAnimalia|Recent\n"
    );
}
