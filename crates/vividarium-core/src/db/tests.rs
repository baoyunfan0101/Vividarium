use super::*;

#[test]
fn initializes_metadata_and_taxonomy_without_a_fake_photo_library() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    let locations = database.locations().unwrap();
    assert_eq!(
        locations.metadata_database,
        path_string(&database.metadata_path())
    );
    assert_eq!(
        schema_version(&database.connect_metadata().unwrap()),
        SCHEMA_VERSION
    );
    assert_eq!(
        schema_version(&database.connect_taxonomy().unwrap()),
        SCHEMA_VERSION
    );
    let photo_path = directory.path().join("fresh-photo.db");
    initialize_file(&photo_path, PHOTO_SCHEMA).unwrap();
    assert_eq!(
        schema_version(&open_existing_connection(&photo_path).unwrap()),
        SCHEMA_VERSION
    );
    assert_ne!(locations.metadata_database, locations.taxonomy_database);
    assert_eq!(database.active_photo_library().unwrap(), None);
    assert!(database.list_photo_libraries().unwrap().is_empty());
    assert_eq!(locations.active_photo_library_uuid, None);
}

#[test]
fn current_taxonomy_schema_adds_operation_input_storage_when_missing() {
    let directory = tempfile::tempdir().unwrap();
    let taxonomy_path = directory.path().join("taxonomy.db");
    initialize_file(&taxonomy_path, TAXONOMY_SCHEMA).unwrap();
    open_existing_connection(&taxonomy_path)
        .unwrap()
        .execute("DROP TABLE operation_inputs", [])
        .unwrap();

    initialize_existing_file(&taxonomy_path, TAXONOMY_SCHEMA).unwrap();

    assert!(
        open_existing_connection(&taxonomy_path)
            .unwrap()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'operation_inputs')",
                [],
                |row| row.get::<_, bool>(0),
            )
            .unwrap()
    );
}

#[test]
fn opens_an_existing_taxonomy_database_and_marks_libraries_for_remap() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open_test(directory.path().join("metadata.db")).unwrap();
    database
        .connect_metadata()
        .unwrap()
        .execute(
            "UPDATE taxonomy_sync_dispatch SET last_dispatched_sync_id = 42 WHERE dispatch_id = 1",
            [],
        )
        .unwrap();
    let taxonomy_path = directory.path().join("alternate-taxonomy.db");
    initialize_taxonomy_database_file(&taxonomy_path).unwrap();
    let expected_identity = open_existing_connection(&taxonomy_path)
        .unwrap()
        .query_row(
            "SELECT taxonomy_identity FROM taxonomy_identity WHERE identity_id = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap();

    let locations = database.open_taxonomy_database(&taxonomy_path).unwrap();

    assert_eq!(
        locations.taxonomy_database,
        path_string(&fs::canonicalize(&taxonomy_path).unwrap())
    );
    assert_eq!(database.taxonomy_identity().unwrap(), expected_identity);
    let metadata = database.connect_metadata().unwrap();
    assert_eq!(
        metadata
            .query_row(
                "SELECT last_dispatched_sync_id FROM taxonomy_sync_dispatch WHERE dispatch_id = 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
    assert_eq!(
        metadata
            .query_row(
                "SELECT COUNT(*) FROM photo_library_taxonomy_pending WHERE full_remap_required = 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
}

#[test]
fn rejects_a_non_taxonomy_database_when_opening_taxonomy_storage() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    let photo_database = directory.path().join("photo.db");
    initialize_file(&photo_database, PHOTO_SCHEMA).unwrap();

    let error = database
        .open_taxonomy_database(&photo_database)
        .unwrap_err();

    assert!(matches!(error, CoreError::InvalidArgument(_)));
    assert_ne!(
        database.taxonomy_path().unwrap(),
        fs::canonicalize(photo_database).unwrap()
    );
}

#[test]
fn opening_taxonomy_storage_does_not_initialize_an_empty_database() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    let empty_database = directory.path().join("empty.db");
    Connection::open(&empty_database).unwrap();

    let error = database
        .open_taxonomy_database(&empty_database)
        .unwrap_err();

    assert!(matches!(error, CoreError::InvalidArgument(_)));
    assert_eq!(
        open_existing_connection(&empty_database)
            .unwrap()
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn switches_between_independent_photo_libraries() {
    let directory = tempfile::tempdir().unwrap();
    let root_a = directory.path().join("photos-a");
    let root_b = directory.path().join("photos-b");
    fs::create_dir_all(&root_a).unwrap();
    fs::create_dir_all(&root_b).unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    let library_a = database
        .register_photo_library(&root_a, &directory.path().join("a.db"), Some("A"))
        .unwrap();
    database
        .connect()
        .unwrap()
        .execute(
            r#"
                INSERT INTO photos (
                    directory_id, filename, file_size, modified_at_ns
                ) SELECT directory_id, 'a.jpg', 1, 1
                  FROM photo_directories WHERE relative_path = ''
                "#,
            [],
        )
        .unwrap();
    let library_b = database
        .register_photo_library(&root_b, &directory.path().join("b.db"), Some("B"))
        .unwrap();
    assert_eq!(
        database
            .connect()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM photos", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    database
        .switch_photo_library(&library_a.library_uuid)
        .unwrap();
    assert_eq!(
        database
            .connect()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM photos", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_ne!(library_a.library_uuid, library_b.library_uuid);
}

#[test]
fn switching_a_missing_registered_database_does_not_recreate_it() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("photos");
    fs::create_dir_all(&root).unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    let database_path = directory.path().join("library.db");
    let library = database
        .register_photo_library(&root, &database_path, Some("Library"))
        .unwrap();
    fs::remove_file(&database_path).unwrap();

    let error = database
        .switch_photo_library(&library.library_uuid)
        .unwrap_err();

    assert!(matches!(error, CoreError::NotFound(_)));
    assert!(!database_path.exists());
}

#[test]
fn ordinary_connections_do_not_recreate_a_missing_active_library() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("photos");
    fs::create_dir_all(&root).unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    let database_path = directory.path().join("library.db");
    database
        .register_photo_library(&root, &database_path, Some("Library"))
        .unwrap();
    fs::remove_file(&database_path).unwrap();

    assert!(matches!(database.connect(), Err(CoreError::NotFound(_))));
    assert!(!database_path.exists());
    database.connect_taxonomy_metadata_context().unwrap();
    assert!(matches!(
        database.connect_taxonomy_photo_context(),
        Err(CoreError::NotFound(_))
    ));
    assert!(!database_path.exists());
    assert!(
        crate::taxonomy::suggest_taxa(&database, "missing", 10)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        crate::taxonomy::get_taxonomy_name_separator(&database).unwrap(),
        ";"
    );
}

#[test]
fn opening_migrates_only_the_online_active_photo_library() {
    let directory = tempfile::tempdir().unwrap();
    let active_root = directory.path().join("active-photos");
    let inactive_root = directory.path().join("inactive-photos");
    fs::create_dir_all(&active_root).unwrap();
    fs::create_dir_all(&inactive_root).unwrap();
    let metadata_path = directory.path().join("metadata.db");
    let database = Database::open(&metadata_path).unwrap();
    let active_path = directory.path().join("active.db");
    let active = database
        .register_photo_library(&active_root, &active_path, Some("Active"))
        .unwrap();
    let inactive_path = directory.path().join("inactive.db");
    let inactive = database
        .register_photo_library(&inactive_root, &inactive_path, Some("Inactive"))
        .unwrap();
    database.switch_photo_library(&active.library_uuid).unwrap();
    for path in [&active_path, &inactive_path] {
        let connection = open_existing_connection(path).unwrap();
        connection
            .execute_batch(
                r#"
                DROP TRIGGER photo_taxon_mapping_au_names;
                DROP TABLE photo_taxon_mapping_names;
                PRAGMA user_version = 2;
                "#,
            )
            .unwrap();
    }
    drop(database);

    let database = Database::open(&metadata_path).unwrap();
    assert_eq!(
        schema_version(&open_existing_connection(&active_path).unwrap()),
        SCHEMA_VERSION
    );
    assert_eq!(
        schema_version(&open_existing_connection(&inactive_path).unwrap()),
        2
    );

    database
        .switch_photo_library(&inactive.library_uuid)
        .unwrap();
    assert_eq!(
        schema_version(&open_existing_connection(&inactive_path).unwrap()),
        SCHEMA_VERSION
    );
}

#[test]
fn opening_retains_an_offline_active_photo_library_registration() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("photos");
    fs::create_dir_all(&root).unwrap();
    let metadata_path = directory.path().join("metadata.db");
    let database = Database::open(&metadata_path).unwrap();
    let database_path = directory.path().join("library.db");
    let library = database
        .register_photo_library(&root, &database_path, Some("Offline"))
        .unwrap();
    fs::remove_file(&database_path).unwrap();
    drop(database);

    let database = Database::open(&metadata_path).unwrap();
    assert_eq!(
        database
            .active_photo_library()
            .unwrap()
            .unwrap()
            .library_uuid,
        library.library_uuid
    );
    assert!(!database_path.exists());
}

#[test]
fn opening_migrates_an_active_photo_library_with_a_missing_root() {
    let directory = tempfile::tempdir().unwrap();
    let missing_root = directory.path().join("missing-photos");
    fs::create_dir_all(&missing_root).unwrap();
    let metadata_path = directory.path().join("metadata.db");
    let database = Database::open(&metadata_path).unwrap();
    let database_path = directory.path().join("library.db");
    let library = database
        .register_photo_library(&missing_root, &database_path, Some("Offline root"))
        .unwrap();
    let connection = open_existing_connection(&database_path).unwrap();
    connection
        .execute_batch(
            r#"
            DROP TRIGGER photo_taxon_mapping_au_names;
            DROP TABLE photo_taxon_mapping_names;
            PRAGMA user_version = 2;
            "#,
        )
        .unwrap();
    fs::remove_dir(&missing_root).unwrap();
    drop(database);

    let database = Database::open(&metadata_path).unwrap();
    assert!(!missing_root.exists());
    assert_eq!(
        database
            .active_photo_library()
            .unwrap()
            .unwrap()
            .library_uuid,
        library.library_uuid
    );
    let connection = open_existing_connection(&database_path).unwrap();
    assert_eq!(schema_version(&connection), SCHEMA_VERSION);
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE name = 'photo_taxon_mapping_names'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );

    let rebound_root = directory.path().join("rebound-photos");
    fs::create_dir_all(&rebound_root).unwrap();
    database
        .rebind_photo_library_root(&library.library_uuid, &rebound_root)
        .unwrap();
    assert!(crate::photos::is_initial_index_complete(&database, &library.library_uuid).is_ok());
}

#[test]
fn rebinds_a_missing_active_library_database_and_restores_sync() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("photos");
    fs::create_dir_all(&root).unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    let old_path = directory.path().join("old.db");
    let new_path = directory.path().join("new.db");
    let library = database
        .register_photo_library(&root, &old_path, Some("Library"))
        .unwrap();
    database
        .connect()
        .unwrap()
        .execute(
            r#"
                INSERT INTO photos (
                    directory_id, filename, file_size, modified_at_ns
                )
                SELECT directory_id, 'photo.jpg', 1, 1
                FROM photo_directories
                WHERE relative_path = ''
                "#,
            [],
        )
        .unwrap();
    fs::rename(&old_path, &new_path).unwrap();
    assert!(matches!(database.connect(), Err(CoreError::NotFound(_))));

    {
        let mut taxonomy = database.connect_taxonomy().unwrap();
        let transaction = taxonomy
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        crate::taxonomy::sync::record_event(&transaction, None, [42], false).unwrap();
        transaction.commit().unwrap();
    }
    crate::taxonomy::sync::synchronize_pending_photo_libraries(&database).unwrap();
    let latest_sync_id = database.latest_taxonomy_sync_id().unwrap();
    database
        .connect_metadata()
        .unwrap()
        .execute(
            "DELETE FROM photo_library_taxonomy_pending WHERE library_uuid = ?",
            [&library.library_uuid],
        )
        .unwrap();

    let rebound = database
        .rebind_photo_library_database(&library.library_uuid, &new_path)
        .unwrap();

    assert_eq!(rebound.db_path, path_string(&new_path));
    assert_eq!(
        database
            .connect_metadata()
            .unwrap()
            .query_row(
                r#"
                    SELECT target_sync_id, full_remap_required
                    FROM photo_library_taxonomy_pending
                    WHERE library_uuid = ?
                    "#,
                [&library.library_uuid],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, bool>(1)?)),
            )
            .unwrap(),
        (latest_sync_id, true)
    );
    crate::taxonomy::sync::synchronize_pending_photo_libraries(&database).unwrap();
    assert_eq!(
        database
            .connect()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM photo_mapping_queue", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        1
    );
}

#[test]
fn rejects_rebinding_to_a_different_library_database() {
    let directory = tempfile::tempdir().unwrap();
    let root_a = directory.path().join("photos-a");
    let root_b = directory.path().join("photos-b");
    fs::create_dir_all(&root_a).unwrap();
    fs::create_dir_all(&root_b).unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    let path_a = directory.path().join("a.db");
    let path_b = directory.path().join("b.db");
    let library_a = database
        .register_photo_library(&root_a, &path_a, Some("A"))
        .unwrap();
    database
        .register_photo_library(&root_b, &path_b, Some("B"))
        .unwrap();

    let error = database
        .rebind_photo_library_database(&library_a.library_uuid, &path_b)
        .unwrap_err();

    assert!(matches!(error, CoreError::InvalidArgument(_)));
    assert_eq!(
        database
            .photo_library(&library_a.library_uuid)
            .unwrap()
            .db_path,
        path_string(&path_a)
    );
}

#[test]
fn photo_library_roots_are_unique() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("photos");
    fs::create_dir_all(&root).unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    database
        .register_photo_library(&root, &directory.path().join("first.db"), Some("First"))
        .unwrap();

    let error = database
        .register_photo_library(&root, &directory.path().join("second.db"), Some("Second"))
        .unwrap_err();

    assert!(matches!(error, CoreError::InvalidArgument(_)));
    assert_eq!(database.list_photo_libraries().unwrap().len(), 1);
    assert!(!directory.path().join("second.db").exists());
}

#[test]
fn renames_a_photo_library_registration() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("photos");
    fs::create_dir_all(&root).unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    let library = database
        .register_photo_library(&root, &directory.path().join("library.db"), Some("Before"))
        .unwrap();

    let renamed = database
        .rename_photo_library(&library.library_uuid, "  After  ")
        .unwrap();

    assert_eq!(renamed.display_name, "After");
    assert_eq!(
        database.list_photo_libraries().unwrap()[0].display_name,
        "After"
    );
    assert!(matches!(
        database.rename_photo_library(&library.library_uuid, "  "),
        Err(CoreError::InvalidArgument(_))
    ));
}

#[test]
fn removes_an_active_photo_library_registration_without_storage_access() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("photos");
    fs::create_dir_all(&root).unwrap();
    let database_path = directory.path().join("library.db");
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    let library = database
        .register_photo_library(&root, &database_path, Some("Library"))
        .unwrap();

    fs::remove_file(&database_path).unwrap();
    fs::remove_dir(&root).unwrap();
    database
        .remove_photo_library(&library.library_uuid)
        .unwrap();

    assert_eq!(database.active_photo_library().unwrap(), None);
    assert!(database.list_photo_libraries().unwrap().is_empty());
    assert!(!database_path.exists());
    assert!(!root.exists());
}

#[test]
fn removing_an_active_registration_preserves_its_files() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("photos");
    fs::create_dir_all(&root).unwrap();
    let database_path = directory.path().join("library.db");
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    let library = database
        .register_photo_library(&root, &database_path, Some("Library"))
        .unwrap();

    database
        .remove_photo_library(&library.library_uuid)
        .unwrap();

    assert_eq!(database.active_photo_library().unwrap(), None);
    assert!(root.is_dir());
    assert!(database_path.is_file());
}

#[test]
fn registering_a_stale_library_queues_a_full_remap() {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    let taxonomy_identity = database.taxonomy_identity().unwrap();
    {
        let mut taxonomy = database.connect_taxonomy().unwrap();
        let transaction = taxonomy
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        crate::taxonomy::sync::record_event(&transaction, None, [42], false).unwrap();
        transaction.commit().unwrap();
    }
    crate::taxonomy::sync::synchronize_pending_photo_libraries(&database).unwrap();
    let latest_sync_id = database.latest_taxonomy_sync_id().unwrap();
    assert!(latest_sync_id > 0);
    assert_eq!(
        database
            .connect_taxonomy()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM taxonomy_sync_events", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        0
    );

    let root = directory.path().join("photos");
    fs::create_dir_all(&root).unwrap();
    let library_path = directory.path().join("stale.db");
    initialize_file(&library_path, PHOTO_SCHEMA).unwrap();
    let library_uuid = new_uuid();
    {
        let connection = open_existing_connection(&library_path).unwrap();
        connection
            .execute(
                r#"
                    INSERT INTO photo_library (
                        library_id, library_uuid, root_path,
                        bound_taxonomy_identity, last_taxonomy_sync_id
                    ) VALUES (1, ?, ?, ?, 0)
                    "#,
                params![library_uuid, path_string(&root), taxonomy_identity],
            )
            .unwrap();
        ensure_photo_root(&connection).unwrap();
        connection
            .execute(
                r#"
                    INSERT INTO photos (
                        directory_id, filename, file_size, modified_at_ns
                    )
                    SELECT directory_id, 'stale.jpg', 1, 1
                    FROM photo_directories
                    WHERE relative_path = ''
                    "#,
                [],
            )
            .unwrap();
    }

    let registered = database
        .register_photo_library(&root, &library_path, Some("Stale"))
        .unwrap();
    crate::taxonomy::sync::synchronize_pending_photo_libraries(&database).unwrap();

    assert_eq!(registered.library_uuid, library_uuid);
    let connection = database.connect().unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT last_taxonomy_sync_id FROM photo_library WHERE library_id = 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        latest_sync_id
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM photo_mapping_queue", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        1
    );
}

#[test]
fn relocation_uses_a_consistent_wal_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("photos");
    fs::create_dir_all(&root).unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    let source = directory.path().join("source.db");
    let library = database
        .register_photo_library(&root, &source, Some("Library"))
        .unwrap();
    let connection = database.connect().unwrap();
    connection
        .execute(
            r#"
                INSERT INTO photos (
                    directory_id, filename, file_size, modified_at_ns
                )
                SELECT directory_id, 'wal-photo.jpg', 1, 1
                FROM photo_directories
                WHERE relative_path = ''
                "#,
            [],
        )
        .unwrap();
    let destination = directory.path().join("destination.db");

    database
        .relocate_photo_library_database(&library.library_uuid, &destination)
        .unwrap();
    drop(connection);

    assert!(!source.exists());
    assert!(destination.exists());
    assert_eq!(
        database
            .connect()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM photos WHERE filename = 'wal-photo.jpg'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
}

#[test]
fn rejects_any_other_schema_version() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("metadata.db");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch("PRAGMA user_version = 1;")
        .unwrap();
    drop(connection);
    let error = Database::open(path).unwrap_err();
    assert!(error.to_string().contains("expected 3"));
}

#[test]
fn migrates_v2_photo_libraries_with_mapping_provenance() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("photos.db");
    initialize_file(&path, PHOTO_SCHEMA).unwrap();
    let connection = open_existing_connection(&path).unwrap();
    connection
        .execute_batch(
            r#"
            DROP TRIGGER photo_taxon_mapping_au_names;
            DROP TABLE photo_taxon_mapping_names;
            INSERT INTO photo_directories (relative_path, name) VALUES ('', 'root');
            INSERT INTO photos (
                directory_id, filename, file_size, modified_at_ns
            ) VALUES (1, 'mapped.jpg', 1, 1);
            INSERT INTO photo_taxon_mapping (photo_id, taxon_id, status)
            VALUES (1, 99, 'matched');
            PRAGMA user_version = 2;
            "#,
        )
        .unwrap();
    drop(connection);

    initialize_existing_file(&path, PHOTO_SCHEMA).unwrap();

    let connection = open_existing_connection(&path).unwrap();
    assert_eq!(schema_version(&connection), SCHEMA_VERSION);
    for object in [
        "photo_taxon_mapping_names",
        "photo_taxon_candidate_names",
        "photo_taxon_mapping_au_names",
        "photo_taxon_candidates_bi",
        "photo_taxon_mapping_au_candidates",
        "idx_photo_taxon_candidates_taxon",
    ] {
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_schema WHERE name = ?",
                    [object],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1,
            "missing migrated schema object {object}"
        );
    }
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM photo_taxon_mapping_names",
                [],
                |row| { row.get::<_, i64>(0) }
            )
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT taxon_id FROM photo_taxon_mapping WHERE photo_id = 1",
                [],
                |row| { row.get::<_, i64>(0) }
            )
            .unwrap(),
        99
    );
}

#[test]
fn failed_v2_photo_migration_keeps_the_original_schema_version() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("photos.db");
    initialize_file(&path, PHOTO_SCHEMA).unwrap();
    let connection = open_existing_connection(&path).unwrap();
    connection.pragma_update(None, "user_version", 2).unwrap();
    drop(connection);

    assert!(initialize_existing_file(&path, PHOTO_SCHEMA).is_err());

    let connection = open_existing_connection(&path).unwrap();
    assert_eq!(schema_version(&connection), 2);
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE name = 'photo_taxon_mapping_names'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
}

#[test]
fn migrates_v2_metadata_database_to_v3() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("metadata.db");
    initialize_file(&path, METADATA_SCHEMA).unwrap();
    let connection = open_existing_connection(&path).unwrap();
    connection.pragma_update(None, "user_version", 2).unwrap();
    drop(connection);

    initialize_existing_file(&path, METADATA_SCHEMA).unwrap();

    assert_eq!(
        schema_version(&open_existing_connection(&path).unwrap()),
        SCHEMA_VERSION
    );
}

#[test]
fn released_v3_0_fixture_upgrades_the_active_library_without_data_loss() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("photos");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("original.JPG"), b"photo").unwrap();
    let metadata_path = directory.path().join("metadata.db");
    let taxonomy_path = directory.path().join("taxonomy.db");
    let photo_path = directory.path().join("photos.db");
    let library_uuid = "released-v3-0-library";

    let metadata = Connection::open(&metadata_path).unwrap();
    metadata
        .execute_batch(include_str!("../../tests/fixtures/schema-v2/metadata.sql"))
        .unwrap();
    metadata
        .execute(
            r#"
            INSERT INTO storage_settings (
                settings_id, taxonomy_db_path,
                default_taxonomy_directory, default_photo_library_directory
            ) VALUES (1, ?, ?, ?)
            "#,
            params![
                path_string(&taxonomy_path),
                path_string(directory.path()),
                path_string(directory.path())
            ],
        )
        .unwrap();
    metadata
        .execute(
            "INSERT INTO photo_libraries (library_uuid, display_name, root_path, db_path) VALUES (?, 'Released', ?, ?)",
            params![library_uuid, path_string(&root), path_string(&photo_path)],
        )
        .unwrap();
    metadata
        .execute(
            "INSERT INTO active_photo_library (active_id, library_uuid) VALUES (1, ?)",
            [library_uuid],
        )
        .unwrap();
    drop(metadata);

    let taxonomy = Connection::open(&taxonomy_path).unwrap();
    taxonomy
        .execute_batch(include_str!("../../tests/fixtures/schema-v2/taxonomy.sql"))
        .unwrap();
    taxonomy
        .execute(
            "INSERT INTO taxonomy_identity (identity_id, taxonomy_identity) VALUES (1, 'released-v3-0-taxonomy')",
            [],
        )
        .unwrap();
    taxonomy
        .execute("INSERT INTO taxa (taxon_id, rank) VALUES (1, 5)", [])
        .unwrap();
    taxonomy
        .execute(
            "INSERT INTO taxon_names (name_id, taxon_id, name_type, name) VALUES (1, 1, 1, 'Canis lupus')",
            [],
        )
        .unwrap();
    drop(taxonomy);

    let photo = Connection::open(&photo_path).unwrap();
    photo
        .execute_batch(include_str!("../../tests/fixtures/schema-v2/photo.sql"))
        .unwrap();
    photo
        .execute(
            r#"
            INSERT INTO photo_library (
                library_id, library_uuid, root_path,
                bound_taxonomy_identity, last_taxonomy_sync_id
            ) VALUES (1, ?, ?, 'released-v3-0-taxonomy', 0)
            "#,
            params![library_uuid, path_string(&root)],
        )
        .unwrap();
    photo
        .execute(
            "INSERT INTO photo_directories (directory_id, name, relative_path) VALUES (1, 'photos', '')",
            [],
        )
        .unwrap();
    photo
        .execute(
            "INSERT INTO photos (photo_id, directory_id, filename, file_size, modified_at_ns) VALUES (1, 1, 'original.JPG', 5, 1)",
            [],
        )
        .unwrap();
    photo
        .execute(
            "INSERT INTO photo_taxon_mapping (photo_id, taxon_id, status) VALUES (1, 1, 'matched')",
            [],
        )
        .unwrap();
    drop(photo);

    let database = Database::open(&metadata_path).unwrap();
    for path in [&metadata_path, &taxonomy_path, &photo_path] {
        let connection = open_existing_connection(path).unwrap();
        assert_eq!(schema_version(&connection), SCHEMA_VERSION);
        assert_eq!(
            connection
                .query_row("PRAGMA foreign_key_check", [], |row| row
                    .get::<_, String>(0))
                .optional()
                .unwrap(),
            None
        );
        assert_eq!(
            connection
                .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
    }
    assert_eq!(
        crate::photos::get_photo(&database, 1)
            .unwrap()
            .unwrap()
            .filename,
        "original.JPG"
    );
    assert_eq!(
        crate::mapping::get_photo_mapping(&database, 1)
            .unwrap()
            .taxon_id,
        Some(1)
    );
    assert!(crate::mapping::get_photo_mapping_detail(&database, 1).is_ok());
    assert_eq!(
        crate::photos::rename_photo_from_taxon(&database, 1)
            .unwrap()
            .filename,
        "Canis lupus.JPG"
    );
    assert!(root.join("Canis lupus.JPG").is_file());
    assert_eq!(
        open_existing_connection(&photo_path)
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE name = 'photo_taxon_mapping_names'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
}

#[test]
fn migrated_v2_photo_library_supports_rename_from_taxonomy() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("photos");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("original.JPG"), b"photo").unwrap();
    let database = Database::open(directory.path().join("metadata.db")).unwrap();
    let taxonomy = database.connect_taxonomy().unwrap();
    taxonomy
        .execute("INSERT INTO taxa (taxon_id, rank) VALUES (1, 5)", [])
        .unwrap();
    taxonomy
        .execute(
            "INSERT INTO taxon_names (name_id, taxon_id, name_type, name) VALUES (1, 1, 1, 'Canis lupus')",
            [],
        )
        .unwrap();
    drop(taxonomy);
    let photo_path = directory.path().join("released-v2-photo.db");
    initialize_file(&photo_path, PHOTO_SCHEMA).unwrap();
    let photo_connection = open_existing_connection(&photo_path).unwrap();
    photo_connection
        .execute_batch(
            r#"
            DROP TRIGGER photo_taxon_mapping_au_names;
            DROP TABLE photo_taxon_mapping_names;
            PRAGMA user_version = 2;
            "#,
        )
        .unwrap();
    drop(photo_connection);

    database
        .register_photo_library(&root, &photo_path, Some("Migrated"))
        .unwrap();
    let library = crate::photos::get_library(&database).unwrap().unwrap();
    crate::photos::refresh_directory(&database, library.root_directory_id).unwrap();
    let photo = crate::photos::list_photos(&database).unwrap().remove(0);
    crate::mapping::set_photo_mapping(&database, photo.photo_id, 1).unwrap();

    let renamed = crate::photos::rename_photo_from_taxon(&database, photo.photo_id).unwrap();

    assert_eq!(renamed.filename, "Canis lupus.JPG");
    assert!(root.join("Canis lupus.JPG").is_file());
}

#[test]
fn migrates_v2_taxonomy_name_family_duplicates() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("taxonomy.db");
    initialize_taxonomy_database_file(&path).unwrap();
    let connection = open_existing_connection(&path).unwrap();
    connection
        .execute_batch(
            r#"
            DROP INDEX idx_taxon_names_scientific_family_name;
            DROP INDEX idx_taxon_names_chinese_family_name;
            DROP INDEX idx_taxon_names_english_family_name;
            INSERT INTO taxa (taxon_id, rank) VALUES (1, 1);
            INSERT INTO taxon_names (
                name_id, taxon_id, name_type, name, authority_year, source
            ) VALUES
                (1, 1, 1, 'Animalia', 'accepted authority', NULL),
                (2, 1, 2, 'Animalia', 'alias authority', 'catalog');
            INSERT INTO taxonomy_base_metadata (
                metadata_id, source_path, taxa_count, taxon_names_count
            ) VALUES (1, 'test', 1, 2);
            PRAGMA user_version = 2;
            "#,
        )
        .unwrap();
    drop(connection);

    initialize_existing_file(&path, TAXONOMY_SCHEMA).unwrap();

    let connection = open_existing_connection(&path).unwrap();
    assert_eq!(schema_version(&connection), SCHEMA_VERSION);
    assert_eq!(
        connection
            .query_row(
                r#"
                SELECT name_id, name_type, authority_year, source
                FROM taxon_names
                WHERE taxon_id = 1 AND name = 'Animalia'
                "#,
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .unwrap(),
        (
            1,
            1,
            Some("accepted authority".into()),
            Some("catalog".into())
        )
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM taxon_names_fts WHERE taxon_names_fts MATCH 'Animalia'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT taxon_names_count FROM taxonomy_base_metadata WHERE metadata_id = 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
    assert!(
        connection
            .execute(
                "INSERT INTO taxon_names (taxon_id, name_type, name) VALUES (1, 2, 'Animalia')",
                [],
            )
            .is_err()
    );
}

fn schema_version(connection: &Connection) -> i64 {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap()
}
