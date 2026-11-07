#!/usr/bin/env sh
# Builds the browser protocol from crates/web into web/src/wasm.
# Needs Rust with the wasm32-unknown-unknown target and wasm-bindgen-cli at the
# version pinned in the workspace.
set -eu
root="$(cd "$(dirname "$0")/../.." && pwd)"
cargo build --manifest-path "$root/Cargo.toml" -p undiluted-web --target wasm32-unknown-unknown --release
wasm-bindgen --target web --out-dir "$root/web/src/wasm" \
  "${CARGO_TARGET_DIR:-$root/target}/wasm32-unknown-unknown/release/undiluted_web.wasm"
