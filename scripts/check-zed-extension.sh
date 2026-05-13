#!/usr/bin/env bash
set -euo pipefail

export PATH="$HOME/.cargo/bin:$PATH"

cargo check --manifest-path zed-extension/Cargo.toml --target wasm32-wasip2
