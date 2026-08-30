#!/usr/bin/env bash
# © 2026 aiaiaiai · aiaiaiai.org
# SPDX-License-Identifier: MPL-2.0

set -Eeuo pipefail

out_dir="${1:-target/core-bindings-wasm}"
wasm_path="target/wasm32-unknown-unknown/release/ox1_bindings_wasm.wasm"

cargo build --locked --release --target wasm32-unknown-unknown -p ox1-bindings-wasm
rm -rf "$out_dir"
mkdir -p "$out_dir"
wasm-bindgen \
  --target web \
  --typescript \
  --out-name index \
  --out-dir "$out_dir" \
  "$wasm_path"

cat >"$out_dir/package.json" <<'JSON'
{
  "name": "@nilx-one/core-bindings-wasm",
  "version": "0.1.0",
  "type": "module",
  "module": "./index.js",
  "types": "./index.d.ts",
  "files": [
    "index.js",
    "index.d.ts",
    "index_bg.wasm",
    "index_bg.wasm.d.ts"
  ],
  "sideEffects": false
}
JSON

(
  cd "$out_dir"
  sha256sum index_bg.wasm > integrity.sha256
)
