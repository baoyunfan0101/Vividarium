# Building and Releasing Vividarium

This guide describes the Vividarium desktop release process.

## Release Matrix

| Platform | Rust target | Bundle | Output directory | Signing model |
| --- | --- | --- | --- | --- |
| macOS Apple Silicon | `aarch64-apple-darwin` | App and DMG | `target/aarch64-apple-darwin/release/bundle/` | Free ad-hoc signature |
| Windows x64 | `x86_64-pc-windows-msvc` | NSIS EXE | `target/x86_64-pc-windows-msvc/release/bundle/nsis/` | Unsigned |

Development tools are required only on the build machine. Destination computers do not need Rust, Node.js, Python, npm, or SQLite.

The platform-specific Tauri settings are stored in:

- `apps/desktop/src-tauri/tauri.macos.conf.json`
- `apps/desktop/src-tauri/tauri.windows.conf.json`

Tauri merges the matching file into `tauri.conf.json` automatically.

## Common Prerequisites

- Git
- Rust 1.85 or newer
- Node.js 24 or newer and npm
- Tauri CLI 2

Install the Tauri CLI:

```bash
cargo install tauri-cli --version "^2.0" --locked
```

Both `Cargo.lock` and `apps/desktop/package-lock.json` are committed. Use `npm ci`, not `npm install`, for release builds.

Every release build also creates signed updater artifacts. Set
`TAURI_SIGNING_PRIVATE_KEY` to the updater private key before running a local
build. The private key must never be committed. The corresponding public key is
stored in `apps/desktop/src-tauri/tauri.conf.json`.

Also set `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` to the private key password.
Environment variables must be set in the shell; a `.env` file is not loaded by
the Tauri CLI.

The release operator keeps the current keypair outside the repository at
`~/.config/Vividarium/updater.key` and
`~/.config/Vividarium/updater.key.pub`. The password is stored in the macOS
Keychain service `io.github.baoyunfan0101.vividarium.updater-key-password`; an
offline backup of both the private key and its password is required. Before
creating a tag, confirm that the public file, the updater `pubkey`, and the two
GitHub Actions secrets all belong to this same keypair.

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
cargo tauri build --target aarch64-apple-darwin --bundles app,dmg
```

The macOS configuration uses `signingIdentity: "-"`. This creates an ad-hoc signature without an Apple Developer account, certificate, or notarization.

### Verify

From the repository root:

```bash
VERSION=$(node -p "require('./apps/desktop/package.json').version")

codesign --verify --deep --strict --verbose=2 \
  target/aarch64-apple-darwin/release/bundle/macos/Vividarium.app

hdiutil verify \
  "target/aarch64-apple-darwin/release/bundle/dmg/Vividarium_${VERSION}_aarch64.dmg"

shasum -a 256 \
  "target/aarch64-apple-darwin/release/bundle/dmg/Vividarium_${VERSION}_aarch64.dmg"
```

Gatekeeper assessment is expected to reject this private build because it is not notarized.

### Install on another Mac

1. Open the DMG and drag Vividarium into Applications.
2. Try to open Vividarium once.
3. Open System Settings, then Privacy and Security.
4. Find the blocked Vividarium message and select Open Anyway.
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

The installer uses the WebView2 download bootstrapper. If WebView2 is absent, installation requires internet access and installs it automatically. After installation, Vividarium can run offline.

### Verify

```powershell
$Version = (Get-Content apps\desktop\package.json | ConvertFrom-Json).version
$Installer = "target\x86_64-pc-windows-msvc\release\bundle\nsis\Vividarium_${Version}_x64-setup.exe"

Get-FileHash $Installer -Algorithm SHA256
Get-AuthenticodeSignature $Installer
```

`Get-AuthenticodeSignature` is expected to report `NotSigned` for this private release.

Test the installer on a clean Windows 10 or Windows 11 virtual machine with no Rust, Node.js, Python, or database software installed.

### Install on another Windows computer

1. Run `Vividarium_<version>_x64-setup.exe`.
2. If SmartScreen appears, select More info.
3. Select Run anyway.
4. Allow the installer to download WebView2 if requested.

Windows 11 Smart App Control can block unsigned software without an individual bypass. If that feature is enabled, turn it off for the private test machine or build the application directly on that machine.

## GitHub Actions Release

`.github/workflows/release.yml` builds both supported packages on native GitHub runners and attaches them to a GitHub release.

The workflow can be started manually from the Actions page or by pushing a matching version tag.

Add the following GitHub Actions repository secret before the first release:

| Secret | Value |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | Complete contents of the updater private key. |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Password for the updater private key. |

The updater signing key is independent of Apple notarization and Windows
Authenticode signing. Losing it prevents installed applications from trusting
future updates, so keep a secure backup outside the repository.

Before releasing, ensure these versions match:

- `Cargo.toml` under `[workspace.package]`
- `apps/desktop/package.json`
- `apps/desktop/package-lock.json`

Tauri reads its application version from `apps/desktop/package.json`.

Run the release checks:

```bash
cargo fmt --all -- --check
cargo test --workspace --locked
node scripts/check-release-version.mjs v$(node -p "require('./apps/desktop/package.json').version")

cd apps/desktop
npm ci
npm run test:desktop
npm run build
```

The GitHub release workflow runs these checks in its `verify` job before either
platform package job may start. Each package job also validates that both
updater signing secrets are non-empty before compiling the application.

Create and push the release tag:

```bash
VERSION=$(node -p "require('./apps/desktop/package.json').version")
git tag -a "v${VERSION}" -m "Vividarium v${VERSION}"
git push origin "v${VERSION}"
```

Before pushing the tag, confirm:

- [ ] Cargo tests pass.
- [ ] Desktop tests pass.
- [ ] The production frontend build passes.
- [ ] Package, Cargo, and tag versions match.
- [ ] Updater signing secrets are configured and the public key is unchanged.
- [ ] Schema-v2 migration tests pass.
- [ ] macOS and Windows packages are produced.
- [ ] `latest.json` contains signed macOS and Windows updater entries.
- [ ] The release remains draft until release asset validation passes.

The workflow creates a draft GitHub release, validates its complete updater
manifest and both platform artifacts, then publishes it. It uploads:

- `Vividarium_<version>_aarch64.dmg`
- `Vividarium_<version>_x64-setup.exe`
- Signed updater artifacts and signature files
- `latest.json`, which the installed application uses to discover updates

No Apple or Windows code-signing secrets are required for this release
configuration. The updater signing secret is still mandatory.
