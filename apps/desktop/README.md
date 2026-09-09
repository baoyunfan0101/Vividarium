# Vividarium Desktop

This package contains the React interface and Tauri desktop adapter for Vividarium.

## Source Layout

```text
src/
  App.tsx                  Top-level navigation and screen routing
  api/                     Typed React-to-Rust IPC wrappers
  app/                     Desktop shell, workspace, and application state
  features/                Admin, Photos, Taxonomy, and Map screens
  shared/                  Shared UI, layout, and interaction utilities
  styles/                  Global, layout, component, and feature styles
src-tauri/
  capabilities/            Narrow desktop permissions
  icons/                   Generated platform icon formats
  src/                     Tauri IPC, state, paths, and media protocol
```

The interface never receives arbitrary file-system privileges. Photos are requested by database ID through the private `vividarium://` protocol, and Rust validates the configured photo root before reading a file.

## Develop

```bash
npm ci
cargo tauri dev
```

## Build

Use the platform scripts from the repository root:

```bash
./scripts/build-macos.sh
```

```powershell
.\scripts\build-windows.ps1
```

See [../../docs/BUILDING.md](../../docs/BUILDING.md) for complete release instructions.
