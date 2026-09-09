# Taxonomy Backend API

This document describes the public interface in
`vividarium_core::taxonomy`. Shared operation DTOs are documented in
[OPERATIONS.md](OPERATIONS.md), naming hooks in [NAMING.md](NAMING.md), and
database locations in [STORAGE.md](STORAGE.md).

All functions take `&Database` as their first parameter unless noted and
return `CoreResult<T>`. IDs are signed 64-bit integers. Serialized enum values
use `snake_case`.

## Core taxonomy types

`TaxonRank` values are `kingdom`, `order`, `family`, `genus`, and `species`.

A kingdom is a root taxon. Every other taxon has an existing parent whose
rank is strictly higher than the child rank. Intermediate ranks may be
omitted; equal-rank, reversed-rank, missing-parent, and cyclic relationships
are invalid.

Taxonomy validation loads the parent graph once and checks cyclic parent
relationships in memory. Each taxon that belongs to a cycle is reported as a
`parent_cycle` issue; taxa that only descend from a cycle retain their own
parent relationship validation results.

Validation checks cover parent structure, accepted-name counts, name-family
uniqueness, orphan names, and normalized stored names. General taxonomy writes
use the complete validation set. SQL Import staging uses the same checks except
raw name-family uniqueness, because its canonical duplicate validation covers
that condition before candidate construction.

Background SQL Import reports taxonomy validation as separate stages for
loading structure, parent cycles, parent relationships, scientific names,
localized names, duplicate names, orphan names, and normalized names. Staging
omits the duplicate-name and normalized-name stages; candidate validation uses
the complete set.

`TaxonomyNameType` values are:

| Value | Stored code | Meaning |
| --- | --- | --- |
| `sci_name` | `1` | Unique accepted scientific name. |
| `synonym` | `2` | Scientific synonym. |
| `zh_name` | `3` | Unique accepted Chinese name. |
| `zh_alias` | `4` | Chinese alias. |
| `en_name` | `5` | Unique accepted English name. |
| `en_alias` | `6` | English alias. |

Every taxon has exactly one `sci_name`. A taxon may have zero or one `zh_name`
and zero or one `en_name`. Synonyms and Chinese or English aliases have no
count limit. Chinese aliases require a Chinese accepted name, and English
aliases require an English accepted name.
Within a taxon, the same case-sensitive stored `name` may appear only once in
each name family: scientific (`sci_name` and `synonym`), Chinese (`zh_name` and
`zh_alias`), or English (`en_name` and `en_alias`).

`TaxonomyPage<T>` contains `items` and an opaque `next_cursor`. Pass `None`
for the first page and reuse a returned cursor only with the same interface
and parent resource. Page limits are clamped to `1..=500`.

## Read views

| Type | Fields | Description |
| --- | --- | --- |
| `TaxonDisplayNames` | `sci_name`, `zh_name`, `en_name` | Compact accepted names. |
| `TaxonBreadcrumbItem` | `taxon_id`, `rank`, `names` | One ancestor. |
| `TaxonSummary` | `taxon_id`, `rank`, `breadcrumb`, `names` | Compact taxon and lineage. |
| `TaxonDisplayItem` | `taxon_id`, `rank`, `names` | One lightweight accepted-name display node. |
| `TaxonDisplaySummary` | `current_rank`, `items` | Family-to-current display path, or only the current node above family. |
| `TaxonChild` | `taxon_id`, `rank`, `names` | Compact direct child. |
| `TaxonNameDetail` | `name_id`, `name`, `authority_year`, `source` | One stable name record. |
| `TaxonNamesDetail` | `sci_name`, `synonyms`, `zh_name`, `zh_aliases`, `en_name`, `en_aliases` | Names grouped by type. |
| `TaxonDetail` | `taxon_id`, `rank`, `parent_taxon_id`, `breadcrumb`, `geological_range`, `names` | Complete editable taxon and ancestor lineage. |

| Function | Parameters after `database` | Return |
| --- | --- | --- |
| `get_taxon_summary` | `taxon_id: i64` | `Option<TaxonSummary>` |
| `get_taxon_display_summary` | `taxon_id: i64` | `Option<TaxonDisplaySummary>` |
| `get_taxon_detail` | `taxon_id: i64` | `Option<TaxonDetail>` |
| `list_taxon_children` | `taxon_id: i64`, `cursor: Option<&str>`, `limit: usize` | `TaxonomyPage<TaxonChild>` |

`TaxonDetail.breadcrumb` lists ancestors from the highest available rank to
the immediate parent. It does not repeat the current taxon. Children are
loaded separately with `list_taxon_children`; they are not embedded in the
detail response.

`TaxonDisplaySummary` is independent from the complete `TaxonSummary`. It
contains accepted scientific, Chinese, and English names only. Family, genus,
and species targets include the available path from family through the target;
kingdom and order targets include only the target itself.

## Search

`TaxonSearchResult` contains `taxon_id`, `rank`, accepted display `names`, and
the `TaxonNameMatch` records responsible for the match. `TaxonSuggestion` is
the compact autocomplete type with the same taxon identity fields.

| Function | Parameters after `database` | Return |
| --- | --- | --- |
| `search_taxa` | `query: &str`, `limit: usize` | `Vec<TaxonSearchResult>` |
| `suggest_taxa` | `query: &str`, `limit: usize` | `Vec<TaxonSuggestion>` |

Both interfaces use the same canonical normalization and ranked order:
exact, full prefix, word prefix, substring, then fuzzy. Blank input returns
an empty vector. Candidates from every eligible match level are combined
before one best matching name is selected per `taxon_id`; distinct taxa are
then globally ranked and the requested limit is applied. A stronger match
therefore ranks first without suppressing lower-tier matches that fit within
the final limit. A Chinese genus query ending in `属` also admits its stem as a
lower-ranked fuzzy candidate when the stem is at least three characters long;
the complete query keeps its stronger exact, prefix, and substring ranking.
These taxonomy-only interfaces remain available when the active photo library
is offline. The desktop `search_taxa` and `suggest_taxa` commands execute
database lookups on blocking workers and resolve asynchronously. The desktop
`get_taxon_detail` command uses the same execution model and reports a missing
taxon as an error.

## Formatted update

### `TaxonInputRow`

| Field | Type | Description |
| --- | --- | --- |
| `kingdom` | `Option<String>` | Kingdom scientific name. |
| `order` | `Option<String>` | Order scientific name. |
| `family` | `Option<String>` | Family scientific name. |
| `genus` | `Option<String>` | Genus scientific name. |
| `species` | `Option<String>` | Species scientific name. |
| `authority_year` | `Option<String>` | Authority text supplied in the row. |
| `synonyms` | `Vec<String>` | Ordered raw scientific synonym strings. |
| `zh_name` | `Option<String>` | First Chinese input name. |
| `zh_alias` | `Vec<String>` | Additional Chinese input names. |
| `en_name` | `Option<String>` | First English input name. |
| `en_alias` | `Vec<String>` | Additional English input names. |
| `geological_range` | `Option<String>` | Geological range for the target taxon. |
| `source` | `Option<String>` | Source applied to supplied names on the target taxon when allowed. |

The CSV columns are:

```text
kingdom,order,family,genus,species,authority_year,synonyms,zh_name,zh_alias,en_name,en_alias,geological_range,source
```

CSV is UTF-8 and requires a header. Its application-wide delimiter is comma by
default and may be configured as comma, semicolon, tab, or pipe. Columns may be
omitted or reordered, but every row must have the same field count as the
header. Multi-name cells use the separate configured one-character name
separator.

When `species` is present and `genus` is blank, normalization copies the first
word of the species scientific name into `genus`. Every later step therefore
treats that row exactly like a row that supplied both fields.

Matching starts at the lowest supplied rank. The rank name and then each input
synonym are considered in input order. For each input name, exact `sci_name`
matches are used when present; `synonym` is queried only when that name has no
`sci_name` candidate. Matching uses the case-sensitive stored `name`, not
`normalized_name`. Zero candidates means the target is new; one candidate
selects it immediately without checking any supplied ancestor. Multiple
candidates are narrowed using supplied ancestor names from the nearest rank
upward, stopping as soon as one remains. Ancestor matching uses the same
`sci_name`-first, `synonym`-fallback rule. After a target is selected or
created, the other supplied scientific names are appended or supplemented as
target synonyms.

The row source is stored only on supplied name records for the target taxon.
Scientific names used to resolve or create its lineage have independent source
values.

Updating a selected target keeps the existing supplement, append, and
overwrite behavior. Creating a target requires its strict parent-rank name.
That parent is resolved with the same exact `sci_name`-first fallback and the
same zero, one, or multiple-candidate rules: one candidate is reused without
consulting higher ranks, zero candidates creates that parent, and multiple
candidates are narrowed by the nearest supplied ancestor. Creating a missing
parent recursively requires and resolves its own strict parent, so one
formatted row can create every missing rank in a complete supplied lineage.
Missing strict-parent input and unresolved ambiguity fail the row without
applying a partial lineage.

### Preview and apply types

`TaxonRowStatus` may include `no_change`, `supplement`, `new_name`,
`new_taxon`, `overwrite`, `invalid`, `not_matched`, and
`multiple_candidates`. One row may contain multiple statuses.

`TaxonRowOutcome` contains `row_number`, `operation_types`, target `summary`,
optional `parent`, field-level `changes`, and `message`.

`TaxonomyPreviewResult` contains `delimiter`, `encoding`, and `rows`.
`TaxonomyOperationResult` additionally contains `operation_id`, row totals,
and the same ordered row log.

### Formatted update interfaces

| Function | Parameters after `database` | Return |
| --- | --- | --- |
| `taxonomy_formatted_update_template` | none | UTF-8 header using the configured CSV delimiter `String` |
| `parse_taxonomy_input_csv` | `input: &str` | `Vec<TaxonInputRow>` |
| `preview_rows` | `rows: &[TaxonInputRow]` | `TaxonomyPreviewResult` |
| `prepare_rows` | `rows: &[TaxonInputRow]` | `PreparedTaxonomyUpdate` |
| `apply_rows` | `rows: &[TaxonInputRow]` | `TaxonomyOperationResult` |
| `apply_prepared_rows` | `prepared: PreparedTaxonomyUpdate` | `TaxonomyOperationResult` |
| `taxonomy_log_csv` | `rows: &[TaxonRowOutcome]` | UTF-8 CSV using the configured delimiter `String` |
| `get_taxonomy_name_separator` | none | `String` |
| `set_taxonomy_name_separator` | `separator: &str` | `()` |

The desktop Formatted Update preview and apply commands return operation
handles. Both keep owner-scoped cancellation while `OperationManager` records
their lifecycle and result. The page waits for the exact returned task ID.

`prepare_rows` evaluates and validates the update in a rolled-back transaction,
retaining its inputs, row outcomes, taxonomy changeset, taxonomy identity, and
operation revision in one in-memory candidate. `apply_prepared_rows` rejects a
stale revision and otherwise applies that precomputed changeset before writing
the operation, audit, replayable input, and synchronization event. It does not
reprocess the formatted rows.

The desktop `preview_taxonomy_rows` command replaces the prior in-memory
candidate and returns `preview_id`, `delimiter`, `encoding`, and `rows`.
`apply_taxonomy_rows` accepts only that `preview_id`; the candidate is consumed
by the attempt, so another apply or an apply after editing requires a new
preview. Both commands execute database work on blocking workers and resolve
asynchronously.

## Direct UI changes

`PromoteTaxonNameInput` and `DeleteTaxonNameInput` both identify a name by
`taxon_id` plus stable `name_id`.

Promotion exchanges the selected alias or synonym with its accepted-name
record. A species synonym does not need to begin with the accepted scientific
name of the parent genus.

`SaveTaxonNameGroupInput` saves one of the six `TaxonomyNameType` groups. Its
`updates` entries identify existing records by `name_id` and replace their
`authority_year` and `source`; `null` clears the corresponding value. Its
`additions` entries contain `name`, `authority_year`, and `source`. A primary
group accepts at most one name, and an alias or synonym can be added only when
its accepted-name group is present. New species scientific names and synonyms
must start with the accepted scientific name of the parent genus. Blank,
family-duplicate, mismatched-group, and otherwise invalid additions are
rejected without applying any part of the group save.

| Function | Parameters after `database` | Return | Description |
| --- | --- | --- | --- |
| `promote_taxon_name` | `input: PromoteTaxonNameInput` | `()` | Exchange an alias type with its accepted type. |
| `save_taxon_name_group` | `input: SaveTaxonNameGroupInput` | `()` | Atomically update metadata and append records in one name group. |
| `delete_taxon_name` | `input: DeleteTaxonNameInput` | `()` | Delete an alias, synonym, or localized accepted name whose alias group is empty. Scientific accepted names cannot be deleted. |
| `delete_taxon` | `taxon_id: i64` | `()` | Delete a childless taxon. |

Group saves, promotion, and deletion create rollbackable audit operations
without formatted input. Desktop direct-change commands execute database work
on blocking workers and resolve asynchronously before scheduling taxonomy
synchronization.

## Custom SQL

Custom SQL and SQL Import use persistent SQL inputs. `SqlInputKind` is `csv`
or `sqlite`. SQLite inputs are read through `alias.object`; CSV inputs are
read as a table named by the alias. Aliases must be valid SQLite identifiers
and cannot use reserved database names. CSV source inspection and loading use
the application-wide CSV delimiter.

| Type | Fields | Description |
| --- | --- | --- |
| `AddSqlInputRequest` | `kind`, `alias`, `path` | Register a source and create a managed copy. |
| `AddSqlInputResult` | `input`, `inputs`, `warnings` | Added source, authoritative source list, and noncritical cleanup warnings. |
| `RemoveSqlInputRequest` | `alias` | Identify one source in the selected workflow. |
| `PersistentSqlInput` | `kind`, `alias`, `original_path`, `available`, `schema` | Registered source, managed-copy availability, and inspected schema. |
| `RemoveSqlInputResult` | `inputs`, `warnings` | Authoritative remaining sources and noncritical cleanup warnings. |

Custom SQL and SQL Import keep separate source registries. Sources persist
across database reopen until explicitly removed. Once registry removal
commits, managed-file cleanup errors are warnings and do not change success.

`CustomTaxonomySqlRequest` contains an SQL script and optional
`maximum_result_rows`. `CustomTaxonomySqlExportRequest` contains the executed
script, a 1-based executable `statement_index`, and an absolute
`destination_path`.

### SQL result types

| Type | Fields | Description |
| --- | --- | --- |
| `CustomSqlExecutionResult` | `operation_id`, `changeset_size`, `result_sets`, `messages`, `script_saved`, `warnings` | Committed SQL result and independent script-save status. |
| `SqlResultSet` | `statement_index`, `columns`, `rows`, `truncated` | One statement result. |
| `SqlColumn` | `name`, `declared_type` | Result or source column metadata. |
| `SqlStatementMessage` | `statement_index`, `affected_rows`, `message` | Per-statement execution summary. |
| `SqlExportResult` | `path`, `row_count` | Completed streaming CSV export. |

`SqlValue` is tagged by `type` and has the variants `null`, `integer`, `real`,
`text`, and `blob`. Blob values use Base64 in both IPC results and CSV export.
`maximum_result_rows` defaults to 1000 and is capped at 1000. Read-only result
previews retain at most that many rows; this preview limit is independent of
the execution limit. Statements that may mutate data always run to completion,
including `UPDATE ... RETURNING`, while retaining only the limited preview rows.

`SqlSourceSchema` contains `alias` and `objects`. Each `SqlSourceObject`
contains `name`, `object_type`, and ordered `columns`. `SqlObjectType` values
are `table`, `view`, and `virtual_table`.

| Function | Parameters after `database` | Return |
| --- | --- | --- |
| `get_custom_taxonomy_sql` | none | Last successful SQL, or the initial built-in script `String` |
| `list_custom_sql_inputs` | none | `Vec<PersistentSqlInput>` |
| `list_custom_sql_database_schemas` | none | `Vec<SqlSourceSchema>` for the readable taxonomy database |
| `add_custom_sql_input` | `request: &AddSqlInputRequest` | `AddSqlInputResult` |
| `remove_custom_sql_input` | `request: &RemoveSqlInputRequest` | `RemoveSqlInputResult` |
| `execute_custom_taxonomy_sql` | `request: &CustomTaxonomySqlRequest` | `CustomSqlExecutionResult` |
| `export_custom_taxonomy_query` | `request: &CustomTaxonomySqlExportRequest` | `SqlExportResult` |

Custom SQL may read taxonomy tables, views, search structures, history, and
internal metadata, but it may directly mutate only `taxa` and `taxon_names`.
Schema changes, transaction control, attachment control, internal-table
writes, unsafe pragmas, and extension loading are denied while each statement
executes.

Custom SQL accepts one or more executable statements and runs them sequentially
in one transaction. Each executable statement has a 30-second execution limit
and may fail through cancellation, timeout, SQLite execution, or validation.
Whitespace and comments do not increment the 1-based statement indexes retained
by result sets and messages. The script succeeds or fails as one unit and the
resulting taxonomy must remain valid. A pure query creates no operation. A
successful mutation script creates one rollbackable operation, retains the
exact executed SQL as operation input, and records photo-library
synchronization. Mutations are validated before commit. Small affected
dependency scopes use incremental validation, while larger scopes use full
taxonomy validation. A validation failure prevents the transaction from
committing. SQL is saved to the current workspace only after prepare,
execution, and transaction commit succeed. Historical operation input remains
independent of that workspace save. A script-save failure returns the committed
execution with `script_saved = false` and a warning; an execution failure does
not replace the last successful SQL.

Export locates one result-producing read-only statement by its executable index,
executes only that statement, and streams its complete rows to the destination
CSV using the application-wide delimiter. Each export query has a 30-second
execution limit and supports cancellation. Successful exports retain the full
CSV. If output creation has started, an unsuccessful export removes its partial
output; validation errors raised before output creation leave an existing
destination unchanged. Export re-executes the query against the current
taxonomy and input sources rather than materializing a database snapshot.

Desktop SQL execution and query export return operation handles and run file
and database work on blocking workers. They keep owner-scoped cancellation and
report input preparation, statement execution, changeset generation, taxonomy
validation, operation recording, and commit or finalization phases through
`OperationManager`.
`list_custom_sql_database_schemas` returns the complete non-SQLite-internal
table and view catalog exposed through the `main` alias. Managed input schemas
remain available from `list_custom_sql_inputs`; clients combine both sources
when presenting every table accessible to Custom SQL.

## Taxonomy imports

`TaxonomyImportMetadata` contains `source_path`, `taxa_count`,
`taxon_names_count`, and `imported_at`.
`TaxonomyImportResult` contains the resulting `metadata` and noncritical
cleanup `warnings`.

### SQL Import types

| Type | Fields |
| --- | --- |
| `ValidateSqlImportRequest` | `sql` |
| `SqlImportExecutionResult` | `statements_executed`, `messages`, `script_saved`, `warnings` |
| `NameTypeCount` | `name_type`, `count` |
| `SqlImportIssue` | `code`, `message`, `taxon_id`, `related_taxon_id`, `table`, `row_identifier` |
| `SqlImportValidationResult` | `valid`, `can_apply`, `taxa_count`, `name_counts`, `normalization_changes`, `total_warning_count`, `total_error_count`, `warnings`, `errors` |
| `ValidateSqlImportResult` | `execution`, `validation`, `warnings`, `can_apply` |

Validation returns at most 100 warning and error samples while the total count
fields remain authoritative.

### SQL Import interfaces

| Function | Parameters after `database` | Return |
| --- | --- | --- |
| `get_sql_import_sql` | none | Last successful SQL, or the initial built-in script `String` |
| `list_sql_import_inputs` | none | `Vec<PersistentSqlInput>` |
| `list_sql_import_database_schemas` | none | `Vec<SqlSourceSchema>` for the current taxonomy database |
| `list_sql_import_staging_schemas` | none | `Vec<SqlSourceSchema>` for the current staging database, or an empty vector |
| `add_sql_import_input` | `request: &AddSqlInputRequest` | `AddSqlInputResult` |
| `remove_sql_import_input` | `request: &RemoveSqlInputRequest` | `RemoveSqlInputResult` |
| `validate_sql_import` | `request: &ValidateSqlImportRequest` | `ValidateSqlImportResult` |
| `validate_sql_import_with_progress` | `request: &ValidateSqlImportRequest`, `progress: &mut FnMut(OperationProgress)` | `ValidateSqlImportResult` |
| `apply_sql_import` | none | `TaxonomyImportResult` |
| `apply_sql_import_with_progress_and_cancellation` | `progress: &mut FnMut(OperationProgress)`, `cancellation: &CancellationToken` | `TaxonomyImportResult` |

SQL Import has one fixed workspace. Persistent inputs and the last successful
SQL outlive tabs, application restarts, and successful Apply. Adding or
removing an input, validating new SQL, or recreating staging
invalidates the prior staging-dependent candidate and validation state.
Removal is rejected while another operation holds the workspace lock.
`list_sql_import_database_schemas` exposes the current taxonomy catalog through
the read-only `taxonomy` alias for the internal-database catalog. Managed input
schemas remain available from `list_sql_import_inputs` and are presented only
with input sources. `list_sql_import_staging_schemas` separately exposes the
`sql_import` staging catalog when staging exists so clients can present it with
input sources without duplicating it in the internal-database catalog.
The built-in script imports retained scientific synonyms except rows equal to
the same taxon's accepted scientific name.

Validate first executes SQL Import SQL, which can read the current taxonomy
through the read-only `taxonomy` alias and every managed input through its
registered alias. The script can attach only the backend-selected
`vividarium_sql_import.db` path with the `sql_import` alias. It may create and
mutate staging objects only in `sql_import`; the execution result never creates
a taxonomy operation or returns Custom SQL result sets. It reports only
per-statement messages or a syntax/runtime error. Each SQL Import statement has
a 90-second execution limit and supports cancellation. Statement failure or
timeout ends execution. Staging finalization and validation run only when SQL
execution succeeds; failures produce no applicable candidate. If SQL execution
fails, times out, or is cancelled, the SQL Import workspace is restored to its
pre-run staging, candidate, and validation state. Script persistence is
attempted separately after a successful commit. Save failure is reported through
`script_saved = false` and `warnings` without changing the execution result.

After SQL succeeds, validation checks staging data, builds the candidate, and
performs one authoritative candidate validation covering file integrity,
required schema and constraints, supported ranks and name types, canonical
normalization, name-family uniqueness, and the complete taxonomy invariants.
Stored and normalization-derived name-family duplicates are reported once as
`duplicate_canonical_name`.
Taxonomy data violations are returned with `valid = false`,
`can_apply = false`, and structured issues; SQL, SQLite, file, and
candidate-build failures remain interface errors. The result contains
authoritative totals and bounded warning and error samples. Any later source
or SQL change requires validation again.

The progress callback reports stable stages for input preparation, SQL
execution, staging finalization, staging fingerprinting, staging integrity and
schema checks, name normalization, staging taxonomy validation, candidate taxa
and name construction, candidate database validation, and the terminal
validation result. During SQL execution, `current` is the active one-based
statement index and `total` is the number of executable statements.
Fingerprinting uses the staging file size and bytes read. Name work uses name
counts, and candidate taxa report their final taxon count after the bulk insert.
Missing counts mean that only the active stage is known; they do not represent a
percentage.

`apply_sql_import` accepts only the latest successfully validated candidate.
Its progress callback reports candidate validation, staging fingerprint bytes,
and applying immediately before taxonomy replacement.
Successful replacement assigns a new taxonomy identity, clears taxonomy
history, and marks every registered photo library for a full remap. It removes
staging, candidate, and validation artifacts while retaining inputs and SQL.
Post-commit cleanup failures are warnings and are queued for later retry. A
failed validation or replacement leaves the current taxonomy unchanged.

### Direct Import types

| Type | Fields |
| --- | --- |
| `DirectImportDatabase` | `source_path`, `schema` |

`schema` is a `SqlSourceSchema` containing the inspection alias and every
visible table or view with its columns.

### Direct Import interfaces

| Function | Parameters after `database` | Return |
| --- | --- | --- |
| `get_taxonomy_import_metadata` | none | `Option<TaxonomyImportMetadata>` |
| `inspect_direct_import_database` | `source_path: &Path` | `DirectImportDatabase` |
| `apply_direct_import` | `source_path: &Path` | `TaxonomyImportResult` |
| `apply_direct_import_with_progress_and_cancellation` | `source_path: &Path`, `progress: &mut FnMut(OperationProgress)`, `cancellation: &CancellationToken` | `TaxonomyImportResult` |

The supplied SQLite file must contain valid `taxa` and `taxon_names` data
using the current schema. Imported names pass through the shared canonical
normalizer and must be unique within each taxon's scientific, Chinese, and
English name families. `inspect_direct_import_database` validates the file,
rejects the active taxonomy database as an input, and returns its canonical
path and schema without changing application data. `apply_direct_import`
repeats validation so a file changed after inspection cannot bypass the
checks.
Successful replacement creates a new taxonomy identity, clears
taxonomy user history, and causes every registered photo library to rebuild
mapping state when synchronized. Immediate synchronization and mapping of the
active photo library are best-effort follow-up work; no active library or an
unavailable library does not change a successful replacement result.

The desktop Direct Import inspection and apply commands accept
`source_path: String` and return `direct_import` operation handles. Inspection
completes with `DirectImportDatabase`. Apply reports validation and applying at
the core operation boundaries, completes with `TaxonomyImportResult`, returns
validation or file failures through the operation error, and schedules
taxonomy/photo synchronization only after replacement commits.

## Photo-library synchronization

`TaxonomySyncResult` contains `library_uuid`, `sync_id`,
`queued_photo_count`, and `full_remap`.

`TaxonomySyncRun` contains successful per-library `synchronized` results and
`pending_library_uuids` for libraries that remain unavailable or invalid.

| Function | Parameters after `database` | Return |
| --- | --- | --- |
| `synchronize_pending_photo_libraries` | none | `TaxonomySyncRun` |

Taxonomy mutations commit their data, operation audit, and synchronization
request together, then return without opening photo-library databases.
`synchronize_pending_photo_libraries` processes the active library first,
merges repeated affected-taxon requests, and lets a full-remap request replace
local requests. Its return value separates successful libraries from
unavailable or invalid libraries that remain pending. A library failure never
changes the already successful taxonomy mutation. Registering or activating a
library also checks whether a full remap is required.

## Taxonomy operation interfaces

The taxonomy module exports the common interfaces below:

| Function | Parameters after `database` | Return |
| --- | --- | --- |
| `list_operations` | `cursor: Option<&str>`, `limit: usize` | `OperationPage<OperationSummary>` |
| `list_operation_audit` | `operation_id: i64`, `cursor: Option<&str>`, `limit: usize` | `OperationPage<OperationAuditRow>` |
| `get_operation_input` | `operation_id: i64` | `Option<OperationInput>` |
| `write_operation_audit` | `operation_id: i64`, `writer: &mut W` where `W: Write` | `()` |
| `write_operations_audit` | `operation_ids: &[i64]`, `writer: &mut W` where `W: Write` | `()` |
| `write_all_operation_audit` | `writer: &mut W` where `W: Write` | `()` |
| `rollback_operation` | `operation_id: i64` | `()` |

Taxonomy adds formatted input export:

| Function | Parameters after `database` | Return |
| --- | --- | --- |
| `export_operation_input` | `operation_id: i64` | Formatted-update CSV `String` |
| `export_operations_input` | `operation_ids: &[i64]` | Formatted-update CSV `String` |
| `export_all_replayable_inputs` | none | Formatted-update CSV `String` |

Selected input export fails if any requested operation has no formatted input;
it never silently skips unsupported operations. Successful rollback applies
the reverse changeset atomically, verifies foreign-key integrity and taxonomy
validity, records pending photo-library synchronization, and deletes the
original operation with its audit, changeset, and input records. A conflict
leaves both taxonomy and operation records unchanged. Desktop history
pagination, rollback, audit
export, and formatted-input export execute database and file work on blocking
workers and resolve asynchronously. Desktop audit and formatted-input export
commands accept an absolute destination path and write the CSV after the caller
selects that path. Audit and replayable-input exports use the application-wide
CSV delimiter.
