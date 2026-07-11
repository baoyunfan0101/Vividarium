use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use base64::Engine;
use chrono::{Local, NaiveDateTime};
use exif::{In, Reader as ExifReader, Tag, Value};
use regex::Regex;
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::db::{Database, photo_from_row};
use crate::error::{CoreError, CoreResult};
use crate::models::{DirectoryListingPage, NewPhoto, Photo, PhotoRootMetadata, PhotoSyncResult};

pub const STATUS_UNCHANGED: &str = "unchanged";
pub const STATUS_DELETED: &str = "deleted";
pub const STATUS_UPDATED: &str = "updated";
pub const STATUS_NEW: &str = "new";

const IMAGE_EXTENSIONS: &[&str] = &[
    "arw", "bmp", "cr2", "cr3", "dng", "gif", "heic", "jpeg", "jpg", "nef", "png", "raf", "rw2",
    "tif", "tiff", "webp",
];

pub type ProgressCallback<'a> = dyn FnMut(u64, Option<u64>, &str) + Send + 'a;

#[derive(Debug, Serialize, Deserialize)]
struct DirectoryCursor {
    section: String,
    name: Option<String>,
    filename: Option<String>,
    photo_id: Option<i64>,
}

#[derive(Debug, Default)]
struct FileMetadata {
    width: Option<i64>,
    height: Option<i64>,
    captured_at: Option<String>,
    camera: Option<String>,
    longitude: Option<f64>,
    latitude: Option<f64>,
    exif_json: Option<String>,
}

#[derive(Debug, Default, PartialEq)]
struct FilenameMetadata {
    binomial_name: Option<String>,
    shoot_date: Option<String>,
    location: Option<String>,
    device: Option<String>,
}

pub fn get_roots_metadata(database: &Database) -> CoreResult<Vec<PhotoRootMetadata>> {
    let connection = database.connect()?;
    let mut statement = connection.prepare(
        r#"
        SELECT
            photos_metadata.root,
            photos_metadata.last_synced_at,
            photos_metadata.sort_order,
            COALESCE(photo_counts.photo_count, 0) AS photo_count
        FROM photos_metadata
        LEFT JOIN (
            SELECT root, COUNT(*) AS photo_count FROM photos GROUP BY root
        ) AS photo_counts ON photo_counts.root = photos_metadata.root
        ORDER BY photos_metadata.sort_order, photos_metadata.root
        "#,
    )?;
    let rows = statement.query_map([], |row| {
        Ok(PhotoRootMetadata {
            root: row.get(0)?,
            last_synced_at: row.get(1)?,
            sort_order: row.get(2)?,
            photo_count: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn save_roots(database: &Database, roots: &[String]) -> CoreResult<Vec<PhotoRootMetadata>> {
    let roots = roots
        .iter()
        .map(|root| normalize_root(root))
        .collect::<CoreResult<Vec<_>>>()?;
    let mut connection = database.connect()?;
    let transaction = connection.transaction()?;
    let existing = {
        let mut statement =
            transaction.prepare("SELECT root, last_synced_at FROM photos_metadata")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?;
        rows.collect::<Result<BTreeMap<_, _>, _>>()?
    };
    transaction.execute("DELETE FROM photos_metadata", [])?;
    for (index, root) in roots.iter().enumerate() {
        transaction.execute(
            "INSERT INTO photos_metadata (root, last_synced_at, sort_order) VALUES (?, ?, ?)",
            params![root, existing.get(root).cloned().flatten(), index as i64],
        )?;
    }
    transaction.commit()?;
    get_roots_metadata(database)
}

pub fn get_photo(database: &Database, photo_id: i64) -> CoreResult<Option<Photo>> {
    let connection = database.connect()?;
    Ok(connection
        .query_row(
            "SELECT * FROM photos WHERE photo_id = ?",
            [photo_id],
            photo_from_row,
        )
        .optional()?)
}

pub fn list_photos(database: &Database) -> CoreResult<Vec<Photo>> {
    let connection = database.connect()?;
    let mut statement = connection.prepare("SELECT * FROM photos ORDER BY photo_id")?;
    let rows = statement.query_map([], photo_from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn list_changed_photos(database: &Database) -> CoreResult<Vec<Photo>> {
    let connection = database.connect()?;
    let mut statement =
        connection.prepare("SELECT * FROM photos WHERE status IN (?, ?) ORDER BY photo_id")?;
    let rows = statement.query_map(params![STATUS_UPDATED, STATUS_NEW], photo_from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn list_map_photos(
    database: &Database,
    bbox: Option<(f64, f64, f64, f64)>,
) -> CoreResult<Vec<Photo>> {
    let connection = database.connect()?;
    let mut statement = connection.prepare(
        r#"
        SELECT * FROM photos
        WHERE status != ? AND longitude IS NOT NULL AND latitude IS NOT NULL
        ORDER BY captured_at, photo_id
        "#,
    )?;
    let rows = statement.query_map([STATUS_DELETED], photo_from_row)?;
    let photos = rows.collect::<Result<Vec<_>, _>>()?;
    Ok(match bbox {
        None => photos,
        Some((min_lng, min_lat, max_lng, max_lat)) => photos
            .into_iter()
            .filter(|photo| {
                matches!((photo.longitude, photo.latitude), (Some(lng), Some(lat)) if
                    lng >= min_lng && lng <= max_lng && lat >= min_lat && lat <= max_lat)
            })
            .collect(),
    })
}

pub fn browse_photos_page(
    database: &Database,
    root: &str,
    relative_dir: &str,
    cursor: Option<&str>,
    limit: usize,
) -> CoreResult<DirectoryListingPage> {
    let root = normalize_root(root)?;
    let directory = normalize_relative_path(relative_dir);
    let limit = limit.clamp(1, 500);
    let cursor = decode_cursor(cursor)?;
    let mut section = cursor
        .as_ref()
        .map(|value| value.section.as_str())
        .unwrap_or("directories");
    let connection = database.connect()?;

    let directory_count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM photos_dir WHERE root = ? AND parent_dir = ?",
        params![root, directory],
        |row| row.get(0),
    )?;
    let file_count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM photos WHERE root = ? AND status != ? AND parent_dir = ?",
        params![root, STATUS_DELETED, directory],
        |row| row.get(0),
    )?;

    let mut directories = Vec::new();
    let mut files = Vec::new();
    let mut remaining = limit;
    if section == "directories" {
        let after_name = cursor.as_ref().and_then(|value| value.name.as_deref());
        let mut statement = if after_name.is_some() {
            connection.prepare(
                "SELECT name FROM photos_dir WHERE root = ? AND parent_dir = ? AND name > ? ORDER BY name LIMIT ?",
            )?
        } else {
            connection.prepare(
                "SELECT name FROM photos_dir WHERE root = ? AND parent_dir = ? ORDER BY name LIMIT ?",
            )?
        };
        directories = match after_name {
            Some(name) => statement
                .query_map(params![root, directory, name, remaining as i64], |row| {
                    row.get(0)
                })?
                .collect::<Result<Vec<String>, _>>()?,
            None => statement
                .query_map(params![root, directory, remaining as i64], |row| row.get(0))?
                .collect::<Result<Vec<String>, _>>()?,
        };
        remaining -= directories.len();
        if remaining == 0 {
            let next_cursor = directories
                .last()
                .map(|name| {
                    encode_cursor(&DirectoryCursor {
                        section: "directories".into(),
                        name: Some(name.clone()),
                        filename: None,
                        photo_id: None,
                    })
                })
                .transpose()?;
            return Ok(DirectoryListingPage {
                root,
                relative_dir: directory,
                directories,
                files,
                next_cursor,
                directory_count,
                file_count,
            });
        }
        section = "files";
    }

    if section == "files" && remaining > 0 {
        let after_filename = cursor.as_ref().and_then(|value| value.filename.as_deref());
        let after_photo_id = cursor.as_ref().and_then(|value| value.photo_id);
        let mut statement = if after_filename.is_some() && after_photo_id.is_some() {
            connection.prepare(
                r#"
                SELECT * FROM photos
                WHERE root = ? AND status != ? AND parent_dir = ?
                  AND (filename > ? OR (filename = ? AND photo_id > ?))
                ORDER BY filename, photo_id LIMIT ?
                "#,
            )?
        } else {
            connection.prepare(
                "SELECT * FROM photos WHERE root = ? AND status != ? AND parent_dir = ? ORDER BY filename, photo_id LIMIT ?",
            )?
        };
        let rows = match (after_filename, after_photo_id) {
            (Some(filename), Some(photo_id)) => statement.query_map(
                params![
                    root,
                    STATUS_DELETED,
                    directory,
                    filename,
                    filename,
                    photo_id,
                    remaining as i64
                ],
                photo_from_row,
            )?,
            _ => statement.query_map(
                params![root, STATUS_DELETED, directory, remaining as i64],
                photo_from_row,
            )?,
        };
        files = rows.collect::<Result<Vec<_>, _>>()?;
    }

    let next_cursor = if let Some(last) = files.last() {
        let exists: i64 = connection.query_row(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM photos
                WHERE root = ? AND status != ? AND parent_dir = ?
                  AND (filename > ? OR (filename = ? AND photo_id > ?))
            )
            "#,
            params![
                root,
                STATUS_DELETED,
                directory,
                last.filename,
                last.filename,
                last.photo_id
            ],
            |row| row.get(0),
        )?;
        if exists != 0 {
            Some(encode_cursor(&DirectoryCursor {
                section: "files".into(),
                name: None,
                filename: Some(last.filename.clone()),
                photo_id: Some(last.photo_id),
            })?)
        } else {
            None
        }
    } else {
        None
    };

    Ok(DirectoryListingPage {
        root,
        relative_dir: directory,
        directories,
        files,
        next_cursor,
        directory_count,
        file_count,
    })
}

pub fn update_photos(
    database: &Database,
    root: &str,
    progress: &mut ProgressCallback<'_>,
) -> CoreResult<PhotoSyncResult> {
    update_photos_inner(database, root, true, progress)
}

pub fn update_photos_many(
    database: &Database,
    roots: &[String],
    progress: &mut ProgressCallback<'_>,
) -> CoreResult<BTreeMap<String, PhotoSyncResult>> {
    if roots.is_empty() {
        return Err(CoreError::InvalidArgument(
            "at least one photo root is required".into(),
        ));
    }
    let connection = database.connect()?;
    connection.execute(
        "UPDATE photos SET status = ? WHERE status != ?",
        params![STATUS_UNCHANGED, STATUS_DELETED],
    )?;
    let mut results = BTreeMap::new();
    for root in roots {
        results.insert(
            root.clone(),
            update_photos_inner(database, root, false, progress)?,
        );
    }
    Ok(results)
}

fn update_photos_inner(
    database: &Database,
    root: &str,
    reset_other_roots: bool,
    progress: &mut ProgressCallback<'_>,
) -> CoreResult<PhotoSyncResult> {
    let root = normalize_root(root)?;
    let image_files = image_files(&root)?;
    let total = image_files.len() as u64;
    progress(0, Some(total), &format!("Updating {root}"));
    let mut connection = database.connect()?;
    let transaction = connection.transaction()?;
    let other_roots_unchanged = if reset_other_roots {
        transaction.execute(
            "UPDATE photos SET status = ? WHERE root != ? AND status != ?",
            params![STATUS_UNCHANGED, root, STATUS_DELETED],
        )?
    } else {
        0
    };
    let mut scanned_paths = HashSet::new();
    let mut unchanged = 0;
    let mut updated = 0;
    let mut inserted = 0;

    for (index, path) in image_files.iter().enumerate() {
        let relative_path = relative_path(&root, path)?;
        scanned_paths.insert(relative_path.clone());
        let metadata = fs::metadata(path)?;
        let file_size = metadata.len() as i64;
        let modified_at = modified_at(&metadata)?;
        let existing = transaction
            .query_row(
                "SELECT * FROM photos WHERE root = ? AND relative_path = ?",
                params![root, relative_path],
                photo_from_row,
            )
            .optional()?;
        match existing {
            None => {
                let record = build_record(&root, path, STATUS_NEW)?;
                insert_photo(&transaction, &record)?;
                inserted += 1;
            }
            Some(photo)
                if photo.file_size == Some(file_size) && photo.modified_at == Some(modified_at) =>
            {
                transaction.execute(
                    "UPDATE photos SET status = ? WHERE photo_id = ?",
                    params![STATUS_UNCHANGED, photo.photo_id],
                )?;
                unchanged += 1;
            }
            Some(photo) => {
                let record = build_record(&root, path, STATUS_UPDATED)?;
                update_photo(&transaction, photo.photo_id, &record)?;
                updated += 1;
            }
        }
        progress((index + 1) as u64, Some(total), &format!("Updating {root}"));
    }

    let existing_paths = {
        let mut statement = transaction.prepare(
            "SELECT photo_id, relative_path, status FROM photos WHERE root = ? ORDER BY relative_path",
        )?;
        let rows = statement.query_map([&root], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let mut deleted = 0;
    for (photo_id, path, status) in existing_paths {
        if !scanned_paths.contains(&path) && status != STATUS_DELETED {
            transaction.execute(
                "UPDATE photos SET status = ? WHERE photo_id = ?",
                params![STATUS_DELETED, photo_id],
            )?;
            deleted += 1;
        }
    }
    refresh_directories(&transaction, &root)?;
    upsert_root(&transaction, &root)?;
    transaction.commit()?;
    Ok(PhotoSyncResult {
        roots: None,
        inserted,
        unchanged,
        updated,
        new: inserted,
        deleted,
        other_roots_unchanged,
        thumbnails_cleared: 0,
    })
}

pub fn rebuild_photos(
    database: &Database,
    roots: &[String],
    thumbnail_root: &Path,
    progress: &mut ProgressCallback<'_>,
) -> CoreResult<PhotoSyncResult> {
    let roots = roots
        .iter()
        .map(|root| normalize_root(root))
        .collect::<CoreResult<Vec<_>>>()?;
    let mut root_files = Vec::new();
    for root in &roots {
        root_files.push((root.clone(), image_files(root)?));
    }
    let total = root_files
        .iter()
        .map(|(_, files)| files.len())
        .sum::<usize>();
    progress(0, Some(total as u64), "Scanning photos");
    let thumbnails_cleared = clear_thumbnail_cache(thumbnail_root)?;
    let mut connection = database.connect()?;
    let transaction = connection.transaction()?;
    transaction.execute("DELETE FROM photos", [])?;
    transaction.execute("DELETE FROM photos_dir", [])?;
    transaction.execute("DELETE FROM sqlite_sequence WHERE name = 'photos'", [])?;
    let mut inserted = 0;
    for (root, files) in root_files {
        for path in files {
            insert_photo(&transaction, &build_record(&root, &path, STATUS_NEW)?)?;
            inserted += 1;
            progress(
                inserted as u64,
                Some(total as u64),
                &format!("Rebuilding {root}"),
            );
        }
        refresh_directories(&transaction, &root)?;
        upsert_root(&transaction, &root)?;
    }
    transaction.commit()?;
    Ok(PhotoSyncResult {
        roots: Some(roots.len()),
        inserted,
        unchanged: 0,
        updated: 0,
        new: inserted,
        deleted: 0,
        other_roots_unchanged: 0,
        thumbnails_cleared,
    })
}

pub fn photo_file_path(database: &Database, photo_id: i64) -> CoreResult<PathBuf> {
    let photo = get_photo(database, photo_id)?
        .ok_or_else(|| CoreError::NotFound(format!("photo {photo_id}")))?;
    safe_photo_path(&photo)
}

pub fn get_or_create_thumbnail(
    database: &Database,
    photo_id: i64,
    thumbnail_root: &Path,
) -> CoreResult<PathBuf> {
    let photo = get_photo(database, photo_id)?
        .ok_or_else(|| CoreError::NotFound(format!("photo {photo_id}")))?;
    if let Some(existing) = &photo.thumbnail_path {
        let path = PathBuf::from(existing);
        if path.is_file() {
            return Ok(path);
        }
    }
    let source = safe_photo_path(&photo)?;
    fs::create_dir_all(thumbnail_root)?;
    let modified = photo.modified_at.unwrap_or_default() as i64;
    let file_size = photo.file_size.unwrap_or_default();
    let output = thumbnail_root.join(format!(
        "photo_{}_{}_{}.webp",
        photo.photo_id, modified, file_size
    ));
    let image = image::open(&source)?;
    let thumbnail = image.thumbnail(256, 256);
    thumbnail.save_with_format(&output, image::ImageFormat::WebP)?;
    let connection = database.connect()?;
    connection.execute(
        "UPDATE photos SET thumbnail_path = ? WHERE photo_id = ?",
        params![output.to_string_lossy(), photo_id],
    )?;
    Ok(output)
}

pub fn rebase_thumbnail_paths(database: &Database, thumbnail_root: &Path) -> CoreResult<usize> {
    let mut connection = database.connect()?;
    let transaction = connection.transaction()?;
    let paths = {
        let mut statement = transaction.prepare(
            "SELECT photo_id, thumbnail_path FROM photos WHERE thumbnail_path IS NOT NULL",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let mut updated = 0;
    for (photo_id, current) in paths {
        let Some(filename) = Path::new(&current).file_name() else {
            continue;
        };
        let candidate = thumbnail_root.join(filename);
        if candidate.is_file() && candidate.as_path() != Path::new(&current) {
            transaction.execute(
                "UPDATE photos SET thumbnail_path = ? WHERE photo_id = ?",
                params![candidate.to_string_lossy(), photo_id],
            )?;
            updated += 1;
        }
    }
    transaction.commit()?;
    Ok(updated)
}

fn normalize_root(root: &str) -> CoreResult<String> {
    let expanded = expand_home(root);
    Ok(expanded.canonicalize()?.to_string_lossy().into_owned())
}

fn expand_home(value: &str) -> PathBuf {
    if value == "~" {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(value));
    }
    if let Some(rest) = value.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(value)
}

fn normalize_relative_path(value: &str) -> String {
    value.replace('\\', "/").trim_matches('/').to_string()
}

fn image_files(root: &str) -> CoreResult<Vec<PathBuf>> {
    let mut paths = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .filter(|path| {
            path.extension()
                .and_then(|value| value.to_str())
                .is_some_and(|extension| {
                    IMAGE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
                })
        })
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

fn relative_path(root: &str, path: &Path) -> CoreResult<String> {
    Ok(path
        .strip_prefix(root)
        .map_err(|_| CoreError::UnsafePath(path.to_path_buf()))?
        .to_string_lossy()
        .replace('\\', "/"))
}

fn modified_at(metadata: &fs::Metadata) -> CoreResult<f64> {
    Ok(metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_err(|error| CoreError::InvalidArgument(error.to_string()))?
        .as_secs_f64())
}

fn build_record(root: &str, path: &Path, status: &str) -> CoreResult<NewPhoto> {
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| CoreError::InvalidArgument("photo filename is not valid UTF-8".into()))?
        .to_string();
    let relative_path = relative_path(root, path)?;
    let parent_dir = Path::new(&relative_path)
        .parent()
        .map(|value| value.to_string_lossy().replace('\\', "/"))
        .filter(|value| value != ".")
        .unwrap_or_default();
    let parsed = parse_filename(&filename);
    let metadata = read_file_metadata(path);
    let stat = fs::metadata(path)?;
    Ok(NewPhoto {
        root: root.into(),
        relative_path,
        path_depth: if parent_dir.is_empty() {
            0
        } else {
            parent_dir.split('/').count() as i64
        },
        parent_dir,
        filename,
        binomial_name: parsed.binomial_name,
        captured_at: metadata.captured_at.or(parsed.shoot_date),
        location: parsed.location,
        camera: metadata.camera.or(parsed.device),
        width: metadata.width,
        height: metadata.height,
        file_size: Some(stat.len() as i64),
        modified_at: Some(modified_at(&stat)?),
        longitude: metadata.longitude,
        latitude: metadata.latitude,
        exif_json: metadata.exif_json,
        thumbnail_path: None,
        status: status.into(),
    })
}

fn insert_photo(transaction: &Transaction<'_>, record: &NewPhoto) -> CoreResult<i64> {
    transaction.execute(
        r#"
        INSERT INTO photos (
            root, relative_path, parent_dir, path_depth, filename, binomial_name,
            captured_at, location, camera, width, height, file_size, modified_at,
            longitude, latitude, exif_json, thumbnail_path, status
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
        params![
            record.root,
            record.relative_path,
            record.parent_dir,
            record.path_depth,
            record.filename,
            record.binomial_name,
            record.captured_at,
            record.location,
            record.camera,
            record.width,
            record.height,
            record.file_size,
            record.modified_at,
            record.longitude,
            record.latitude,
            record.exif_json,
            record.thumbnail_path,
            record.status
        ],
    )?;
    Ok(transaction.last_insert_rowid())
}

fn update_photo(transaction: &Transaction<'_>, photo_id: i64, record: &NewPhoto) -> CoreResult<()> {
    transaction.execute(
        r#"
        UPDATE photos SET
            root = ?, relative_path = ?, parent_dir = ?, path_depth = ?, filename = ?,
            binomial_name = ?, captured_at = ?, location = ?, camera = ?, width = ?, height = ?,
            file_size = ?, modified_at = ?, longitude = ?, latitude = ?, exif_json = ?,
            thumbnail_path = ?, status = ?
        WHERE photo_id = ?
        "#,
        params![
            record.root,
            record.relative_path,
            record.parent_dir,
            record.path_depth,
            record.filename,
            record.binomial_name,
            record.captured_at,
            record.location,
            record.camera,
            record.width,
            record.height,
            record.file_size,
            record.modified_at,
            record.longitude,
            record.latitude,
            record.exif_json,
            record.thumbnail_path,
            record.status,
            photo_id
        ],
    )?;
    Ok(())
}

fn refresh_directories(transaction: &Transaction<'_>, root: &str) -> CoreResult<()> {
    let relative_paths = {
        let mut statement = transaction
            .prepare("SELECT relative_path FROM photos WHERE root = ? AND status != ?")?;
        let rows =
            statement.query_map(params![root, STATUS_DELETED], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let mut directories = BTreeMap::<String, (String, String, i64)>::new();
    for path in relative_paths {
        let parent = Path::new(&path)
            .parent()
            .map(|value| value.to_string_lossy().replace('\\', "/"));
        let Some(parent) = parent.filter(|value| !value.is_empty() && value != ".") else {
            continue;
        };
        let parts = parent.split('/').collect::<Vec<_>>();
        for index in 0..parts.len() {
            let relative_dir = parts[..=index].join("/");
            directories.insert(
                relative_dir,
                (
                    parts[..index].join("/"),
                    parts[index].into(),
                    (index + 1) as i64,
                ),
            );
        }
    }
    transaction.execute("DELETE FROM photos_dir WHERE root = ?", [root])?;
    for (relative_dir, (parent_dir, name, depth)) in directories {
        transaction.execute(
            "INSERT INTO photos_dir (root, relative_dir, parent_dir, name, path_depth) VALUES (?, ?, ?, ?, ?)",
            params![root, relative_dir, parent_dir, name, depth],
        )?;
    }
    Ok(())
}

fn upsert_root(transaction: &Transaction<'_>, root: &str) -> CoreResult<()> {
    let sort_order = transaction
        .query_row(
            "SELECT sort_order FROM photos_metadata WHERE root = ?",
            [root],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or(transaction.query_row(
            "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM photos_metadata",
            [],
            |row| row.get(0),
        )?);
    let now = Local::now().format("%Y-%m-%d %H:%M:%S%.6f").to_string();
    transaction.execute(
        r#"
        INSERT INTO photos_metadata (root, last_synced_at, sort_order) VALUES (?, ?, ?)
        ON CONFLICT(root) DO UPDATE SET last_synced_at = excluded.last_synced_at,
            sort_order = excluded.sort_order
        "#,
        params![root, now, sort_order],
    )?;
    Ok(())
}

fn safe_photo_path(photo: &Photo) -> CoreResult<PathBuf> {
    let root = PathBuf::from(&photo.root).canonicalize()?;
    let relative = Path::new(&photo.relative_path);
    if relative.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(CoreError::UnsafePath(relative.to_path_buf()));
    }
    let candidate = root.join(relative).canonicalize()?;
    if !candidate.starts_with(&root) {
        return Err(CoreError::UnsafePath(candidate));
    }
    Ok(candidate)
}

fn clear_thumbnail_cache(root: &Path) -> CoreResult<usize> {
    if !root.exists() {
        return Ok(0);
    }
    let count = WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .count();
    fs::remove_dir_all(root)?;
    Ok(count)
}

fn encode_cursor(cursor: &DirectoryCursor) -> CoreResult<String> {
    let value = serde_json::to_vec(cursor)
        .map_err(|error| CoreError::InvalidArgument(error.to_string()))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value))
}

fn decode_cursor(value: Option<&str>) -> CoreResult<Option<DirectoryCursor>> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| CoreError::InvalidArgument("invalid directory cursor".into()))?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| CoreError::InvalidArgument("invalid directory cursor".into()))
}

fn parse_filename(filename: &str) -> FilenameMetadata {
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(filename);
    let pattern =
        Regex::new(r"^(?P<binomial_name>.+?)(?P<date>\d{8})_(?P<time>\d{6})(?P<trailing>.*)$")
            .expect("valid filename pattern");
    let Some(captures) = pattern.captures(stem) else {
        return FilenameMetadata::default();
    };
    let trailing = captures
        .name("trailing")
        .map(|value| value.as_str().trim())
        .unwrap_or_default();
    let device_pattern = Regex::new(
        r"(?P<device>(?:iPhone|iPad|Pixel|Canon|Nikon|Sony|FUJIFILM|Fujifilm|HUAWEI|Huawei|Xiaomi|OPPO|vivo|Vivo)[A-Za-z0-9 _.-]*)$",
    )
    .expect("valid device pattern");
    let (location, device) = match device_pattern.captures(trailing) {
        None => (nonempty(trailing), None),
        Some(device_capture) => {
            let matched = device_capture.name("device").expect("device capture");
            (
                nonempty(trailing[..matched.start()].trim()),
                nonempty(matched.as_str().trim()),
            )
        }
    };
    let date = captures
        .name("date")
        .map(|value| value.as_str())
        .unwrap_or_default();
    let time = captures
        .name("time")
        .map(|value| value.as_str())
        .unwrap_or_default();
    let shoot_date = NaiveDateTime::parse_from_str(&format!("{date}_{time}"), "%Y%m%d_%H%M%S")
        .ok()
        .map(|value| value.format("%Y-%m-%d %H:%M:%S").to_string());
    FilenameMetadata {
        binomial_name: captures
            .name("binomial_name")
            .and_then(|value| nonempty(value.as_str().trim())),
        shoot_date,
        location,
        device,
    }
}

fn nonempty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}

fn read_file_metadata(path: &Path) -> FileMetadata {
    let dimensions = image::ImageReader::open(path)
        .ok()
        .and_then(|reader| reader.with_guessed_format().ok())
        .and_then(|reader| reader.into_dimensions().ok());
    let exif = File::open(path).ok().and_then(|file| {
        ExifReader::new()
            .read_from_container(&mut BufReader::new(file))
            .ok()
    });
    let mut result = FileMetadata {
        width: dimensions.map(|value| value.0 as i64),
        height: dimensions.map(|value| value.1 as i64),
        ..Default::default()
    };
    let Some(exif) = exif else {
        return result;
    };
    let mut values = BTreeMap::new();
    for field in exif.fields() {
        values.insert(
            format!("{}", field.tag),
            field.display_value().with_unit(&exif).to_string(),
        );
    }
    result.exif_json =
        (!values.is_empty()).then(|| serde_json::to_string(&values).unwrap_or_default());
    result.captured_at = [Tag::DateTimeOriginal, Tag::DateTimeDigitized, Tag::DateTime]
        .into_iter()
        .find_map(|tag| exif.get_field(tag, In::PRIMARY))
        .and_then(|field| parse_exif_datetime(&field.display_value().to_string()));
    let make = exif
        .get_field(Tag::Make, In::PRIMARY)
        .map(|field| clean_exif_text(&field.display_value().to_string()));
    let model = exif
        .get_field(Tag::Model, In::PRIMARY)
        .map(|field| clean_exif_text(&field.display_value().to_string()));
    result.camera = match (make, model) {
        (Some(make), Some(model)) if !model.contains(&make) => Some(format!("{make} {model}")),
        (_, Some(model)) => Some(model),
        (Some(make), None) => Some(make),
        _ => None,
    };
    result.latitude = gps_coordinate(
        exif.get_field(Tag::GPSLatitude, In::PRIMARY),
        exif.get_field(Tag::GPSLatitudeRef, In::PRIMARY),
    );
    result.longitude = gps_coordinate(
        exif.get_field(Tag::GPSLongitude, In::PRIMARY),
        exif.get_field(Tag::GPSLongitudeRef, In::PRIMARY),
    );
    result
}

fn clean_exif_text(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_end_matches('\0')
        .trim()
        .to_string()
}

fn parse_exif_datetime(value: &str) -> Option<String> {
    let value = clean_exif_text(value);
    ["%Y:%m:%d %H:%M:%S", "%Y-%m-%d %H:%M:%S"]
        .into_iter()
        .find_map(|format| NaiveDateTime::parse_from_str(&value, format).ok())
        .map(|date| date.format("%Y-%m-%d %H:%M:%S").to_string())
}

fn gps_coordinate(value: Option<&exif::Field>, reference: Option<&exif::Field>) -> Option<f64> {
    let Value::Rational(values) = &value?.value else {
        return None;
    };
    if values.len() < 3 {
        return None;
    }
    let mut coordinate =
        values[0].to_f64() + values[1].to_f64() / 60.0 + values[2].to_f64() / 3600.0;
    let direction = clean_exif_text(&reference?.display_value().to_string()).to_ascii_uppercase();
    if direction == "S" || direction == "W" {
        coordinate *= -1.0;
    }
    Some(coordinate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_filename_metadata() {
        let parsed = parse_filename("Lilium brownii20260504_123525GardenA iPhone12.jpg");
        assert_eq!(parsed.binomial_name.as_deref(), Some("Lilium brownii"));
        assert_eq!(parsed.shoot_date.as_deref(), Some("2026-05-04 12:35:25"));
        assert_eq!(parsed.location.as_deref(), Some("GardenA"));
        assert_eq!(parsed.device.as_deref(), Some("iPhone12"));
    }

    #[test]
    fn cursor_round_trip() {
        let cursor = DirectoryCursor {
            section: "files".into(),
            name: None,
            filename: Some("image.jpg".into()),
            photo_id: Some(10),
        };
        let encoded = encode_cursor(&cursor).unwrap();
        let decoded = decode_cursor(Some(&encoded)).unwrap().unwrap();
        assert_eq!(decoded.filename, cursor.filename);
        assert_eq!(decoded.photo_id, cursor.photo_id);
    }

    #[test]
    fn multi_root_update_preserves_changes_from_every_root() {
        let directory = tempfile::tempdir().unwrap();
        let root_a = directory.path().join("root-a");
        let root_b = directory.path().join("root-b");
        fs::create_dir_all(&root_a).unwrap();
        fs::create_dir_all(&root_b).unwrap();
        image::RgbImage::new(1, 1)
            .save(root_a.join("a.png"))
            .unwrap();
        image::RgbImage::new(1, 1)
            .save(root_b.join("b.png"))
            .unwrap();
        let database = Database::open(directory.path().join("phytoindex.db")).unwrap();
        let roots = vec![
            root_a.to_string_lossy().into_owned(),
            root_b.to_string_lossy().into_owned(),
        ];
        let mut progress = |_: u64, _: Option<u64>, _: &str| {};
        rebuild_photos(
            &database,
            &roots,
            &directory.path().join("thumbnails"),
            &mut progress,
        )
        .unwrap();

        image::RgbImage::new(2, 2)
            .save(root_a.join("a.png"))
            .unwrap();
        image::RgbImage::new(3, 3)
            .save(root_b.join("b.png"))
            .unwrap();
        update_photos_many(&database, &roots, &mut progress).unwrap();

        let changed = list_changed_photos(&database).unwrap();
        assert_eq!(changed.len(), 2);
        assert!(changed.iter().all(|photo| photo.status == STATUS_UPDATED));
    }
}
