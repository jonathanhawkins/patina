# COMPAT_MATRIX.md - Compatibility Status Tracking

This document tracks the implementation and compatibility status of each Patina Engine subsystem relative to upstream Godot behavior.

**Last updated**: 2026-03-19 (pat-qv4: measured vs claimed vs deferred split)

---

## Status Definitions

| Status | Meaning |
|--------|---------|
| **Measured** | Automated tests compare Patina output against Godot oracle fixtures or deterministic goldens. Test files are cited. |
| **Claimed** | Code exists and appears to work, but no dedicated parity test proves it matches Godot behavior. |
| **Deferred** | Not started or explicitly out of scope for the current milestone. |

---

## Compatibility Matrix

> Each row cites the specific test files that prove the subsystem's status. A subsystem is "Measured" only if those tests exist and pass. "Claimed" means code is present but lacks parity evidence.

| Subsystem | Crate | Status | Tests | Goldens | Parity | Test files |
|-----------|-------|--------|-------|---------|--------|------------|
| Core Runtime | `gdcore` | **Measured** | 142 | — | ~100% | `gdcore` unit tests (math, IDs, strings) |
| Variant System | `gdvariant` | **Measured** | 93 | — | ~100% | `gdvariant` unit tests (enum, conversion, serialization) |
| Object Model | `gdobject` | **Measured** | 55 + 25 + 20 | — | ~80% | `gdobject` units (55), `object_property_reflection_test` (25), `classdb_parity_test` (20) |
| Signals | `gdobject` | **Measured** | 16 + 12 | — | ~60% | `signal_dispatch_parity_test` (16), `signal_trace_parity_test` (12) |
| Notifications | `gdobject` | **Measured** | 16 + 14 | — | ~85% | `notification_coverage_test` (16), `lifecycle_trace_parity_test` (14) |
| Resources | `gdresource` | **Measured** | 135 + 16 + 15 + 23 | 5 | ~95% | `gdresource` units (135), `cache_regression_test` (16), `unified_loader_test` (15), `resource_uid_cache_test` (23) |
| Scene System | `gdscene` | **Measured** | 666 + 11 + 15 + 22 + 32 | 11 | ~90% | `gdscene` units (666), `golden_tests` (11), `instancing_ownership_test` (15), `packed_scene_edge_cases_test` (22), `frame_processing_semantics_test` (32) |
| GDScript Interop | `gdscript-interop` | **Measured** | 368 + 13 | — | ~85% | `gdscript_interop` units (368), `demo_scenes_test` (13) |
| Trace Parity | `gdscene` | **Measured** | 10 + 7 + 8 | 16 | — | `trace_parity_test` (10), `multi_scene_trace_parity_test` (7), `frame_trace_test` (8) |
| Oracle Parity | `gdscene` | **Measured** | 32 + 43 | 11 scenes | 37.4% | `oracle_parity_test` (32), `oracle_regression_test` (43) |
| 2D Rendering | `gdrender2d` | **Measured** | 84 + 37 + 29 | 9 | Golden-based | `gdrender2d` units (84), `render_pipeline` (37), `render_golden_test` (29) |
| 2D Physics | `gdphysics2d` | **Measured** | 86 + 54 | 8 | Deterministic | `gdphysics2d` units (86), `physics_integration_test` (54) |
| Input | `gdplatform` | **Measured** | 120 + 16 + 10 | — | Measured | `gdplatform` units (120), `input_map_loading_test` (16), `input_action_coverage_test` (10) |
| 2D Vertical Slice | all | **Measured** | 16 | — | End-to-end | `vertical_slice_test` (16): scene→scripts→input→physics→process→render |
| Audio | `gdaudio` | **Claimed** | 17 | — | N/A | `gdaudio` units (17) — stub behavior only, no Godot parity |
| Platform / Windowing | `gdplatform` | **Claimed** | 24 | — | N/A | `window_lifecycle_test` (24) — windowing stubs, no Godot parity test |
| Editor | `gdeditor` | **Claimed** | 267 + 24 | — | N/A | `gdeditor` units (267), `editor_test` (24) — maintenance-only, no parity target |
| 3D Runtime | — | **Deferred** | — | — | N/A | Out of scope for 2D milestone (Phase 6+) |

**Total test count**: ~2,300+ across workspace (665 integration tests + crate unit tests)
**Total golden files**: 49 (8 physics, 16 traces, 11 scenes, 5 resources, 9 render)

---

## Measured vs Claimed vs Deferred

### Measured (has test evidence proving Godot parity)

| Subsystem | Evidence |
|-----------|----------|
| Core Runtime | 142 unit tests; all math types and operations verified |
| Variant System | 93 unit tests; all variant types serialize/deserialize correctly |
| Object Model | 100 tests across units + reflection + ClassDB parity |
| Signals | 28 tests: dispatch parity (16) + trace parity (12) |
| Notifications | 30 tests: coverage (16) + lifecycle traces (14) |
| Resources | 189 tests: parsing, caching, UID, unified loading; 5 resource goldens |
| Scene System | 746 tests: hierarchy, lifecycle, instancing, packed scenes, frame processing; 11 scene goldens |
| GDScript Interop | 381 tests: tokenizer, parser, interpreter, built-ins, bindings |
| Trace Parity | 25 tests against 16 trace goldens (patina vs upstream mock) |
| Oracle Parity | 75 tests against 11 scene goldens; 37.4% property parity |
| 2D Rendering | 150 tests + 9 render goldens (pixel-level comparison) |
| 2D Physics | 140 tests + 8 physics goldens (deterministic trace comparison) |
| Input | 146 tests: input map loading, action coverage, snapshot routing |
| 2D Vertical Slice | 16 end-to-end tests: full pipeline from scene load to frame output |

### Claimed (code exists, no Godot parity test)

| Subsystem | What exists | What's missing |
|-----------|-------------|----------------|
| Audio | 17 stub tests | No audio playback; no Godot comparison |
| Platform / Windowing | 24 lifecycle tests | No Godot windowing behavior comparison |
| Editor | 291 tests (units + integration) | Maintenance-only; no parity target |

### Deferred (not in 2D milestone)

| Subsystem | Milestone | Notes |
|-----------|-----------|-------|
| 3D Math (`Vector3`, `Basis`, `Transform3D`, `Quaternion`) | Phase 6+ | — |
| 3D Nodes (`Node3D`, `MeshInstance3D`, `Camera3D`, `Light3D`) | Phase 6+ | Explicitly out of scope for 2D milestone (pat-bwg) |
| 3D Physics (`PhysicsServer3D`, `RigidBody3D`) | Phase 6+ | — |
| 3D Servers (`RenderingServer` 3D paths, `XRServer`) | Phase 6+ | — |
| Audio (full playback) | Audio milestone | Stub exists; see EXIT_CRITERIA.md audio gate (pat-dd3) |

---

## Oracle Parity Summary

Measured against 9 Godot oracle outputs (147 comparisons, 55 matched = 37.4%):

- **Overall**: 37.4% (55/147 property comparisons match)
- **Node structure**: 100% (all nodes present with correct names/classes)
- **Explicit properties**: ~70% (positions, script vars match)
- **Default properties**: Fixed but fixtures need regeneration

Test files: `oracle_parity_test.rs` (32 tests), `oracle_regression_test.rs` (43 tests)

---

## Golden File Inventory

| Category | Count | Location | Validated by |
|----------|-------|----------|-------------|
| Physics traces | 8 | `fixtures/golden/physics/` | `physics_integration_test` (54 tests) |
| Lifecycle traces | 16 | `fixtures/golden/traces/` | `trace_parity_test` (10) + `multi_scene_trace_parity_test` (7) |
| Scene trees | 11 | `fixtures/golden/scenes/` | `golden_tests` (11) + `oracle_parity_test` (32) |
| Resources | 5 | `fixtures/golden/resources/` | `golden_tests` (11) |
| Render images | 9 | `fixtures/golden/render/` | `render_golden_test` (29) + `render_pipeline` (37) |
| **Total** | **49** | | `golden_staleness_test` (5 cross-cutting checks) |

---

## Platform Support Matrix

| Platform | Status | Notes |
|----------|--------|-------|
| macOS (aarch64) | **Measured** | Primary dev target; all 2,300+ tests pass |
| Linux (x86_64) | Deferred | Primary CI target (not yet configured) |
| macOS (x86_64) | Deferred | Developer workstation target |
| Windows (x86_64) | Deferred | Phase 7 |
| Android | Deferred | Phase 7+ |
| iOS | Deferred | Phase 7+ |
| Web (WASM) | Deferred | Phase 7+ |

---

## Update Protocol

This matrix is updated when:

1. A subsystem transitions between status levels.
2. New fixture coverage is added for a subsystem.
3. Parity test results change materially.
4. A new subsystem row is added.

Each update should include the date and the bead or PR that prompted the change.
