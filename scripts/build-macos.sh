#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DESKTOP_DIR="$ROOT_DIR/apps/desktop"

cd "$DESKTOP_DIR"

echo "Installing frontend dependencies..."
npm ci

echo "Building the macOS Apple Silicon DMG..."
cargo tauri build --target aarch64-apple-darwin --bundles dmg

echo "Build complete: $ROOT_DIR/target/aarch64-apple-darwin/release/bundle/dmg"
