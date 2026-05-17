# Photos Module

The `photos` module tracks all imported photo files across all configured photo
roots. It stores every photo in a single `photos` table and stores the known
root directories in `photos_metadata`.

## Files

- `db.py`: owns the SQLite schema and low-level photo/root operations.
- `scanner.py`: walks roots and extracts file metadata and EXIF data.
- `filename_parser.py`: optional filename metadata parser. A filename does not
  have to match its heuristic; unmatched fields are stored as `NULL`.
- `service.py`: exposes the public module API.
- `__init__.py`: re-exports the public service API.

## Public API

Import from `app.photos` for normal use:

```python
from app.photos import (
    update_photos,
    rebuild_photos,
    list_directory,
    list_photos,
    list_changed_photos,
    get_photo,
    get_roots,
    save_roots,
    get_latest_update,
    export_table_csv,
)
```

### `update_photos(root, db_path=DEFAULT_DB_PATH)`

Updates one root in place.

- Adds the root to `photos_metadata` if it has not been seen before.
- Uses `file_size` and `modified_at` to detect unchanged files.
- Marks unchanged files as `unchanged`.
- Re-reads metadata for changed files and marks them as `updated`.
- Inserts unseen files with a new `photo_id` and status `new`.
- Marks previously indexed files that no longer exist as `deleted`.

### `rebuild_photos(roots, db_path=DEFAULT_DB_PATH)`

Rebuilds photo rows for the specified roots.

- Clears existing `photos` rows only for the specified roots.
- Scans every image file under each root.
- Inserts every scanned image with status `new`.
- Updates `photos_metadata.last_synced_at` for each rebuilt root.
- `photo_id` values for rebuilt roots are recreated and may change.

### `list_directory(root, relative_dir="", db_path=DEFAULT_DB_PATH)`

Returns direct child directories and files under `root/relative_dir`.

Deleted rows are excluded from directory listings.

### `list_photos(db_path=DEFAULT_DB_PATH)`

Returns all rows from the `photos` table ordered by `photo_id`.

### `list_changed_photos(db_path=DEFAULT_DB_PATH)`

Returns rows whose status is `updated` or `new`, ordered by `photo_id`.

### `get_photo(photo_id, db_path=DEFAULT_DB_PATH)`

Returns one `photos` row by `photo_id`, or `None` when it does not exist.

### `get_roots(db_path=DEFAULT_DB_PATH)`

Returns all recorded photo roots.

### `save_roots(roots, db_path=DEFAULT_DB_PATH)`

Replaces the stored root list and root order without scanning files.

- Preserves `last_synced_at` for roots already present in `photos_metadata`.
- Stores new roots with `last_synced_at = NULL`.
- Removes deleted roots from `photos_metadata` only; existing `photos` rows are not removed by this metadata operation.

### `get_latest_update(db_path=DEFAULT_DB_PATH)`

Returns each root and the latest time when the internal `photos` database state
for that root was rebuilt or updated:

```python
[
    {
        "root": "...",
        "last_synced_at": "YYYY-MM-DD HH:MM:SS",
        "sort_order": 0,
    }
]
```

### `export_table_csv(table_name, output_path, db_path=DEFAULT_DB_PATH)`

Exports a photos module table to CSV. Valid table names:

- `photos`
- `photos_metadata`

Returns the number of exported data rows.

## Database Tables

### `photos_metadata`

Stores all known photo roots and the internal database update time for each
root.

| Column | Type | Meaning |
| --- | --- | --- |
| `root` | `TEXT PRIMARY KEY` | Absolute photo root path. |
| `last_synced_at` | `TEXT` | Local time when the `photos` rows for this root were last rebuilt or updated. |
| `sort_order` | `INTEGER NOT NULL` | Display order for root management UIs. |

### `photos`

Stores all photo metadata in one table.

| Column | Type | Meaning |
| --- | --- | --- |
| `photo_id` | `INTEGER PRIMARY KEY AUTOINCREMENT` | Photo id. Stable across updates, recreated by rebuilds. |
| `root` | `TEXT NOT NULL` | Absolute photo root path. |
| `relative_path` | `TEXT NOT NULL` | File path relative to `root`. |
| `filename` | `TEXT NOT NULL` | Base filename. |
| `binomial_name` | `TEXT` | Scientific name parsed from filename when available; aligned with `taxa.binomial_name`. |
| `captured_at` | `TEXT` | Capture time from EXIF or filename when available. |
| `location` | `TEXT` | Location parsed from filename when available. |
| `camera` | `TEXT` | Camera/device from EXIF or filename when available. |
| `width` | `INTEGER` | Image width in pixels. |
| `height` | `INTEGER` | Image height in pixels. |
| `file_size` | `INTEGER` | File size in bytes. |
| `modified_at` | `REAL` | Filesystem modification timestamp. |
| `longitude` | `REAL` | GPS longitude from EXIF when available. |
| `latitude` | `REAL` | GPS latitude from EXIF when available. |
| `exif_json` | `TEXT` | Other serialized EXIF fields as JSON. |
| `thumbnail_path` | `TEXT DEFAULT NULL` | Thumbnail path; currently defaults to `NULL`. |
| `status` | `TEXT NOT NULL` | One of `unchanged`, `deleted`, `updated`, `new`. |

The unique key is `(root, relative_path)`.

Indexes:

- `idx_photos_root_path` on `(root, relative_path)`
- `idx_photos_status` on `status`
- `idx_photos_binomial_name` on `binomial_name`

## Implementation Notes

The update path intentionally avoids re-reading EXIF for unchanged files. A file
is considered unchanged when both `file_size` and filesystem `modified_at` match
the previous database row.

The filename parser is a best-effort metadata source only. Photos are still
indexed when filenames do not match the parser's heuristic format.

The `last_synced_at` values are module-level update markers. Downstream
modules such as `photos_taxa_mapping` can compare them against `taxa` update
metadata to detect stale mappings.
