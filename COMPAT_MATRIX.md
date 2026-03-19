# COMPAT_MATRIX.md - Compatibility Status Tracking

This document tracks the implementation and compatibility status of each Patina Engine subsystem relative to upstream Godot behavior.

**Last updated**: 2026-03-19 (B013 vertical-slice update)

---

## Status Definitions

| Status | Meaning |
|--------|---------|
| **Not Started** | No implementation work has begun |
| **In Progress** | Active implementation underway; not yet testable against fixtures |
| **Partial** | Initial implementation exists; limited fixture coverage; gaps remain |
| **Complete** | Oracle-backed parity tests passing for all supported fixtures in this area |

---

## Compatibility Matrix

> **Reading this table:** "Measured" means automated tests compare Patina output against Godot oracle fixtures or deterministic golden files. "Estimated" means the rate is derived from manual inspection or partial coverage. "N/A" means no parity measurement applies (stubs, deferred work). Deferred subsystems are listed for completeness but do not count toward the 2D vertical slice milestone.

| Subsystem | Crate | Status | Fixture Coverage | Parity Rate | Measurement Source | Notes |
|-----------|-------|--------|-----------------|-------------|-------------------|-------|
| Core Runtime | `gdcore` | **Complete** | 4 scenes | ~100% | `trace_parity_test` (oracle fixtures) | Math types, IDs, strings, error handling; all oracle fixtures match |
| Variant System | `gdvariant` | **Complete** | 4 scenes | ~100% | `trace_parity_test` (oracle fixtures) | Variant enum, type conversions, serialization; all types serialize correctly |
| Object Model | `gdobject` | **Partial** | 4 scenes | ~80% | `trace_parity_test` (estimated, ClassDB stub gaps) | Properties, signals, notifications; ClassDB stub only |
| Signals | `gdobject` | **Partial** | signal_test.tscn | ~30% | `signal_trace_parity_test` (measured against oracle) | Declaration + emit work; cross-node dispatch limited |
| Notifications | `gdobject` | **Partial** | 4 scenes | ~80% | `trace_parity_test` (oracle fixtures) | enter_tree/ready/process/physics_process implemented |
| Resources | `gdresource` | **Partial** | 3 .tres fixtures | ~95% | `unified_loader_test` + `cache_regression_test` (measured) | .tres/.tscn parsing works; UID/caching not yet implemented |
| Scene System | `gdscene` | **Partial** | 4 oracle scenes + runtime unit coverage | ~90% | `trace_parity_test` + `instancing_ownership_test` (measured) | Node hierarchy, SceneTree, lifecycle, PackedScene working; tree-order script dispatch, pause handling, live-subtree lifecycle mutation tests, and traced scripted frame evolution now covered |
| GDScript Interop | `gdscript-interop` | **Partial** | 4 scenes | ~85% | `trace_parity_test` (estimated, built-in gaps) | 30+ built-ins, get_child_count added; class system, cross-node access |
| 2D Rendering | `gdrender2d` | **Partial** | 6 `.tscn` render fixtures | Golden-based | `render_golden_test` (pixel-level golden comparison) | Scene-driven rendering is covered by `render_golden_test`; fixture scenes render to golden PNGs for demo_2d, hierarchy, space_shooter, render_test_simple, render_test_camera, and render_test_sprite |
| 2D Physics | `gdphysics2d` | **Partial** | physics_playground.tscn | Measured | `physics_integration_test` + physics golden traces (deterministic) | PhysicsServer integrated into MainLoop; body sync, fixed-step advance, trace recording working (B011) |
| Input | `gdplatform` | **Partial** | vertical_slice_test | Measured | `input_map_loading_test` + `vertical_slice_test` (measured) | Engine-owned InputSnapshot routes through MainLoop::set_input(); scripts read Input.is_action_pressed(); auto-cleared per frame (B012) |
| **2D Vertical Slice** | `gdscene` + all | **Partial** | vertical_slice_test (11 tests) | Measured | `vertical_slice_test` end-to-end (measured) | End-to-end: scene→scripts→input→physics→process→render via MainLoop::step() (B013) |
| Audio | `gdaudio` | Not Started | -- | N/A | — (deferred) | Stub only |
| Platform | `gdplatform` | Not Started | -- | N/A | — (deferred) | Windowing, timing stub only |
| 3D Runtime | — | Not Started | -- | N/A | — (deferred, out of scope) | Deferred to Phase 6; see [3D Out-of-Scope note](#3d-types-out-of-scope-for-2d-vertical-slice) below |
| Editor | `gdeditor` | Maintenance | — | N/A | — (maintenance-only) | HTTP server + basic viewport; maintenance-only until runtime parity exits; see AGENTS.md |

---

## 3D Types: Out of Scope for 2D Vertical Slice

The following subsystems are explicitly **out of scope** for the 2D vertical slice milestone. They are listed here to prevent confusion in progress reporting — their "Not Started" status does not affect 2D milestone completion criteria.

| Category | Types / Areas | Milestone |
|----------|--------------|-----------|
| 3D Math | `Vector3`, `Basis`, `Transform3D`, `Quaternion`, `Plane`, `AABB` | Phase 6+ |
| 3D Nodes | `Node3D`, `MeshInstance3D`, `Camera3D`, `DirectionalLight3D`, etc. | Phase 6+ |
| 3D Physics | `PhysicsServer3D`, `RigidBody3D`, `CharacterBody3D`, `CollisionShape3D` | Phase 6+ |
| 3D Servers | `RenderingServer` (3D paths), `XRServer`, `NavigationServer3D` | Phase 6+ |

Any scaffolding that exists for 3D types is classified as deferred and is **not counted** in 2D vertical slice parity or coverage metrics.

---

## Oracle Parity Summary

Measured against 9 Godot oracle outputs (147 comparisons, 55 matched = 37.4%):

- **Overall**: 37.4% (55/147 property comparisons match)
- **Node structure**: 100% (all nodes present with correct names/classes)
- **Explicit properties**: ~70% (positions, script vars match)
- **Default properties**: Fixed but fixtures need regeneration

> Headless oracle parity is at 37.4% across 9 scenes. The full engine-owned runtime pipeline (scene→input→physics→process→render) is now exercised end-to-end by `vertical_slice_test.rs` (11 integration tests) via `MainLoop::step()`.

---

## Platform Support Matrix

| Platform | Status | Notes |
|----------|--------|-------|
| macOS (aarch64) | **Partial** | Primary development target; engine builds and tests pass |
| Linux (x86_64) | Not Started | Primary CI target |
| macOS (x86_64) | Not Started | Developer workstation target |
| Windows (x86_64) | Not Started | Deferred to Phase 7 |
| Android | Not Started | Deferred to Phase 7+ |
| iOS | Not Started | Deferred to Phase 7+ |
| Web (WASM) | Not Started | Deferred to Phase 7+ |

---

## Update Protocol

This matrix is updated when:

1. A subsystem transitions between status levels.
2. New fixture coverage is added for a subsystem.
3. Parity test results change materially.
4. A new subsystem row is added.

Each update should include the date and the bead or PR that prompted the change.
