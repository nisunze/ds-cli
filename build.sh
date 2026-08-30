#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

cargo test
wasm-pack test --node --features wasm
wasm-pack build --target web --release --out-dir dist --features wasm

echo "ds-command-kernel native/WASM login parity verified"
