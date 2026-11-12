# Patina Engine Examples

Run any example from the `engine-rs/` directory with:

```
cargo run --example <name>
```

For a detailed gallery with feature coverage and screenshots, see
[docs/EXAMPLE_GALLERY.md](../docs/EXAMPLE_GALLERY.md).

---

## space_shooter

**File:** `examples/space_shooter.rs`

Complete 2D space shooter mini-game. Player ship, enemies, projectiles,
explosions, particles, and audio — 300-frame deterministic simulation.
Exercises scene tree, physics (Area2D overlap), input, rendering, and audio.

```
cargo run --example space_shooter
```

---

## space_shooter_live

**File:** `examples/space_shooter_live.rs`

Interactive space shooter with HTTP frame server. Play in your browser with
real-time keyboard input. Supports custom port and headless mode.

```
cargo run --example space_shooter_live
# Open http://localhost:8080 in your browser
```

---

## platformer_demo

**File:** `examples/platformer_demo.rs`

Full-featured platformer simulation: player character, coins, moving
platforms, enemy AI, tweens, particles, and audio. 120-frame deterministic run.

```
cargo run --example platformer_demo
```

---

## demo_2d

**File:** `examples/demo_2d.rs`

End-to-end 2D pipeline demo. Loads a `.tscn` scene, simulates 60 frames of
physics, renders the final frame with the software rasterizer.

```
cargo run --example demo_2d
```

---

## hello_gdscript

**File:** `examples/hello_gdscript.rs`

Minimal GDScript interpreter example. Runs a short GDScript program with
variables, arithmetic, strings, and `print` calls.

```
cargo run --example hello_gdscript
```

---

## run_project

**File:** `examples/run_project.rs`

Loads a complete Godot project from disk — parses `project.godot`, loads the
main scene, instances nodes, attaches scripts, runs 60 frames of lifecycle
callbacks.

```
cargo run --example run_project
cargo run --example run_project -- /path/to/project
```

---

## editor

**File:** `examples/editor.rs`

Launches the Patina Editor — web-based scene editor with scene tree viewer,
inspector, viewport, and VCS integration.

```
cargo run --example editor
# Open http://localhost:8082
```

---

## benchmarks

**File:** `examples/benchmarks.rs`

Performance benchmark suite for core subsystems. Run in release mode for
accurate numbers.

```
cargo run --example benchmarks --release
```
