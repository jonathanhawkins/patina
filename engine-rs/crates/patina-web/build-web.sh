#!/usr/bin/env bash
# Builds the patina-web wasm bundle for the canvas render harness
# (pat-u3l8c.110) into crates/patina-web/web/.
#
#   engine-rs/crates/patina-web/build-web.sh [--release]
#
# Requirements (both already present on the dev box):
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli   # version must match the wasm-bindgen dep
#
# Set WASM_BINDGEN=/path/to/wasm-bindgen to use a CLI other than the one on
# PATH. `scripts/web.sh --runtime` does this with a version-matched copy it installs
# under target/, so most people should just run that instead.
#
# Then serve the output over HTTP — file:// cannot instantiate wasm:
#   python3 -m http.server 8099 --directory engine-rs/crates/patina-web/web
#
# The shipping export pipeline (HTML shell, .pck bundling) is pat-u3l8c.114;
# this script exists so the browser half of .110 is reproducible today.
set -euo pipefail

crate_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
workspace_dir="$(cd "$crate_dir/../.." && pwd)"
out_dir="$crate_dir/web"
wasm_bindgen="${WASM_BINDGEN:-wasm-bindgen}"

profile_dir="debug"
cargo_args=(build -p patina-web --target wasm32-unknown-unknown)
if [[ "${1:-}" == "--release" ]]; then
  profile_dir="release"
  cargo_args+=(--release)
fi

cd "$workspace_dir"

# The wasm-bindgen CLI and the wasm-bindgen crate embed the same schema version
# and refuse to talk to each other when they differ. Checking up front turns a
# confusing post-build "schema version mismatch" into an actionable message.
# As of 2026-07-27 the box has CLI 0.2.126 while Cargo.lock pins 0.2.114.
lock_version="$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/[",]/, ""); print $3; exit }' \
  "$workspace_dir/Cargo.lock")"
cli_version="$("$wasm_bindgen" --version | awk '{ print $2 }')"
if [[ "$lock_version" != "$cli_version" ]]; then
  cat >&2 <<EOF
error: wasm-bindgen version mismatch — the bundle step would fail.
  crate (Cargo.lock): $lock_version
  CLI (wasm-bindgen): $cli_version

Fix it either way round, from $workspace_dir:
  cargo update -p wasm-bindgen --precise $cli_version   # move the crate to the CLI
  cargo install wasm-bindgen-cli --version $lock_version   # move the CLI to the crate
EOF
  exit 1
fi

cargo "${cargo_args[@]}"

wasm_path="$workspace_dir/target/wasm32-unknown-unknown/$profile_dir/patina_web.wasm"
if [[ ! -f "$wasm_path" ]]; then
  echo "error: expected wasm artifact at $wasm_path" >&2
  exit 1
fi

"$wasm_bindgen" "$wasm_path" --out-dir "$out_dir" --target web

echo "bundle written to $out_dir"
echo "serve it with: python3 -m http.server 8099 --directory $out_dir"
