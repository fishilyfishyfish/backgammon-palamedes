#!/usr/bin/env bash
# 엔진을 WASM으로 빌드해 web/pkg에 둔다. 필요: rustup target wasm32-unknown-unknown,
# Cargo.lock의 wasm-bindgen과 같은 버전의 wasm-bindgen-cli.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release --target wasm32-unknown-unknown --manifest-path engine/Cargo.toml
wasm-bindgen --target web --no-typescript --out-dir web/pkg \
  engine/target/wasm32-unknown-unknown/release/palamedes.wasm
ls -l web/pkg
