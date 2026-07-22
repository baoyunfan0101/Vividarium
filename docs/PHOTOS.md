# Photos Backend API

This document describes the public backend surface for the single-root photo
library. The implementation is split between `phytoindex_core::photos` for
file and directory operations and `phytoindex_core::mapping` for taxonomy
matching and taxonomy-based browsing.

The backend indexes one open photo root at a time. Paths stored on photo and
directory records are relative to that root. The root absolute path is stored
once in `photo_library`; it is not repeated on every photo row.

All Rust functions return `CoreResult<T>`. Serialized struct fields and enum
values use `snake_case`.

## Storage behavior

The v3 photos layout uses these application tables:

| Table | Purpose |
| --- | --- |
| `photo_library` | Single current root path. |
| `photo_directories` | Indexed directory hierarchy, including the root node. |
| `photos` | Narrow file facts used by directory browsing and change detection. |
| `photo_metadata` | Lazily extracted EXIF and image dimensions. |
| `photo_taxon_mapping` | Current mapping status and selected taxon. |
| `photo_taxon_usage` | Sparse direct and subtree photo counts for taxonomy browsing. |

The schema remains at development schema version 2, but the legacy photos
layout is not accepted. Open a new database when moving from the old layout.

## Photos models

### `PhotoLibrary`

| Field | Type | Description |
| --- | --- | --- |
| `root_path` | `String` | Canonical absolute path of the open photo root. |
| `root_directory_id` | `i64` | ID of the root `PhotoDirectory` node. |
| `photo_count` | `i64` | Number of currently indexed photos. |

### `PhotoDirectory`

| Field | Type | Description |
| --- | --- | --- |
| `directory_id` | `i64` | Stable database ID for this indexed directory. |
| `parent_directory_id` | `Option<i64>` | Parent ID, or `null` for the root node. |
| `name` | `String` | Immediate directory name. The root uses an empty name. |
| `relative_path` | `String` | Slash-separated path relative to the open root. The root uses an empty path. |

### `Photo`

| Field | Type | Description |
| --- | --- | --- |
| `photo_id` | `i64` | Stable database ID. |
| `directory_id` | `i64` | Directory containing the file. |
| `relative_path` | `String` | Complete file path relative to the open root. |
| `filename` | `String` | Real filesystem filename. |
| `file_size` | `i64` | File size in bytes. |
| `modified_at_ns` | `i64` | Filesystem modification time in nanoseconds since the Unix epoch. Used only for change detection. |
| `thumbnail_path` | `Option<String>` | Cached thumbnail path when one has been generated. |

`modified_at_ns` is a filesystem fact rather than an application update time.
The backend does not store scan, parse, or mapping timestamps.

### `PhotoMetadata`

Contains `photo_id`, `captured_at`, `camera`, `width`, `height`, `longitude`,
`latitude`, and `exif_json`. It is loaded from `photo_metadata` when present and
extracted from the real file on first request when absent. Directory refresh
does not read EXIF for new or unchanged files.

### `DirectoryListingPage`

| Field | Type | Description |
| --- | --- | --- |
| `directory` | `PhotoDirectory` | The directory being listed. |
| `directories` | `Vec<PhotoDirectory>` | Child directories in this page. |
| `files` | `Vec<Photo>` | Photos in this page. |
| `next_cursor` | `Option<String>` | Opaque cursor for the next page. |
| `directory_count` | `i64` | Total immediate child-directory count. |
| `file_count` | `i64` | Total immediate photo count. |

Directories are returned before files. Limits are clamped to `1..=500`.

### `PhotoSyncResult`

Returns `directory_id`, `inserted`, `unchanged`, `updated`, `deleted`,
`directories_inserted`, and `directories_deleted` for one directory refresh.

## Photos Rust API

### `open_library`

```rust
pub fn open_library(database: &Database, root: &str) -> CoreResult<PhotoLibrary>
```

Canonicalizes and opens one directory as the current photo root. Opening a
different root clears the old photo index and mapping data but preserves the
taxonomy knowledge base. The call creates the root directory node; use
`refresh_directory` to index its immediate contents.

### `get_library`

```rust
pub fn get_library(database: &Database) -> CoreResult<Option<PhotoLibrary>>
```

Returns the open library or `null` when no photo root has been opened.

### `browse_directory`

```rust
pub fn browse_directory(
    database: &Database,
    directory_id: i64,
    cursor: Option<&str>,
    limit: usize,
) -> CoreResult<DirectoryListingPage>
```

Reads only the SQLite index. It does not touch the filesystem or extract
metadata. A cursor is valid only for the same directory listing.

### `refresh_directory`

```rust
pub fn refresh_directory(
    database: &Database,
    directory_id: i64,
) -> CoreResult<PhotoSyncResult>
```

Reads only the immediate filesystem entries of `directory_id`. It does not
recursively scan child directories. Existing rows are compared in memory by
filename, `file_size`, and `modified_at_ns`.

Unchanged files cause no database or mapping writes. New, changed, and removed
files update `photos`, `photo_taxon_mapping`, and `photo_taxon_usage` in one
database transaction. Removed directories delete their cached descendants.

### `get_photo` and `list_photos`

```rust
pub fn get_photo(database: &Database, photo_id: i64) -> CoreResult<Option<Photo>>
pub fn list_photos(database: &Database) -> CoreResult<Vec<Photo>>
```

`get_photo` treats absence as a normal result. `list_photos` is intended for
administrative or rebuild work; interactive views should use paginated
directory or taxon browsing.

### `rename_photo`

```rust
pub fn rename_photo(
    database: &Database,
    photo_id: i64,
    new_filename: &str,
) -> CoreResult<Photo>
```

Renames the real file in its current directory and then updates the database
filename and taxonomy mapping. `new_filename` must be one supported image
filename without directory components. Existing destinations are rejected.
If the database update fails, the backend attempts to restore the old
filesystem name.

### File, metadata, and thumbnail reads

```rust
pub fn photo_file_path(database: &Database, photo_id: i64) -> CoreResult<PathBuf>
pub fn get_photo_metadata(database: &Database, photo_id: i64) -> CoreResult<PhotoMetadata>
pub fn get_or_create_thumbnail(
    database: &Database,
    photo_id: i64,
    thumbnail_root: &Path,
) -> CoreResult<PathBuf>
pub fn rebase_thumbnail_paths(
    database: &Database,
    thumbnail_root: &Path,
) -> CoreResult<usize>
```

All real-file paths are resolved under the canonical open root. Parent-path,
absolute-path, and root-escape attempts are rejected.

## Mapping models

### `PhotoTaxonStatus`

| Value | Meaning |
| --- | --- |
| `matched` | The longest filename name match identifies one taxon. |
| `unmatched` | No taxonomy name occurs in the filename. |
| `ambiguous` | Longest matches identify more than one taxon. |
| `stale` | A previously referenced taxon was removed before remapping completed. |

There is deliberately no `resolved_by` field. A user selection writes the
same current mapping model as an automatic unique match. An unchanged
directory refresh does not recompute that mapping; a filename or taxonomy
change may recompute it.

### `PhotoTaxonCandidate`

| Field | Type | Description |
| --- | --- | --- |
| `summary` | `TaxonSummary` | Candidate taxon and breadcrumb. |
| `matched_names` | `Vec<PhotoMatchedName>` | Taxonomy names actually found in the filename. |
| `accepted_names` | `TaxonDisplayNames` | Current accepted scientific, English, and Chinese names. |

`PhotoMatchedName` contains `name_id`, `name_kind`, `name`, and `is_accepted`.
Accepted names are joined from taxonomy when returned; they are not copied to
`photo_taxon_mapping`.

### `PhotoTaxonUsage`

Contains `taxon_id`, `rank`, accepted `names`, `direct_photo_count`, and
`subtree_photo_count`.

### `PhotoTaxonNode`

Contains an optional current `taxon`, immediate `children`, and the current
node's `subtree_photo_count`. A root request uses `taxon = null`.

### `PhotoTaxonPhotoPage`

Contains `items: Vec<Photo>` and `next_photo_id: Option<i64>`. Pass the returned
ID as `after_photo_id` for the next page.

## Mapping Rust API

### Photo match reads and selection

```rust
pub fn get_photo_mapping(
    database: &Database,
    photo_id: i64,
) -> CoreResult<Option<PhotoTaxonMapping>>

pub fn get_photo_taxon_match(
    database: &Database,
    photo_id: i64,
) -> CoreResult<PhotoTaxonMatch>

pub fn select_photo_taxon(
    database: &Database,
    photo_id: i64,
    taxon_id: i64,
) -> CoreResult<PhotoTaxonMapping>
```

`get_photo_taxon_match` returns the stored mapping plus current candidates and
their accepted names. `select_photo_taxon` accepts only a taxon present in the
current filename candidates and updates sparse usage counts atomically.

### Sparse taxonomy browsing

```rust
pub fn get_photo_taxon_node(
    database: &Database,
    taxon_id: Option<i64>,
    show_empty: bool,
) -> CoreResult<PhotoTaxonNode>

pub fn list_photos_for_taxon(
    database: &Database,
    taxon_id: Option<i64>,
    include_descendants: bool,
    after_photo_id: Option<i64>,
    limit: usize,
) -> CoreResult<PhotoTaxonPhotoPage>
```

With `show_empty = false`, child queries are driven by
`photo_taxon_usage.subtree_photo_count > 0`; taxonomy branches without photos
are not returned. Set `show_empty = true` to include zero-count children.

`taxon_id = null` addresses the virtual root. For photo pages,
`include_descendants = true` includes mappings on the selected taxon and every
descendant. Limits are clamped to `1..=500`.

### Mapping maintenance

```rust
pub fn get_metadata(database: &Database) -> CoreResult<MappingMetadata>
pub fn rebuild_mapping(database: &Database) -> CoreResult<MappingSyncResult>
```

Photo refresh and rename update mapping automatically. Taxonomy writes also
trigger rematching. Photo refresh, candidate reads, user selection, and full
remapping share a process-wide Aho-Corasick matcher cache. A monotonic name
revision invalidates the cached matcher only when matching-relevant taxon name
fields change; metadata-only name updates keep using the current matcher. The
rematcher writes only mappings whose taxon or status changed. Sparse usage is
rebuilt from grouped matched taxon counts so taxonomy parent changes are
reflected without copying taxonomy rows into the photos module.

## Tauri commands

The desktop adapter exposes these primary commands:

| Command | Parameters | Return |
| --- | --- | --- |
| `get_photo_library` | none | `PhotoLibrary | null` |
| `open_photo_library` | `root` | `PhotoLibrary` |
| `browse_photo_directory` | `directory_id`, optional `cursor`, optional `limit` | `DirectoryListingPage` |
| `refresh_photo_directory` | `directory_id` | `PhotoSyncResult` |
| `rename_photo` | `photo_id`, `new_filename` | `Photo` |
| `get_photo` | `photo_id` | `Photo` |
| `get_photo_metadata` | `photo_id` | `PhotoMetadata` |
| `get_photo_taxon_match` | `photo_id` | `PhotoTaxonMatch` |
| `select_photo_taxon` | `photo_id`, `taxon_id` | `PhotoTaxonMapping` |
| `get_photo_taxon_node` | optional `taxon_id`, optional `show_empty` | `PhotoTaxonNode` |
| `list_photos_for_taxon` | optional `taxon_id`, optional `include_descendants`, optional `after_photo_id`, optional `limit` | `PhotoTaxonPhotoPage` |

The adapter uses `160` as the default directory and taxon photo-page limit.
It defaults to hiding empty taxonomy branches and including descendants in a
taxon photo page.
