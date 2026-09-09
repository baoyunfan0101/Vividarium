# API Modules

Location: `apps/desktop/src/api`

API modules provide typed calls from feature code to Tauri commands. Each file
owns one backend domain, its request types, and response types.

## Modules

| Module | Interface group |
| --- | --- |
| `photos` | Photo records, directories, refresh, search, rename, metadata, media URLs, and file-manager reveal or open actions. |
| `mapping` | Mapping states, candidates, assignment, remapping, mapping queues, and photographed-taxonomy browsing. |
| `taxonomy` | Taxon views, search, suggestions, children, taxon photos, formatted input, staged preview tokens, and prepared apply. |
| `operations` | Photo and taxonomy operation summaries, audit pages, rollback, and single or selected-operation CSV exports to an absolute destination path. |
| `customSql` | Custom SQL execution, result sets, managed SQL inputs, readable database schemas, and full-query export. |
| `sqlImport` | SQL Import workspace, validation, apply, managed inputs, readable taxonomy schemas, and staging schemas. |
| `directImport` | Direct Import database inspection and confirmed replacement. |
| `taxonomyImport` | Shared taxonomy import metadata and result types. |
| `general` | Application-wide theme, workspace, search, taxon-tree display, and CSV delimiter settings. |
| `storage` | Database locations, Taxonomy Database selection, Photo Library activation results, registration lifecycle, and file-manager opening for displayed storage paths. |
| `settings` | Naming settings, Rhai hooks, hook tests, and taxonomy name separator. |
| `map` | Map settings, viewport bounds, and geotagged photo pages. |
| `tasks` | Task-keyed Background status, progress state, recovery snapshots, and exact-task completion waiting for foreground import workflows. |
| `updater` | Application version, update check, and installation. |
| `dialogs` | Native file, directory, and destination selection. |
| `external` | Project and author contact constants plus scoped system URL opening. |
| `common` | Cursor page shape, error text, byte formatting, and browser CSV download. |

## Call contract

Exported API functions accept domain values rather than UI events and return
typed promises. Cursor interfaces use:

```ts
type Page<T> = {
  items: T[];
  next_cursor: string | null;
};
```

The first request passes a null cursor. Subsequent requests pass the returned
`next_cursor` without interpreting it. A null next cursor means the page is
complete.

Mutating calls return the committed backend result or an operation handle.
Feature code uses the returned state as authoritative and presents warnings
without converting a committed operation into a failure.

External links call the Tauri opener only in the desktop runtime. The desktop
capability limits URL opening to the exact project repository and author email
values; browser development uses the browser's ordinary external navigation.

SQL Import validation returns a `sql_import` operation handle. Its structured
progress contains a machine-readable stage, optional current and total values,
and an optional progress unit. Statement execution uses `statements`, staging
fingerprinting uses `bytes`, name processing uses `names`, and candidate taxon
construction uses `taxa`. Integrity, schema, and taxonomy checks are
indeterminate stages when no reliable total is available.
For statement execution, `current` is the active one-based statement index and
`total` is the number of executable statements. For example, `current = 2`,
`total = 5`, and `unit = statements` identifies statement 2 of 5. Apply reports
candidate validation, staging fingerprint bytes, and
the applying stage at the corresponding core operation boundaries.
The completed result distinguishes a valid candidate from structured taxonomy
validation issues; execution failures remain operation errors.
Direct Import inspection and apply return `direct_import` operation handles.
Inspection completes with the normalized path plus table and column metadata
without replacing the taxonomy. Apply completes with `TaxonomyImportResult` and
reports validation and applying at the core replacement boundaries. Schema,
integrity, and taxonomy validation failures are operation errors and leave the
current taxonomy unchanged.

Custom SQL execution and export plus Formatted Update preview and apply return
operation handles. Their owner IDs remain registered for tab-close
cancellation while `OperationManager` owns Background visibility and results.
The foreground reads the result from the exact returned task ID.

Photo Library open, register, switch, and rebind calls return
`PhotoLibraryActivation<T>`, which contains `library` and the first scheduled
`operation`. Activation enqueues task-keyed Photo Scan, Metadata Index, and
Photo Mapping stages. Refresh, mapping, and bulk rename return operation handles
immediately and continue through the unified Background source. Task status
includes `task_id`, `task_kind`, `task_scope`, and queued/running/completed/failed
state. Live Background state comes from `operation-progress`; status fetches are
used for startup and recovery. Callers that need a completed foreground import
result resolve that exact task rather than a module slot. Audit details are
loaded from Rename History.

`OperationState.state` is the task lifecycle source. Running task progress is
read exclusively from `OperationState.progress`; completed and failed tasks use
their timestamps, result, and optional error. Determinate progress has both
`current` and `total`. For statement execution, `current` identifies the active
one-based statement; for other countable work, it represents accumulated
progress. Work without a reliable total is indeterminate. The unit is one of
items, files, photos, names, taxa, bytes, or statements.
