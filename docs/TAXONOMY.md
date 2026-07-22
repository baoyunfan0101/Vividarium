# Taxonomy Knowledge Base Backend API

This document describes the public backend surface of the taxonomy knowledge
base. It covers the Rust API exported from `phytoindex_core::taxonomy` and the
Tauri commands that adapt part of that API for desktop IPC.

This surface owns taxonomy records, names, identifiers, structured updates,
direct management actions, custom SQL changes, operation history, and
operation rollback. Photo browsing, photo-to-taxon mapping, workbook parsing,
and frontend behavior are outside this document.

## Public module

All Rust types and functions described below are available from:

```rust
use phytoindex_core::taxonomy::*;
```

The module hides its storage and query implementation. Callers work with the
public models and functions only.

All Rust functions return `CoreResult<T>`. Invalid input and invalid cursors
return `CoreError::InvalidArgument`; database failures return
`CoreError::Database`. Read functions use `Option<T>` where absence is a normal
result. Delete actions may return `CoreError::NotFound`. Structured and direct
update planning reports row-level failures in `TaxonRowOutcome` when possible.

## Serialization conventions

- Struct fields use `snake_case` in serialized data.
- Enum values use `snake_case`.
- `Option<T>` is serialized as either its value or `null`.
- Taxon, batch, and operation IDs are signed 64-bit integers in Rust and JSON
  numbers over IPC.
- Database timestamps are returned as SQLite UTC timestamp strings.
- Cursor strings are opaque. A caller must not inspect, edit, or reuse a
  cursor with a different endpoint, parent taxon, or batch.

## Common enums

### `TaxonRank`

The supported hierarchy, from root to leaf, is:

```text
kingdom -> order -> family -> genus -> species
```

Serialized values are `kingdom`, `order`, `family`, `genus`, and `species`.

### `TaxonomyNameKind`

The supported name groups are `scientific`, `english`, and `chinese`.

### `TaxonomyPage<T>`

The serialized page model is `TaxonomyPage`; `T` is the endpoint-specific
item type.

| Field | Type | Description |
| --- | --- | --- |
| `items` | `Vec<T>` | Items in the current page. |
| `next_cursor` | `Option<String>` | Opaque cursor for the next page, or `null` when the page is final. |

Page limits are clamped to `1..=500` by the core API. Tauri list commands use
`50` when `limit` is omitted.

## Read models

### `TaxonDisplayNames`

Compact display names for a taxon.

| Field | Type | Description |
| --- | --- | --- |
| `scientific` | `Option<String>` | Accepted scientific name when present. |
| `english` | `Option<String>` | Accepted English name when present. |
| `chinese` | `Option<String>` | Accepted Chinese name when present. |

If imported data lacks an accepted name for a populated name group, the
backend uses the first name in lexical order as a display fallback.

### `TaxonBreadcrumbItem`

| Field | Type | Description |
| --- | --- | --- |
| `taxon_id` | `i64` | Ancestor taxon ID. |
| `rank` | `TaxonRank` | Ancestor rank. |
| `names` | `TaxonDisplayNames` | Compact names for the ancestor. |

### `TaxonSummary`

| Field | Type | Description |
| --- | --- | --- |
| `taxon_id` | `i64` | ID of the current taxon. |
| `rank` | `TaxonRank` | Rank of the current taxon. |
| `breadcrumb` | `Vec<TaxonBreadcrumbItem>` | All ancestors in root-to-parent order. The current taxon is not repeated here. |
| `names` | `TaxonDisplayNames` | Compact names for the current taxon. |

The complete ID path is therefore
`breadcrumb[*].taxon_id + taxon_id`: every ancestor ID is in `breadcrumb`,
and the current ID is in the top-level `taxon_id` field.

### `TaxonChild`

Lightweight immediate-child model.

| Field | Type | Description |
| --- | --- | --- |
| `taxon_id` | `i64` | Child taxon ID. |
| `rank` | `TaxonRank` | Child rank. |
| `names` | `TaxonDisplayNames` | Compact child names. |

Children do not include a repeated breadcrumb or full detail.

### `TaxonNameDetail`

| Field | Type | Description |
| --- | --- | --- |
| `name` | `String` | Name text. |
| `is_accepted` | `bool` | Whether this is the accepted name in its name group. |
| `authority_year` | `Option<String>` | Scientific authority/year metadata. |
| `category` | `Option<String>` | Name category metadata. |
| `source` | `Option<String>` | Name source metadata. |

### `TaxonNamesDetail`

Contains `scientific`, `english`, and `chinese` arrays of
`TaxonNameDetail`. Within each array, accepted names are returned before other
names, followed by lexical name order.

### `TaxonIdentifierDetail`

| Field | Type | Description |
| --- | --- | --- |
| `source` | `String` | Identifier namespace or provider. |
| `external_id` | `String` | Identifier in that namespace. |

### `TaxonDetail`

| Field | Type | Description |
| --- | --- | --- |
| `taxon_id` | `i64` | Current taxon ID. |
| `rank` | `TaxonRank` | Current rank. |
| `parent_taxon_id` | `Option<i64>` | Immediate parent ID; `null` for a root taxon. |
| `geological_range` | `Option<String>` | Geological range metadata. |
| `names` | `TaxonNamesDetail` | All names and name metadata. |
| `identifiers` | `Vec<TaxonIdentifierDetail>` | External identifiers. |

`TaxonDetail` deliberately does not contain a summary or children.

### `TaxonDetailNode`

| Field | Type | Description |
| --- | --- | --- |
| `summary` | `TaxonSummary` | Current taxon display data and ancestor breadcrumb. |
| `detail` | `TaxonDetail` | Full current-taxon data. |
| `children` | `TaxonomyPage<TaxonChild>` | First or requested page of immediate children. |

## Read and search API

### `search_taxa`

```rust
pub fn search_taxa(
    database: &Database,
    query: &str,
    limit: usize,
) -> CoreResult<Vec<TaxonSearchResult>>
```

Parameters:

| Parameter | Description |
| --- | --- |
| `query` | Name text to search across scientific, English, and Chinese names. Leading, trailing, and repeated whitespace is normalized. An empty normalized query returns an empty list. |
| `limit` | Maximum number of taxa to return after clamping to `1..=500`. |

Results are accumulated in this priority order:

1. Exact full-name match.
2. Full-name prefix match.
3. Prefix of any word in a name.
4. Match in the middle of a name.
5. Fuzzy full-name match.

Earlier tiers always remain ahead of later tiers, and a taxon is returned only
once. Word-prefix matching requires at least two characters. Middle and fuzzy
matching require at least three characters.

The fuzzy tier uses trigram candidates followed by character-level Levenshtein
distance. The maximum accepted distance is 1 for queries up to 4 characters,
2 for queries of 5 through 8 characters, and 3 for longer queries.

`TaxonSearchResult` contains:

| Field | Type | Description |
| --- | --- | --- |
| `summary` | `TaxonSummary` | Compact current taxon and complete ancestor breadcrumb. |
| `detail` | `TaxonDetail` | Full current-taxon detail. |
| `matches` | `Vec<TaxonNameMatch>` | Names on this taxon that satisfied the selected search tiers. |

`TaxonNameMatch` contains `name_kind`, `name`, and `is_accepted`. Search
results do not load children; use `list_taxon_children` or
`get_taxon_detail_node` when children are needed.

### `get_taxon_summary`

```rust
pub fn get_taxon_summary(
    database: &Database,
    taxon_id: i64,
) -> CoreResult<Option<TaxonSummary>>
```

Returns the compact current-taxon model and its root-to-parent breadcrumb.
Returns `None` when `taxon_id` does not exist.

### `get_taxon_detail`

```rust
pub fn get_taxon_detail(
    database: &Database,
    taxon_id: i64,
) -> CoreResult<Option<TaxonDetail>>
```

Returns all names, name metadata, identifiers, parent ID, and geological range
for one taxon. Returns `None` when the taxon does not exist.

### `get_taxon_detail_node`

```rust
pub fn get_taxon_detail_node(
    database: &Database,
    taxon_id: i64,
    children_cursor: Option<&str>,
    children_limit: usize,
) -> CoreResult<Option<TaxonDetailNode>>
```

Returns summary, detail, and one page of immediate children. Pass `None` for
the first child page. Returns `None` when the taxon does not exist.

### `list_taxon_children`

```rust
pub fn list_taxon_children(
    database: &Database,
    taxon_id: i64,
    cursor: Option<&str>,
    limit: usize,
) -> CoreResult<TaxonomyPage<TaxonChild>>
```

Returns immediate children ordered by rank and taxon ID. Pass `None` for the
first page and then pass the previous `next_cursor` unchanged.

## Structured batch update API

Structured updates locate a target from rank scientific names, preview the
planned changes, and optionally apply them. This API accepts already parsed
rows; CSV and workbook parsing are adapter responsibilities.

### `TaxonNameInput`

| Field | Type | Description |
| --- | --- | --- |
| `name` | `String` | Required when the enclosing name input is present. |
| `is_accepted` | `Option<bool>` | Requested accepted-name state. `null` preserves existing state, or selects the default for a new name. |
| `authority_year` | `Option<String>` | Metadata to supplement or overwrite. |
| `category` | `Option<String>` | Metadata to supplement or overwrite. |
| `source` | `Option<String>` | Metadata to supplement or overwrite. |

Blank optional strings are normalized as absent values. Existing values are
never cleared by passing blank or `null`.

### `TaxonInputRow`

| Field | Type | Description |
| --- | --- | --- |
| `selected_taxon_id` | `Option<i64>` | Resolves an ambiguous locator. It must be one of the candidates produced by the row locator. |
| `kingdom` | `Option<String>` | Kingdom scientific name locator/filter. |
| `order` | `Option<String>` | Order scientific name locator/filter. |
| `family` | `Option<String>` | Family scientific name locator/filter. |
| `genus` | `Option<String>` | Genus scientific name locator/filter. |
| `species` | `Option<String>` | Species scientific name locator/filter. |
| `geological_range` | `Option<String>` | Taxon metadata to supplement or overwrite. |
| `scientific` | `Option<TaxonNameInput>` | Scientific name change. |
| `english` | `Option<TaxonNameInput>` | English name change. |
| `chinese` | `Option<TaxonNameInput>` | Chinese name change. |

The deepest non-empty rank field is the target rank and target scientific
name. Higher rank fields narrow the lineage match. When creation is enabled,
a new non-species taxon requires its immediate parent locator. A new species
may derive its genus locator from the first word of a binomial scientific
name.

### `TaxonUpdateOptions`

All flags default to `false`.

| Field | Effect when `true` |
| --- | --- |
| `allow_new_names` | Allows a name absent from an existing taxon to be appended. |
| `allow_new_taxa` | Allows creation when the row locator finds no taxon. It does not create missing ancestors. |
| `allow_overwrite` | Allows replacement of non-empty geological-range or name metadata values. Empty existing values may be supplemented without this flag. |
| `allow_switch_accepted_name` | Allows a different name in the same name group to become accepted. The previous accepted name is demoted atomically. |

An accepted name cannot be demoted without promoting a replacement. The first
name in any populated name group must be accepted.

### `preview_rows`

```rust
pub fn preview_rows(
    database: &Database,
    rows: &[TaxonInputRow],
    options: TaxonUpdateOptions,
) -> CoreResult<TaxonBatchResult>
```

Executes the planned rows in one temporary transaction and then rolls it back.
It performs no persistent write and creates no operation history.

The returned `TaxonBatchResult.batch_id` is always `None`. A valid changing row
has status `ready`; a row with no effective change has status `no_change`.

### `apply_rows`

```rust
pub fn apply_rows(
    database: &Database,
    rows: &[TaxonInputRow],
    options: TaxonUpdateOptions,
) -> CoreResult<TaxonBatchResult>
```

Applies each row in its own transaction. A row-level validation or matching
failure is returned in that row's outcome and does not roll back successful
rows. A batch record is created lazily when the first row produces a real
change. If every row fails or produces no change, `batch_id` is `None`.

Each changed row creates one operation and returns status `applied` with an
`operation_id`. Unchanged and rejected rows have no operation ID.

### `TaxonBatchResult` and `TaxonRowOutcome`

`TaxonBatchResult` contains an optional `batch_id` and one `rows` outcome per
input row, in input order.

| `TaxonRowOutcome` field | Type | Description |
| --- | --- | --- |
| `row_number` | `usize` | One-based input row number. |
| `operation_id` | `Option<i64>` | Created operation ID for an applied change. |
| `status` | `TaxonRowStatus` | Machine-readable row result. |
| `message` | `String` | Human-readable result or rejection reason. |
| `target` | `Option<TaxonSummary>` | Located or newly created target when available. |
| `candidates` | `Vec<TaxonSummary>` | Candidate taxa when status is `ambiguous`. |
| `changes` | `Vec<TaxonChange>` | Planned or applied semantic changes. |

`TaxonRowStatus` values:

| Value | Meaning |
| --- | --- |
| `ready` | Preview found a valid plan with changes. |
| `applied` | Apply committed the row and logged an operation. |
| `no_change` | Target matched, but the input produced no effective change. |
| `not_found` | No target or required parent matched and creation was not possible. |
| `ambiguous` | Multiple taxa matched; inspect `candidates` and retry with `selected_taxon_id`. |
| `conflict` | The requested change violates options or taxonomy update rules. |
| `invalid` | The row shape, selected candidate, or required value is invalid. |

`TaxonChange` contains a `kind`, `field`, `old_value`, and `new_value`. Its
`TaxonChangeKind` values are `create_taxon`, `append_name`, `supplement`,
`overwrite`, and `change_accepted_name`.

## Direct management actions

### `update_taxon`

```rust
pub fn update_taxon(
    database: &Database,
    input: TaxonUpdateInput,
    options: TaxonUpdateOptions,
) -> CoreResult<TaxonomyUpdateActionResult>
```

`TaxonUpdateInput` contains `taxon_id`, `geological_range`, and optional
`scientific`, `english`, and `chinese` `TaxonNameInput` values. Because the
target is already identified, this function performs no locator or candidate
search. `allow_new_taxa` has no effect here; the other update options retain
their structured-update meanings.

`TaxonomyUpdateActionResult` contains:

| Field | Type | Description |
| --- | --- | --- |
| `batch_id` | `Option<i64>` | Created query-update batch, or `null` for no change. |
| `outcome` | `TaxonRowOutcome` | Row-style result with `row_number = 1`. |

### `delete_taxon_name`

```rust
pub fn delete_taxon_name(
    database: &Database,
    input: DeleteTaxonNameInput,
) -> CoreResult<TaxonomyActionResult>
```

`DeleteTaxonNameInput` fields:

| Field | Type | Description |
| --- | --- | --- |
| `taxon_id` | `i64` | Owner taxon. |
| `name_kind` | `TaxonomyNameKind` | Name group. |
| `name` | `String` | Exact normalized name to delete. |
| `replacement_accepted_name` | `Option<String>` | Existing name to promote when deleting the accepted name while other names remain. |

`replacement_accepted_name` is required only when the deleted name is accepted
and other names of the same kind remain. It must differ from `name` and already
exist on the same taxon in the same name group.

### `delete_taxon`

```rust
pub fn delete_taxon(
    database: &Database,
    taxon_id: i64,
) -> CoreResult<TaxonomyActionResult>
```

Deletes one taxon and its owned names/identifiers. The taxon must exist, have
no child taxa, and have no photo mappings. This is a single-item management
action; there is no public batch-delete interface.

### `TaxonomyActionResult`

Successful name and taxon deletion returns only:

| Field | Type | Description |
| --- | --- | --- |
| `batch_id` | `i64` | Created management-action batch. |
| `operation_id` | `i64` | Reversible operation in that batch. |

Call a read API when a refreshed taxon view is needed.

## Custom SQL action

### `execute_custom_taxonomy_sql`

```rust
pub fn execute_custom_taxonomy_sql(
    database: &Database,
    sql: &str,
    input: Option<TaxonomyCustomSqlTempTable>,
) -> CoreResult<TaxonomyCustomSqlResult>
```

`sql` must be non-empty. It may contain multiple statements. The action is
transactional and restricted to taxonomy-domain reads and writes. It cannot
change schema, load extensions, access unrelated application tables, or access
the taxonomy search-index tables directly. The complete taxonomy is validated
before a changing transaction commits.

This action does not return SQL result rows. It is intended for controlled
taxonomy changes, not general-purpose querying.

An optional `TaxonomyCustomSqlTempTable` creates `temp.input` on the same
connection before SQL execution:

| Field | Type | Description |
| --- | --- | --- |
| `columns` | `Vec<String>` | Unique ASCII identifiers beginning with a letter or underscore. At least one column is required. |
| `rows` | `Vec<Vec<String>>` | Text values. Every row width must equal `columns.len()`. |

The table exists only for this call and disappears with the connection. Batch
history stores only `TaxonomyCustomSqlTempTableMetadata`, which contains the
normalized column names and row count, not the uploaded row values.

`TaxonomyCustomSqlResult` contains:

| Field | Type | Description |
| --- | --- | --- |
| `batch_id` | `Option<i64>` | Created batch, or `null` when SQL changed no tracked taxonomy row. |
| `operation_id` | `Option<i64>` | Created operation, or `null` when there was no tracked change. |
| `changeset_size` | `usize` | Serialized SQLite changeset size in bytes, not an affected-row count. |

A no-change SQL call succeeds with both IDs set to `null` and
`changeset_size = 0`; it creates no history records.

## Operation history and rollback

Every applied structured row and every changing direct action is recorded as
an operation. Related operations share a batch that stores invocation context
and input.

### `TaxonomyBatchContext`

This is a tagged enum serialized with a `source` field:

| `source` value | Additional fields | Meaning |
| --- | --- | --- |
| `batch_update` | `options` | Structured `apply_rows` batch. |
| `query_update` | `options` | Direct `update_taxon` action. |
| `query_delete_name` | none | Direct name deletion. |
| `query_delete_taxon` | none | Direct taxon deletion. |
| `custom_sql` | `input` | Custom SQL action and optional temp-input metadata. |

### `TaxonomyOperationBatch`

| Field | Type | Description |
| --- | --- | --- |
| `batch_id` | `i64` | Batch ID. |
| `context` | `TaxonomyBatchContext` | Typed source and options/metadata. |
| `input` | `serde_json::Value` | Original invocation input stored for audit. Shape depends on `context.source`. |
| `created_at` | `String` | Batch creation timestamp. |

Typical `input` shapes are an array of `TaxonInputRow` for `batch_update`, a
single action input object for query actions, and `{ "sql": "..." }` for
`custom_sql`.

### `TaxonomyOperation`

| Field | Type | Description |
| --- | --- | --- |
| `operation_id` | `i64` | Operation ID. |
| `batch_id` | `i64` | Parent batch ID. |
| `row_number` | `usize` | One-based source row; direct actions use `1`. |
| `status` | `TaxonomyOperationStatus` | `applied` or `reverted`. |
| `changeset_size` | `usize` | Stored SQLite changeset size in bytes. The changeset itself is not exposed. |
| `applied_at` | `String` | Apply timestamp. |
| `reverted_at` | `Option<String>` | Rollback timestamp when reverted. |

### History list functions

```rust
pub fn list_taxonomy_operation_batches(
    database: &Database,
    cursor: Option<&str>,
    limit: usize,
) -> CoreResult<TaxonomyPage<TaxonomyOperationBatch>>
```

Returns batches newest first, ordered by creation time and batch ID.

```rust
pub fn list_taxonomy_operations(
    database: &Database,
    cursor: Option<&str>,
    limit: usize,
) -> CoreResult<TaxonomyPage<TaxonomyOperation>>
```

Returns global operations newest first by operation ID.

```rust
pub fn list_taxonomy_operations_for_batch(
    database: &Database,
    batch_id: i64,
    cursor: Option<&str>,
    limit: usize,
) -> CoreResult<TaxonomyPage<TaxonomyOperation>>
```

Returns operations for one batch in source-row and operation-ID order. A
cursor from one batch cannot be used with another batch.

### `revert_taxonomy_operation`

```rust
pub fn revert_taxonomy_operation(
    database: &Database,
    operation_id: i64,
) -> CoreResult<()>
```

Reverts one `applied` operation from its stored changeset and marks it
`reverted`. It does not revert the rest of the batch and returns no refreshed
view. A missing, already reverted, or conflicting operation returns an error.
Later changes to the same records may prevent rollback.

## Tauri command surface

Tauri converts core errors to error strings. JavaScript invoke argument names
use `camelCase`; serialized input and output object fields remain `snake_case`.

| Command | JavaScript arguments | Return value |
| --- | --- | --- |
| `search_taxa` | `{ query, limit? }`; default limit `50` | `TaxonSearchResult[]` |
| `get_taxon_detail_node` | `{ taxonId, childrenCursor?, childrenLimit? }`; default child limit `50` | `TaxonDetailNode`; a missing taxon is an IPC error |
| `list_taxon_children` | `{ taxonId, cursor?, limit? }`; default limit `50` | `TaxonomyPage<TaxonChild>` |
| `delete_taxon_name` | `{ input: DeleteTaxonNameInput }` | `TaxonomyActionResult` |
| `update_taxon` | `{ input: TaxonUpdateInput, options? }`; omitted options use all-false defaults | `TaxonomyUpdateActionResult` |
| `delete_taxon` | `{ taxonId }` | `TaxonomyActionResult` |
| `execute_custom_taxonomy_sql` | `{ sql, input? }` | `TaxonomyCustomSqlResult` |
| `list_taxonomy_operation_batches` | `{ cursor?, limit? }`; default limit `50` | `TaxonomyPage<TaxonomyOperationBatch>` |
| `list_taxonomy_operations` | `{ cursor?, limit? }`; default limit `50` | `TaxonomyPage<TaxonomyOperation>` |
| `list_taxonomy_operations_for_batch` | `{ batchId, cursor?, limit? }`; default limit `50` | `TaxonomyPage<TaxonomyOperation>` |

The following public Rust functions currently have no Tauri command adapter:

- `get_taxon_summary`
- `get_taxon_detail`
- `preview_rows`
- `apply_rows`
- `revert_taxonomy_operation`

They are available to Rust callers and can be exposed later without changing
the core taxonomy interface.
