#!/bin/sh
# Build the macOS capture/OCR helper as a universal binary for the desktop app bundle.
# Called by Tauri before a macOS build (native/crates/app/tauri.macos.conf.json).
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT="$ROOT/capture/build"
mkdir -p "$OUT"
for arch in arm64 x86_64; do
  swiftc -O -target "$arch-apple-macos14" "$ROOT/capture/ns2capture.swift" -o "$OUT/ns2capture-$arch"
done
lipo -create "$OUT/ns2capture-arm64" "$OUT/ns2capture-x86_64" -output "$OUT/ns2capture"
rm -f "$OUT/ns2capture-arm64" "$OUT/ns2capture-x86_64"
# Nested code must be signed before Tauri signs the app (ad-hoc unless an identity is set).
codesign --force --options runtime --entitlements "$ROOT/native/crates/app/Entitlements.plist" -s "${APPLE_SIGNING_IDENTITY:--}" "$OUT/ns2capture"
echo "built $OUT/ns2capture ($(lipo -archs "$OUT/ns2capture"))"
