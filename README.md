# Patina Engine

[![CI](https://github.com/jonathanhawkins/patina/actions/workflows/ci.yml/badge.svg)](https://github.com/jonathanhawkins/patina/actions/workflows/ci.yml)

A Rust-native, behavior-compatible Godot game engine runtime.

## Repository Layout

| Path | Description |
|------|-------------|
| `engine-rs/` | Rust engine workspace |
| `apps/web/` | Marketing website (patinaengine.com) |
| `apps/godot/` | Planned GDExtension compatibility lab |
| `upstream/godot/` | Pinned upstream Godot oracle submodule |
| `tools/` | Development tooling |
| `fixtures/` | Scene, physics, and render golden data |
| `tests/` | Compatibility, integration, and performance suites |
| `docs/` | Architecture docs and flywheel methodology |

## Quick Start

```sh
# Sync the pinned upstream oracle
git submodule update --init --recursive

# Build the engine
cd engine-rs && cargo build

# Run engine tests
cd engine-rs && cargo test

# Run the website locally
cd apps/web && pnpm dev
```

## Oracle Pin

Patina uses the pinned `upstream/godot/` submodule as the behavioral oracle for
fixture generation and parity checks. The current pin is Godot
`4.5.1-stable` (`f62fdbde15035c5576dad93e586201f4d41ef0cb`).

To update the oracle pin intentionally:

```sh
git -C upstream/godot fetch --tags
git -C upstream/godot checkout <tag-or-commit>
git add upstream/godot .gitmodules
```

See [AGENTS.md](AGENTS.md) for contribution guidelines and coding conventions.
