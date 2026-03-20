# Examples to Fixture and Target Map

Every maintained example exists to feed measurable test fixtures and golden
targets, not as standalone proof of engine completion. This document maps each
example to the subsystems it exercises, the test files that verify it, and the
golden artifacts it produces.

For the per-fixture path table, see also `examples/FIXTURE_MAP.md`.

## platformer_demo.rs

**Classification**: Active fixture feeder
**Subsystems**: Scene tree, 2D physics (gravity, friction, static blocking), input, rendering, tweens, particles, audio

| Target Type | Test / Fixture | Path |
|-------------|---------------|------|
| Integration | Platformer test suite (physics, input, particles, tweens, audio) | `tests/platformer_test.rs` |
| Trace parity | Lifecycle + signal ordering golden | `fixtures/golden/traces/platformer_patina.json` |
| Oracle parity | Upstream mock comparison | `fixtures/golden/traces/platformer_upstream_mock.json` |
| Physics golden | Gravity, friction, static blocking | `fixtures/golden/physics/gravity_fall_30frames.json`, `friction_decel_30frames.json`, `static_blocking_60frames.json` |
| Render | Frame capture | `output/platformer_frame.ppm` |
| Input | Action-map binding coverage | `tests/input_map_loading_test.rs` |
| Benchmark | Frame stepping, loading, physics baselines | `tests/bench_runtime_baselines.rs` |

**Parity gaps**: Tween property interpolation not yet golden-tested. Particle determinism not compared against upstream oracle.

## space_shooter.rs

**Classification**: Active fixture feeder
**Subsystems**: Scene tree, Area2D physics (overlap detection, collision), input, rendering, spawning, particles, audio

| Target Type | Test / Fixture | Path |
|-------------|---------------|------|
| Integration | Score, movement, spawn, bullet validation | `tests/space_shooter_test.rs` |
| Trace parity | Lifecycle + signal ordering golden | `fixtures/golden/traces/space_shooter_patina.json` |
| Oracle parity | Upstream mock comparison | `fixtures/golden/traces/space_shooter_upstream_mock.json` |
| Render | Frame capture | `output/space_shooter_frame.ppm` |
| Benchmark | Frame stepping baselines | `tests/bench_runtime_baselines.rs` |

**Parity gaps**: Area2D overlap ordering not golden-compared against upstream. Render diff not automated in CI.

## space_shooter_live.rs

**Classification**: Windowed variant of space_shooter
**Subsystems**: Same as `space_shooter` + winit backend + HTTP frame server

| Target Type | Test / Fixture | Path |
|-------------|---------------|------|
| Integration | Server HTML/BMP/JSON response, frame serving, input | `tests/space_shooter_live_test.rs` |

No independent fixture targets. Shares `space_shooter` golden data.

## demo_2d.rs

**Classification**: Minimal 2D rendering smoke test
**Subsystems**: Scene tree, physics (rigid bodies, gravity), software rendering

| Target Type | Test / Fixture | Path |
|-------------|---------------|------|
| Integration | Demo 2D test suite | `tests/demo_2d_test.rs` |
| Render | Basic 2D draw ordering | `output/demo_2d_frame.ppm` |
| Scene | Demo scene fixture | `fixtures/scenes/demo_2d.tscn` |

## hello_gdscript.rs

**Classification**: GDScript interpreter smoke test
**Subsystems**: GDScript interpreter (`gdscript-interop`)

| Target Type | Test / Fixture | Path |
|-------------|---------------|------|
| Trace parity | Script execution traces | `fixtures/golden/traces/test_scripts_patina.json` |
| Oracle parity | Upstream mock comparison | `fixtures/golden/traces/test_scripts_upstream_mock.json` |

## run_project.rs

**Classification**: Headless project runner (utility)
**Subsystems**: Project loader, scene tree, lifecycle management, main loop

| Target Type | Test / Fixture | Path |
|-------------|---------------|------|
| Trace parity | All scene trace goldens | `fixtures/golden/traces/*_patina.json` |
| Multi-scene | Trace parity across all scenes | `tests/multi_scene_trace_parity_test.rs` |

Used internally by trace parity tests. Not a demo.

## editor.rs

**Classification**: Maintenance-only workflow support
**Subsystems**: Scene tree, editor server (HTTP), web UI, rendering

| Target Type | Test / Fixture | Path |
|-------------|---------------|------|
| Integration | Server endpoints, tree operations, property mutations | `tests/editor_test.rs` |

The editor example is classified outside active engine milestone progress.
It supports developer workflow (scene inspection, property editing) and should
receive only stability/maintenance work until runtime parity exits are met.
See `AGENTS.md` rule: "No editor work until runtime milestones are stable."

## benchmarks.rs

**Classification**: Performance baseline (not a parity target)
**Subsystems**: Scene loading, resource loading, physics stepping, rendering

| Target Type | Test / Fixture | Path |
|-------------|---------------|------|
| Benchmark | Runtime baselines (all subsystems) | `tests/bench_runtime_baselines.rs` |

Used for regression detection, not parity measurement.

## Related Test Coverage

These test files exercise engine subsystems across multiple examples:

| Test File | Scope |
|-----------|-------|
| `tests/frame_trace_test.rs` | Frame-by-frame trace validation |
| `tests/trace_parity_test.rs` | Single-scene trace parity |
| `tests/multi_scene_trace_parity_test.rs` | All-scene trace parity |
| `tests/oracle_parity_test.rs` | Oracle comparison coverage |
| `tests/physics_integration_test.rs` | Physics subsystem integration |
| `tests/render_golden_test.rs` | Render golden comparison |
| `tests/render_vertical_slice_test.rs` | End-to-end 2D render slice |
| `tests/window_lifecycle_test.rs` | Window lifecycle and resize flow |
| `tests/platform_backend_test.rs` | PlatformBackend + MainLoop integration |
