use super::*;

#[test]
fn thumbnails_are_isolated_between_photo_libraries() {
    use std::fs::{FileTimes, OpenOptions};
    use std::time::{Duration, UNIX_EPOCH};

    let data = tempfile::tempdir().unwrap();
    let root_a = tempfile::tempdir().unwrap();
    let root_b = tempfile::tempdir().unwrap();
    let photo_a = root_a.path().join("same.bmp");
    let photo_b = root_b.path().join("same.bmp");
    image::RgbImage::from_pixel(4, 4, image::Rgb([255, 0, 0]))
        .save(&photo_a)
        .unwrap();
    image::RgbImage::from_pixel(4, 4, image::Rgb([0, 0, 255]))
        .save(&photo_b)
        .unwrap();
    let times = FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(1_700_000_000));
    OpenOptions::new()
        .write(true)
        .open(&photo_a)
        .unwrap()
        .set_times(times)
        .unwrap();
    OpenOptions::new()
        .write(true)
        .open(&photo_b)
        .unwrap()
        .set_times(times)
        .unwrap();

    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    let thumbnail_root = data.path().join("thumbnails");
    open_library(&database, root_a.path().to_str().unwrap()).unwrap();
    let library_a = database.active_photo_library().unwrap().unwrap();
    let mut progress = |_: OperationProgress| {};
    initial_index_photo_library(&database, &library_a.library_uuid, &mut progress).unwrap();
    let indexed_a = list_photos(&database).unwrap().remove(0);
    let thumbnail_a = get_or_create_thumbnail_for_library(
        &database,
        &library_a.library_uuid,
        indexed_a.photo_id,
        &thumbnail_root,
    )
    .unwrap();

    open_library(&database, root_b.path().to_str().unwrap()).unwrap();
    let library_b = database.active_photo_library().unwrap().unwrap();
    initial_index_photo_library(&database, &library_b.library_uuid, &mut progress).unwrap();
    let indexed_b = list_photos(&database).unwrap().remove(0);
    let thumbnail_b = get_or_create_thumbnail_for_library(
        &database,
        &library_b.library_uuid,
        indexed_b.photo_id,
        &thumbnail_root,
    )
    .unwrap();

    assert_eq!(indexed_a.photo_id, indexed_b.photo_id);
    assert_eq!(indexed_a.file_size, indexed_b.file_size);
    assert_eq!(indexed_a.modified_at_ns, indexed_b.modified_at_ns);
    assert_ne!(thumbnail_a, thumbnail_b);
    assert!(thumbnail_a.starts_with(thumbnail_root.join(&library_a.library_uuid)));
    assert!(thumbnail_b.starts_with(thumbnail_root.join(&library_b.library_uuid)));
    assert_ne!(
        fs::read(thumbnail_a).unwrap(),
        fs::read(thumbnail_b).unwrap()
    );
}

#[test]
fn initial_index_is_durable_and_does_not_generate_thumbnails() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("first.jpg"), b"first").unwrap();
    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    open_library(&database, root.path().to_str().unwrap()).unwrap();
    let library = database.active_photo_library().unwrap().unwrap();
    assert!(!is_initial_index_complete(&database, &library.library_uuid).unwrap());
    let mut updates = Vec::new();
    let indexed = {
        let mut progress = |progress: OperationProgress| {
            updates.push(progress);
        };
        initial_index_photo_library(&database, &library.library_uuid, &mut progress)
            .unwrap()
            .unwrap()
    };

    assert_eq!(indexed.inserted, 1);
    assert!(is_initial_index_complete(&database, &library.library_uuid).unwrap());
    assert_eq!(list_photos(&database).unwrap()[0].thumbnail_path, None);
    assert_eq!(
        updates.last().map(|update| update.stage.as_str()),
        Some("photo_index_complete")
    );

    fs::write(root.path().join("later.jpg"), b"later").unwrap();
    let mut progress = |_: OperationProgress| {};
    assert!(
        initial_index_photo_library(&database, &library.library_uuid, &mut progress)
            .unwrap()
            .is_none()
    );
    assert_eq!(get_photo_count(&database).unwrap(), 1);
}

#[test]
fn repeat_photo_scan_skips_unchanged_files_and_queues_new_or_changed_files() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("first.jpg"), b"first").unwrap();
    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    open_library(&database, root.path().to_str().unwrap()).unwrap();
    let library = database.active_photo_library().unwrap().unwrap();
    let mut progress = |_: OperationProgress| {};

    let initial = scan_photo_library(&database, &library.library_uuid, &mut progress).unwrap();
    assert_eq!(initial.inserted, 1);
    crate::mapping::process_pending_photo_matches(&database, &mut progress).unwrap();
    assert!(!crate::mapping::has_pending_photo_matches(&database).unwrap());

    let unchanged = scan_photo_library(&database, &library.library_uuid, &mut progress).unwrap();
    assert_eq!(unchanged.unchanged, 1);
    assert_eq!(unchanged.inserted, 0);
    assert_eq!(unchanged.updated, 0);
    assert!(!crate::mapping::has_pending_photo_matches(&database).unwrap());

    fs::write(root.path().join("first.jpg"), b"first changed").unwrap();
    fs::write(root.path().join("second.jpg"), b"second").unwrap();
    let changed = scan_photo_library(&database, &library.library_uuid, &mut progress).unwrap();
    assert_eq!(changed.inserted, 1);
    assert_eq!(changed.updated, 1);
    assert!(crate::mapping::has_pending_photo_matches(&database).unwrap());
}

#[test]
fn metadata_index_skips_completed_photos_and_reports_progress() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    image::RgbImage::from_pixel(8, 6, image::Rgb([10, 20, 30]))
        .save(root.path().join("first.bmp"))
        .unwrap();
    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    open_library(&database, root.path().to_str().unwrap()).unwrap();
    let library = database.active_photo_library().unwrap().unwrap();
    let mut progress = |_: OperationProgress| {};
    initial_index_photo_library(&database, &library.library_uuid, &mut progress).unwrap();
    assert!(has_pending_photo_metadata(&database, &library.library_uuid).unwrap());

    let mut updates = Vec::new();
    let first =
        index_photo_metadata_for_library(&database, &library.library_uuid, &mut |progress| {
            updates.push(progress)
        })
        .unwrap();
    assert_eq!(first.total, 1);
    assert_eq!(first.previously_indexed, 0);
    assert_eq!(first.indexed, 1);
    assert_eq!(updates.last().and_then(|value| value.current), Some(1));
    assert!(!has_pending_photo_metadata(&database, &library.library_uuid).unwrap());

    let second =
        index_photo_metadata_for_library(&database, &library.library_uuid, &mut progress).unwrap();
    assert_eq!(second.previously_indexed, 1);
    assert_eq!(second.indexed, 0);

    image::RgbImage::from_pixel(10, 7, image::Rgb([30, 20, 10]))
        .save(root.path().join("first.bmp"))
        .unwrap();
    let root_id = get_library(&database).unwrap().unwrap().root_directory_id;
    let refreshed = refresh_directory(&database, root_id).unwrap();
    assert_eq!(refreshed.updated, 1);
    assert!(has_pending_photo_metadata(&database, &library.library_uuid).unwrap());
    let changed =
        index_photo_metadata_for_library(&database, &library.library_uuid, &mut progress).unwrap();
    assert_eq!(changed.indexed, 1);
    let photo_id = list_photos(&database).unwrap()[0].photo_id;
    assert_eq!(
        get_photo_metadata(&database, photo_id).unwrap().width,
        Some(10)
    );
}

#[test]
fn background_metadata_makes_gps_photo_queryable_without_opening_details() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    write_gps_tiff(&root.path().join("located.tif"));
    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    open_library(&database, root.path().to_str().unwrap()).unwrap();
    let library = database.active_photo_library().unwrap().unwrap();
    let mut progress = |_: OperationProgress| {};
    initial_index_photo_library(&database, &library.library_uuid, &mut progress).unwrap();

    assert!(
        crate::map::list_map_photos(&database, None, None, 10)
            .unwrap()
            .items
            .is_empty()
    );
    index_photo_metadata_for_library(&database, &library.library_uuid, &mut progress).unwrap();
    let located = crate::map::list_map_photos(&database, None, None, 10).unwrap();
    assert_eq!(located.items.len(), 1);
    assert!((located.items[0].latitude - 39.9).abs() < 0.000_1);
    assert!((located.items[0].longitude - (116.0 + 23.0 / 60.0)).abs() < 0.000_1);
}

fn write_gps_tiff(path: &std::path::Path) {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"II");
    bytes.extend_from_slice(&42_u16.to_le_bytes());
    bytes.extend_from_slice(&8_u32.to_le_bytes());
    bytes.extend_from_slice(&3_u16.to_le_bytes());
    tiff_entry(&mut bytes, 0x0100, 4, 1, 1);
    tiff_entry(&mut bytes, 0x0101, 4, 1, 1);
    tiff_entry(&mut bytes, 0x8825, 4, 1, 50);
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    tiff_entry(&mut bytes, 1, 2, 2, u32::from_le_bytes(*b"N\0\0\0"));
    tiff_entry(&mut bytes, 2, 5, 3, 104);
    tiff_entry(&mut bytes, 3, 2, 2, u32::from_le_bytes(*b"E\0\0\0"));
    tiff_entry(&mut bytes, 4, 5, 3, 128);
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    for value in [39_u32, 54, 0, 116, 23, 0] {
        bytes.extend_from_slice(&value.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
    }
    fs::write(path, bytes).unwrap();
}

fn tiff_entry(bytes: &mut Vec<u8>, tag: u16, field_type: u16, count: u32, value: u32) {
    bytes.extend_from_slice(&tag.to_le_bytes());
    bytes.extend_from_slice(&field_type.to_le_bytes());
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&value.to_le_bytes());
}

#[test]
fn failed_initial_index_remains_retryable() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    open_library(&database, root.path().to_str().unwrap()).unwrap();
    let library = database.active_photo_library().unwrap().unwrap();
    root.close().unwrap();
    let mut progress = |_: OperationProgress| {};

    assert!(initial_index_photo_library(&database, &library.library_uuid, &mut progress).is_err());
    assert!(!is_initial_index_complete(&database, &library.library_uuid).unwrap());
}

#[test]
fn legacy_library_without_an_index_marker_requires_one_incremental_scan() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    open_library(&database, root.path().to_str().unwrap()).unwrap();
    let library = database.active_photo_library().unwrap().unwrap();
    database
        .connect()
        .unwrap()
        .execute("DROP TABLE photo_library_index_state", [])
        .unwrap();

    assert!(!is_initial_index_complete(&database, &library.library_uuid).unwrap());
}

#[test]
fn opens_and_refreshes_the_requested_directory_subtree() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("nested")).unwrap();
    fs::write(root.path().join("first.jpg"), b"first").unwrap();
    fs::write(root.path().join("nested").join("second.jpg"), b"second").unwrap();
    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
    let mut progress = Vec::new();
    let result =
        refresh_directory_with_progress(&database, library.root_directory_id, &mut |event| {
            progress.push(event)
        })
        .unwrap();
    assert_eq!(result.inserted, 2);
    assert_eq!(result.directories_inserted, 1);
    assert_eq!(list_photos(&database).unwrap().len(), 2);
    assert!(progress.iter().any(|event| event.stage == "scanning_files"));
    assert!(
        progress
            .iter()
            .any(|event| event.stage == "updating_photo_index")
    );
    assert!(
        progress
            .iter()
            .all(|event| event.current.is_none() && event.total.is_none())
    );
    let listing = browse_directory(&database, library.root_directory_id, None, 20).unwrap();
    assert_eq!(listing.items.len(), 2);
    match &listing.items[0] {
        PhotoDirectoryItem::Directory { .. } => {}
        PhotoDirectoryItem::Photo { .. } => panic!("expected a directory first"),
    };
    assert!(matches!(listing.items[1], PhotoDirectoryItem::Photo { .. }));
    assert_eq!(
        get_directory_counts(&database, library.root_directory_id).unwrap(),
        DirectoryEntryCounts {
            directory_count: 1,
            file_count: 1,
        }
    );
    assert_eq!(get_photo_count(&database).unwrap(), 2);
}

#[test]
fn refresh_removes_missing_directory_subtrees() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("nested")).unwrap();
    fs::create_dir(root.path().join("nested").join("deep")).unwrap();
    fs::write(root.path().join("root.jpg"), b"root").unwrap();
    fs::write(root.path().join("nested").join("nested.jpg"), b"nested").unwrap();
    fs::write(
        root.path().join("nested").join("deep").join("deep.jpg"),
        b"deep",
    )
    .unwrap();
    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
    let initial = refresh_directory(&database, library.root_directory_id).unwrap();
    assert_eq!(initial.inserted, 3);
    assert_eq!(initial.directories_inserted, 2);

    fs::remove_dir_all(root.path().join("nested")).unwrap();
    let result = refresh_directory(&database, library.root_directory_id).unwrap();

    assert_eq!(result.unchanged, 1);
    assert_eq!(result.deleted, 2);
    assert_eq!(result.directories_deleted, 2);
    assert_eq!(list_photos(&database).unwrap().len(), 1);
    assert_eq!(get_photo_count(&database).unwrap(), 1);
}

#[test]
fn resolves_photo_directory_path_inside_the_library_root() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("nested")).unwrap();
    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
    refresh_directory(&database, library.root_directory_id).unwrap();
    let listing = browse_directory(&database, library.root_directory_id, None, 20).unwrap();
    let nested_directory_id = match &listing.items[0] {
        PhotoDirectoryItem::Directory { directory } => directory.directory_id,
        PhotoDirectoryItem::Photo { .. } => panic!("expected a directory first"),
    };

    assert_eq!(
        photo_directory_path(&database, nested_directory_id).unwrap(),
        root.path().join("nested").canonicalize().unwrap(),
    );
}

#[test]
fn renames_photo_directory_and_updates_descendant_paths() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("nested")).unwrap();
    fs::create_dir(root.path().join("nested").join("deep")).unwrap();
    fs::write(root.path().join("nested").join("nested.jpg"), b"nested").unwrap();
    fs::write(
        root.path().join("nested").join("deep").join("deep.jpg"),
        b"deep",
    )
    .unwrap();
    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
    refresh_directory(&database, library.root_directory_id).unwrap();
    let listing = browse_directory(&database, library.root_directory_id, None, 20).unwrap();
    let nested_directory_id = match &listing.items[0] {
        PhotoDirectoryItem::Directory { directory } => directory.directory_id,
        PhotoDirectoryItem::Photo { .. } => panic!("expected a directory first"),
    };

    let renamed = rename_directory(&database, nested_directory_id, "renamed").unwrap();

    assert_eq!(renamed.name, "renamed");
    assert_eq!(renamed.relative_path, "renamed");
    assert!(!root.path().join("nested").exists());
    assert!(root.path().join("renamed").is_dir());
    assert!(
        root.path()
            .join("renamed")
            .join("deep")
            .join("deep.jpg")
            .is_file()
    );
    let photos = list_photos(&database).unwrap();
    assert_eq!(photos.len(), 2);
    assert!(
        photos
            .iter()
            .any(|photo| photo.relative_path == "renamed/nested.jpg")
    );
    assert!(
        photos
            .iter()
            .any(|photo| photo.relative_path == "renamed/deep/deep.jpg")
    );
}

#[test]
fn rejects_renaming_photo_library_root() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();

    let error = rename_directory(&database, library.root_directory_id, "renamed").unwrap_err();

    assert!(error.to_string().contains("root cannot be renamed"));
}

#[test]
fn refresh_queues_a_photo_without_mapping_state() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("photo.jpg"), b"photo").unwrap();
    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
    refresh_directory(&database, library.root_directory_id).unwrap();
    let photo = list_photos(&database).unwrap().remove(0);
    database
        .connect()
        .unwrap()
        .execute(
            "DELETE FROM photo_mapping_queue WHERE photo_id = ?",
            [photo.photo_id],
        )
        .unwrap();

    let result = refresh_directory(&database, library.root_directory_id).unwrap();

    assert_eq!(result.unchanged, 1);
    assert_eq!(
        mapping::get_photo_mapping(&database, photo.photo_id)
            .unwrap()
            .status,
        mapping::PhotoTaxonStatus::Processing
    );
}

#[test]
fn directory_cursor_is_absent_on_the_last_page() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("a")).unwrap();
    fs::create_dir(root.path().join("b")).unwrap();
    fs::write(root.path().join("photo.jpg"), b"photo").unwrap();
    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
    refresh_directory(&database, library.root_directory_id).unwrap();

    let first = browse_directory(&database, library.root_directory_id, None, 2).unwrap();
    assert_eq!(first.items.len(), 2);
    assert!(
        first
            .items
            .iter()
            .all(|item| matches!(item, PhotoDirectoryItem::Directory { .. }))
    );
    let first_directory_id = match first.items[0] {
        PhotoDirectoryItem::Directory { ref directory } => directory.directory_id,
        PhotoDirectoryItem::Photo { .. } => unreachable!(),
    };
    let error = browse_directory(
        &database,
        first_directory_id,
        first.next_cursor.as_deref(),
        2,
    )
    .unwrap_err();
    assert!(error.to_string().contains("invalid photo cursor"));
    let second = browse_directory(
        &database,
        library.root_directory_id,
        first.next_cursor.as_deref(),
        2,
    )
    .unwrap();
    assert_eq!(second.items.len(), 1);
    assert!(matches!(second.items[0], PhotoDirectoryItem::Photo { .. }));
    assert_eq!(second.next_cursor, None);
}

#[test]
fn renames_the_real_file_and_updates_the_database() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("before.jpg"), b"photo").unwrap();
    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
    refresh_directory(&database, library.root_directory_id).unwrap();
    let photo = list_photos(&database).unwrap().remove(0);
    let renamed = rename_photo(&database, photo.photo_id, "after.jpg").unwrap();
    assert_eq!(renamed.filename, "after.jpg");
    assert_eq!(
        mapping::get_photo_mapping(&database, photo.photo_id)
            .unwrap()
            .status,
        mapping::PhotoTaxonStatus::Unmatched
    );
    assert!(!root.path().join("before.jpg").exists());
    assert!(root.path().join("after.jpg").is_file());
}

#[test]
fn records_and_reverts_photo_rename_operations() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("before.jpg"), b"photo").unwrap();
    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
    refresh_directory(&database, library.root_directory_id).unwrap();
    let photo = list_photos(&database).unwrap().remove(0);

    rename_photo(&database, photo.photo_id, "after.jpg").unwrap();

    let operations = list_operations(&database, None, 10).unwrap().items;
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0].source, "manual_rename");
    assert_eq!(operations[0].total_items, 1);
    assert_eq!(operations[0].succeeded_items, 1);
    let audit = list_operation_audit(&database, operations[0].operation_id, None, 10).unwrap();
    assert_eq!(audit.items.len(), 1);
    let photo_id = photo.photo_id.to_string();
    assert_eq!(audit.items[0].entity_id.as_deref(), Some(photo_id.as_str()));
    assert_eq!(audit.items[0].action, "rename");

    rollback_operation(&database, operations[0].operation_id).unwrap();

    assert!(root.path().join("before.jpg").is_file());
    assert!(!root.path().join("after.jpg").exists());
    assert_eq!(
        get_photo(&database, photo.photo_id)
            .unwrap()
            .unwrap()
            .filename,
        "before.jpg"
    );
    assert!(
        list_operations(&database, None, 10)
            .unwrap()
            .items
            .is_empty()
    );
    let error = rollback_operation(&database, operations[0].operation_id).unwrap_err();
    assert!(error.to_string().contains("not found"));
}

#[test]
fn revert_requires_the_current_renamed_filename() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("first.jpg"), b"photo").unwrap();
    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
    refresh_directory(&database, library.root_directory_id).unwrap();
    let photo = list_photos(&database).unwrap().remove(0);
    rename_photo(&database, photo.photo_id, "second.jpg").unwrap();
    let first_operation_id = list_operations(&database, None, 10).unwrap().items[0].operation_id;
    rename_photo(&database, photo.photo_id, "third.jpg").unwrap();
    let second_operation_id = list_operations(&database, None, 10).unwrap().items[0].operation_id;
    let newest_operation = list_operations(&database, None, 1).unwrap();
    assert_eq!(newest_operation.items[0].operation_id, second_operation_id);
    assert!(newest_operation.next_cursor.is_some());
    let older_operation =
        list_operations(&database, newest_operation.next_cursor.as_deref(), 1).unwrap();
    assert_eq!(older_operation.items[0].operation_id, first_operation_id);
    assert_eq!(older_operation.next_cursor, None);
    let error = rollback_operation(&database, first_operation_id).unwrap_err();
    assert!(error.to_string().contains("expected 'second.jpg'"));
    assert!(root.path().join("third.jpg").is_file());

    rollback_operation(&database, second_operation_id).unwrap();
    assert!(root.path().join("second.jpg").is_file());
    rollback_operation(&database, first_operation_id).unwrap();
    assert!(root.path().join("first.jpg").is_file());
}

#[test]
fn groups_taxon_renames_as_one_operation() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("first.jpg"), b"first").unwrap();
    fs::write(root.path().join("second.png"), b"second").unwrap();
    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
    refresh_directory(&database, library.root_directory_id).unwrap();
    let photos = list_photos(&database).unwrap();
    let connection = database.connect().unwrap();
    connection
        .execute("INSERT INTO taxa (rank) VALUES (5)", [])
        .unwrap();
    let taxon_id = connection.last_insert_rowid();
    connection
        .execute(
            r#"
                INSERT INTO taxon_names (taxon_id, name_type, name)
                VALUES (?, 1, 'Canis lupus')
                "#,
            [taxon_id],
        )
        .unwrap();
    for photo in &photos {
        connection
            .execute(
                r#"
                    INSERT INTO photo_taxon_mapping (photo_id, taxon_id, status)
                    VALUES (?, ?, 'matched')
                    ON CONFLICT(photo_id) DO UPDATE
                    SET taxon_id = excluded.taxon_id, status = 'matched'
                    "#,
                params![photo.photo_id, taxon_id],
            )
            .unwrap();
        connection
            .execute(
                "DELETE FROM photo_mapping_queue WHERE photo_id = ?",
                [photo.photo_id],
            )
            .unwrap();
    }
    connection
        .execute(
            r#"
                INSERT INTO photo_taxon_usage (
                    taxon_id, direct_photo_count, subtree_photo_count
                ) VALUES (?, ?, ?)
                "#,
            params![taxon_id, photos.len() as i64, photos.len() as i64],
        )
        .unwrap();
    drop(connection);
    let photo_ids = photos
        .iter()
        .map(|photo| photo.photo_id)
        .collect::<Vec<_>>();

    let renamed = rename_photos_from_taxa(&database, &photo_ids).unwrap();

    let mut renamed_filenames = renamed
        .rows
        .iter()
        .map(|row| row.photo.as_ref().unwrap().filename.as_str())
        .collect::<Vec<_>>();
    renamed_filenames.sort_unstable();
    assert_eq!(renamed_filenames, ["Canis lupus.jpg", "Canis lupus.png"]);
    assert!(
        renamed
            .rows
            .iter()
            .all(|row| row.status == PhotoRenameRowStatus::Applied)
    );
    let operation = list_operations(&database, None, 10)
        .unwrap()
        .items
        .remove(0);
    assert_eq!(renamed.operation_id, Some(operation.operation_id));
    assert_eq!(operation.source, "taxon_selection_rename");
    assert_eq!(operation.succeeded_items, 2);
    let audit = list_operation_audit(&database, operation.operation_id, None, 10).unwrap();
    assert_eq!(audit.items.len(), 2);
    assert_eq!(audit.items[0].sequence, 1);
    assert_eq!(audit.items[1].sequence, 2);
    crate::general::update_general_settings(
        &database,
        &crate::general::GeneralSettings {
            csv_delimiter: "\t".into(),
            ..crate::general::GeneralSettings::default()
        },
    )
    .unwrap();
    let mut output = Vec::new();
    write_operation_audit(&database, operation.operation_id, &mut output).unwrap();
    let exported = String::from_utf8(output).unwrap();
    assert_eq!(exported.lines().count(), 3);
    assert!(exported.starts_with("operation_id\tsequence\t"));

    rollback_operation(&database, operation.operation_id).unwrap();
    assert!(root.path().join("first.jpg").is_file());
    assert!(root.path().join("second.png").is_file());
}

#[test]
fn directory_taxon_rename_selects_only_current_mappings() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("mapped.jpg"), b"mapped").unwrap();
    fs::write(root.path().join("processing.png"), b"processing").unwrap();
    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
    refresh_directory(&database, library.root_directory_id).unwrap();
    let mut photos = list_photos(&database).unwrap();
    photos.sort_by(|left, right| left.filename.cmp(&right.filename));
    let mapped_photo = &photos[0];
    let connection = database.connect().unwrap();
    connection
        .execute("INSERT INTO taxa (rank) VALUES (5)", [])
        .unwrap();
    let taxon_id = connection.last_insert_rowid();
    connection
        .execute(
            r#"
                INSERT INTO taxon_names (taxon_id, name_type, name)
                VALUES (?, 1, 'Canis lupus')
                "#,
            [taxon_id],
        )
        .unwrap();
    connection
        .execute(
            r#"
                INSERT INTO photo_taxon_mapping (photo_id, taxon_id, status)
                VALUES (?, ?, 'matched')
                "#,
            params![mapped_photo.photo_id, taxon_id],
        )
        .unwrap();
    connection
        .execute(
            "DELETE FROM photo_mapping_queue WHERE photo_id = ?",
            [mapped_photo.photo_id],
        )
        .unwrap();
    drop(connection);

    let result =
        rename_photos_in_directory_from_taxa(&database, library.root_directory_id, false).unwrap();

    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].photo_id, mapped_photo.photo_id);
    assert_eq!(result.rows[0].status, PhotoRenameRowStatus::Applied);
    assert!(root.path().join("Canis lupus.jpg").is_file());
    assert!(root.path().join("processing.png").is_file());
}

#[test]
fn continues_taxon_selection_rename_after_a_row_fails() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("first.jpg"), b"first").unwrap();
    fs::write(root.path().join("second.jpg"), b"second").unwrap();
    fs::write(root.path().join("third.png"), b"third").unwrap();
    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
    refresh_directory(&database, library.root_directory_id).unwrap();
    let mut photos = list_photos(&database).unwrap();
    photos.sort_by(|left, right| left.filename.cmp(&right.filename));
    let connection = database.connect().unwrap();
    connection
        .execute("INSERT INTO taxa (rank) VALUES (5)", [])
        .unwrap();
    let taxon_id = connection.last_insert_rowid();
    connection
        .execute(
            r#"
                INSERT INTO taxon_names (taxon_id, name_type, name)
                VALUES (?, 1, 'Canis lupus')
                "#,
            [taxon_id],
        )
        .unwrap();
    for photo in &photos {
        connection
            .execute(
                r#"
                    INSERT INTO photo_taxon_mapping (photo_id, taxon_id, status)
                    VALUES (?, ?, 'matched')
                    ON CONFLICT(photo_id) DO UPDATE
                    SET taxon_id = excluded.taxon_id, status = 'matched'
                    "#,
                params![photo.photo_id, taxon_id],
            )
            .unwrap();
        connection
            .execute(
                "DELETE FROM photo_mapping_queue WHERE photo_id = ?",
                [photo.photo_id],
            )
            .unwrap();
    }
    drop(connection);
    let photo_ids = photos
        .iter()
        .map(|photo| photo.photo_id)
        .collect::<Vec<_>>();

    let result = rename_photos_from_taxa(&database, &photo_ids).unwrap();

    assert!(result.operation_id.is_some());
    assert_eq!(result.rows.len(), 3);
    assert_eq!(result.rows[0].status, PhotoRenameRowStatus::Applied);
    assert!(result.rows[0].operation_id.is_some());
    assert_eq!(result.rows[1].status, PhotoRenameRowStatus::Failed);
    assert_eq!(result.rows[1].operation_id, result.operation_id);
    assert!(
        result.rows[1]
            .message
            .contains("rename destination already exists")
    );
    assert_eq!(result.rows[2].status, PhotoRenameRowStatus::Applied);
    assert!(result.rows[2].operation_id.is_some());
    let operation =
        list_operation_audit(&database, result.operation_id.unwrap(), None, 10).unwrap();
    assert_eq!(
        operation
            .items
            .iter()
            .map(|item| item.sequence)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert!(!operation.items[1].succeeded);
    assert!(root.path().join("Canis lupus.jpg").is_file());
    assert!(root.path().join("second.jpg").is_file());
    assert!(root.path().join("Canis lupus.png").is_file());
}

#[test]
fn records_an_operation_when_every_rename_row_fails() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("unmapped.jpg"), b"photo").unwrap();
    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
    refresh_directory(&database, library.root_directory_id).unwrap();
    let photo = list_photos(&database).unwrap().remove(0);

    let result = rename_photos_from_taxa(&database, &[photo.photo_id]).unwrap();

    let operation_id = result.operation_id.unwrap();
    assert_eq!(result.rows[0].status, PhotoRenameRowStatus::Failed);
    assert_eq!(result.rows[0].operation_id, Some(operation_id));
    let summary = list_operations(&database, None, 1).unwrap().items.remove(0);
    assert_eq!(summary.operation_id, operation_id);
    assert_eq!(summary.total_items, 1);
    assert_eq!(summary.succeeded_items, 0);
    assert_eq!(summary.failed_items, 1);
    let audit = list_operation_audit(&database, operation_id, None, 10).unwrap();
    assert_eq!(audit.items.len(), 1);
    assert!(!audit.items[0].succeeded);
}

#[test]
fn renames_a_matched_photo_with_its_accepted_scientific_name() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("Canis lupus.JPG"), b"photo").unwrap();
    let database = Database::open_test(data.path().join("vividarium.db")).unwrap();
    let connection = database.connect().unwrap();
    connection
        .execute("INSERT INTO taxa (rank) VALUES (5)", [])
        .unwrap();
    let taxon_id = connection.last_insert_rowid();
    connection
        .execute(
            r#"
                INSERT INTO taxon_names (taxon_id, name_type, name)
                VALUES (?, 1, 'Canis lupus')
                "#,
            [taxon_id],
        )
        .unwrap();
    drop(connection);
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
    refresh_directory(&database, library.root_directory_id).unwrap();
    let photo = list_photos(&database).unwrap().remove(0);
    let mut progress = |_: OperationProgress| {};
    mapping::process_pending_photo_matches(&database, &mut progress).unwrap();
    mapping::set_photo_mapping(&database, photo.photo_id, taxon_id).unwrap();
    let mut taxonomy = database.connect_taxonomy().unwrap();
    let transaction = taxonomy.transaction().unwrap();
    crate::taxonomy::sync::record_event(&transaction, None, [taxon_id], false).unwrap();
    transaction.commit().unwrap();
    crate::taxonomy::sync::synchronize_pending_photo_libraries(&database).unwrap();
    let error = rename_photo_from_taxon(&database, photo.photo_id).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("must have a current matched taxon")
    );
    mapping::process_pending_photo_matches(&database, &mut progress).unwrap();

    let renamed = rename_photo_from_taxon(&database, photo.photo_id).unwrap();

    assert_eq!(renamed.filename, "Canis lupus.JPG");
    let filenames = fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(filenames, ["Canis lupus.JPG"]);
}

#[test]
fn restores_case_only_rename_when_the_database_update_fails() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("ABC.jpg"), b"photo").unwrap();
    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
    refresh_directory(&database, library.root_directory_id).unwrap();
    let photo = list_photos(&database).unwrap().remove(0);
    let connection = database.connect().unwrap();
    connection
        .execute_batch(
            r#"
                CREATE TRIGGER reject_photo_rename
                BEFORE UPDATE OF filename ON photos BEGIN
                    SELECT RAISE(ABORT, 'forced photo rename failure');
                END;
                "#,
        )
        .unwrap();

    let error = rename_photo(&database, photo.photo_id, "abc.jpg").unwrap_err();
    assert!(error.to_string().contains("forced photo rename failure"));
    assert_eq!(
        get_photo(&database, photo.photo_id)
            .unwrap()
            .unwrap()
            .filename,
        "ABC.jpg"
    );
    let filenames = fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    assert!(filenames.contains(&"ABC.jpg".to_string()));
    assert!(!filenames.contains(&"abc.jpg".to_string()));
    assert!(
        !root
            .path()
            .join(format!(".vividarium-rename-{}.tmp", photo.photo_id))
            .exists()
    );
}

#[test]
fn restores_case_only_revert_when_the_database_update_fails() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("ABC.jpg"), b"photo").unwrap();
    let database = Database::open(data.path().join("vividarium.db")).unwrap();
    let library = open_library(&database, root.path().to_str().unwrap()).unwrap();
    refresh_directory(&database, library.root_directory_id).unwrap();
    let photo = list_photos(&database).unwrap().remove(0);
    rename_photo(&database, photo.photo_id, "abc.jpg").unwrap();
    let operation_id = list_operations(&database, None, 1).unwrap().items[0].operation_id;
    let connection = database.connect().unwrap();
    connection
        .execute_batch(
            r#"
                CREATE TRIGGER reject_photo_revert
                BEFORE UPDATE OF filename ON photos
                WHEN new.filename = 'ABC.jpg' BEGIN
                    SELECT RAISE(ABORT, 'forced photo revert failure');
                END;
                "#,
        )
        .unwrap();

    let error = rollback_operation(&database, operation_id).unwrap_err();

    assert!(error.to_string().contains("forced photo revert failure"));
    assert_eq!(
        get_photo(&database, photo.photo_id)
            .unwrap()
            .unwrap()
            .filename,
        "abc.jpg"
    );
    let filenames = fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    assert!(filenames.contains(&"abc.jpg".to_string()));
    assert!(!filenames.contains(&"ABC.jpg".to_string()));
    assert!(
        !root
            .path()
            .join(format!(".vividarium-revert-{operation_id}.tmp"))
            .exists()
    );
}
