#!/bin/bash

set -e

rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli

(cd code; cargo build --release --target wasm32-unknown-unknown)
(cd code; wasm-bindgen target/wasm32-unknown-unknown/release/wasm.wasm --out-dir ./web/pkg --target web)
(cd code/web; python3 -m http.server)
