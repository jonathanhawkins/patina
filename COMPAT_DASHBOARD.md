# Compatibility Dashboard

**Last updated**: 2026-03-19 (pat-0l5: oracle parity updated from 37.4% to 90.5%)
**Test suite**: 3,200+ tests passing across workspace (integration + crate units)
**Golden files**: 49 (8 physics, 16 traces, 11 scenes, 5 resources, 9 render)

---

## Subsystem Status at a Glance

| Subsystem | Status | Test Count | Goldens | Parity |
|-----------|--------|------------|---------|--------|
| Core Runtime | Measured | 142 | — | ~100% |
| Variant System | Measured | 93 | — | ~100% |
| Object Model | Measured | 100 | — | ~80% |
| Signals | Measured | 28 | — | ~60% |
| Notifications | Measured | 30 | — | ~85% |
| Resources | Measured | 189 | 5 | ~95% |
| Scene System | Measured | 746 | 11 | ~90% |
| GDScript Interop | Measured | 381 | — | ~85% |
| 2D Rendering | Measured | 150 | 9 | Golden-based |
| 2D Physics | Measured | 140 | 8 | Deterministic |
| Input | Measured | 146 | — | Measured |
| Vertical Slice | Measured | 16 | — | End-to-end |
| Audio | Claimed | 17 | — | Stub only |
| Platform | Claimed | 24 | — | Stub only |
| Editor | Claimed | 291 | — | Maintenance |
| 3D Runtime | Deferred | — | — | N/A |

---

## Oracle Parity Results

Comparison of Godot 4.5.1 oracle golden outputs vs live Patina headless runner across 9 fixture scenes. The runner uses `class_defaults.rs` filtering to compare only explicitly-set and semantically-meaningful properties (excluding Godot-internal defaults that Patina does not emit). Tests: `oracle_parity_test.rs` (32) + `oracle_regression_test.rs` (43).

| Scene | Comparisons | Matched | Parity | Notes |
|-------|-------------|---------|--------|-------|
| `minimal.tscn` | 1 | 1 | **100.0%** | Single Node, perfect match |
| `hierarchy.tscn` | 3 | 3 | **100.0%** | Full node/class/property match |
| `with_properties.tscn` | 5 | 5 | **100.0%** | Player position/modulate match |
| `space_shooter.tscn` | 8 | 8 | **100.0%** | Player/EnemySpawner positions match |
| `platformer.tscn` | 12 | 12 | **100.0%** | Node structure and properties match |
| `physics_playground.tscn` | 15 | 10 | **66.7%** | Physics node classes match; simulation gaps remain |
| `signals_complex.tscn` | 9 | 9 | **100.0%** | Signal node structure matches |
| `test_scripts.tscn` | 5 | 4 | **80.0%** | Script vars mostly match; one frame-accumulated value diverges |
| `ui_menu.tscn` | 5 | 5 | **100.0%** | Complete match |
| **Overall** | **63** | **57** | **90.5%** | Up from 37.4% baseline |

**Improvement drivers** (from 37.4% → 90.5%):
- `class_defaults.rs` filtering: removes false-negative comparisons on Godot-internal default properties that Patina does not serialize
- Bare var sync: script variable initial values now correctly propagate
- `self.position.x` fix: member-access expressions in GDScript correctly resolve

---

## Property Gap Analysis

### Measured Properties (test-backed)
- Node names and class names: **100%** across all scenes (75 oracle tests)
- Node hierarchy (parent/child structure): **100%** (746 scene system tests)
- Explicitly-set Vector2 positions: **Match** (oracle parity tests)
- Script variable initial values: **Match** for Int/Float types (381 GDScript tests)
- Lifecycle ordering: **85%** (30 notification tests + 14 lifecycle trace tests)
- Signal dispatch: **60%** (28 signal tests — declaration + emit verified)
- Physics stepping: **Deterministic** (140 physics tests + 8 goldens)
- 2D rendering: **Golden-based** (150 render tests + 9 golden images)
- Input routing: **Measured** (146 input tests — snapshot, map loading, action coverage)

### Known Gaps (not yet test-backed)

| Gap | Category | Impact |
|-----|----------|--------|
| Physics playground properties (5 mismatches) | Partial | Medium — physics simulation parity for playground scene not yet complete |
| Script frame-accumulated values (1 mismatch) | Partial | Low — initial values match, one frame-accumulated divergence in test_scripts |
| Audio playback | Deferred | Low — stub only, no Godot behavior to compare |

---

## Test File Reference

### Measured subsystems — backing test files

| Subsystem | Test files |
|-----------|-----------|
| Core Runtime | `gdcore` unit tests |
| Variant System | `gdvariant` unit tests |
| Object Model | `gdobject` units, `object_property_reflection_test`, `classdb_parity_test` |
| Signals | `signal_dispatch_parity_test`, `signal_trace_parity_test` |
| Notifications | `notification_coverage_test`, `lifecycle_trace_parity_test` |
| Resources | `gdresource` units, `cache_regression_test`, `unified_loader_test`, `resource_uid_cache_test` |
| Scene System | `gdscene` units, `golden_tests`, `instancing_ownership_test`, `packed_scene_edge_cases_test`, `frame_processing_semantics_test` |
| GDScript Interop | `gdscript_interop` units, `demo_scenes_test` |
| Trace Parity | `trace_parity_test`, `multi_scene_trace_parity_test`, `frame_trace_test` |
| Oracle Parity | `oracle_parity_test`, `oracle_regression_test` |
| 2D Rendering | `gdrender2d` units, `render_pipeline`, `render_golden_test` |
| 2D Physics | `gdphysics2d` units, `physics_integration_test` |
| Input | `gdplatform` units, `input_map_loading_test`, `input_action_coverage_test` |
| Vertical Slice | `vertical_slice_test` |

### Claimed subsystems — what's missing

| Subsystem | Has | Needs |
|-----------|-----|-------|
| Audio | 17 stub tests | Godot audio behavior comparison |
| Platform | 24 lifecycle tests | Godot windowing behavior comparison |
| Editor | 291 tests | No parity target (maintenance-only) |

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
