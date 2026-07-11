# PhytoIndex Desktop

This package contains the React interface and Tauri desktop adapter for PhytoIndex.

## Source Layout

```text
src/
  App.tsx                  Top-level navigation and screen routing
  bridge.ts                Typed React-to-Rust IPC and dialog bridge
  components/              Shared navigation, status, virtual list, and viewer
  features/                Admin, Photos, Taxonomy, and Map screens
  lib/                     Browser, path, storage, taxon, and split helpers
  styles/                  Global, layout, component, and feature styles
src-tauri/
  capabilities/            Narrow desktop permissions
  icons/                   Icon source and generated platform formats
  src/                     Tauri IPC, state, paths, and media protocol
  tauri.conf.json          Shared application and bundle configuration
  tauri.macos.conf.json    Apple Silicon DMG and ad-hoc signing settings
  tauri.windows.conf.json  Windows NSIS and WebView2 settings
```

The interface never receives arbitrary file-system privileges. Photos are requested by database ID through the private `phytoindex://` protocol, and Rust validates the configured photo root before reading a file.

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
