# Contributing to Patina Engine

Thank you for your interest in contributing. This guide covers everything you need to build, test, and submit changes.

---

## Prerequisites

| Tool | Version | Purpose |
|------|---------|---------|
| Rust (stable) | ≥ 1.78 | Engine crates |
| Node.js | ≥ 20 | Website |
| pnpm | ≥ 9 | JS package manager |

Install Rust via [rustup](https://rustup.rs/). Install pnpm via `npm install -g pnpm` or `corepack enable`.

---

## Building

### Engine

```bash
cd engine-rs
cargo build             # debug build
cargo build --release   # release build
```

### Website

```bash
cd apps/web
pnpm install
pnpm build
```

---

## Testing

### Engine tests

```bash
cd engine-rs
cargo test --workspace
```

### Website lint

```bash
# From repo root
pnpm lint
```

### Running examples

```bash
cd engine-rs
cargo run --example demo_2d
cargo run --example platformer_demo
cargo run --example hello_gdscript
cargo run --example benchmarks --release
```

See [engine-rs/examples/README.md](engine-rs/examples/README.md) for full descriptions.

---

## Code Style

### Rust

- Format with `rustfmt` (run `cargo fmt --all`)
- Lint with `clippy -D warnings`: `cargo clippy --workspace -- -D warnings`
- Every public API item must have a doc comment (`///` or `//!`)
- `gdcore` enforces `#![warn(missing_docs)]`
- Use `thiserror` for error types; `tracing` for logging
- Every `unsafe` block must have a `// SAFETY:` comment

### TypeScript / React

- TypeScript strict mode
- Functional components with hooks
- Use `shadcn/ui` components; Tailwind utilities only
- Use `next/image` and `next/link`

---

## Crate Overview

The engine is a Cargo workspace under `engine-rs/`. There are 13 crates:

| Crate | Description |
|-------|-------------|
| `gdcore` | Foundational primitives: math types (`Vector2`, `Transform2D`, `Color`, …), IDs, `NodePath`, `StringName`, error types, and diagnostics. Every other crate depends on this one. |
| `gdvariant` | The `Variant` type system — a discriminated union that mirrors Godot's built-in `Variant`. Handles typed values, conversion rules, and JSON/binary serialization. |
| `gdobject` | Object model: class metadata, inheritance hierarchy, signals, notifications, and reference counting. |
| `gdresource` | Resources, loaders, savers, UID registry, and path semantics. Includes a `.tres` parser/writer and an import-file reader. |
| `gdscene` | Scene graph: `Node`, `SceneTree`, `PackedScene` (`.tscn` parser), lifecycle management, main loop, animations, tweens, particles, tilemaps, and navigation. |
| `gdserver2d` | Abstract server surface for 2D (and 3D) rendering: canvas items, draw commands, viewports, materials, and shaders. |
| `gdrender2d` | Software rasteriser implementing the `gdserver2d` traits. Also provides a test adapter for golden-frame capture. |
| `gdphysics2d` | 2D physics: collision shapes, rigid/static/kinematic bodies, broad-phase, and a narrow-phase SAT solver. |
| `gdaudio` | Audio runtime: stream plumbing, basic mixer, and playback control. |
| `gdplatform` | Platform integration: windowing, input mapping, timing, OS queries, and export packaging. |
| `gdscript-interop` | Scripting bridge: tokenizer, parser, and tree-walk interpreter for GDScript. Defines `ScriptInstance` and `ScriptBridge` traits. |
| `gdeditor` | Editor infrastructure: property inspection, undo/redo, gizmos, and plugin API. Not required for runtime use. |
| `patina-runner` | Top-level binary crate that wires all subsystems together into a runnable engine. |

### Dependency order (simplified)

```
gdcore
  └── gdvariant
        ├── gdobject
        ├── gdresource
        │     └── gdscene
        │           ├── gdserver2d
        │           │     ├── gdrender2d
        │           │     └── gdphysics2d
        │           ├── gdaudio
        │           └── gdplatform
        ├── gdscript-interop
        └── gdeditor
              └── patina-runner
```

See [ARCHITECTURE_MAP.md](ARCHITECTURE_MAP.md) for the full Godot-subsystem → crate mapping.

---

## Pull Request Process

1. **Branch from `main`**: `git checkout -b your-feature-name`
2. **One logical change per PR** — keep scope tight
3. **All CI checks must pass**: `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo doc --workspace --no-deps` (zero warnings), `pnpm lint`
4. **Squash merge** into `main` — the team lead will squash on merge
5. Reference the bead/task number in your PR description if applicable

---

## Architecture

- Full subsystem → crate mapping: [ARCHITECTURE_MAP.md](ARCHITECTURE_MAP.md)
- Crate dependency rules: [CRATE_BOUNDARIES.md](CRATE_BOUNDARIES.md)
- Godot compatibility scope: [PORT_SCOPE.md](PORT_SCOPE.md)
- Third-party strategy: [THIRDPARTY_STRATEGY.md](THIRDPARTY_STRATEGY.md)

---

## Where to Start

**Most approachable crates for new contributors:**

- `gdcore` — pure math types, no external dependencies, extensive tests
- `gdvariant` — well-scoped type system with clear Godot semantics
- `gdscript-interop` — tokenizer/parser/interpreter, all self-contained

**Good first tasks:**

- Add a missing built-in function to the GDScript interpreter (`gdscript-interop/src/interpreter.rs`)
- Add a new math helper to `gdcore::math` or `gdcore::math3d`
- Improve compatibility test coverage in `tests/`
- Fix a `TODO` or `FIXME` comment anywhere in the codebase

**Before starting a larger task**, check [MILESTONES.md](MILESTONES.md) and [RISK_REGISTER.md](RISK_REGISTER.md) to understand current priorities and known risks.
