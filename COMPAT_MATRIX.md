# COMPAT_MATRIX.md - Compatibility Status Tracking

This document tracks the implementation and compatibility status of each Patina Engine subsystem relative to upstream Godot behavior.

**Last updated**: 2026-03-19 (oracle/render/runtime verification)

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

| Subsystem | Crate | Status | Fixture Coverage | Parity Rate | Notes |
|-----------|-------|--------|-----------------|-------------|-------|
| Core Runtime | `gdcore` | **Complete** | 4 scenes | ~100% | Math types, IDs, strings, error handling; all oracle fixtures match |
| Variant System | `gdvariant` | **Complete** | 4 scenes | ~100% | Variant enum, type conversions, serialization; all types serialize correctly |
| Object Model | `gdobject` | **Partial** | 4 scenes | ~80% | Properties, signals, notifications; ClassDB stub only |
| Signals | `gdobject` | **Partial** | signal_test.tscn | ~30% | Declaration + emit work; cross-node dispatch limited |
| Notifications | `gdobject` | **Partial** | 4 scenes | ~80% | enter_tree/ready/process/physics_process implemented |
| Resources | `gdresource` | **Partial** | 3 .tres fixtures | ~95% | .tres/.tscn parsing works; UID/caching not yet implemented |
| Scene System | `gdscene` | **Partial** | 4 oracle scenes + runtime unit coverage | ~90% | Node hierarchy, SceneTree, lifecycle, PackedScene working; tree-order script dispatch, pause handling, live-subtree lifecycle mutation tests, and traced scripted frame evolution now covered |
| GDScript Interop | `gdscript-interop` | **Partial** | 4 scenes | ~85% | 30+ built-ins, get_child_count added; class system, cross-node access |
| 2D Rendering | `gdrender2d` | **Partial** | 6 `.tscn` render fixtures | Golden-based | Scene-driven rendering is covered by `render_golden_test`; fixture scenes render to golden PNGs for demo_2d, hierarchy, space_shooter, render_test_simple, render_test_camera, and render_test_sprite |
| 2D Physics | `gdphysics2d` | Not Started | -- | -- | Shapes, collision detection stub only |
| Audio | `gdaudio` | Not Started | -- | -- | Stub only |
| Input | `gdplatform` | Not Started | -- | -- | Stub only |
| Platform | `gdplatform` | Not Started | -- | -- | Windowing, timing stub only |
| 3D Runtime | -- | Not Started | -- | -- | Deferred to Phase 6 |
| Editor | `gdeditor` | In Progress | -- | -- | HTTP server + basic viewport working |

---

## Oracle Parity Summary

Measured against 4 Godot oracle outputs (main, simple_hierarchy, signal_test, multi_script):

- **Overall**: 32.2% (28/87 property comparisons match)
- **Node structure**: 100% (all nodes present with correct names/classes)
- **Explicit properties**: ~70% (positions, script vars match)
- **Default properties**: Fixed but fixtures need regeneration

> Headless oracle parity still needs broader fixture regeneration, but the runtime frame/lifecycle contract and scene-driven 2D render golden path are now both covered by passing tests.

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
