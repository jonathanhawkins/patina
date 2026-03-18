# Patina Engine

[![CI](https://github.com/jonathanhawkins/patina/actions/workflows/ci.yml/badge.svg)](https://github.com/jonathanhawkins/patina/actions/workflows/ci.yml)

A Rust-native, behavior-compatible Godot game engine runtime.

## Repository Layout

| Path | Description |
|------|-------------|
| `engine-rs/` | Rust engine workspace |
| `apps/web/` | Marketing website (patinaengine.com) |
| `apps/godot/` | GDExtension compatibility lab |
| `tools/` | Development tooling |
| `fixtures/` | Scene, physics, and render golden data |
| `tests/` | Compatibility, integration, and performance suites |
| `docs/` | Architecture docs and flywheel methodology |

## Quick Start

```sh
# Build the engine
cd engine-rs && cargo build

# Run engine tests
cd engine-rs && cargo test

# Run the website locally
cd apps/web && pnpm dev
```

See [AGENTS.md](AGENTS.md) for contribution guidelines and coding conventions.
