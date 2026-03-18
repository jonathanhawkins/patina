# Patina Engine Examples

Run any example from the `engine-rs/` directory with:

```
cargo run --example <name>
```

---

## demo_2d

**File:** `examples/demo_2d.rs`

End-to-end 2D pipeline demo. Loads a scene from `fixtures/scenes/demo_2d.tscn`, simulates 60 frames of physics (rigid player, rigid enemy, static ground), renders the final frame with the software rasteriser, and writes `output/demo_frame.ppm`.

```
cargo run --example demo_2d
```

---

## platformer_demo

**File:** `examples/platformer_demo.rs`

Full-featured platformer simulation exercising every major subsystem: scene tree, 2D physics, input mapping, rendering, tweens, particles, and audio. Simulates 120 frames, prints a JSON summary, and writes `output/platformer_frame.ppm`.

```
cargo run --example platformer_demo
```

---

## hello_gdscript

**File:** `examples/hello_gdscript.rs`

Minimal GDScript interpreter example. Runs a short GDScript program — variable assignment, arithmetic, string concatenation, and `print` calls — and prints the output to stdout. Good starting point for scripting integration.

```
cargo run --example hello_gdscript
```

---

## benchmarks

**File:** `examples/benchmarks.rs`

Performance benchmark suite for core subsystems. Runs timed benchmarks and prints results as JSON to stdout. No external dependencies — uses `std::time::Instant`.

```
cargo run --example benchmarks --release
```
