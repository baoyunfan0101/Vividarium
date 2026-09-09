# Vividarium Architecture

Vividarium separates UI composition, native desktop integration, and domain
logic so each layer can be understood and tested independently.

## Layers

```text
apps/desktop/src
  React pages, interactions, shared UI, and typed API wrappers
        |
        | Tauri IPC and events
        v
apps/desktop/src-tauri
  Native dialogs, updater, file-manager integration, media protocol,
  command adapters, and background-operation coordination
        |
        | typed Rust calls
        v
crates/vividarium-core
  Storage, photos, mapping, taxonomy, naming, map, and operation services
        |
        v
Local SQLite databases and photo files
```

## Frontend boundaries

`apps/desktop/src/app` owns tabs, navigation history, workspace restoration,
the activity bar, and application-wide status. Feature domains under
`apps/desktop/src/features` own user-facing workflows:

| Domain | Responsibility |
| --- | --- |
| `photos` | Folders, Taxon Tree, Photo Sets, map, photo media, detail, and file actions. |
| `mapping` | Mapping queues, candidates, and explicit photo-to-taxon assignment. |
| `taxonomy` | Search, hierarchy, name editing, formatted update, Custom SQL, SQL Import, and Direct Import. |
| `operations` | Photo and taxonomy history, audit display, export, and rollback. |
| `settings` | General metadata, storage, libraries, naming, hooks, map, import, update, and contact settings. |

Typed wrappers under `apps/desktop/src/api` are the only frontend layer that
knows Tauri command names. API modules do not import React. Shared UI and state
helpers do not import feature domains. Cross-domain navigation is passed as a
typed handler rather than performed directly by a feature.

## Desktop adapter boundaries

`apps/desktop/src-tauri` contains platform-specific behavior and thin command
adapters. `commands.rs` owns the shared adapter surface, while focused command
groups may live under `commands/` when a domain has an independent request and
response boundary. Command adapters validate desktop-only inputs, translate
errors to IPC strings, and delegate business behavior to `vividarium-core`.

Long-running work is registered with the desktop operation coordinator and is
reported through a shared task lifecycle and structured progress. Each
`OperationState` has an explicit queued, running, completed, or failed state.
`OperationProgress` contains a stable stage identifier, optional current and
total values, and an optional items, files, photos, names, taxa, bytes, or
statements unit. For statements, `current` identifies the active one-based
statement. For ordinary countable work, `current / total` represents accumulated
progress. A stage without a reliable total reports both values as absent and is
indeterminate. Taxonomy validation reports structure loading, parent-cycle,
parent-relationship, accepted-name, duplicate-name, orphan-name, and normalized-name
stages; its in-memory taxon passes report determinate taxa progress. `OperationManager`
owns every user-visible long-running task lifecycle, progress, exact result, and error.
`ActiveTaskRegistry` independently
owns owner/tab cancellation for tasks that stop when their tab closes. The
task-keyed status map is the single source for the bottom-right Background UI,
and foreground workflows wait for the exact returned `task_id`.
`BackgroundTaskScheduler` identifies
photo work by `(kind, scope)`, coalesces duplicate queued or running work, and
runs Photo Scan, Metadata Index, and Photo Mapping through one FIFO worker.
Queued work never blocks the queue head; only an incompatible operation that is
already running delays execution. Each stage uses bounded database batches and
yields between batches.
Photo Library lifecycle changes and task startup share one coordinator lock;
short foreground queries remain ordinary asynchronous commands. Native paths,
dialogs, the private
`vividarium://` media protocol, updates, and system application opening remain
outside the core crate.

## Core domain boundaries

`vividarium-core` has no Tauri dependency. Its public modules are grouped by
domain:

| Module | Responsibility |
| --- | --- |
| `storage` | Database locations and Photo Library registry. |
| `photos` | Indexing, browsing, metadata, thumbnails, rename, and availability. |
| `mapping` | Persistent mapping state, filename matching, candidates, and photographed taxonomy. |
| `taxonomy` | Search, hierarchy, mutations, imports, SQL, validation, synchronization, and taxonomy history. |
| `naming` | Name normalization, filename parsing, synonym parsing, Rhai hooks, and project tests. |
| `map` | Tile-provider settings, aggregate photo bounds, and viewport photo pages. |
| `operations` | Shared operation summaries, audit records, and cursor pagination. |

The core returns typed results and `CoreError`. It owns transactions and domain
invariants; the frontend does not reproduce them.

## Storage roles

Vividarium uses separate SQLite roles:

- The metadata database stores application settings, registered resources,
  workspace state, and durable cross-library synchronization events.
- The taxonomy database stores taxa, names, search structures, source
  metadata, and taxonomy operations.
- Each Photo Library database stores its directory tree, indexed photos,
  extracted metadata, thumbnail references, durable initial-index state,
  mapping state, and rename operations.

Vividarium 3.0.0 databases use schema 2. Vividarium 3.1.0 upgrades supported
schema-2 metadata, taxonomy, and Photo Library databases directly to schema 3.
Fresh databases use schema 3. The upgrade is forward-only, so databases opened
by 3.1.0 are not supported by Vividarium 3.0.0.

One taxonomy can therefore serve several independently registered Photo
Libraries. A taxonomy identity change schedules remapping for every registered
library without requiring all libraries to be online at the same time.
Thumbnail files live in library-UUID cache namespaces, and media requests carry
that identity so independently numbered photos cannot share cached content.

## Mutation flow

1. A feature calls one typed frontend API wrapper.
2. The Tauri command delegates to a core domain service or schedules a
   background operation.
3. The core commits one transaction and records audit state when applicable.
4. The desktop publishes queued, running, progress, completed, or failed state
   through `operation-progress`.
5. The frontend emits a domain mutation notification and refreshes only
   affected views.

Formatted taxonomy updates are preview-first. Preview prepares a candidate and
Apply consumes that prepared state, subject to taxonomy revision validation.
SQL Import and Direct Import also separate inspection or validation from the
final replacement action. SQL Import staging maintains validation-oriented
indexes independently from the final taxonomy database indexes.

## Verification

Backend domain behavior is covered by Rust workspace tests. Frontend pure
state and interaction helpers use Node tests, while TypeScript and production
bundling are checked by the desktop build.

```bash
cargo test --workspace --locked

cd apps/desktop
npm run test:desktop
npm run build
```
