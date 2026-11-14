#!/bin/sh
# Builds the game to WebAssembly into ./web (serve that folder with any static server).
# PROFILE=web-dev gives a quicker, larger build for iteration.
set -e
P="${PROFILE:-release}"
cargo build --profile "$P" --target wasm32-unknown-unknown
wasm-bindgen --no-typescript --target web --out-dir web \
  "target/wasm32-unknown-unknown/$P/neon_district.wasm" 2>/dev/null
ls -lh web/*.wasm
