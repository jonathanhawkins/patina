# Third-Party Dependency Strategy

Inventory of all external (non-workspace) crates used by the Patina Engine,
with license, purpose, and retention strategy.

**Last updated**: 2026-03-19 (pat-bq7)

---

## Strategy Definitions

| Strategy | Meaning |
|----------|---------|
| **Keep** | Stable, well-maintained, no reasonable alternative. Will remain a dependency. |
| **Evaluate** | Currently used but may not be needed long-term. Revisit before v1 freeze. |
| **Replace** | Want to own this functionality or switch to a lighter alternative. |

---

## Direct Dependencies

These crates are explicitly listed in at least one workspace `Cargo.toml`.

### Runtime dependencies

| Crate | Version | License | Used by | Purpose | Strategy |
|-------|---------|---------|---------|---------|----------|
| `serde` | 1 | MIT OR Apache-2.0 | `gdvariant`, `gdplatform`, `gdeditor` | Serialization framework for JSON golden files, input maps, editor protocol | **Keep** — industry standard, no alternative |
| `serde_json` | 1 | MIT OR Apache-2.0 | `gdvariant`, `gdscene`, `gdeditor`, `gdplatform`, root, `patina-runner` | JSON parsing/generation for goldens, editor API, input maps | **Keep** — needed for JSON interop |
| `thiserror` | 2 | MIT OR Apache-2.0 | all crates | Derive macro for `Error` trait implementations | **Keep** — zero runtime cost, eliminates boilerplate |
| `tracing` | 0.1 | MIT | all crates | Structured logging and instrumentation | **Keep** — standard Rust observability |
| `tracing-subscriber` | 0.3 | MIT | `patina-runner` | Console log output for the runner binary | **Keep** — required by `tracing` |
| `miniz_oxide` | 0.8 | MIT OR Zlib OR Apache-2.0 | `gdrender2d` | PNG DEFLATE decompression for texture loading | **Evaluate** — only used for PNG decode; could use `png` crate instead or own a minimal decoder |

### Feature-gated dependencies (opt-in via `windowed` feature)

| Crate | Version | License | Used by | Purpose | Strategy |
|-------|---------|---------|---------|---------|----------|
| `winit` | 0.30 | Apache-2.0 | `gdplatform` (optional) | Cross-platform windowing for interactive examples | **Keep** — standard Rust windowing; only compiled when `windowed` feature enabled |
| `softbuffer` | 0.4 | MIT OR Apache-2.0 | `gdplatform` (optional) | Software framebuffer for non-GPU rendering to window | **Evaluate** — may want GPU backend (wgpu) eventually; fine for current software renderer |

### Dev/test-only dependencies

| Crate | Version | License | Used by | Purpose | Strategy |
|-------|---------|---------|---------|---------|----------|
| `tempfile` | 3 | MIT OR Apache-2.0 | root (dev), `gdeditor` (dev), `gdresource` (dev) | Temporary files/dirs for test isolation | **Keep** — test infrastructure only, no runtime cost |

---

## Transitive Dependencies

These are pulled in by direct dependencies. Listed for license audit completeness.

### Via `serde` / `serde_json`

| Crate | License | Notes |
|-------|---------|-------|
| `serde_derive` | MIT OR Apache-2.0 | Proc macro for `#[derive(Serialize, Deserialize)]` |
| `serde_core` | MIT OR Apache-2.0 | Trait definitions |
| `itoa` | MIT OR Apache-2.0 | Fast integer-to-string |
| `memchr` | Unlicense OR MIT | Fast byte search |
| `indexmap` | Apache-2.0 OR MIT | Ordered map (used by serde_json) |
| `hashbrown` | MIT OR Apache-2.0 | Hash map impl |
| `equivalent` | Apache-2.0 OR MIT | Key comparison trait |
| `foldhash` | Zlib | Hash function |

### Via `tracing` / `tracing-subscriber`

| Crate | License | Notes |
|-------|---------|-------|
| `tracing-core` | MIT | Core trait definitions |
| `tracing-attributes` | MIT | `#[instrument]` proc macro |
| `tracing-log` | MIT | Bridge to `log` crate |
| `nu-ansi-term` | MIT | Terminal color output |
| `sharded-slab` | MIT | Concurrent slab for subscriber |
| `thread_local` | MIT OR Apache-2.0 | Thread-local storage |
| `once_cell` | MIT OR Apache-2.0 | Lazy statics |
| `lazy_static` | MIT OR Apache-2.0 | Lazy statics (legacy) |
| `log` | MIT OR Apache-2.0 | Logging facade |
| `valuable` | MIT | Structured value inspection |
| `smallvec` | MIT OR Apache-2.0 | Stack-allocated vectors |

### Via `thiserror`

| Crate | License | Notes |
|-------|---------|-------|
| `thiserror-impl` | MIT OR Apache-2.0 | Proc macro implementation |
| `proc-macro2` | MIT OR Apache-2.0 | Proc macro token streams |
| `quote` | MIT OR Apache-2.0 | Quasi-quoting for proc macros |
| `syn` | MIT OR Apache-2.0 | Rust syntax parser for proc macros |
| `unicode-ident` | MIT OR Apache-2.0 AND Unicode-3.0 | Identifier character detection |

### Via `winit` (only when `windowed` feature enabled)

| Crate | License | Notes |
|-------|---------|-------|
| `bitflags` | MIT OR Apache-2.0 | Bit flag types |
| `cfg-if` | MIT OR Apache-2.0 | Conditional compilation |
| `libc` | MIT OR Apache-2.0 | Platform FFI bindings |
| `rustix` | Apache-2.0 OR MIT | Safe syscall bindings |

### Via `tempfile` (dev only)

| Crate | License | Notes |
|-------|---------|-------|
| `fastrand` | Apache-2.0 OR MIT | Random number generation |
| `getrandom` | MIT OR Apache-2.0 | System random source |
| `errno` | MIT OR Apache-2.0 | Cross-platform errno |

### Via `miniz_oxide`

| Crate | License | Notes |
|-------|---------|-------|
| `adler2` | 0BSD OR MIT OR Apache-2.0 | Adler-32 checksum |

---

## License Summary

| License | Crate count | Restrictive? |
|---------|-------------|-------------|
| MIT OR Apache-2.0 | ~30 | No |
| MIT | ~10 | No |
| Apache-2.0 | ~2 | No |
| Zlib | 2 | No |
| 0BSD | 1 | No (maximally permissive) |
| Unlicense OR MIT | 1 | No |
| Unicode-3.0 (AND) | 1 | No (data license) |
| Apache-2.0 WITH LLVM-exception | ~10 | No (WASM/WASI transitive deps) |

**No GPL, LGPL, AGPL, or other copyleft licenses found.**

All dependencies use permissive licenses compatible with the project's MIT license.

---

## Evaluation Notes

### `miniz_oxide` — Evaluate

Currently used only in `gdrender2d` for PNG DEFLATE decompression. Options:
1. **Keep**: lightweight (one transitive dep: `adler2`), MIT/Zlib/Apache licensed
2. **Replace with `png`**: more complete PNG support but heavier dependency tree
3. **Own it**: write a minimal inflate for the subset of PNG we actually need

Recommendation: keep for now — tiny footprint, well-maintained.

### `softbuffer` — Evaluate

Used for software-rendered window display. If we move to GPU rendering:
1. Replace with `wgpu` for GPU-accelerated rendering
2. Or keep `softbuffer` as a fallback for headless/CI environments

Recommendation: keep until GPU rendering is prioritized (Phase 6+).

### WASM/WASI transitive dependencies

Several `wit-*`, `wasm-*`, and `wasip*` crates appear in the dependency tree.
These are pulled in transitively (likely via `tracing-subscriber` or platform
detection) and are not actively used. They add compile time but no runtime cost.
Consider feature-flagging if compile times become an issue.
