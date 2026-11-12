# Patina Engine

[![CI](https://github.com/jonathanhawkins/patina/actions/workflows/ci.yml/badge.svg)](https://github.com/jonathanhawkins/patina/actions/workflows/ci.yml)

A Rust-native, behavior-compatible Godot game engine runtime.

## Repository Layout

| Path | Description |
|------|-------------|
| `engine-rs/` | Rust engine workspace (see crates below) |
| `apps/web/` | Marketing website (patinaengine.com) |
| `apps/godot/` | Planned GDExtension compatibility lab |
| `apps/orchestrator/` | Multi-agent flywheel coordinator |
| `upstream/godot/` | Pinned upstream Godot oracle submodule |
| `fixtures/` | Scene, physics, and render golden data |
| `tools/`, `scripts/` | Development tooling and helper scripts |
| `docs/` | Architecture docs and flywheel methodology |

### Engine crates (`engine-rs/crates/`)

| Crate | Role |
|-------|------|
| `gdcore`, `gdvariant`, `gdobject` | Math, Variant, Object/ClassDB core |
| `gdresource`, `gdscene` | Resource loading, scene tree, main loop |
| `gdscript-interop` | GDScript support |
| `gdphysics2d`, `gdphysics3d` | Physics |
| `gdserver2d`, `gdserver3d`, `gdrender2d`, `gdrender3d` | Rendering servers and renderers |
| `gdaudio`, `gdplatform` | Audio mixer, input, windowing, OS layer |
| `gdeditor` | Editor |
| `patina-runner` | Native runner binary |
| `patina-web` | WebAssembly runtime for the browser |

## Quick Start

Prerequisites: Rust (via [rustup](https://rustup.rs)), Node.js with pnpm 9,
and optionally [cargo-nextest](https://nexte.st) and Python 3 (for the web build).

```sh
# Sync the pinned upstream oracle
git submodule update --init --recursive

# Build the engine
cd engine-rs && cargo build

# Run engine tests (nextest preferred; `cargo test` also works)
cd engine-rs && cargo nextest run --workspace

# Run the editor in your browser (see below)
scripts/web.sh

# Run the website locally
cd apps/web && pnpm dev
```

## Running in a Browser

One command builds and starts the **Patina editor** and opens it in your browser:

```sh
scripts/web.sh                                   # http://localhost:8080/editor
scripts/web.sh engine-rs/fixtures/scenes/demo_2d.tscn   # open a specific scene
```

The editor is a native Rust server (`engine-rs/examples/editor.rs`) with a
browser UI: scene tree, viewport, inspector, script editor, and Play. It's the
same as running `cargo run --example editor` from `engine-rs/`. The first build
takes a while.

| Option | Effect |
|--------|--------|
| `--release` | Optimized build (slower to compile, faster to use) |
| `--port <n>` | Serve on a different port (editor default `8080`) |
| `--no-open` | Don't open a browser |
| `--build-only` | Build without starting |

### Wasm runtime harness

`scripts/web.sh --runtime` instead compiles the engine runtime itself to
WebAssembly (`engine-rs/crates/patina-web`) and serves a test page at
<http://localhost:8099/>. It isn't the editor: it only checks that the runtime renders to a
`<canvas>` (WebGPU, falling back to WebGL2) and plays audio through WebAudio
(click the page, then **play tone**). `--serve-only` restarts it without
rebuilding.

On first run it adds the `wasm32-unknown-unknown` target and, if the
`wasm-bindgen` CLI on your `PATH` doesn't match `Cargo.lock`, installs a
matching copy under `engine-rs/target/` (your global install and the lockfile
are left alone). To only check that the web crate compiles for wasm, as CI
does, run `scripts/check-wasm.sh`.

## Oracle Pin

Patina uses the pinned `upstream/godot/` submodule as the behavioral oracle for
fixture generation and parity checks. The current pin is Godot
`4.6.1-stable` (`14d19694e0c88a3f9e82d899a0400f27a24c176e`).

To update the oracle pin intentionally:

```sh
git -C upstream/godot fetch --tags
git -C upstream/godot checkout <tag-or-commit>
git add upstream/godot .gitmodules
```

See [AGENTS.md](AGENTS.md) for contribution guidelines and coding conventions.
