use super::*;
use crate::taxonomy::{TaxonInputRow, apply_rows, get_taxon_detail, list_operations};

#[test]
fn inspects_path_and_tables_without_replacing_taxonomy() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("vividarium.db")).unwrap();
    let old_taxon_ids = seed_old_taxonomy_tree(&database);
    let source_path = directory.path().join("direct-import.db");
    create_direct_import_database(&source_path);

    let inspected = inspect_direct_import_database(&database, &source_path).unwrap();

    assert_eq!(
        inspected.source_path,
        source_path.canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(
        inspected
            .schema
            .objects
            .iter()
            .map(|object| object.name.as_str())
            .collect::<Vec<_>>(),
        ["taxa", "taxon_names"]
    );
    for taxon_id in old_taxon_ids {
        assert!(get_taxon_detail(&database, taxon_id).unwrap().is_some());
    }
}

#[test]
fn replaces_taxonomy_preserves_source_ids_and_queues_all_photos() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open_test(directory.path().join("vividarium.db")).unwrap();
    let old_taxon_ids = seed_old_taxonomy_tree(&database);
    let old_identity = database.taxonomy_identity().unwrap();
    let old_taxon_id = old_taxon_ids[2];
    let connection = database.connect().unwrap();
    connection
        .execute(
            "UPDATE photo_library SET root_path = '/photos' WHERE library_id = 1",
            [],
        )
        .unwrap();
    connection
        .execute(
            r#"
                INSERT INTO photo_directories (
                    parent_directory_id, name, relative_path
                ) VALUES (NULL, '', '')
                "#,
            [],
        )
        .unwrap();
    let directory_id = connection.last_insert_rowid();
    connection
        .execute(
            r#"
                INSERT INTO photos (
                    directory_id, filename, file_size, modified_at_ns
                ) VALUES (?, 'New species.jpg', 1, 1)
                "#,
            [directory_id],
        )
        .unwrap();
    let photo_id = connection.last_insert_rowid();
    connection
        .execute(
            r#"
                INSERT INTO photo_taxon_mapping (photo_id, taxon_id, status)
                VALUES (?, ?, 'matched')
                "#,
            params![photo_id, old_taxon_id],
        )
        .unwrap();
    connection
        .execute(
            r#"
                INSERT INTO photo_taxon_usage (
                    taxon_id, direct_photo_count, subtree_photo_count
                ) VALUES (?, 1, 1)
                "#,
            [old_taxon_id],
        )
        .unwrap();
    drop(connection);

    let source_path = directory.path().join("direct-import.db");
    create_direct_import_database(&source_path);
    let mut progress = Vec::new();
    let result = apply_direct_import_with_progress_and_cancellation(
        &database,
        &source_path,
        &mut |event| {
            if event.stage == APPLYING_DIRECT_IMPORT {
                assert_eq!(database.taxonomy_identity().unwrap(), old_identity);
            }
            progress.push(event);
        },
        &CancellationToken::new(),
    )
    .unwrap();
    let sync = sync::synchronize_pending_photo_libraries(&database).unwrap();

    assert_eq!(result.metadata.taxa_count, 2);
    assert_eq!(result.metadata.taxon_names_count, 2);
    assert_eq!(
        progress
            .iter()
            .map(|event| event.stage.as_str())
            .collect::<Vec<_>>(),
        [VALIDATING_DIRECT_IMPORT_DATABASE, APPLYING_DIRECT_IMPORT]
    );
    assert_ne!(database.taxonomy_identity().unwrap(), old_identity);
    assert_eq!(sync.synchronized[0].queued_photo_count, 1);
    for taxon_id in old_taxon_ids {
        assert!(get_taxon_detail(&database, taxon_id).unwrap().is_none());
    }
    assert!(get_taxon_detail(&database, 101).unwrap().is_some());
    assert!(get_taxon_detail(&database, 102).unwrap().is_some());
    assert!(
        list_operations(&database, None, 10)
            .unwrap()
            .items
            .is_empty()
    );
    let rebased = apply_rows(
        &database,
        &[TaxonInputRow {
            order: Some("New order".into()),
            en_name: Some("new order".into()),
            ..TaxonInputRow::default()
        }],
    )
    .unwrap();
    assert_eq!(rebased.operation_id, 1);
    let connection = database.connect().unwrap();
    let mapping_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM photo_taxon_mapping", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(mapping_count, 0);
    let queued_reason: String = connection
        .query_row(
            "SELECT reason FROM photo_mapping_queue WHERE photo_id = ?",
            [photo_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(queued_reason, "taxonomy");
    connection
        .execute("INSERT INTO taxa (rank) VALUES (1)", [])
        .unwrap();
    assert!(connection.last_insert_rowid() > LOCAL_TAXON_ID_FLOOR);
}

#[test]
fn rejects_an_invalid_direct_import_without_changing_taxonomy() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("vividarium.db")).unwrap();
    let taxon_ids = seed_old_taxonomy_tree(&database);
    let invalid_path = directory.path().join("invalid.db");
    create_invalid_direct_import_database(&invalid_path);

    let error = apply_direct_import(&database, &invalid_path).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("Kingdom taxon 202 must be a root taxon.")
    );
    for taxon_id in taxon_ids {
        assert!(get_taxon_detail(&database, taxon_id).unwrap().is_some());
    }
    assert_eq!(parent_taxon_id(&database, taxon_ids[1]), Some(taxon_ids[0]));
    assert_eq!(parent_taxon_id(&database, taxon_ids[2]), Some(taxon_ids[1]));
    assert_eq!(list_operations(&database, None, 10).unwrap().items.len(), 1);
}

#[test]
fn rejects_a_direct_import_database_with_text_name_types() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("vividarium.db")).unwrap();
    let source_path = directory.path().join("direct-import.db");
    create_direct_import_database_with_name_type(&source_path, "TEXT");

    let error = apply_direct_import(&database, &source_path).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("taxon_names column name_type must use INTEGER")
    );
}

#[test]
fn rejects_duplicate_names_within_a_direct_import_name_family() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("vividarium.db")).unwrap();
    let source_path = directory.path().join("direct-import.db");
    create_direct_import_database(&source_path);
    Connection::open(&source_path)
        .unwrap()
        .execute(
            r#"
            INSERT INTO taxon_names (name_id, taxon_id, name_type, name)
            VALUES (1003, 101, 2, 'New kingdom')
            "#,
            [],
        )
        .unwrap();

    let error = apply_direct_import(&database, &source_path).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("duplicate name 'New kingdom' in one name family")
    );
}

fn seed_old_taxonomy_tree(database: &Database) -> [i64; 3] {
    let result = apply_rows(
        database,
        &[
            TaxonInputRow {
                kingdom: Some("Old kingdom".into()),
                ..TaxonInputRow::default()
            },
            TaxonInputRow {
                kingdom: Some("Old kingdom".into()),
                order: Some("Old order".into()),
                ..TaxonInputRow::default()
            },
            TaxonInputRow {
                kingdom: Some("Old kingdom".into()),
                order: Some("Old order".into()),
                family: Some("Old family".into()),
                ..TaxonInputRow::default()
            },
        ],
    )
    .unwrap();
    assert_eq!(result.succeeded_rows, 3);
    [
        taxon_id_by_name(database, "Old kingdom"),
        taxon_id_by_name(database, "Old order"),
        taxon_id_by_name(database, "Old family"),
    ]
}

fn create_direct_import_database(path: &Path) {
    create_direct_import_database_with_name_type(path, "INTEGER");
}

fn create_direct_import_database_with_name_type(path: &Path, name_type: &str) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(&format!(
            r#"
                PRAGMA foreign_keys = ON;
                CREATE TABLE taxa (
                    taxon_id INTEGER PRIMARY KEY AUTOINCREMENT,
                    parent_taxon_id INTEGER,
                    rank INTEGER NOT NULL,
                    geological_range TEXT,
                    FOREIGN KEY (parent_taxon_id) REFERENCES taxa(taxon_id)
                );
                CREATE TABLE taxon_names (
                    name_id INTEGER PRIMARY KEY AUTOINCREMENT,
                    taxon_id INTEGER NOT NULL,
                    name_type {name_type} NOT NULL,
                    name TEXT NOT NULL,
                    normalized_name TEXT GENERATED ALWAYS AS (lower(name)) STORED,
                    authority_year TEXT,
                    source TEXT,
                    FOREIGN KEY (taxon_id) REFERENCES taxa(taxon_id)
                );
                INSERT INTO taxa (
                    taxon_id, parent_taxon_id, rank, geological_range
                ) VALUES
                    (101, NULL, 1, NULL),
                    (102, 101, 2, 'Recent');
                INSERT INTO taxon_names (
                    name_id, taxon_id, name_type, name
                ) VALUES
                    (1001, 101, 1, 'New_kingdom'),
                    (1002, 102, 1, '  New   order  ');
                "#
        ))
        .unwrap();
}

fn create_invalid_direct_import_database(path: &Path) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            r#"
                PRAGMA foreign_keys = ON;
                CREATE TABLE taxa (
                    taxon_id INTEGER PRIMARY KEY AUTOINCREMENT,
                    parent_taxon_id INTEGER,
                    rank INTEGER NOT NULL,
                    geological_range TEXT,
                    FOREIGN KEY (parent_taxon_id) REFERENCES taxa(taxon_id)
                );
                CREATE TABLE taxon_names (
                    name_id INTEGER PRIMARY KEY AUTOINCREMENT,
                    taxon_id INTEGER NOT NULL,
                    name_type INTEGER NOT NULL,
                    name TEXT NOT NULL,
                    normalized_name TEXT GENERATED ALWAYS AS (lower(name)) STORED,
                    authority_year TEXT,
                    source TEXT,
                    FOREIGN KEY (taxon_id) REFERENCES taxa(taxon_id)
                );
                INSERT INTO taxa (
                    taxon_id, parent_taxon_id, rank, geological_range
                ) VALUES
                    (201, NULL, 1, NULL),
                    (202, 201, 1, NULL);
                INSERT INTO taxon_names (
                    name_id, taxon_id, name_type, name
                ) VALUES
                    (2001, 201, 1, 'Invalid kingdom'),
                    (2002, 202, 1, 'Invalid family');
                "#,
        )
        .unwrap();
}

fn taxon_id_by_name(database: &Database, name: &str) -> i64 {
    database
        .connect_taxonomy_metadata_context()
        .unwrap()
        .query_row(
            "SELECT taxon_id FROM taxon_names WHERE name_type = 1 AND name = ?",
            [name],
            |row| row.get(0),
        )
        .unwrap()
}

fn parent_taxon_id(database: &Database, taxon_id: i64) -> Option<i64> {
    database
        .connect_taxonomy_metadata_context()
        .unwrap()
        .query_row(
            "SELECT parent_taxon_id FROM taxa WHERE taxon_id = ?",
            [taxon_id],
            |row| row.get(0),
        )
        .unwrap()
}
