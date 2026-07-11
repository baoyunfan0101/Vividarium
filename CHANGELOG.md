# Changelog

All notable changes to PhytoIndex are documented in this file.

## [2.0.0] - 2026-07-11

### Changed

- Replaced the Python application service with a Rust workspace.
- Replaced the separately served frontend with a Tauri 2 desktop shell.
- Kept the React and TypeScript interface while moving IPC and file access behind typed Tauri commands.
- Split reusable domain, SQLite, scanning, import, mapping, and export logic into `phytoindex-core`.
- Adopted the permanent application identifier `io.github.baoyunfan0101.phytoindex`.
- Added native Apple Silicon DMG and Windows x64 NSIS release pipelines.
- Added automatic WebView2 bootstrapping for Windows computers without the runtime.
- Added migration support for the version 1 database and thumbnail directory.

### Removed

- Removed the Python runtime, FastAPI service, PyInstaller configuration, and Python dependency files.
- Removed the separate frontend and backend top-level directories.

## [1.0.0]

- Initial Python and React desktop release.
