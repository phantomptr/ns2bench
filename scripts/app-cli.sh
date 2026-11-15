#!/bin/sh
# Build the ns2bench CLI for the app bundle (Tauri "externalBin" sidecar).
# Usage: scripts/app-cli.sh [rust-target]   (default: this machine)
# Then: cargo tauri build --config tauri.cli.conf.json   (from native/crates/app)
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
host=$(rustc -vV | sed -n 's/^host: //p')
target=${1:-$host}
cd "$ROOT/native"
cargo build --locked --release --target "$target" -p ns2bench
ext=""; case "$target" in *windows*) ext=".exe" ;; esac
mkdir -p crates/app/bin
cp "${CARGO_TARGET_DIR:-target}/$target/release/ns2bench$ext" "crates/app/bin/ns2bench-$target$ext"
echo "bundling crates/app/bin/ns2bench-$target$ext"
