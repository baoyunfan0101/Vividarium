# Photos Backend API

This document describes the public backend surface of the single-root photo
library. Filesystem operations are exposed by `phytoindex_core::photos`.
Taxonomy matching and taxon-based photo browsing are exposed by
`phytoindex_core::mapping`.

The backend indexes one open photo root at a time. Stored paths are relative to
that root. All Rust functions return `CoreResult<T>`. Serialized fields and enum
values use `snake_case`.

## Storage behavior

| Table | Purpose |
| --- | --- |
| `photo_library` | Stores the one currently open root path. |
| `photo_directories` | Stores the indexed Finder-like directory hierarchy. |
| `photos` | Stores narrow filesystem facts used for browsing and change detection. |
| `photo_metadata` | Stores lazily extracted EXIF data and image dimensions. |
| `photo_taxon_mapping` | Stores the current selected taxon and resolved status. |
| `photo_taxon_usage` | Stores sparse direct and subtree photo counts for taxon browsing. |
| `photo_mapping_queue` | Stores photos waiting for knowledge-base matching. |
| `photo_mapping_state` | Stores taxonomy and processed-taxonomy revisions. |

The queue is durable. A process restart does not lose photos waiting to be
matched. The `processing` API status is derived from the queue or from a pending
taxonomy revision; it is not a timestamp or an `entry_revision`.

For ID batches of at most 500 items, the mapping layer uses ordinary SQLite
placeholders. Larger batches are loaded into temporary tables and joined from
SQL. This applies to photo and directory removal, mapping loads, and queue
cleanup, so a large cached subtree does not exceed SQLite parameter limits.

## Photos models

### `PhotoLibrary`

| Field | Type | Description |
| --- | --- | --- |
| `root_path` | `String` | Canonical absolute path of the open root. |
| `root_directory_id` | `i64` | ID of the root `PhotoDirectory` node. |

The library model does not count the entire photos table. Use
`get_photo_count` only when that count is needed.

### `PhotoDirectory`

| Field | Type | Description |
| --- | --- | --- |
| `directory_id` | `i64` | Stable database ID. |
| `parent_directory_id` | `Option<i64>` | Parent ID, or `null` for the root. |
| `name` | `String` | Immediate directory name. The root name is empty. |
| `relative_path` | `String` | Slash-separated path under the root. The root path is empty. |

### `Photo`

| Field | Type | Description |
| --- | --- | --- |
| `photo_id` | `i64` | Stable database ID. |
| `directory_id` | `i64` | Directory containing the real file. |
| `relative_path` | `String` | Complete file path relative to the open root. |
| `filename` | `String` | Real filesystem filename. |
| `file_size` | `i64` | File size in bytes. |
| `modified_at_ns` | `i64` | Filesystem modification time in nanoseconds since the Unix epoch. |
| `thumbnail_path` | `Option<String>` | Cached thumbnail path, if generated. |

`modified_at_ns` is a filesystem fact used for change detection. The backend
does not store scan, parse, or mapping timestamps.

### `PhotoMetadata`

Contains `photo_id`, `captured_at`, `camera`, `width`, `height`, `longitude`,
`latitude`, and `exif_json`. Metadata is read from `photo_metadata` when cached
and extracted from the real file on the first request otherwise. Directory
refresh does not read EXIF.

### `DirectoryListingPage`

| Field | Type | Description |
| --- | --- | --- |
| `directory` | `PhotoDirectory` | Directory being listed. |
| `directories` | `Vec<PhotoDirectory>` | Child directories in this page. |
| `files` | `Vec<Photo>` | Photos in this page. |
| `next_cursor` | `Option<String>` | Opaque next-page cursor, or `null` on the actual last page. |

Directories are returned before files. Limits are clamped to `1..=500`. The
query reads `limit + 1` entries internally, so it does not produce a redundant
cursor on the final page.

### `DirectoryEntryCounts`

Contains `directory_count` and `file_count` for the immediate entries of one
directory. Counts are separate from paginated browsing and are computed only
when explicitly requested.

### `PhotoSyncResult`

Contains `directory_id`, `inserted`, `unchanged`, `updated`, `deleted`,
`directories_inserted`, and `directories_deleted`.

## Photos Rust API

### Library and directory browsing

```rust
pub fn open_library(database: &Database, root: &str) -> CoreResult<PhotoLibrary>
pub fn get_library(database: &Database) -> CoreResult<Option<PhotoLibrary>>
pub fn get_photo_count(database: &Database) -> CoreResult<i64>
pub fn get_directory_counts(
    database: &Database,
    directory_id: i64,
) -> CoreResult<DirectoryEntryCounts>
pub fn browse_directory(
    database: &Database,
    directory_id: i64,
    cursor: Option<&str>,
    limit: usize,
) -> CoreResult<DirectoryListingPage>
```

`open_library` canonicalizes `root`. Opening a different root clears the old
photo index, mappings, and usage counts, but preserves taxonomy data.

`browse_directory` reads only SQLite. It does not scan the filesystem, count
entries, or extract metadata. A cursor is valid only for the same directory
listing.

### Directory refresh

```rust
pub fn refresh_directory(
    database: &Database,
    directory_id: i64,
) -> CoreResult<PhotoSyncResult>
```

Refresh scans only the immediate entries of `directory_id`. Child directories
are indexed as nodes but are not recursively scanned. The scan is not sorted;
browse order is provided by indexed SQLite queries.

New or changed photos are committed to `photos` and placed in
`photo_mapping_queue`. Their mapping reads as `processing` until
`process_pending_photo_matches` handles them. Removed photos and directory
subtrees synchronously remove their mappings and update sparse usage counts.
Unchanged photos cause no database or mapping writes.

The desktop `refresh_photo_directory` command starts refresh and matching in a
background operation and returns immediately with an operation descriptor.

### Photo reads

```rust
pub fn get_photo(database: &Database, photo_id: i64) -> CoreResult<Option<Photo>>
pub fn list_photos(database: &Database) -> CoreResult<Vec<Photo>>
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

`list_photos` is intended for administrative and rebuild work. Interactive
views should use paginated directory or taxon browsing.

All real-file paths are resolved under the canonical root. Absolute paths,
parent traversal, and root escapes are rejected.

### Manual and taxonomy-based rename

```rust
pub fn rename_photo(
    database: &Database,
    photo_id: i64,
    new_filename: &str,
) -> CoreResult<Photo>
pub fn rename_photo_from_taxon(
    database: &Database,
    photo_id: i64,
) -> CoreResult<Photo>
pub fn rename_photos_from_taxa(
    database: &Database,
    photo_ids: &[i64],
) -> CoreResult<Vec<Photo>>
```

`rename_photo` changes the real file and updates the database and mapping in
one serialized workflow. `new_filename` must be a supported image filename
without directory components. Existing destinations are rejected. Case-only
renames use the same temporary-path helper in the forward and rollback paths.
If both the database update and filesystem rollback fail, a runtime consistency
error reports both failures.

The taxonomy-based functions require every photo to have `matched` status and
an accepted scientific name. A logically `processing` photo cannot be renamed
from taxonomy even if its previous stored row was matched. The current
placeholder format is:

```text
{accepted scientific name}.{original extension}
```

The final configurable filename format is intentionally deferred.
`rename_photos_from_taxa` processes IDs in input order. A later collision or
error does not undo earlier successful renames.

## Mapping models

### `PhotoTaxonStatus`

| Value | Meaning |
| --- | --- |
| `processing` | The photo is queued, or taxonomy changes have not been processed yet. |
| `unmatched` | Taxonomy search returned no candidates. |
| `ambiguous` | One or more candidates await user selection. |
| `matched` | A current candidate taxon has been selected. |
| `stale` | A previously referenced taxon no longer exists. |

There is no `resolved_by` field. Background rematching preserves a selected
taxon while it remains among the current candidates. Otherwise it clears the
selection and returns the photo to `ambiguous` or `unmatched`.

### `PhotoTaxonMapping`

Contains `photo_id`, optional `taxon_id`, and `status`. `taxon_id` is present
only for `matched`, except that a synthesized `processing` response may retain
the previously selected ID while revalidation is pending.

### Candidate and match models

`PhotoTaxonMatch` contains:

| Field | Type | Description |
| --- | --- | --- |
| `mapping` | `PhotoTaxonMapping` | Current stored or synthesized state. |
| `candidates` | `Vec<PhotoTaxonCandidate>` | Current taxonomy search results. |

Each `PhotoTaxonCandidate` contains:

| Field | Type | Description |
| --- | --- | --- |
| `summary` | `TaxonSummary` | Candidate taxon and breadcrumb. |
| `matched_names` | `Vec<PhotoMatchedName>` | Names that matched the taxonomy search. |
| `accepted_names` | `TaxonDisplayNames` | Current accepted scientific, English, and Chinese names. |

`PhotoMatchedName` contains `name_id`, `name_kind`, `name`, and `is_accepted`.
Accepted names are read from taxonomy when returned; they are not duplicated in
the mapping table.

### Taxon browsing models

`PhotoTaxonUsage` contains `taxon_id`, `rank`, accepted `names`,
`direct_photo_count`, and `subtree_photo_count`.

`PhotoTaxonNode` contains an optional current `taxon`, immediate `children`,
and the current node's `subtree_photo_count`. A root request uses
`taxon_id = null`.

`PhotoTaxonPhotoPage` contains `items: Vec<Photo>` and
`next_photo_id: Option<i64>`.

`MappingMetadata` contains `mapped_photo_count`, `unmatched_photo_count`,
`ambiguous_photo_count`, `processing_photo_count`, and `mapping_taxa_count`.

`PhotoMappingRunResult` contains:

| Field | Type | Description |
| --- | --- | --- |
| `processed` | `usize` | Photos evaluated in this run. |
| `changed` | `usize` | Mapping rows whose selected taxon or status changed. |
| `pending` | `i64` | Remaining queued photos plus a pending taxonomy revision flag. |

## Matching logic and API

### Match extraction and taxonomy search

The configurable filename parser is reserved but not implemented yet. The
current extractor removes only the final extension and uses the entire
remaining filename as the taxonomy query.

No punctuation, symbols, or other non-alphanumeric name characters are
discarded or replaced. The extracted text is passed to the same taxonomy
search implementation used by `taxonomy::search_taxa`, including its exact,
full-name prefix, word-prefix, middle, and trigram-plus-edit-distance fuzzy
tiers. Matching queries the indexed taxonomy tables; it never builds an
in-memory matcher from all taxon names.

### Match reads, processing, and selection

```rust
pub fn get_metadata(database: &Database) -> CoreResult<MappingMetadata>
pub fn get_photo_mapping(
    database: &Database,
    photo_id: i64,
) -> CoreResult<Option<PhotoTaxonMapping>>
pub fn get_photo_taxon_match(
    database: &Database,
    photo_id: i64,
) -> CoreResult<PhotoTaxonMatch>
pub fn process_pending_photo_matches(
    database: &Database,
    progress: &mut MappingProgressCallback<'_>,
) -> CoreResult<PhotoMappingRunResult>
pub fn select_photo_taxon(
    database: &Database,
    photo_id: i64,
    taxon_id: i64,
) -> CoreResult<PhotoTaxonMapping>
pub fn rebuild_mapping(database: &Database) -> CoreResult<MappingSyncResult>
```

`get_photo_taxon_match` returns current candidates without selecting one.
`select_photo_taxon` accepts only a taxon in those current candidates and
updates mapping and sparse usage in one transaction.

`process_pending_photo_matches` works in batches of 200. For a photo refresh it
consumes only queued IDs. A taxonomy mutation increments a durable global
revision because a change can affect any taxonomy search tier. The next run
evaluates photos against the new revision, writes only changed mapping rows,
rebuilds sparse usage, and advances the processed revision. This is the
correctness-first affected-photo detection path; it performs indexed per-photo
taxonomy searches instead of loading all taxonomy names.

Deleting a selected taxon changes its mapping rows to `stale` before the
taxonomy row is removed. The pending taxonomy revision then re-evaluates those
photos without blocking the taxonomy delete.

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

With `show_empty = false`, only nodes with
`photo_taxon_usage.subtree_photo_count > 0` are returned. Taxonomy branches
without matched photos are therefore absent. Set `show_empty = true` to include
zero-count children.

When mappings change, all changed taxon IDs are loaded as recursive CTE seeds.
Their overlapping ancestor deltas are aggregated before batched usage updates,
avoiding one lineage query per taxon.

`include_descendants = true` includes direct mappings on the selected taxon and
all descendants. Page limits are clamped to `1..=500`.

## Desktop commands

| Command | Parameters | Return |
| --- | --- | --- |
| `get_photo_library` | none | `PhotoLibrary \| null` |
| `get_photo_library_count` | none | `i64` |
| `open_photo_library` | `root` | `PhotoLibrary` |
| `browse_photo_directory` | `directory_id`, optional `cursor`, optional `limit` | `DirectoryListingPage` |
| `get_photo_directory_counts` | `directory_id` | `DirectoryEntryCounts` |
| `refresh_photo_directory` | `directory_id` | Background operation descriptor |
| `start_photo_mapping` | none | Background operation descriptor |
| `rename_photo` | `photo_id`, `new_filename` | `Photo` |
| `rename_photo_from_taxon` | `photo_id` | `Photo` |
| `rename_photos_from_taxa` | `photo_ids` | `Vec<Photo>` |
| `get_photo` | `photo_id` | `Photo` |
| `get_photo_metadata` | `photo_id` | `PhotoMetadata` |
| `get_mapping_metadata` | none | `MappingMetadata` |
| `get_photo_taxon_match` | `photo_id` | `PhotoTaxonMatch` |
| `select_photo_taxon` | `photo_id`, `taxon_id` | `PhotoTaxonMapping` |
| `get_photo_taxon_node` | optional `taxon_id`, optional `show_empty` | `PhotoTaxonNode` |
| `list_photos_for_taxon` | optional `taxon_id`, optional `include_descendants`, optional `after_photo_id`, optional `limit` | `PhotoTaxonPhotoPage` |

`refresh_photo_directory`, `start_photo_mapping`, `start_taxa_update`, and
`start_taxa_rebuild` return after scheduling work. Progress and final results
are available through `get_operations_status`. Taxonomy update and rebuild
operations run pending photo matching after the taxonomy transaction finishes.

The adapter uses 160 as the default directory and taxon photo-page limit. It
defaults to hiding empty taxonomy branches and including descendants in a taxon
photo page.
