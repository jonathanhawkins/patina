#!/usr/bin/env bash
# CI check (pat-u3l8c.109): the patina-web crate must compile for the browser
# target (wasm32-unknown-unknown). This guards against native-only APIs
# (std::time::Instant::now, cpal, native wgpu, std::net/fs) creeping into the
# web runtime's dependency graph.
#
# Requires the wasm target once:  rustup target add wasm32-unknown-unknown
# Run from anywhere; extra args are forwarded to cargo build.
set -euo pipefail
cd "$(dirname "$0")/../engine-rs"
exec cargo build --target wasm32-unknown-unknown -p patina-web "$@"
