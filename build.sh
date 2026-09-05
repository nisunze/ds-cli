#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

cargo test --workspace
wasm-pack test --node --features wasm
solar_source_sha="$(git -C ../ds-solar rev-parse HEAD)"
DS_SOLAR_SOURCE_SHA="$solar_source_sha" \
  wasm-pack build --target web --release --out-dir dist --features wasm

echo "ds-command-kernel native/WASM auth, Solar portfolio, and Project Control parity verified"
