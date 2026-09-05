#!/usr/bin/env bash
# Pack the Linux release binary into a versioned tarball for GitHub Releases.
# Tauri has no native "tarball" bundle target; .deb + this archive cover Linux.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

version="$(python3 -c 'import json; print(json.load(open("src-tauri/tauri.conf.json"))["version"])')"
binary="src-tauri/target/release/lyn"
if [[ ! -x "$binary" ]]; then
  echo "missing $binary — run: pnpm tauri build" >&2
  exit 1
fi

stage="dist-packages/staging/lyn-${version}-linux-x86_64"
out="dist-packages/lyn-${version}-linux-x86_64.tar.gz"
rm -rf "$stage"
mkdir -p "$stage"
cp -a "$binary" "$stage/lyn"
cp -a LICENSE README.md "$stage/"
mkdir -p dist-packages
tar -C "dist-packages/staging" -czf "$out" "$(basename "$stage")"
rm -rf dist-packages/staging
echo "wrote $out"
