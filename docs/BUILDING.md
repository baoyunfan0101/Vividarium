# Building and Releasing PhytoIndex

This guide describes the complete release process for PhytoIndex `v2.0.0`.

## Release Matrix

| Platform | Rust target | Bundle | Output directory | Signing model |
| --- | --- | --- | --- | --- |
| macOS Apple Silicon | `aarch64-apple-darwin` | DMG | `target/aarch64-apple-darwin/release/bundle/dmg/` | Free ad-hoc signature |
| Windows x64 | `x86_64-pc-windows-msvc` | NSIS EXE | `target/x86_64-pc-windows-msvc/release/bundle/nsis/` | Unsigned |

Development tools are required only on the build machine. Destination computers do not need Rust, Node.js, Python, npm, or SQLite.

The platform-specific Tauri settings are stored in:

- `apps/desktop/src-tauri/tauri.macos.conf.json`
- `apps/desktop/src-tauri/tauri.windows.conf.json`

Tauri merges the matching file into `tauri.conf.json` automatically.

## Common Prerequisites

- Git
- Rust 1.85 or newer
- Node.js 20 or newer and npm
- Tauri CLI 2

Install the Tauri CLI:

```bash
cargo install tauri-cli --version "^2.0" --locked
```

Both `Cargo.lock` and `apps/desktop/package-lock.json` are committed. Use `npm ci`, not `npm install`, for release builds.

## macOS Apple Silicon

### Build-machine prerequisites

- An Apple Silicon Mac
- macOS 11 or newer
- Xcode Command Line Tools

Install the Apple command-line tools if needed:

```bash
xcode-select --install
```

### Build

From the repository root:

```bash
./scripts/build-macos.sh
```

Equivalent manual commands:

```bash
cd apps/desktop
npm ci
cargo tauri build --target aarch64-apple-darwin --bundles dmg
```

The macOS configuration uses `signingIdentity: "-"`. This creates an ad-hoc signature without an Apple Developer account, certificate, or notarization.

### Verify

From the repository root:

```bash
codesign --verify --deep --strict --verbose=2 \
  target/aarch64-apple-darwin/release/bundle/macos/PhytoIndex.app

hdiutil verify \
  target/aarch64-apple-darwin/release/bundle/dmg/PhytoIndex_2.0.0_aarch64.dmg

shasum -a 256 \
  target/aarch64-apple-darwin/release/bundle/dmg/PhytoIndex_2.0.0_aarch64.dmg
```

Gatekeeper assessment is expected to reject this private build because it is not notarized.

### Install on another Mac

1. Open the DMG and drag PhytoIndex into Applications.
2. Try to open PhytoIndex once.
3. Open System Settings, then Privacy and Security.
4. Find the blocked PhytoIndex message and select Open Anyway.
5. Confirm with the local account password.

The exception is stored on that Mac and does not need to be repeated for each launch.

## Windows x64

### Build-machine prerequisites

- Windows 10 or 11 x64
- Microsoft C++ Build Tools with Desktop development with C++ selected
- WebView2 Runtime on the build machine
- PowerShell 5.1 or newer

Install Rust, Node.js, Tauri CLI, and the Microsoft build tools before building.

### Build

From PowerShell in the repository root:

```powershell
.\scripts\build-windows.ps1
```

Equivalent manual commands:

```powershell
Set-Location apps\desktop
npm ci
cargo tauri build --target x86_64-pc-windows-msvc --bundles nsis
```

Only the NSIS installer is built. MSI and WiX are intentionally not part of the release process.

The installer uses the WebView2 download bootstrapper. If WebView2 is absent, installation requires internet access and installs it automatically. After installation, PhytoIndex can run offline.

### Verify

```powershell
$Installer = "target\x86_64-pc-windows-msvc\release\bundle\nsis\PhytoIndex_2.0.0_x64-setup.exe"

Get-FileHash $Installer -Algorithm SHA256
Get-AuthenticodeSignature $Installer
```

`Get-AuthenticodeSignature` is expected to report `NotSigned` for this private release.

Test the installer on a clean Windows 10 or Windows 11 virtual machine with no Rust, Node.js, Python, or database software installed.

### Install on another Windows computer

1. Run `PhytoIndex_2.0.0_x64-setup.exe`.
2. If SmartScreen appears, select More info.
3. Select Run anyway.
4. Allow the installer to download WebView2 if requested.

Windows 11 Smart App Control can block unsigned software without an individual bypass. If that feature is enabled, turn it off for the private test machine or build the application directly on that machine.

## GitHub Actions Release

`.github/workflows/release.yml` builds both supported packages on native GitHub runners and attaches them to a GitHub release.

The workflow can be started manually from the Actions page or by pushing a matching version tag.

Before releasing, ensure these versions match:

- `Cargo.toml` under `[workspace.package]`
- `apps/desktop/package.json`
- `apps/desktop/package-lock.json`

Tauri reads its application version from `apps/desktop/package.json`.

Run the release checks:

```bash
cargo test --workspace

cd apps/desktop
npm ci
npm run build
```

Create and push the release tag:

```bash
git tag -a v2.0.0 -m "PhytoIndex v2.0.0"
git push origin v2.0.0
```

The workflow creates the `v2.0.0` GitHub release and uploads:

- `PhytoIndex_2.0.0_aarch64.dmg`
- `PhytoIndex_2.0.0_x64-setup.exe`

No Apple or Windows signing secrets are required for this private-release configuration.

## Version Updates

For a later patch release, update the JavaScript version and lock file from `apps/desktop`:

```bash
npm version 2.0.1 --no-git-tag-version
```

Then update `[workspace.package].version` in the root `Cargo.toml`, update `CHANGELOG.md`, run all checks, and create the matching `v2.0.1` tag.
