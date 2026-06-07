# Photos

Owns photo roots, photo metadata, directory browsing, and lazy thumbnail generation.

## Files

- `db.py`: SQLite schema and row operations.
- `scanner.py`: image discovery and EXIF/GPS extraction.
- `filename_parser.py`: best-effort filename parser for binomial name, date, location, and device.
- `service.py`: public module API.

## Public API

Import from `app.photos`.

- `update_photos(root, db_path=DEFAULT_DB_PATH, progress=None)`: update one root. Uses `file_size` and `modified_at` to skip unchanged files, marks deleted files, refreshes `photos_dir`, and marks other roots' non-deleted photos as `unchanged`.
- `rebuild_photos(roots, db_path=DEFAULT_DB_PATH, thumbnail_root="data/thumbnails", progress=None)`: clear `photos` and `photos_dir`, reset photo ids, clear thumbnail cache, rescan all roots, insert all photos as `new`.
- `list_directory(root, relative_dir="", db_path=DEFAULT_DB_PATH)`: return direct child directories and non-deleted photos.
- `list_directory_page(root, relative_dir="", cursor=None, limit=100, db_path=DEFAULT_DB_PATH)`: return one keyset-cursor page of direct child directories and non-deleted photos.
- `list_photos(db_path=DEFAULT_DB_PATH)`: return all photo rows.
- `list_changed_photos(db_path=DEFAULT_DB_PATH)`: return photos with status `new` or `updated`.
- `get_photo(photo_id, db_path=DEFAULT_DB_PATH)`: return one row or `None`.
- `get_or_create_thumbnail(photo_id, db_path=DEFAULT_DB_PATH, thumbnail_root="data/thumbnails", size=(256, 256))`: create/reuse a WebP thumbnail and store `thumbnail_path`.
- `get_roots(db_path=DEFAULT_DB_PATH)`: return configured roots.
- `save_roots(roots, db_path=DEFAULT_DB_PATH)`: replace root metadata and sort order without scanning.
- `get_latest_update(db_path=DEFAULT_DB_PATH)`: return `photos_metadata`.
- `export_table_csv(table_name, output_path, db_path=DEFAULT_DB_PATH)`: export `photos`, `photos_dir`, or `photos_metadata`.

## Tables

### `photos_metadata`

| Column | Meaning |
| --- | --- |
| `root` | absolute photo root, primary key |
| `last_synced_at` | last update/rebuild time for this root |
| `sort_order` | root display order |

### `photos`

| Column | Meaning |
| --- | --- |
| `photo_id` | autoincrement id; stable on update, reset on rebuild |
| `root` | absolute root |
| `relative_path` | path relative to root |
| `parent_dir` | parent directory relative to root |
| `path_depth` | depth of `parent_dir` |
| `filename` | base filename |
| `binomial_name` | parsed scientific name |
| `captured_at` | EXIF or filename capture time |
| `location` | parsed location |
| `camera` | EXIF or filename camera/device |
| `width`, `height` | pixel dimensions |
| `file_size` | bytes |
| `modified_at` | filesystem modified timestamp |
| `longitude`, `latitude` | EXIF GPS |
| `exif_json` | serialized remaining EXIF |
| `thumbnail_path` | lazy thumbnail path |
| `status` | `unchanged`, `deleted`, `updated`, or `new` |

Unique key: `(root, relative_path)`.

### `photos_dir`

Derived directory index for fast direct-child browsing.

| Column | Meaning |
| --- | --- |
| `root` | absolute root |
| `relative_dir` | directory relative to root |
| `parent_dir` | parent directory |
| `name` | directory basename |
| `path_depth` | directory depth |

Primary key: `(root, relative_dir)`.

## Indexes

| Index | Columns | Used for |
| --- | --- | --- |
| `idx_photos_root_path` | `root, relative_path` | exact path lookup and uniqueness-adjacent queries |
| `idx_photos_browse` | `root, parent_dir, status, filename` | folder browsing ordered by filename |
| `idx_photos_browse_cursor` | `root, parent_dir, status, filename, photo_id` | keyset cursor paging for folder files |
| `idx_photos_status` | `status` | changed/new/deleted filtering |
| `idx_photos_binomial_name` | `binomial_name` | mapping and name lookup helpers |
| `idx_photos_dir_unique` | `root, relative_dir` | directory identity |
| `idx_photos_dir_browse` | `root, parent_dir, name` | direct child directory browsing |

## Cursor Browsing

`list_directory_page` returns folders first, then files, matching the UI order. The cursor encodes the last returned directory name or file `(filename, photo_id)` key, so the next page uses keyset predicates instead of `OFFSET`.

## Notes

- Rebuild deletes thumbnail files under `data/thumbnails`.
- Thumbnails are generated as compressed WebP files only when requested.
- Image URLs should include a version based on `root`, `relative_path`, `modified_at`, and `file_size`; versioned image responses are cacheable.
- Development stores data in repository `data/`; packaged builds store data in the user's application data directory.
