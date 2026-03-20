# Example → Fixture Map

Each maintained example exists to feed measurable fixtures and golden targets —
not as proof of engine completion. This file records the mapping.

## platformer_demo.rs

**Status**: Active fixture feeder

| Subsystem | Fixture / Golden | Path |
|-----------|-----------------|------|
| Traces | Lifecycle + signal ordering | `fixtures/golden/traces/platformer_patina.json` |
| Physics | Gravity, friction, static blocking | `fixtures/golden/physics/gravity_fall_30frames.json`, `friction_decel_30frames.json`, `static_blocking_60frames.json` |
| Render | Platformer frame capture | `engine-rs/output/platformer_frame.ppm` |
| Input | Action-map binding coverage | `engine-rs/tests/input_map_loading_test.rs` |

**Parity gaps**: Tween property interpolation not yet golden-tested. Particle determinism not yet compared against upstream oracle.

## space_shooter.rs

**Status**: Active fixture feeder

| Subsystem | Fixture / Golden | Path |
|-----------|-----------------|------|
| Traces | Lifecycle + signal ordering | `fixtures/golden/traces/space_shooter_patina.json` |
| Physics | Area2D overlap / collision | (covered by physics integration tests) |
| Render | Shooter frame capture | `engine-rs/output/space_shooter_frame.ppm` |
| Oracle | Runtime trace parity | `fixtures/golden/traces/space_shooter_upstream_mock.json` |

**Parity gaps**: Area2D overlap ordering not yet golden-compared against upstream. Render diff not yet automated in CI.

## space_shooter_live.rs

**Status**: Windowed variant of space_shooter — exercises `winit` backend path. No independent fixture targets; shares shooter golden data.

## demo_2d.rs

**Status**: Minimal 2D rendering smoke test.

| Subsystem | Fixture / Golden | Path |
|-----------|-----------------|------|
| Render | Basic 2D draw ordering | `engine-rs/output/demo_2d_frame.ppm` |

## hello_gdscript.rs

**Status**: GDScript interpreter smoke test. Exercises `gdscript-interop` bindings.

| Subsystem | Fixture / Golden | Path |
|-----------|-----------------|------|
| Scripting | Script execution traces | `fixtures/golden/traces/test_scripts_patina.json` |

## run_project.rs

**Status**: Headless project runner. Used by trace parity tests to load `.tscn` scenes and emit lifecycle traces.

| Subsystem | Fixture / Golden | Path |
|-----------|-----------------|------|
| Traces | All scene trace goldens | `fixtures/golden/traces/*_patina.json` |

## editor.rs

**Status**: **Maintenance-only** — workflow support tool, not a milestone target.

The editor example is classified outside active engine milestone progress.
It exists to support developer workflow (scene inspection, property editing)
and should receive only stability/maintenance work until runtime parity exits
are met. See `AGENTS.md` rule: "No editor work until runtime milestones are stable."

Editor tests are maintenance coverage, not feature-expansion evidence.

## benchmarks.rs

**Status**: Performance baseline. Feeds `engine-rs/output/` benchmark data.
Not a parity target — used for regression detection only.
