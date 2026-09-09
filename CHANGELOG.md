# Changelog

All notable changes to Vividarium are documented in this file.

## [3.1.0] - 2026-09-09

### Added

- Added taxonomy-aware photo display paths and mapping match provenance across photo and mapping views.
- Added clearer Custom SQL result navigation, export, and execution feedback.
- Added safer background-task ownership and cancellation behavior.

### Changed

- Made ordinary Settings autosave while keeping remapping and taxonomy-based renaming explicitly user-triggered.
- Improved taxonomy matching, name-family handling, hierarchy display, and mapping review UX.
- Improved photo headers, status bars, context menus, fullscreen behavior, keyboard navigation, and taxonomy display.
- Expanded Operation History with source-aware inputs and safer atomic rollback conflict handling.
- Standardized selectable and copyable record surfaces while keeping command controls non-selectable across photo, taxonomy, mapping, and history views.
- Upgraded Vividarium 3.0.0 schema-2 databases automatically to schema 3.
- Hardened the desktop release and updater pipeline with version, signature, manifest, and artifact validation.

### Fixed

- Fixed non-cancellable operations being dismissible while mutations were still running.
- Fixed taxonomy-based rename failures caused by missing mapping provenance storage after upgrade.
- Fixed stale Settings save/test status and cross-Hook async result presentation.
- Fixed several Custom SQL result-layout and multi-statement execution issues.

## [3.0.0] - 2026-08-14

### Added

- Added multiple independently registered Photo Libraries with root and database rebind, relocation, and availability recovery.
- Added global photo search, recent searches, Photo Sets, photographed Taxon Tree browsing, thumbnail grids, full-image mode, and keyboard navigation.
- Added MapLibre browsing with aggregate initial bounds, retained viewport, viewport-paged markers, and reusable photo previews.
- Added persistent photo mapping states, ambiguous candidates, explicit mapping overrides, filename remapping, and photographed-taxonomy usage trees.
- Added taxonomy autocomplete, complete ranked search, hierarchy navigation, inline six-group name editing, promotion, deletion, and childless taxon deletion.
- Added preview-first Formatted Update with prepared apply, progressive lineage creation, accepted-or-synonym matching, strict-parent validation, template download, and rule help.
- Added persistent Custom SQL inputs, readable internal schemas, typed result sets, safe mutation execution, and complete read-only query export.
- Added staged SQL Import and validated Direct Import taxonomy replacement workflows.
- Added configurable application theme, workspace restoration, recent-search limit, taxon-tree display fields, mapping priority, filename output fields, taxonomy name separator, CSV delimiter, map provider, and Rhai hook project tests.
- Added selectable Rename History and Taxonomy History with formatted audit JSON, batch audit export, replayable taxonomy input export, and rollback.
- Added signed in-app update checks and installation from Vividarium GitHub Releases.
- Added native file, folder, database, external repository, and author email actions.
- Added automatic retryable Photo Scan, Metadata Index, and Photo Mapping tasks without eager thumbnail generation.

### Changed

- Reorganized the application into React feature domains, typed frontend API wrappers, thin Tauri adapters, and a Tauri-independent `vividarium-core` service crate.
- Separated application metadata, taxonomy, and each Photo Library into distinct SQLite roles so one taxonomy can serve several libraries.
- Replaced unbounded collection loads with cursor pages, virtualized lists, and adjustable workbench panes.
- Made long-running mapping, refresh, import, SQL, and update work report structured event-driven background progress without blocking the interface.
- Scoped photo indexing, refresh, mapping, and bulk rename loading states to photo-dependent tabs while unrelated work remains interactive.
- Standardized tab names, icons, compressed tab layout, status isolation, context-menu icons, button feedback, and shared photo interactions.
- Kept SQLite schema version `2`; databases with other schema versions remain incompatible and are rejected.

### Fixed

- Fixed taxonomy searches that dropped lower-priority or Chinese fuzzy matches before pagination.
- Fixed stale hierarchy positions being reused after a new formal search.
- Fixed filename-format-only settings changes unnecessarily queueing every photo for remapping.
- Fixed map initialization, tab viewport retention, marker preview reuse, and preview hover rendering.
- Fixed Photo Detail metadata layout, scrolling, image aspect transitions, and context-menu access.
- Fixed SQL source sidebar grouping, schema duplication, scrolling, and collapsed-content flicker.
- Fixed history row hover layering, selection coverage, oversized errors, JSON alignment, and code-block rendering.
- Fixed project hook tests, long settings paths, Direct Import prompts, and several long-running actions that previously lacked visible feedback.
- Fixed active and unavailable Photo Library removal, cross-library thumbnail collisions, and library switching during photo work.

### Removed

- Removed the legacy frontend bridge, old V3 parallel structure, unpaged all-photo loading, generic raw-table export commands, and the earlier Base Import naming.

## [2.1.0] - 2026-07-12

### Added

- Added explicit warnings when indexed photo files are unavailable on disk.
- Added Tianditu as an optional map tile provider for networks where OpenStreetMap tiles are unavailable.
- Added local map provider metadata and masked Tianditu application-token configuration.

### Changed

- Refreshed the application logo assets.
- Improved progress reporting for long-running photo, taxa, and mapping operations.
- Kept map provider credentials in local configuration and excluded them from source control.

### Fixed

- Fixed Windows rebuild operations that appeared stalled or caused the interface to become unresponsive.
- Hid the Windows extended-path prefix in displayed photo-root paths.
- Preserved photo markers and displayed a clear configuration error when a base map cannot be configured.

## [2.0.0] - 2026-07-11

### Changed

- Replaced the Python application service with a Rust workspace.
- Replaced the separately served frontend with a Tauri 2 desktop shell.
- Kept the React and TypeScript interface while moving IPC and file access behind typed Tauri commands.
- Split reusable domain, SQLite, scanning, import, mapping, and export logic into `vividarium-core`.
- Adopted the permanent application identifier `io.github.baoyunfan0101.vividarium`.
- Added native Apple Silicon DMG and Windows x64 NSIS release pipelines.
- Added automatic WebView2 bootstrapping for Windows computers without the runtime.

### Removed

- Removed the Python runtime, FastAPI service, PyInstaller configuration, and Python dependency files.
- Removed the separate frontend and backend top-level directories.

## [1.0.0]

- Initial Python and React desktop release.
