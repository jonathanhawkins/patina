# Third-Party Implementation Strategy

**Rule (from AGENTS.md):** Do not start reimplementing third-party code until classification is recorded here.

This document records the "reimplement vs. wrap vs. vendor" decision for every significant dependency in the Patina workspace before that dependency's subsystem expands.

---

## Decision Framework

| Option | When to choose |
|--------|---------------|
| **Keep / wrap** | Library provides correct semantics, is well-maintained, and has acceptable compile time and binary size impact. |
| **Vendor** | Library is abandoned or we need deep behavioral patches not suitable for upstream contribution. |
| **Reimplement** | Library semantics diverge from Godot's documented behavior, or the crate surface is too large to audit safely. |
| **Optional feature flag** | Dependency is needed only for a specific backend or platform; make it `optional = true`. |

---

## Current Dependency Classifications

### `thiserror` — **Keep**

- **Use:** Derive macros for error types across all crates.
- **Decision:** Keep. Generates minimal boilerplate-free error types. No behavioral constraints from Godot.
- **Review trigger:** None expected.

---

### `serde` + `serde_json` — **Keep**

- **Use:** Serialization/deserialization for probe output, golden fixtures, and config.
- **Decision:** Keep. Standard Rust ecosystem. Not used in hot game-loop paths.
- **Review trigger:** If serialization needs to match a Godot-specific binary format exactly, evaluate whether serde can be driven to that format or a custom codec is needed.

---

### `tracing` + `tracing-subscriber` — **Keep**

- **Use:** Structured logging across the engine workspace.
- **Decision:** Keep. Replaces `log`. No behavioral conflict with Godot.
- **Review trigger:** None expected.

---

### `miniz_oxide` — **Keep (conditional evaluation)**

- **Use:** Potential future use for `.res` binary resource decompression (Godot uses zlib/deflate internally).
- **Decision:** Keep as `optional` dependency when first introduced. Pure Rust, no unsafe that we own.
- **Review trigger:** When `.res` binary loader requires compression support, confirm `miniz_oxide` output matches Godot's zlib stream byte-for-byte on a fixture. If it diverges, evaluate `flate2` (which can back to `miniz` or `zlib-ng`).

---

### `winit` — **Keep (optional, platform feature)**

- **Use:** Window creation and event loop for the `gdplatform` crate.
- **Decision:** Keep as `optional = true` behind a `platform-winit` feature flag. Headless mode (CI, tests) must not require winit.
- **Review trigger:** If input event semantics diverge from Godot's `InputEvent` model, add a translation layer rather than replacing winit.

---

### `glam` — **Keep**

- **Use:** Math types (`Vec2`, `Vec3`, `Mat4`, etc.) in `gdcore` and rendering crates.
- **Decision:** Keep. `glam` uses SIMD-friendly layout and is widely audited. Map Godot math types to `glam` internally; expose Godot-named types as wrappers.
- **Review trigger:** If a Godot math operation produces a documented result that `glam` cannot replicate (e.g., specific `basis_xform` edge cases), implement the divergent operation manually inside the wrapper rather than forking `glam`.

---

## Godot `thirdparty/` Subsystems — Classification Pending

The following Godot upstream `thirdparty/` directories are **not yet imported** into Patina. Classification is required before work begins.

| Upstream dir | Area | Preliminary verdict | Owner bead |
|---|---|---|---|
| `thirdparty/zlib` | Compression | Use `miniz_oxide` or `flate2` | — |
| `thirdparty/zstd` | Compression | Use `zstd` crate (Rust binding) | — |
| `thirdparty/freetype` | Font rendering | Evaluate `ab_glyph` or `rusttype` | — |
| `thirdparty/bullet` | Physics (3D) | Defer to v2; evaluate `rapier3d` | — |
| `thirdparty/embree3` | Raycast (3D) | Defer to v2 | — |
| `thirdparty/openxr` | XR | Defer; not in v1 scope | — |

**Policy:** No code importing from these areas may land until a bead exists that records the classification decision and links it here.

---

## Audit Log

| Date | Dependency | Decision | Rationale |
|---|---|---|---|
| 2026-03-19 | `thiserror`, `serde`, `serde_json`, `tracing`, `glam` | Keep | Core ergonomics; no behavioral conflict |
| 2026-03-19 | `miniz_oxide` | Keep (optional) | Deferred to when `.res` compression is needed |
| 2026-03-19 | `winit` | Keep (optional, feature-gated) | Headless CI must work without it |
