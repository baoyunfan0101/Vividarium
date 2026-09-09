# Photos Backend API

This document describes the public interfaces in `vividarium_core::photos`.
Photo-to-taxon interfaces are documented in [MAPPING.md](MAPPING.md), storage
management in [STORAGE.md](STORAGE.md), shared history interfaces in
[OPERATIONS.md](OPERATIONS.md), and naming hooks in [NAMING.md](NAMING.md).

All functions take `&Database` as their first parameter and return
`CoreResult<T>`. IDs are signed 64-bit integers. Serialized enum values use
`snake_case`.

## Cursor contract

Photo list interfaces return `PhotoPage<T>`:

| Field | Type | Description |
| --- | --- | --- |
| `items` | `Vec<T>` | Items in the current page. |
| `next_cursor` | `Option<String>` | Opaque next-page cursor, or `None` on the last page. |

Pass `None` for the first page and pass a returned cursor back unchanged.
Cursors are scoped to the interface and all filter parameters. `limit` is
clamped to `1..=500`.

## `vividarium_core::photos`

### Core types

| Type | Fields | Description |
| --- | --- | --- |
| `PhotoLibrary` | `root_path`, `root_directory_id` | Active library root and browse entry point. |
| `PhotoDirectory` | `directory_id`, `parent_directory_id`, `name`, `relative_path` | One indexed directory. |
| `Photo` | `photo_id`, `directory_id`, `relative_path`, `filename`, `file_size`, `modified_at_ns`, `thumbnail_path` | One indexed image file. |
| `PhotoMetadata` | `photo_id`, `captured_at`, `camera`, `width`, `height`, `longitude`, `latitude`, `exif_json` | Cached image metadata. |
| `DirectoryEntryCounts` | `directory_count`, `file_count` | Immediate child counts. |
| `PhotoSyncResult` | `directory_id`, `inserted`, `unchanged`, `updated`, `deleted`, `directories_inserted`, `directories_deleted` | Recursive refresh result for the requested directory subtree. |

`PhotoDirectoryItem` is a tagged enum with either
`directory: PhotoDirectory` or `photo: Photo`. Directory browse pages return
child directories before photos.

### Library and browse interfaces

| Function | Parameters after `database` | Return | Description |
| --- | --- | --- | --- |
| `open_library` | `root: &str` | `PhotoLibrary` | Register or activate the library at an existing photo root. |
| `get_library` | none | `Option<PhotoLibrary>` | Return the active library. |
| `get_photo_count` | none | `i64` | Count photos in the active library. |
| `get_directory_counts` | `directory_id: i64` | `DirectoryEntryCounts` | Count immediate directories and photos. |
| `get_photo` | `photo_id: i64` | `Option<Photo>` | Load one photo. |
| `browse_directory` | `directory_id: i64`, `cursor: Option<&str>`, `limit: usize` | `PhotoPage<PhotoDirectoryItem>` | Browse one directory as a cursor page. |
| `refresh_directory` | `directory_id: i64` | `PhotoSyncResult` | Reconcile the requested directory and every descendant directory. |
| `is_initial_index_complete` | `library_uuid: &str` | `bool` | Return the durable first-index state for one registered library. |
| `initial_index_photo_library` | `library_uuid: &str`, progress callback | `Option<PhotoSyncResult>` | Recursively reconcile one captured registration and mark it complete only after success. Return `None` when it is already complete. |
| `scan_photo_library` | `library_uuid: &str`, progress callback | `PhotoSyncResult` | Incrementally reconcile the complete registered root; also completes the durable initial-index marker when needed. |
| `has_pending_photo_metadata` | `library_uuid: &str` | `bool` | Report whether indexed photos still need metadata extraction. |
| `index_photo_metadata_for_library` | `library_uuid: &str`, progress callback | `PhotoMetadataIndexResult` | Extract missing metadata in bounded batches and skip completed rows. |

Every newly discovered, changed, or unqueued indexed photo is queued for automatic mapping.
Filesystem indexing performs the same incremental comparison as Refresh.
Directories are scanned without holding the photo write lock and committed one
directory and at most 256 photo rows at a time. Metadata is extracted separately
in batches of 64, committed in short transactions, and shares the same cache as
foreground `get_photo_metadata`. Both loops yield between batches. Neither phase
requests or creates thumbnails.

### Search interfaces

| Function | Parameters after `database` | Return | Description |
| --- | --- | --- | --- |
| `search_photos_by_filename` | `query: &str`, `cursor: Option<&str>`, `limit: usize` | `PhotoPage<Photo>` | Search filenames only. |
| `search_photos` | `query: &str`, `cursor: Option<&str>`, `limit: usize` | `PhotoPage<Photo>` | Search filename and current mapped taxon, then deduplicate by photo. |

Blank search text returns an empty page.
General search uses the complete ranked taxonomy candidate relation before
expanding matched taxa to descendant photos, unions those photos with filename
matches, deduplicates by `photo_id`, and applies the page limit to the final
photo set.
The desktop `search_photos` command executes the database lookup on a blocking
worker and resolves asynchronously.

### Media interfaces

| Function | Parameters after `database` | Return | Description |
| --- | --- | --- | --- |
| `photo_file_path` | `photo_id: i64` | `PathBuf` | Resolve and validate the current absolute file path. |
| `photo_file_path_for_library` | `library_uuid: &str`, `photo_id: i64` | `PathBuf` | Resolve a photo against one explicit registered library. |
| `photo_directory_path` | `directory_id: i64` | `PathBuf` | Resolve and validate the current absolute directory path. |
| `get_photo_metadata` | `photo_id: i64` | `PhotoMetadata` | Return cached metadata or extract and cache it. |
| `get_or_create_thumbnail` | `photo_id: i64`, `thumbnail_root: &Path` | `PathBuf` | Return or create a WebP thumbnail. |
| `get_or_create_thumbnail_for_library` | `library_uuid: &str`, `photo_id: i64`, `thumbnail_root: &Path` | `PathBuf` | Return or create a WebP thumbnail in that library's UUID namespace. |
| `rebase_thumbnail_paths` | `thumbnail_root: &Path` | `usize` | Rebind stored thumbnail paths to files found under a new cache root. |

The desktop media URL includes the active library UUID. The private media
protocol rejects stale requests for another library, and thumbnail files are
stored below a UUID-specific cache directory. Photo IDs and file timestamps
therefore cannot collide across libraries. Thumbnails are generated lazily by
near-viewport media requests, never as part of indexing. Image decoding does
not hold the photo-index write lock.

### Rename interfaces

| Function | Parameters after `database` | Return | Description |
| --- | --- | --- | --- |
| `rename_photo` | `photo_id: i64`, `new_filename: &str` | `Photo` | Rename one file to an explicit filename. |
| `rename_directory` | `directory_id: i64`, `new_name: &str` | `PhotoDirectory` | Rename one indexed directory and update descendant directory paths. |
| `rename_photo_from_taxon` | `photo_id: i64` | `Photo` | Rename one currently matched photo with naming settings. |
| `rename_photos_from_taxa` | `photo_ids: &[i64]` | `PhotoRenameOperationResult` | Rename selected photos; rows may succeed independently. |
| `rename_photos_in_directory_from_taxa` | `directory_id: i64`, `include_descendants: bool` | `PhotoRenameOperationResult` | Rename current matched photos in the requested scope. |

`PhotoRenameOperationResult` contains a shared `operation_id` for every
non-empty request and one `PhotoRenameRowOutcome` per input. Each row contains
`row_number`, the same `operation_id`, `photo_id`, `status`, `message`, and
optional current `photo`. `PhotoRenameRowStatus` is `applied`, `no_change`, or
`failed`. An empty request returns no operation.

The desktop bulk-rename commands return an operation immediately. Completed
operation state contains only `operation_id`, `total`, `applied`, `no_change`,
and `failed`; per-photo audit rows remain in Rename History instead of being
retained in the operation-status payload.

### Filename format settings

`PhotoFilenameFormatSettings` contains six booleans: `family_zh`,
`family_sci`, `genus_zh`, `genus_sci`, `species_zh`, and `species_sci`.

| Function | Parameters after `database` | Return | Description |
| --- | --- | --- | --- |
| `get_photo_filename_format_settings` | none | `PhotoFilenameFormatSettings` | Read active rename fields. |
| `set_photo_filename_format_settings` | `settings: &PhotoFilenameFormatSettings` | `()` | Save settings; at least one field must be enabled. |
| `format_photo_filename` | `info: &TaxonomicNameInfo`, `suffix: &str`, `settings: &PhotoFilenameFormatSettings` | `String` | Format a filename without reading the database. |

Saving filename-format settings does not rename existing files. Rename from
taxonomy is an explicit operation and reads the latest saved format each time.

### Photo operation interfaces

The photo module exports the common interfaces below:

| Function | Parameters after `database` | Return |
| --- | --- | --- |
| `list_operations` | `cursor: Option<&str>`, `limit: usize` | `OperationPage<OperationSummary>` |
| `list_operation_audit` | `operation_id: i64`, `cursor: Option<&str>`, `limit: usize` | `OperationPage<OperationAuditRow>` |
| `write_operation_audit` | `operation_id: i64`, `writer: &mut W` where `W: Write` | `()` |
| `write_operations_audit` | `operation_ids: &[i64]`, `writer: &mut W` where `W: Write` | `()` |
| `write_all_operation_audit` | `writer: &mut W` where `W: Write` | `()` |
| `rollback_operation` | `operation_id: i64` | `()` |

Successful rollback restores all recorded filenames and deletes the original
operation. See [OPERATIONS.md](OPERATIONS.md) for shared DTOs and CSV columns.
