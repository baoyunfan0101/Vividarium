//! Photo-library browsing, indexing, media access, naming, and rename history.
//!
//! Photo-to-taxon behavior is exposed separately through [`crate::mapping`].

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::UNIX_EPOCH;

use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::{Database, ensure_photo_library_index_state, photo_from_row};
use crate::error::{CoreError, CoreResult};
use crate::mapping;
use crate::models::{
    DirectoryEntryCounts, NewPhoto, OperationProgress, OperationProgressUnit, Photo,
    PhotoDirectory, PhotoLibrary, PhotoSyncResult,
};

pub use crate::models::{PhotoDirectoryItem, PhotoPage};

mod media;
mod naming;
mod operations;
mod page;
mod search;

pub use media::{
    PhotoMetadataIndexResult, get_or_create_thumbnail, get_or_create_thumbnail_for_library,
    get_photo_metadata, has_pending_photo_metadata, index_photo_metadata_for_library,
    photo_directory_path, photo_file_path, photo_file_path_for_library, rebase_thumbnail_paths,
};
pub use naming::{
    PhotoFilenameFormatSettings, format_photo_filename, get_photo_filename_format_settings,
    set_photo_filename_format_settings,
};
use operations::{
    PhotoOperationSource, insert_photo_operation_item, record_operation_outcomes,
    start_photo_operation,
};
pub use operations::{
    list_operation_audit, list_operations, rollback_operation, write_all_operation_audit,
    write_operation_audit, write_operations_audit,
};
pub(crate) use page::{
    PhotoCursor, PhotoPageSection, decode_photo_cursor, encode_photo_cursor, invalid_photo_cursor,
    photo_page_limit,
};
pub(crate) use search::photo_search_relation;
pub use search::{search_photos, search_photos_by_filename};

const IMAGE_EXTENSIONS: &[&str] = &[
    "arw", "bmp", "cr2", "cr3", "dng", "gif", "heic", "jpeg", "jpg", "nef", "png", "raf", "rw2",
    "tif", "tiff", "webp",
];
static PHOTO_WRITE_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn lock_photo_workspace() -> CoreResult<MutexGuard<'static, ()>> {
    PHOTO_WRITE_LOCK
        .lock()
        .map_err(|_| CoreError::InvalidArgument("photo workspace lock is poisoned".into()))
}

pub type ProgressCallback<'a> = dyn FnMut(OperationProgress) + Send + 'a;

#[derive(Debug)]
struct ScannedDirectory {
    name: String,
}

#[derive(Debug)]
struct ScannedDirectoryContents {
    directories: Vec<ScannedDirectory>,
    photos: Vec<ScannedPhoto>,
}

#[derive(Debug, Clone)]
struct ScannedPhoto {
    filename: String,
    file_size: i64,
    modified_at_ns: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PhotoRenameRowStatus {
    Applied,
    NoChange,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PhotoRenameRowOutcome {
    pub row_number: usize,
    pub photo_id: i64,
    pub operation_id: Option<i64>,
    pub status: PhotoRenameRowStatus,
    pub message: String,
    pub photo: Option<Photo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PhotoRenameOperationResult {
    pub operation_id: Option<i64>,
    pub rows: Vec<PhotoRenameRowOutcome>,
}

struct PhotoRenameResult {
    photo: Photo,
    operation_id: Option<i64>,
}

#[derive(Default)]
struct DirectoryRefreshStats {
    inserted: usize,
    unchanged: usize,
    updated: usize,
    deleted: usize,
    directories_inserted: usize,
    directories_deleted: usize,
    changed_photo_ids: Vec<i64>,
}

impl DirectoryRefreshStats {
    fn merge(&mut self, mut other: Self) {
        self.inserted += other.inserted;
        self.unchanged += other.unchanged;
        self.updated += other.updated;
        self.deleted += other.deleted;
        self.directories_inserted += other.directories_inserted;
        self.directories_deleted += other.directories_deleted;
        self.changed_photo_ids.append(&mut other.changed_photo_ids);
    }
}

pub fn open_library(database: &Database, root: &str) -> CoreResult<PhotoLibrary> {
    let root = normalize_root(root)?;
    if let Some(existing) = database
        .list_photo_libraries()?
        .into_iter()
        .find(|library| library.root_path == root)
    {
        database.switch_photo_library(&existing.library_uuid)?;
    } else {
        let directory = PathBuf::from(database.locations()?.default_photo_library_directory);
        let path = directory.join(format!("photo-library-{}.db", Uuid::new_v4()));
        database.register_photo_library(Path::new(&root), &path, None)?;
    }
    get_library(database)?.ok_or_else(|| CoreError::NotFound("photo library".into()))
}

pub fn get_library(database: &Database) -> CoreResult<Option<PhotoLibrary>> {
    if database.active_photo_library()?.is_none() {
        return Ok(None);
    }
    let connection = database.connect()?;
    connection
        .query_row(
            r#"
            SELECT photo_library.root_path, root.directory_id
            FROM photo_library
            JOIN photo_directories AS root ON root.relative_path = ''
            WHERE photo_library.library_id = 1
            "#,
            [],
            |row| {
                Ok(PhotoLibrary {
                    root_path: row.get(0)?,
                    root_directory_id: row.get(1)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub fn is_initial_index_complete(database: &Database, library_uuid: &str) -> CoreResult<bool> {
    let library = database.photo_library(library_uuid)?;
    let connection = database.connect_photo_library_registration(&library)?;
    ensure_photo_library_index_state(&connection)?;
    connection
        .query_row(
            r#"
            SELECT initial_index_complete
            FROM photo_library_index_state
            WHERE state_id = 1
            "#,
            [],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

pub fn initial_index_photo_library(
    database: &Database,
    library_uuid: &str,
    progress: &mut ProgressCallback<'_>,
) -> CoreResult<Option<PhotoSyncResult>> {
    let library = database.photo_library(library_uuid)?;
    let connection = database.connect_photo_library_registration(&library)?;
    ensure_photo_library_index_state(&connection)?;
    let complete = connection.query_row(
        r#"
        SELECT initial_index_complete
        FROM photo_library_index_state
        WHERE state_id = 1
        "#,
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if complete {
        return Ok(None);
    }
    let root_directory_id = connection.query_row(
        "SELECT directory_id FROM photo_directories WHERE relative_path = ''",
        [],
        |row| row.get(0),
    )?;
    drop(connection);
    refresh_directory_for_library(database, &library, root_directory_id, true, progress).map(Some)
}

pub fn scan_photo_library(
    database: &Database,
    library_uuid: &str,
    progress: &mut ProgressCallback<'_>,
) -> CoreResult<PhotoSyncResult> {
    let library = database.photo_library(library_uuid)?;
    let connection = database.connect_photo_library_registration(&library)?;
    ensure_photo_library_index_state(&connection)?;
    let complete = connection.query_row(
        r#"
        SELECT initial_index_complete
        FROM photo_library_index_state
        WHERE state_id = 1
        "#,
        [],
        |row| row.get::<_, bool>(0),
    )?;
    let root_directory_id = connection.query_row(
        "SELECT directory_id FROM photo_directories WHERE relative_path = ''",
        [],
        |row| row.get(0),
    )?;
    drop(connection);
    refresh_directory_for_library(database, &library, root_directory_id, !complete, progress)
}

pub fn get_photo_count(database: &Database) -> CoreResult<i64> {
    if database.active_photo_library()?.is_none() {
        return Ok(0);
    }
    let connection = database.connect()?;
    Ok(connection.query_row("SELECT COUNT(*) FROM photos", [], |row| row.get(0))?)
}

pub fn get_directory_counts(
    database: &Database,
    directory_id: i64,
) -> CoreResult<DirectoryEntryCounts> {
    let connection = database.connect()?;
    if load_directory(&connection, directory_id)?.is_none() {
        return Err(CoreError::NotFound(format!(
            "photo directory {directory_id}"
        )));
    }
    Ok(DirectoryEntryCounts {
        directory_count: connection.query_row(
            "SELECT COUNT(*) FROM photo_directories WHERE parent_directory_id = ?",
            [directory_id],
            |row| row.get(0),
        )?,
        file_count: connection.query_row(
            "SELECT COUNT(*) FROM photos WHERE directory_id = ?",
            [directory_id],
            |row| row.get(0),
        )?,
    })
}

pub fn get_photo(database: &Database, photo_id: i64) -> CoreResult<Option<Photo>> {
    let connection = database.connect()?;
    connection
        .query_row(
            &photo_select("WHERE photos.photo_id = ?"),
            [photo_id],
            photo_from_row,
        )
        .optional()
        .map_err(Into::into)
}

#[cfg(test)]
pub(crate) fn list_photos(database: &Database) -> CoreResult<Vec<Photo>> {
    let connection = database.connect()?;
    let mut statement = connection.prepare(&photo_select("ORDER BY photos.photo_id"))?;
    let rows = statement.query_map([], photo_from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn browse_directory(
    database: &Database,
    directory_id: i64,
    cursor: Option<&str>,
    limit: usize,
) -> CoreResult<PhotoPage<PhotoDirectoryItem>> {
    let connection = database.connect()?;
    load_directory(&connection, directory_id)?
        .ok_or_else(|| CoreError::NotFound(format!("photo directory {directory_id}")))?;
    let (section, after_name, after_id) = match decode_photo_cursor(cursor)? {
        None => (PhotoPageSection::Containers, String::new(), 0),
        Some(PhotoCursor::DirectoryEntries {
            directory_id: cursor_directory_id,
            section,
            name,
            item_id,
        }) if cursor_directory_id == directory_id => (section, name, item_id),
        Some(_) => return Err(invalid_photo_cursor()),
    };
    let limit = photo_page_limit(limit);
    let mut directories = Vec::new();
    let mut remaining = limit;
    let mut has_more = false;

    if section == PhotoPageSection::Containers {
        let mut statement = connection.prepare(
            r#"
            SELECT directory_id, parent_directory_id, name, relative_path
            FROM photo_directories
            WHERE parent_directory_id = ?1
              AND (?2 = '' OR name > ?2 OR (name = ?2 AND directory_id > ?3))
            ORDER BY name, directory_id
            LIMIT ?4
            "#,
        )?;
        let rows = statement.query_map(
            params![
                directory_id,
                after_name.as_str(),
                after_id,
                remaining as i64 + 1
            ],
            directory_from_row,
        )?;
        directories = rows.collect::<Result<Vec<_>, _>>()?;
        if directories.len() > remaining {
            directories.pop();
            let next_cursor = directories
                .last()
                .map(|value| {
                    encode_photo_cursor(&PhotoCursor::DirectoryEntries {
                        directory_id,
                        section: PhotoPageSection::Containers,
                        name: value.name.clone(),
                        item_id: value.directory_id,
                    })
                })
                .transpose()?;
            return Ok(PhotoPage {
                items: directories
                    .into_iter()
                    .map(|directory| PhotoDirectoryItem::Directory { directory })
                    .collect(),
                next_cursor,
            });
        }
        remaining -= directories.len();
    }

    let (after_name, after_id) = if section == PhotoPageSection::Photos {
        (after_name.as_str(), after_id)
    } else {
        ("", 0)
    };
    let mut statement = connection.prepare(&photo_select(
        r#"
        WHERE photos.directory_id = ?1
          AND (?2 = '' OR photos.filename > ?2
               OR (photos.filename = ?2 AND photos.photo_id > ?3))
        ORDER BY photos.filename, photos.photo_id
        LIMIT ?4
        "#,
    ))?;
    let rows = statement.query_map(
        params![directory_id, after_name, after_id, remaining as i64 + 1],
        photo_from_row,
    )?;
    let mut files = rows.collect::<Result<Vec<_>, _>>()?;
    if files.len() > remaining {
        files.pop();
        has_more = true;
    }

    let next_cursor = if has_more {
        if let Some(value) = files.last() {
            Some(encode_photo_cursor(&PhotoCursor::DirectoryEntries {
                directory_id,
                section: PhotoPageSection::Photos,
                name: value.filename.clone(),
                item_id: value.photo_id,
            })?)
        } else {
            directories
                .last()
                .map(|value| {
                    encode_photo_cursor(&PhotoCursor::DirectoryEntries {
                        directory_id,
                        section: PhotoPageSection::Containers,
                        name: value.name.clone(),
                        item_id: value.directory_id,
                    })
                })
                .transpose()?
        }
    } else {
        None
    };
    let mut items = directories
        .into_iter()
        .map(|directory| PhotoDirectoryItem::Directory { directory })
        .collect::<Vec<_>>();
    items.extend(
        files
            .into_iter()
            .map(|photo| PhotoDirectoryItem::Photo { photo }),
    );
    Ok(PhotoPage { items, next_cursor })
}

pub fn refresh_directory(database: &Database, directory_id: i64) -> CoreResult<PhotoSyncResult> {
    let mut progress = |_: OperationProgress| {};
    refresh_directory_with_progress(database, directory_id, &mut progress)
}

pub fn refresh_directory_with_progress(
    database: &Database,
    directory_id: i64,
    progress: &mut ProgressCallback<'_>,
) -> CoreResult<PhotoSyncResult> {
    let library = database.active_photo_library()?.ok_or_else(|| {
        CoreError::InvalidArgument("no active photo library is registered".into())
    })?;
    refresh_directory_for_library(database, &library, directory_id, false, progress)
}

fn refresh_directory_for_library(
    database: &Database,
    library: &crate::models::PhotoLibraryRegistration,
    directory_id: i64,
    complete_initial_index: bool,
    progress: &mut ProgressCallback<'_>,
) -> CoreResult<PhotoSyncResult> {
    let connection = database.connect_photo_library_registration(library)?;
    ensure_photo_library_index_state(&connection)?;
    let directory = load_directory(&connection, directory_id)?
        .ok_or_else(|| CoreError::NotFound(format!("photo directory {directory_id}")))?;
    let root = library_root(&connection)?;
    let path = safe_directory_path(&root, &directory.relative_path)?;
    progress(OperationProgress {
        stage: "scanning_files".into(),
        current: None,
        total: None,
        unit: Some(OperationProgressUnit::Files),
    });
    let mut processed = 0;
    let mut stats = DirectoryRefreshStats::default();
    let mut pending = VecDeque::from([(directory, path)]);
    drop(connection);

    while let Some((directory, path)) = pending.pop_front() {
        let scanned = scan_directory(&path, &mut processed, progress)?;
        progress(OperationProgress {
            stage: "updating_photo_index".into(),
            current: None,
            total: None,
            unit: Some(OperationProgressUnit::Files),
        });
        let scanned_photo_names = scanned
            .photos
            .iter()
            .map(|value| value.filename.as_str())
            .collect::<HashSet<_>>();
        let _guard = lock_photo_workspace()?;
        let mut connection = database.connect_photo_library_registration(library)?;
        let transaction = connection.transaction()?;
        let mut directory_stats = DirectoryRefreshStats::default();
        let children = refresh_directory_structure(
            &transaction,
            &directory,
            scanned.directories,
            &scanned_photo_names,
            &mut directory_stats,
        )?;
        transaction.commit()?;
        drop(connection);
        drop(_guard);

        let connection = database.connect_photo_library_registration(library)?;
        let existing_photos = direct_photos(&connection, directory.directory_id)?;
        let photos_with_mapping_state =
            direct_photos_with_mapping_state(&connection, directory.directory_id)?;
        drop(connection);
        for photo_batch in scanned.photos.chunks(256) {
            let _guard = lock_photo_workspace()?;
            let mut connection = database.connect_photo_library_registration(library)?;
            let transaction = connection.transaction()?;
            let mut batch_stats = DirectoryRefreshStats::default();
            refresh_directory_photos(
                &transaction,
                directory.directory_id,
                photo_batch.to_vec(),
                &existing_photos,
                &photos_with_mapping_state,
                &mut batch_stats,
            )?;
            mapping::queue_photo_ids(&transaction, &batch_stats.changed_photo_ids, "refresh")?;
            transaction.commit()?;
            drop(connection);
            drop(_guard);
            batch_stats.changed_photo_ids.clear();
            directory_stats.merge(batch_stats);
            std::thread::yield_now();
        }
        for child in children {
            let child_path =
                safe_directory_path(&PathBuf::from(&library.root_path), &child.relative_path)?;
            pending.push_back((child, child_path));
        }
        stats.merge(directory_stats);
        std::thread::yield_now();
    }

    if complete_initial_index {
        let _guard = lock_photo_workspace()?;
        let connection = database.connect_photo_library_registration(library)?;
        connection.execute(
            r#"
            UPDATE photo_library_index_state
            SET initial_index_complete = 1,
                completed_at = CURRENT_TIMESTAMP
            WHERE state_id = 1
            "#,
            [],
        )?;
    }
    if complete_initial_index {
        progress(OperationProgress {
            stage: "photo_index_complete".into(),
            current: Some(processed),
            total: Some(processed),
            unit: Some(OperationProgressUnit::Files),
        });
    }
    Ok(PhotoSyncResult {
        directory_id,
        inserted: stats.inserted,
        unchanged: stats.unchanged,
        updated: stats.updated,
        deleted: stats.deleted,
        directories_inserted: stats.directories_inserted,
        directories_deleted: stats.directories_deleted,
    })
}

fn refresh_directory_structure(
    transaction: &Transaction<'_>,
    directory: &PhotoDirectory,
    scanned_directories: Vec<ScannedDirectory>,
    scanned_photo_names: &HashSet<&str>,
    stats: &mut DirectoryRefreshStats,
) -> CoreResult<Vec<PhotoDirectory>> {
    let existing_directories = direct_directories(transaction, directory.directory_id)?;
    let existing_photos = direct_photos(transaction, directory.directory_id)?;
    let scanned_directory_names = scanned_directories
        .iter()
        .map(|value| value.name.as_str())
        .collect::<HashSet<_>>();
    let removed_directory_ids = existing_directories
        .iter()
        .filter_map(|(name, value)| {
            (!scanned_directory_names.contains(name.as_str())).then_some(value.directory_id)
        })
        .collect::<Vec<_>>();
    let removed_photo_ids = existing_photos
        .iter()
        .filter_map(|(name, value)| {
            (!scanned_photo_names.contains(name.as_str())).then_some(value.photo_id)
        })
        .collect::<Vec<_>>();
    let (removed_directory_count, removed_subtree_photo_count) =
        removed_directory_subtree_stats(transaction, &removed_directory_ids)?;

    mapping::remove_directory_mappings(transaction, &removed_directory_ids)?;
    mapping::remove_photo_mappings(transaction, &removed_photo_ids)?;
    for id in &removed_directory_ids {
        transaction.execute("DELETE FROM photo_directories WHERE directory_id = ?", [id])?;
    }
    for id in &removed_photo_ids {
        transaction.execute("DELETE FROM photos WHERE photo_id = ?", [id])?;
    }
    stats.directories_deleted += removed_directory_count;
    stats.deleted += removed_subtree_photo_count + removed_photo_ids.len();

    let mut children = Vec::new();
    for entry in scanned_directories {
        let ScannedDirectory { name } = entry;
        let child = if let Some(existing) = existing_directories.get(&name) {
            existing.clone()
        } else {
            let relative_path = join_relative_path(&directory.relative_path, &name);
            transaction.execute(
                "INSERT INTO photo_directories (parent_directory_id, name, relative_path) VALUES (?, ?, ?)",
                params![directory.directory_id, &name, &relative_path],
            )?;
            stats.directories_inserted += 1;
            PhotoDirectory {
                directory_id: transaction.last_insert_rowid(),
                parent_directory_id: Some(directory.directory_id),
                name,
                relative_path,
            }
        };
        children.push(child);
    }

    Ok(children)
}

fn refresh_directory_photos(
    transaction: &Transaction<'_>,
    directory_id: i64,
    scanned_photos: Vec<ScannedPhoto>,
    existing_photos: &HashMap<String, Photo>,
    photos_with_mapping_state: &HashSet<i64>,
    stats: &mut DirectoryRefreshStats,
) -> CoreResult<()> {
    for entry in scanned_photos {
        match existing_photos.get(&entry.filename) {
            None => {
                let photo_id = insert_photo(
                    transaction,
                    &NewPhoto {
                        directory_id,
                        filename: entry.filename,
                        file_size: entry.file_size,
                        modified_at_ns: entry.modified_at_ns,
                        thumbnail_path: None,
                    },
                )?;
                stats.changed_photo_ids.push(photo_id);
                stats.inserted += 1;
            }
            Some(photo)
                if photo.file_size == entry.file_size
                    && photo.modified_at_ns == entry.modified_at_ns =>
            {
                if !photos_with_mapping_state.contains(&photo.photo_id) {
                    stats.changed_photo_ids.push(photo.photo_id);
                }
                stats.unchanged += 1;
            }
            Some(photo) => {
                transaction.execute(
                    r#"
                    UPDATE photos
                    SET file_size = ?, modified_at_ns = ?, thumbnail_path = NULL
                    WHERE photo_id = ?
                    "#,
                    params![entry.file_size, entry.modified_at_ns, photo.photo_id],
                )?;
                transaction.execute(
                    "DELETE FROM photo_metadata WHERE photo_id = ?",
                    [photo.photo_id],
                )?;
                stats.changed_photo_ids.push(photo.photo_id);
                stats.updated += 1;
            }
        }
    }
    Ok(())
}

fn removed_directory_subtree_stats(
    transaction: &Transaction<'_>,
    directory_ids: &[i64],
) -> CoreResult<(usize, usize)> {
    if directory_ids.is_empty() {
        return Ok((0, 0));
    }
    let mut deleted_directory_ids = HashSet::new();
    let mut deleted_photo_ids = HashSet::new();
    for directory_id in directory_ids {
        let mut statement = transaction.prepare(
            r#"
            WITH RECURSIVE descendants(directory_id) AS (
                SELECT directory_id FROM photo_directories WHERE directory_id = ?
                UNION ALL
                SELECT child.directory_id
                FROM photo_directories AS child
                JOIN descendants ON child.parent_directory_id = descendants.directory_id
            )
            SELECT directory_id FROM descendants
            "#,
        )?;
        let rows = statement.query_map([directory_id], |row| row.get::<_, i64>(0))?;
        for id in rows.collect::<Result<Vec<_>, _>>()? {
            deleted_directory_ids.insert(id);
        }
        let mut statement = transaction.prepare(
            r#"
            WITH RECURSIVE descendants(directory_id) AS (
                SELECT directory_id FROM photo_directories WHERE directory_id = ?
                UNION ALL
                SELECT child.directory_id
                FROM photo_directories AS child
                JOIN descendants ON child.parent_directory_id = descendants.directory_id
            )
            SELECT photos.photo_id
            FROM photos
            JOIN descendants USING (directory_id)
            "#,
        )?;
        let rows = statement.query_map([directory_id], |row| row.get::<_, i64>(0))?;
        for id in rows.collect::<Result<Vec<_>, _>>()? {
            deleted_photo_ids.insert(id);
        }
    }
    Ok((deleted_directory_ids.len(), deleted_photo_ids.len()))
}

pub fn rename_photo(database: &Database, photo_id: i64, new_filename: &str) -> CoreResult<Photo> {
    let _guard = PHOTO_WRITE_LOCK
        .lock()
        .map_err(|_| CoreError::InvalidArgument("photo workspace lock is poisoned".into()))?;
    let operation_id = start_photo_operation(database, PhotoOperationSource::Manual, 1)?;
    finish_single_photo_rename(
        database,
        operation_id,
        photo_id,
        rename_photo_locked(database, photo_id, new_filename, 1, operation_id),
    )
}

fn finish_single_photo_rename(
    database: &Database,
    operation_id: i64,
    photo_id: i64,
    result: CoreResult<PhotoRenameResult>,
) -> CoreResult<Photo> {
    match result {
        Ok(result) => {
            if result.operation_id.is_none() {
                record_operation_outcomes(
                    database,
                    Some(operation_id),
                    &[(1, photo_id, "no_change", true, "no change".into())],
                )?;
            }
            Ok(result.photo)
        }
        Err(error) => {
            let audit_result = record_operation_outcomes(
                database,
                Some(operation_id),
                &[(1, photo_id, "rename", false, error.to_string())],
            );
            match audit_result {
                Ok(()) => Err(error),
                Err(audit_error) => Err(CoreError::Consistency(format!(
                    "photo rename failed: {error}; audit update failed: {audit_error}"
                ))),
            }
        }
    }
}

pub fn rename_photo_from_taxon(database: &Database, photo_id: i64) -> CoreResult<Photo> {
    let _guard = PHOTO_WRITE_LOCK
        .lock()
        .map_err(|_| CoreError::InvalidArgument("photo workspace lock is poisoned".into()))?;
    let operation_id = start_photo_operation(database, PhotoOperationSource::Taxon, 1)?;
    let result = taxon_filename(database, photo_id).and_then(|new_filename| {
        rename_photo_locked(database, photo_id, &new_filename, 1, operation_id)
    });
    finish_single_photo_rename(database, operation_id, photo_id, result)
}

pub fn rename_directory(
    database: &Database,
    directory_id: i64,
    new_name: &str,
) -> CoreResult<PhotoDirectory> {
    let _guard = PHOTO_WRITE_LOCK
        .lock()
        .map_err(|_| CoreError::InvalidArgument("photo workspace lock is poisoned".into()))?;
    let new_name = validate_directory_name(new_name)?;
    let connection = database.connect()?;
    let root = library_root(&connection)?;
    let directory = load_directory(&connection, directory_id)?
        .ok_or_else(|| CoreError::NotFound(format!("photo directory {directory_id}")))?;
    let parent_id = directory
        .parent_directory_id
        .ok_or_else(|| CoreError::InvalidArgument("photo library root cannot be renamed".into()))?;
    if directory.name == new_name {
        return Ok(directory);
    }
    let parent = load_directory(&connection, parent_id)?
        .ok_or_else(|| CoreError::NotFound(format!("photo directory {parent_id}")))?;
    let parent_path = safe_directory_path(&root, &parent.relative_path)?;
    let source = safe_directory_path(&root, &directory.relative_path)?;
    let destination = parent_path.join(&new_name);
    let temporary = parent_path.join(format!(".vividarium-rename-directory-{directory_id}.tmp"));
    let new_relative_path = join_relative_path(&parent.relative_path, &new_name);
    rename_file(&source, &destination, &temporary)?;

    let result = (|| -> CoreResult<PhotoDirectory> {
        let mut connection = database.connect()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            r#"
            UPDATE photo_directories
            SET name = ?, relative_path = ?
            WHERE directory_id = ?
            "#,
            params![new_name, new_relative_path, directory_id],
        )?;
        let old_prefix = format!("{}/", directory.relative_path);
        let new_prefix = format!("{}/", new_relative_path);
        let descendants = transaction
            .prepare(
                r#"
                WITH RECURSIVE descendants(directory_id, relative_path) AS (
                    SELECT directory_id, relative_path
                    FROM photo_directories
                    WHERE parent_directory_id = ?1
                    UNION ALL
                    SELECT child.directory_id, child.relative_path
                    FROM photo_directories AS child
                    JOIN descendants
                      ON child.parent_directory_id = descendants.directory_id
                )
                SELECT directory_id, relative_path FROM descendants
                "#,
            )?
            .query_map([directory_id], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (descendant_id, relative_path) in descendants {
            let suffix = relative_path.strip_prefix(&old_prefix).ok_or_else(|| {
                CoreError::Consistency(format!(
                    "photo directory {} is not under {}",
                    descendant_id, directory.relative_path
                ))
            })?;
            transaction.execute(
                "UPDATE photo_directories SET relative_path = ? WHERE directory_id = ?",
                params![format!("{new_prefix}{suffix}"), descendant_id],
            )?;
        }
        let updated = load_directory(&transaction, directory_id)?
            .ok_or_else(|| CoreError::NotFound(format!("photo directory {directory_id}")))?;
        transaction.commit()?;
        Ok(updated)
    })();

    match result {
        Ok(directory) => Ok(directory),
        Err(error) => match rename_file(&destination, &source, &temporary) {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(CoreError::Consistency(format!(
                "photo directory database update failed: {error}; filesystem rollback failed: {rollback_error}"
            ))),
        },
    }
}

pub fn rename_photos_from_taxa(
    database: &Database,
    photo_ids: &[i64],
) -> CoreResult<PhotoRenameOperationResult> {
    let mut progress = |_: OperationProgress| {};
    rename_photos_from_taxa_with_progress(database, photo_ids, &mut progress)
}

pub fn rename_photos_from_taxa_with_progress(
    database: &Database,
    photo_ids: &[i64],
    progress: &mut ProgressCallback<'_>,
) -> CoreResult<PhotoRenameOperationResult> {
    let _guard = PHOTO_WRITE_LOCK
        .lock()
        .map_err(|_| CoreError::InvalidArgument("photo workspace lock is poisoned".into()))?;
    let total = photo_ids.len() as u64;
    progress(OperationProgress {
        stage: "renaming_photos".into(),
        current: Some(0),
        total: Some(total),
        unit: Some(OperationProgressUnit::Photos),
    });
    if photo_ids.is_empty() {
        return Ok(PhotoRenameOperationResult {
            operation_id: None,
            rows: Vec::new(),
        });
    }
    let operation_id = start_photo_operation(
        database,
        PhotoOperationSource::TaxonSelection,
        photo_ids.len(),
    )?;
    let mut rows = Vec::with_capacity(photo_ids.len());
    for (index, photo_id) in photo_ids.iter().copied().enumerate() {
        let row_number = index + 1;
        let result = taxon_filename(database, photo_id).and_then(|new_filename| {
            rename_photo_locked(database, photo_id, &new_filename, row_number, operation_id)
        });
        rows.push(match result {
            Ok(result) => {
                let status = if result.operation_id.is_some() {
                    PhotoRenameRowStatus::Applied
                } else {
                    PhotoRenameRowStatus::NoChange
                };
                PhotoRenameRowOutcome {
                    row_number,
                    photo_id,
                    operation_id: Some(operation_id),
                    status,
                    message: match status {
                        PhotoRenameRowStatus::Applied => "applied".into(),
                        PhotoRenameRowStatus::NoChange => "no change".into(),
                        PhotoRenameRowStatus::Failed => unreachable!(),
                    },
                    photo: Some(result.photo),
                }
            }
            Err(error) if is_photo_rename_row_error(&error) => PhotoRenameRowOutcome {
                row_number,
                photo_id,
                operation_id: Some(operation_id),
                status: PhotoRenameRowStatus::Failed,
                message: error.to_string(),
                photo: None,
            },
            Err(error) => return Err(error),
        });
        progress(OperationProgress {
            stage: "renaming_photos".into(),
            current: Some(row_number as u64),
            total: Some(total),
            unit: Some(OperationProgressUnit::Photos),
        });
    }
    let audit_rows = rows
        .iter()
        .filter_map(|row| match row.status {
            PhotoRenameRowStatus::Applied => None,
            PhotoRenameRowStatus::NoChange => Some((
                row.row_number,
                row.photo_id,
                "no_change",
                true,
                row.message.clone(),
            )),
            PhotoRenameRowStatus::Failed => Some((
                row.row_number,
                row.photo_id,
                "rename",
                false,
                row.message.clone(),
            )),
        })
        .collect::<Vec<_>>();
    record_operation_outcomes(database, Some(operation_id), &audit_rows)?;
    Ok(PhotoRenameOperationResult {
        operation_id: Some(operation_id),
        rows,
    })
}

pub fn rename_photos_in_directory_from_taxa(
    database: &Database,
    directory_id: i64,
    include_descendants: bool,
) -> CoreResult<PhotoRenameOperationResult> {
    let mut progress = |_: OperationProgress| {};
    rename_photos_in_directory_from_taxa_with_progress(
        database,
        directory_id,
        include_descendants,
        &mut progress,
    )
}

pub fn rename_photos_in_directory_from_taxa_with_progress(
    database: &Database,
    directory_id: i64,
    include_descendants: bool,
    progress: &mut ProgressCallback<'_>,
) -> CoreResult<PhotoRenameOperationResult> {
    let connection = database.connect()?;
    if load_directory(&connection, directory_id)?.is_none() {
        return Err(CoreError::NotFound(format!(
            "photo directory {directory_id}"
        )));
    }
    let sql = if include_descendants {
        r#"
        WITH RECURSIVE directories(directory_id) AS (
            SELECT ?1
            UNION ALL
            SELECT photo_directories.directory_id
            FROM photo_directories
            JOIN directories
              ON photo_directories.parent_directory_id = directories.directory_id
        )
        SELECT photos.photo_id
        FROM photos
        JOIN directories USING (directory_id)
        JOIN current_photo_taxon_mapping USING (photo_id)
        ORDER BY photos.directory_id, photos.filename, photos.photo_id
        "#
    } else {
        r#"
        SELECT photos.photo_id
        FROM photos
        JOIN current_photo_taxon_mapping USING (photo_id)
        WHERE photos.directory_id = ?1
        ORDER BY photos.filename, photos.photo_id
        "#
    };
    let photo_ids = connection
        .prepare(sql)?
        .query_map([directory_id], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(connection);
    rename_photos_from_taxa_with_progress(database, &photo_ids, progress)
}

fn is_photo_rename_row_error(error: &CoreError) -> bool {
    matches!(
        error,
        CoreError::Io(_)
            | CoreError::InvalidArgument(_)
            | CoreError::UnsafePath(_)
            | CoreError::NotFound(_)
    )
}

fn rename_photo_locked(
    database: &Database,
    photo_id: i64,
    new_filename: &str,
    row_number: usize,
    operation_id: i64,
) -> CoreResult<PhotoRenameResult> {
    let new_filename = validate_filename(new_filename)?;
    let old_photo = get_photo(database, photo_id)?
        .ok_or_else(|| CoreError::NotFound(format!("photo {photo_id}")))?;
    if old_photo.filename == new_filename {
        return Ok(PhotoRenameResult {
            photo: old_photo,
            operation_id: None,
        });
    }
    let connection = database.connect()?;
    let root = library_root(&connection)?;
    let directory = load_directory(&connection, old_photo.directory_id)?.ok_or_else(|| {
        CoreError::NotFound(format!("photo directory {}", old_photo.directory_id))
    })?;
    let directory_path = safe_directory_path(&root, &directory.relative_path)?;
    let source = directory_path.join(&old_photo.filename);
    let destination = directory_path.join(&new_filename);
    let temporary = directory_path.join(format!(".vividarium-rename-{photo_id}.tmp"));
    rename_file(&source, &destination, &temporary)?;

    let result = (|| -> CoreResult<Photo> {
        let mut connection = database.connect()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE photos SET filename = ? WHERE photo_id = ?",
            params![new_filename, photo_id],
        )?;
        mapping::remap_photo_ids(&transaction, &[photo_id])?;
        transaction.execute(
            "DELETE FROM photo_mapping_queue WHERE photo_id = ?",
            [photo_id],
        )?;
        insert_photo_operation_item(
            &transaction,
            operation_id,
            row_number,
            photo_id,
            &directory.relative_path,
            &old_photo.filename,
            &new_filename,
        )?;
        let photo = transaction
            .query_row(
                &photo_select("WHERE photos.photo_id = ?"),
                [photo_id],
                photo_from_row,
            )
            .optional()?
            .ok_or_else(|| CoreError::NotFound(format!("photo {photo_id}")))?;
        transaction.commit()?;
        Ok(photo)
    })();
    match result {
        Ok(photo) => Ok(PhotoRenameResult {
            photo,
            operation_id: Some(operation_id),
        }),
        Err(error) => match rename_file(&destination, &source, &temporary) {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(CoreError::Consistency(format!(
                "photo database update failed: {error}; filesystem rollback failed: {rollback_error}"
            ))),
        },
    }
}

fn taxon_filename(database: &Database, photo_id: i64) -> CoreResult<String> {
    let photo = get_photo(database, photo_id)?
        .ok_or_else(|| CoreError::NotFound(format!("photo {photo_id}")))?;
    let connection = database.connect()?;
    let taxon_id = connection
        .query_row(
            r#"
            SELECT taxon_id
            FROM current_photo_taxon_mapping
            WHERE photo_id = ?1
            "#,
            [photo_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .ok_or_else(|| {
            CoreError::InvalidArgument(format!(
                "photo {photo_id} must have a current matched taxon"
            ))
        })?;
    naming::filename_for_taxon(&connection, taxon_id, &photo.filename)
}

fn rename_file(source: &Path, destination: &Path, temporary: &Path) -> CoreResult<()> {
    let destination_is_source = destination.exists()
        && matches!(
            (source.canonicalize(), destination.canonicalize()),
            (Ok(source), Ok(destination)) if source == destination
        );
    if destination.exists() && !destination_is_source {
        return Err(CoreError::InvalidArgument(format!(
            "rename destination already exists: {}",
            destination.display()
        )));
    }
    let case_only_rename = destination_is_source
        || source.parent() == destination.parent()
            && source
                .file_name()
                .and_then(|value| value.to_str())
                .zip(destination.file_name().and_then(|value| value.to_str()))
                .is_some_and(|(left, right)| left != right && left.eq_ignore_ascii_case(right));
    if !case_only_rename {
        fs::rename(source, destination)?;
        return Ok(());
    }
    if temporary.exists() {
        return Err(CoreError::InvalidArgument(format!(
            "temporary rename path already exists: {}",
            temporary.display()
        )));
    }
    fs::rename(source, temporary)?;
    if let Err(error) = fs::rename(temporary, destination) {
        return match fs::rename(temporary, source) {
            Ok(()) => Err(error.into()),
            Err(restore_error) => Err(CoreError::Consistency(format!(
                "rename failed: {error}; source restoration failed: {restore_error}"
            ))),
        };
    }
    Ok(())
}

pub(crate) fn photo_select(suffix: &str) -> String {
    photo_select_with("", suffix)
}

pub(crate) fn photo_select_with(extra_columns: &str, suffix: &str) -> String {
    format!(
        r#"
        SELECT photos.photo_id, photos.directory_id,
               CASE WHEN photo_directories.relative_path = '' THEN photos.filename
                    ELSE photo_directories.relative_path || '/' || photos.filename END AS relative_path,
               photos.filename, photos.file_size, photos.modified_at_ns, photos.thumbnail_path
               {extra_columns}
        FROM photos
        JOIN photo_directories ON photo_directories.directory_id = photos.directory_id
        {suffix}
        "#
    )
}

fn load_directory(
    connection: &rusqlite::Connection,
    directory_id: i64,
) -> CoreResult<Option<PhotoDirectory>> {
    connection
        .query_row(
            r#"
            SELECT directory_id, parent_directory_id, name, relative_path
            FROM photo_directories WHERE directory_id = ?
            "#,
            [directory_id],
            directory_from_row,
        )
        .optional()
        .map_err(Into::into)
}

fn directory_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PhotoDirectory> {
    Ok(PhotoDirectory {
        directory_id: row.get(0)?,
        parent_directory_id: row.get(1)?,
        name: row.get(2)?,
        relative_path: row.get(3)?,
    })
}

fn direct_directories(
    transaction: &Transaction<'_>,
    directory_id: i64,
) -> CoreResult<HashMap<String, PhotoDirectory>> {
    let mut statement = transaction.prepare(
        r#"
        SELECT directory_id, parent_directory_id, name, relative_path
        FROM photo_directories WHERE parent_directory_id = ?
        "#,
    )?;
    let rows = statement.query_map([directory_id], directory_from_row)?;
    Ok(rows
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|value| (value.name.clone(), value))
        .collect())
}

fn direct_photos(
    connection: &rusqlite::Connection,
    directory_id: i64,
) -> CoreResult<HashMap<String, Photo>> {
    let mut statement = connection.prepare(&photo_select("WHERE photos.directory_id = ?"))?;
    let rows = statement.query_map([directory_id], photo_from_row)?;
    Ok(rows
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|value| (value.filename.clone(), value))
        .collect())
}

fn direct_photos_with_mapping_state(
    connection: &rusqlite::Connection,
    directory_id: i64,
) -> CoreResult<HashSet<i64>> {
    let mut statement = connection.prepare(
        r#"
        SELECT photos.photo_id
        FROM photos
        LEFT JOIN photo_taxon_mapping USING (photo_id)
        LEFT JOIN photo_mapping_queue USING (photo_id)
        WHERE photos.directory_id = ?
          AND (
              photo_taxon_mapping.photo_id IS NOT NULL
              OR photo_mapping_queue.photo_id IS NOT NULL
          )
        "#,
    )?;
    let rows = statement.query_map([directory_id], |row| row.get::<_, i64>(0))?;
    Ok(rows.collect::<Result<HashSet<_>, _>>()?)
}

fn insert_photo(transaction: &Transaction<'_>, photo: &NewPhoto) -> CoreResult<i64> {
    transaction.execute(
        r#"
        INSERT INTO photos (directory_id, filename, file_size, modified_at_ns, thumbnail_path)
        VALUES (?, ?, ?, ?, ?)
        "#,
        params![
            photo.directory_id,
            photo.filename,
            photo.file_size,
            photo.modified_at_ns,
            photo.thumbnail_path,
        ],
    )?;
    Ok(transaction.last_insert_rowid())
}

fn scan_directory(
    path: &Path,
    processed: &mut u64,
    progress: &mut ProgressCallback<'_>,
) -> CoreResult<ScannedDirectoryContents> {
    let mut directories = Vec::new();
    let mut photos = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| CoreError::InvalidArgument("photo path is not valid UTF-8".into()))?;
        if file_type.is_dir() {
            directories.push(ScannedDirectory { name });
        } else if file_type.is_file() && is_image_filename(&name) {
            let metadata = entry.metadata()?;
            photos.push(ScannedPhoto {
                filename: name,
                file_size: i64::try_from(metadata.len()).map_err(|_| {
                    CoreError::InvalidArgument("photo file size exceeds i64".into())
                })?,
                modified_at_ns: modified_at_ns(&metadata)?,
            });
        }
        *processed += 1;
        progress(OperationProgress {
            stage: "scanning_files".into(),
            current: None,
            total: None,
            unit: Some(OperationProgressUnit::Files),
        });
    }
    Ok(ScannedDirectoryContents {
        directories,
        photos,
    })
}

fn normalize_root(root: &str) -> CoreResult<String> {
    let path = PathBuf::from(root);
    let canonical = path.canonicalize()?;
    if !canonical.is_dir() {
        return Err(CoreError::InvalidArgument(format!(
            "photo root is not a directory: {}",
            canonical.display()
        )));
    }
    canonical
        .to_str()
        .map(str::to_string)
        .ok_or_else(|| CoreError::InvalidArgument("photo root is not valid UTF-8".into()))
}

fn library_root(connection: &rusqlite::Connection) -> CoreResult<PathBuf> {
    let value = connection
        .query_row(
            "SELECT root_path FROM photo_library WHERE library_id = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| CoreError::NotFound("photo library".into()))?;
    Ok(PathBuf::from(value).canonicalize()?)
}

fn safe_directory_path(root: &Path, relative_path: &str) -> CoreResult<PathBuf> {
    let relative = Path::new(relative_path);
    if relative.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(CoreError::UnsafePath(relative.into()));
    }
    safe_file_path(root, &root.join(relative))
}

fn safe_file_path(root: &Path, candidate: &Path) -> CoreResult<PathBuf> {
    let candidate = candidate.canonicalize()?;
    if !candidate.starts_with(root) {
        return Err(CoreError::UnsafePath(candidate));
    }
    Ok(candidate)
}

fn validate_filename(value: &str) -> CoreResult<String> {
    let value = value.trim();
    let path = Path::new(value);
    if value.is_empty()
        || path.components().count() != 1
        || matches!(value, "." | "..")
        || value.contains(['/', '\\'])
        || !is_image_filename(value)
    {
        return Err(CoreError::InvalidArgument(
            "photo filename must be one image filename without a path".into(),
        ));
    }
    Ok(value.into())
}

fn validate_directory_name(value: &str) -> CoreResult<String> {
    let value = value.trim();
    let path = Path::new(value);
    if value.is_empty()
        || path.components().count() != 1
        || matches!(value, "." | "..")
        || value.contains(['/', '\\'])
    {
        return Err(CoreError::InvalidArgument(
            "photo directory name must be one folder name without a path".into(),
        ));
    }
    Ok(value.into())
}

fn is_image_filename(filename: &str) -> bool {
    Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            IMAGE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
        })
}

fn join_relative_path(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.into()
    } else {
        format!("{parent}/{name}")
    }
}

fn modified_at_ns(metadata: &fs::Metadata) -> CoreResult<i64> {
    let value = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_err(|error| CoreError::InvalidArgument(error.to_string()))?
        .as_nanos();
    i64::try_from(value)
        .map_err(|_| CoreError::InvalidArgument("photo modified time exceeds i64".into()))
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
