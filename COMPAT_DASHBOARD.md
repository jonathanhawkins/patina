# Compatibility Dashboard

**Last updated**: 2026-03-28 (pat-cygub: improved compatibility matrix — updated stale test counts, added patina-runner, corrected totals)
**Test suite**: 15,700 tests passing across workspace (6,355 crate units + 9,338 integration tests in 393 files)
**Golden files**: 150 (17 physics, 23 traces, 37 scenes, 5 resources, 64 render, 3 signals, 1 version)
**Oracle outputs**: 115 files (measured against Godot 4.6.1)

---

## Subsystem Status at a Glance

| Subsystem | Status | Test Count | Goldens | Parity |
|-----------|--------|------------|---------|--------|
| Core Runtime | Measured | 828 | — | ~100% |
| Variant System | Measured | 131 | — | ~100% |
| Object Model | Measured | 75+ | — | ~80% |
| Signals | Measured | integration | 3 | ~60% |
| Notifications | Measured | integration | — | ~85% |
| Resources | Measured | 319+ | 5 | ~95% |
| Scene System | Measured | 959+ | 37 | ~90% |
| GDScript Interop | Measured | 464+ | — | ~85% |
| 2D Rendering | Measured | 168+ | 64 | Golden-based |
| 2D Rendering Server | Measured | 108 | — | Measured |
| 2D Physics | Measured | 111+ | 17 | Deterministic |
| Input / Platform | Measured | 504 | — | Measured |
| Vertical Slice | Measured | integration | — | End-to-end |
| Audio | Claimed | 88 | — | Stub only |
| Editor | Measured | 1963+ | — | Parity |
| 3D Rendering | Measured | 95+ | 64 | Golden-based |
| 3D Rendering Server | Measured | 338 | — | Measured |
| 3D Physics | Measured | 124+ | — | Deterministic |
| 3D Comparison Tooling | Measured | 44 | — | Tooling |
| Runner | Measured | 80 | — | N/A |

---

## Oracle Parity Results

All oracle golden outputs regenerated against **Godot 4.6.1** (v4.6.1.stable.official.14d19694e). Comparison uses `class_defaults.rs` filtering to compare only explicitly-set and semantically-meaningful properties. Tests: `oracle_parity_test.rs` (32) + `oracle_regression_test.rs` (43).

| Scene | Comparisons | Matched | Parity | Notes |
|-------|-------------|---------|--------|-------|
| `minimal.tscn` | 1 | 1 | **100.0%** | Single Node, perfect match |
| `hierarchy.tscn` | 3 | 3 | **100.0%** | Full node/class/property match |
| `with_properties.tscn` | 5 | 5 | **100.0%** | Player position/modulate match |
| `space_shooter.tscn` | 13 | 13 | **100.0%** | All properties match including script-exported vars |
| `platformer.tscn` | 12 | 12 | **100.0%** | Node structure and properties match |
| `physics_playground.tscn` | 12 | 12 | **100.0%** | All physics node classes, positions, and collision_mask match |
| `signals_complex.tscn` | 9 | 9 | **100.0%** | Signal node structure matches |
| `test_scripts.tscn` | 11 | 11 | **100.0%** | All script vars match; Mover position within f32 tolerance |
| `ui_menu.tscn` | 5 | 5 | **100.0%** | Complete match |
| **Overall** | **71** | **71** | **100.0%** | Measured against Godot 4.6.1 |

**Parity change notes** (4.6.1 repin, historical):
- `physics_playground`: improved from 66.7% → 100% (Godot 4.6.1 oracle outputs now align with Patina's collision_mask handling)
- `space_shooter`: resolved — was 61.5%, now 100.0% (script-exported vars implemented)
- `test_scripts`: resolved — was 36.4%, now 100.0% (script-exported vars implemented)
- Overall property count increased from 63 → 71 due to richer oracle capture in 4.6.1; all 71 now match
- See `fixtures/oracle_outputs/PARITY_REPORT.md` for per-property detail

---

## Property Gap Analysis

### Measured Properties (test-backed)
- Node names and class names: **100%** across all scenes (oracle tests)
- Node hierarchy (parent/child structure): **100%** (959 scene system unit tests + integration)
- Explicitly-set Vector2 positions: **Match** (oracle parity tests)
- Script variable initial values: **Match** for Int/Float types (464 GDScript tests)
- Lifecycle ordering: **85%** (notification + lifecycle trace tests)
- Signal dispatch: **60%** (signal dispatch + trace parity tests)
- 2D Physics stepping: **Deterministic** (111 unit tests + 17 physics goldens)
- 3D Physics stepping: **Deterministic** (124 unit tests + trace comparison tooling)
- 2D rendering: **Golden-based** (168 render tests + 64 golden images)
- 3D rendering: **Golden-based** (95 render tests + framebuffer comparison)
- Input routing: **Measured** (504 platform tests — snapshot, map loading, action coverage)
- Editor-facing compatibility layer: **Measured for bounded slice** (1859 tests — browser shell, script editor, menus, server, selected tooling)

### Resolved Gaps (historical)

| Gap | Category | Resolution |
|-----|----------|------------|
| Script variable export in space_shooter (5 vars) | Resolved | speed, can_shoot, shoot_cooldown, spawn_interval, spawn_timer — all now emitted and verified |
| Script variable export in test_scripts (7 vars) | Resolved | direction, speed, health, is_alive, name_str, velocity — all now emitted; Mover position within f32 tolerance |

### Remaining Gaps

| Gap | Category | Impact |
|-----|----------|--------|
| Audio playback | Deferred | Low — stub only, no Godot behavior to compare |

---

## Test File Reference

### Measured subsystems — backing test files

| Subsystem | Test files |
|-----------|-----------|
| Core Runtime | `gdcore` unit tests (828) |
| Variant System | `gdvariant` unit tests (103) |
| Object Model | `gdobject` units (75), `object_property_reflection_test`, `classdb_parity_test` |
| Signals | `signal_dispatch_parity_test`, `signal_trace_parity_test` |
| Notifications | `notification_coverage_test`, `lifecycle_trace_parity_test` |
| Resources | `gdresource` units (319), `cache_regression_test`, `unified_loader_test`, `resource_uid_cache_test` |
| Scene System | `gdscene` units (959), `golden_tests`, `instancing_ownership_test`, `packed_scene_edge_cases_test`, `frame_processing_semantics_test` |
| GDScript Interop | `gdscript_interop` units (464), `demo_scenes_test` |
| Trace Parity | `trace_parity_test`, `multi_scene_trace_parity_test`, `frame_trace_test` |
| Oracle Parity | `oracle_parity_test`, `oracle_regression_test` |
| 2D Rendering | `gdrender2d` units (168), `render_pipeline`, `render_golden_test` |
| 2D Rendering Server | `gdserver2d` units (108) |
| 2D Physics | `gdphysics2d` units (111), `physics_integration_test` |
| Input / Platform | `gdplatform` units (504), `input_map_loading_test`, `input_action_coverage_test`, `platform_first_stable_layer_test`, `platform_targets_validation_test`, `startup_runtime_packaging_flow_test` |
| Vertical Slice | `vertical_slice_test` |
| Editor-Facing Compatibility Layer | `gdeditor` units (1859), `editor_smoke_test`, `editor_461_revalidation_test`, `editor_interface_compat_test`, `editor_menu_parity_test`, `editor_systems_parity_test` |

Editor note: the strongest measured `gdeditor` evidence is the browser-served
editor shell, compatibility-layer APIs, and selected tooling slices. That is
substantial, but it is still narrower than blanket parity with the full Godot
editor feature surface.
| 3D Rendering | `gdrender3d` units (95), `render_3d_parity_test`, `comparison_tooling_3d_test` |
| 3D Rendering Server | `gdserver3d` units (338) |
| 3D Physics | `gdphysics3d` units (124), `physics_integration_test` |
| 3D Comparison Tooling | `compare3d` units (34), `comparison_tooling_3d_test` (10) |

Platform note: the strongest measured `gdplatform` evidence is the bounded
headless/stable-layer slice plus target-matrix and startup/packaging coverage.
Native Linux/macOS/Windows shell behavior is implemented and partly tested, but
is still narrower than full platform parity.

### Claimed subsystems — what's missing

| Subsystem | Has | Needs |
|-----------|-----|-------|
| Audio | 88 tests (bus routing, WAV decode, playback) | Godot audio behavior comparison |

---

## Render Fixture Coverage

Scene-driven 2D render fixtures currently pass for:

- `demo_2d.tscn`
- `hierarchy.tscn`
- `space_shooter.tscn`
- `render_test_simple.tscn`
- `render_test_camera.tscn`
- `render_test_sprite.tscn`

Tests: `render_golden_test.rs` (29 tests) — pixel-level golden comparison, determinism verification, zoom/pan behavior.

---

## 2D Vertical-Slice Coverage

The `vertical_slice_test.rs` integration test exercises the full engine-owned pipeline end-to-end:

**Scene**: `space_shooter.tscn` with real GDScript scripts (`player.gd`, `enemy_spawner.gd`)

| Test | What it proves |
|------|---------------|
| Scene loads with correct structure | PackedScene → SceneTree instancing (6 nodes) |
| Player starts at expected position | Property sync from .tscn (320, 400) |
| 60 frames no input | MainLoop::step() runs full pipeline without crash |
| Player moves right with input | InputSnapshot → MainLoop → script _process → position update |
| Player moves left with input | Bidirectional input routing works |
| Player clamped to viewport | Script `clamp()` built-in works correctly |
| Diagonal movement | Multiple simultaneous actions in InputSnapshot |
| FrameOutput matches | step() returns correct frame_count and delta each frame |
| Enemy spawner timer advances | Script variable accumulation across 60 frames (~1.0s) |
| Deterministic | Two identical runs produce identical final positions |
| Input does not persist | Auto-clear after step() prevents stale input leak |

**Pipeline exercised**: scene load → script attach → lifecycle (enter_tree/ready) → input routing → fixed-timestep physics → process callbacks → script execution → frame bookkeeping → input cleanup

---

## CI Tier Summary

See `engine-rs/TESTING.md` for full tier definitions and commands.

| Tier | Scope | Time |
|------|-------|------|
| Tier 1 | Fast unit + integration (skip golden/stress/bench) | <10s |
| Tier 2 | + golden comparisons + staleness checks | ~30s |
| Tier 3 | Everything including stress, render goldens, benchmarks | ~60s+ |
