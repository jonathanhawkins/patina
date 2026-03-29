# COMPAT_MATRIX.md - Compatibility Status Tracking

This document tracks the implementation and compatibility status of each Patina Engine subsystem relative to upstream Godot behavior.

**Last updated**: 2026-03-28 (pat-cygub: improved compatibility matrix — updated stale test counts, added patina-runner, corrected totals)

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
| Core Runtime | `gdcore` | **Measured** | 828 | — | ~100% | `gdcore` unit tests (math, IDs, strings, compare3d, perf_comparison, crash_triage, memory_profiler) |
| Variant System | `gdvariant` | **Measured** | 131 | — | ~100% | `gdvariant` unit tests (enum, conversion, serialization, fuzz) |
| Object Model | `gdobject` | **Measured** | 75 + integration | — | ~80% | `gdobject` units (75), `object_property_reflection_test`, `classdb_parity_test` |
| Signals | `gdobject` | **Measured** | included in gdobject | — | ~60% | `signal_dispatch_parity_test`, `signal_trace_parity_test` |
| Notifications | `gdobject` | **Measured** | included in gdobject | — | ~85% | `notification_coverage_test`, `lifecycle_trace_parity_test` |
| Resources | `gdresource` | **Measured** | 319 + integration | 5 | ~95% | `gdresource` units (319), `cache_regression_test`, `unified_loader_test`, `resource_uid_cache_test` |
| Scene System | `gdscene` | **Measured** | 959 + integration | 37 | ~90% | `gdscene` units (959), `golden_tests`, `instancing_ownership_test`, `packed_scene_edge_cases_test`, `frame_processing_semantics_test` |
| GDScript Interop | `gdscript-interop` | **Measured** | 464 + integration | — | ~85% | `gdscript_interop` units (464), `demo_scenes_test` |
| Trace Parity | `gdscene` | **Measured** | integration | 23 | — | `trace_parity_test`, `multi_scene_trace_parity_test`, `frame_trace_test` |
| Oracle Parity | `gdscene` | **Measured** | integration | 115 oracle outputs | 100% | `oracle_parity_test`, `oracle_regression_test` — measured against Godot 4.6.1 |
| 2D Rendering | `gdrender2d` | **Measured** | 168 + integration | 64 | Golden-based | `gdrender2d` units (168), `render_pipeline`, `render_golden_test` |
| 2D Rendering Server | `gdserver2d` | **Measured** | 108 | — | Measured | `gdserver2d` units (108) — draw ordering, canvas, sprite server |
| 2D Physics | `gdphysics2d` | **Measured** | 111 + integration | 17 | Deterministic | `gdphysics2d` units (111), `physics_integration_test` |
| Input / Platform Stable Layer | `gdplatform` | **Measured** | 504 + integration | — | Measured for bounded slice | `gdplatform` units (504) — input, timers, OS info, target metadata, export config; `platform_first_stable_layer_test`, `platform_targets_validation_test`, `ci_build_matrix_platform_test`, `startup_runtime_packaging_flow_test` |
| 2D Vertical Slice | all | **Measured** | integration | — | End-to-end | `vertical_slice_test`: scene→scripts→input→physics→process→render |
| Audio | `gdaudio` | **Claimed** | 88 | — | N/A | Bus routing, playback state, WAV decode. No real audio output. See `AUDIO_MILESTONE.md`. |
| Editor-Facing Compatibility Layer | `gdeditor` | **Measured** | 1963 + integration | — | Measured for bounded slice | `gdeditor` units (1963), `editor_smoke_test`, `editor_461_revalidation_test`, `editor_interface_compat_test`, `editor_menu_parity_test`, `editor_systems_parity_test` — browser editor shell, compatibility layer, selected tooling slices |
| 3D Rendering | `gdrender3d` | **Measured** | 95 + integration | 64 render | Golden-based | `gdrender3d` units (95), `render_3d_parity_test`, `comparison_tooling_3d_test` — software renderer, framebuffer comparison, diff imaging |
| 3D Rendering Server | `gdserver3d` | **Measured** | 338 | — | Measured | `gdserver3d` units (338) — mesh, camera, lighting, viewport, material |
| 3D Physics | `gdphysics3d` | **Measured** | 124 + integration | — | Deterministic | `gdphysics3d` units (124) — rigid body, collision, trace comparison |
| 3D Comparison Tooling | `gdcore` | **Measured** | 44 | — | Tooling | `compare3d` units (34), `comparison_tooling_3d_test` (10) — scene tree, physics trace, render, unified parity reports |
| Runner | `patina-runner` | **Measured** | 80 | — | N/A | `patina-runner` unit tests (CLI runner, project loading, scene execution) |

**Total test count**: ~15,700 across workspace (6,355 crate unit tests + 9,338 integration tests in 393 test files)
**Total golden files**: 150 (17 physics, 23 traces, 37 scenes, 5 resources, 64 render, 3 signals, 1 version)
**Oracle output files**: 115 (measured against Godot 4.6.1)

---

## Measured vs Claimed vs Deferred

### Measured (has test evidence proving Godot parity)

| Subsystem | Evidence |
|-----------|----------|
| Core Runtime | 828 unit tests; math, IDs, strings, 3D comparison tooling, crash triage, memory profiler |
| Variant System | 131 unit tests; all variant types serialize/deserialize correctly, fuzz testing |
| Object Model | 75 unit tests + integration tests for reflection + ClassDB parity |
| Signals | Integration tests: dispatch parity + trace parity |
| Notifications | Integration tests: coverage + lifecycle traces |
| Resources | 319 unit tests + integration; parsing, caching, UID, unified loading; 5 resource goldens |
| Scene System | 959 unit tests + integration; hierarchy, lifecycle, instancing, packed scenes, frame processing; 37 scene goldens |
| GDScript Interop | 464 unit tests + integration; tokenizer, parser, interpreter, built-ins, bindings |
| Trace Parity | Integration tests against 23 trace goldens (patina vs upstream mock) |
| Oracle Parity | Integration tests against 115 oracle output files (Godot 4.6.1); 100% property parity |
| 2D Rendering | 168 unit tests + integration + 64 render goldens (pixel-level comparison) |
| 2D Rendering Server | 108 unit tests; draw ordering, canvas, sprite server |
| 2D Physics | 111 unit tests + integration + 17 physics goldens (deterministic trace comparison) |
| Input / Platform Stable Layer | 504 unit tests + integration; input map loading, action coverage, snapshot routing, headless windowing, timers, target metadata, startup/packaging flow |
| 2D Vertical Slice | Integration end-to-end tests: full pipeline from scene load to frame output |
| Editor-Facing Compatibility Layer | 1963 unit tests + integration; browser editor shell, editor menus, compatibility layer, script editor, inspector, animation/theme/tilemap tooling, selected editor systems |
| 3D Rendering | 95 unit tests + integration; software renderer, framebuffer comparison, diff imaging; 64 render goldens |
| 3D Rendering Server | 338 unit tests; mesh, camera, lighting, viewport, material |
| 3D Physics | 124 unit tests + integration; rigid body, collision, trace comparison |
| 3D Comparison Tooling | 44 tests; scene tree diff, physics trace comparison, render comparison, unified parity reports (JSON + text) |

### Claimed (code exists, no Godot parity test)

| Subsystem | What exists | What's missing |
|-----------|-------------|----------------|
| Audio | 88 tests covering bus routing, playback state machine, and WAV decode | No real audio output; no Godot parity comparison. See `AUDIO_MILESTONE.md`. |

### Deferred (not in current milestone)

| Subsystem | Milestone | Notes |
|-----------|-----------|-------|
| 3D Oracle Parity | Phase 6+ | 3D comparison tooling exists; oracle outputs for 3D scenes not yet captured from Godot |
| Audio (full playback) | Audio milestone | Stub exists; see EXIT_CRITERIA.md audio gate (pat-dd3) |
| XR Server | Phase 7+ | Not started |

---

## Oracle Parity Summary

Measured against **Godot 4.6.1** (v4.6.1.stable.official.14d19694e) — 21+ oracle scenes measured, 115 oracle output files, 71 property comparisons:

- **Overall**: 100% (71/71 property comparisons match — all parity gaps closed)
- **Node structure**: 100% (all nodes present with correct names/classes across all measured scenes)
- **Explicit properties**: 100% for all measured scenes (positions, collision masks, modulate all match)
- **Script-exported properties**: Closed — all script-exported variables verified across space_shooter, test_scripts, and other scenes
- **3D Scene Trees**: 5 3D fixtures measured (minimal_3d, hierarchy_3d, indoor_3d, multi_light_3d, physics_3d_playground)

Test files: `oracle_parity_test.rs`, `oracle_regression_test.rs`, `comparison_tooling_3d_test.rs`

---

## Golden File Inventory

| Category | Count | Location | Validated by |
|----------|-------|----------|-------------|
| Physics traces | 17 | `fixtures/golden/physics/` | `physics_integration_test` |
| Lifecycle traces | 23 | `fixtures/golden/traces/` | `trace_parity_test`, `multi_scene_trace_parity_test` |
| Scene trees | 37 | `fixtures/golden/scenes/` | `golden_tests`, `oracle_parity_test` |
| Resources | 5 | `fixtures/golden/resources/` | `golden_tests` |
| Render images | 64 | `fixtures/golden/render/` | `render_golden_test`, `render_pipeline`, `render_3d_parity_test` |
| Signals | 3 | `fixtures/golden/signals/` | Signal parity tests |
| Oracle outputs | 115 | `fixtures/oracle_outputs/` | `oracle_parity_test`, `oracle_regression_test` |
| **Total golden** | **150** | `fixtures/golden/` | `golden_staleness_test` (cross-cutting checks) |

---

## Platform Support Matrix

| Platform | Status | Notes |
|----------|--------|-------|
| macOS (aarch64) | **Measured** | Primary dev target for the current headless/runtime slice; native shell parity remains narrower |
| Linux (x86_64) | Claimed | Target metadata and CI matrix coverage exist; native Linux runtime parity is only partly measured |
| macOS (x86_64) | Claimed | Target metadata exists; native macOS parity beyond the bounded slice is not fully measured |
| Windows (x86_64) | Claimed | Platform-layer tests exist; full native runtime parity remains Phase 7 work |
| Android | Deferred | Phase 7+ |
| iOS | Deferred | Phase 7+ |
| Web (WASM) | Deferred | Phase 7+ |

---

## Measured Scene Inventory

### Oracle-Measured Scenes

31 oracle scenes measured against Godot 4.6.1, with 21 scenes having full parity coverage:

| Scene | Fixture | Oracle Golden | Status |
|-------|---------|---------------|--------|
| `minimal.tscn` | `fixtures/scenes/minimal.tscn` | `oracle_outputs/minimal_tree.json` | Measured |
| `simple_2d.tscn` | `fixtures/scenes/simple_2d.tscn` | `oracle_outputs/simple_2d_tree.json` | Measured |
| `hierarchy.tscn` | `fixtures/scenes/hierarchy.tscn` | `oracle_outputs/hierarchy_tree.json` | Measured |
| `simple_hierarchy.tscn` | `fixtures/scenes/simple_hierarchy.tscn` | `oracle_outputs/simple_hierarchy_tree.json` | Measured |
| `with_properties.tscn` | `fixtures/scenes/with_properties.tscn` | `oracle_outputs/with_properties_tree.json` | Measured |
| `platformer.tscn` | `fixtures/scenes/platformer.tscn` | `oracle_outputs/platformer_tree.json` | Measured |
| `space_shooter.tscn` | `fixtures/scenes/space_shooter.tscn` | `oracle_outputs/space_shooter_tree.json` | Measured |
| `physics_playground.tscn` | `fixtures/scenes/physics_playground.tscn` | `oracle_outputs/physics_playground_tree.json` | Measured |
| `physics_playground_extended.tscn` | `fixtures/scenes/physics_playground_extended.tscn` | `oracle_outputs/physics_playground_extended_tree.json` | Measured |
| `signal_test.tscn` | `fixtures/scenes/signal_test.tscn` | `oracle_outputs/signal_test_tree.json` | Measured |
| `signal_instantiation.tscn` | `fixtures/scenes/signal_instantiation.tscn` | `oracle_outputs/signal_instantiation_tree.json` | Measured |
| `signals_complex.tscn` | `fixtures/scenes/signals_complex.tscn` | `oracle_outputs/signals_complex_tree.json` | Measured |
| `ui_menu.tscn` | `fixtures/scenes/ui_menu.tscn` | `oracle_outputs/ui_menu_tree.json` | Measured |
| `unique_name_resolution.tscn` | `fixtures/scenes/unique_name_resolution.tscn` | `oracle_outputs/unique_name_resolution_tree.json` | Measured |
| `character_body_test.tscn` | `fixtures/scenes/character_body_test.tscn` | `oracle_outputs/character_body_test_tree.json` | Measured |
| `test_scripts.tscn` | — | `oracle_outputs/test_scripts_tree.json` | Measured |
| `minimal_3d.tscn` | `fixtures/scenes/minimal_3d.tscn` | `oracle_outputs/minimal_3d_tree.json` | Measured |
| `hierarchy_3d.tscn` | `fixtures/scenes/hierarchy_3d.tscn` | `oracle_outputs/hierarchy_3d_tree.json` | Measured |
| `indoor_3d.tscn` | `fixtures/scenes/indoor_3d.tscn` | `oracle_outputs/indoor_3d_tree.json` | Measured |
| `multi_light_3d.tscn` | `fixtures/scenes/multi_light_3d.tscn` | `oracle_outputs/multi_light_3d_tree.json` | Measured |
| `physics_3d_playground.tscn` | `fixtures/scenes/physics_3d_playground.tscn` | `oracle_outputs/physics_3d_playground_tree.json` | Measured |

Test files: `oracle_parity_test.rs` (32 tests), `oracle_regression_test.rs` (43 tests), `v1_acceptance_gate_test.rs`

---

## Update Protocol

This matrix is updated when:

1. A subsystem transitions between status levels.
2. New fixture coverage is added for a subsystem.
3. Parity test results change materially.
4. A new subsystem row is added.

Each update should include the date and the bead or PR that prompted the change.
