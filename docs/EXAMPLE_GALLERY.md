# Example Project Gallery

A curated collection of demo games and projects built with the Patina Engine.
Each example demonstrates different engine subsystems and serves as a starting
point for your own projects.

Run any example from the `engine-rs/` directory:

```bash
cd engine-rs
cargo run --example <name>
```

---

## 1. Space Shooter

**File:** `examples/space_shooter.rs`

A complete 2D space shooter mini-game running as a deterministic 300-frame
simulation. The player ship dodges enemies and fires projectiles while
explosions and particles fill the screen.

**Subsystems demonstrated:**
- Scene tree with dynamic node spawning and removal
- 2D physics with Area2D overlap detection (bullet-enemy hits)
- Input mapping (arrow keys for movement, space to fire)
- Software 2D rendering with sprites, particles, and draw commands
- Audio playback (shoot and explosion sounds)
- CPU particle effects for explosions
- Main loop with fixed timestep physics

**How to run:**
```bash
cargo run --example space_shooter
```

**Output:** Renders to `output/space_shooter_frame.ppm` and prints a JSON
summary with score, enemies destroyed, and frame count.

---

## 2. Space Shooter Live (Interactive)

**File:** `examples/space_shooter_live.rs`

The same space shooter, but interactive! Starts an HTTP frame server that
streams rendered frames to your browser. Play with keyboard input in real time.

**Subsystems demonstrated:**
- Everything from Space Shooter, plus:
- HTTP frame server for live browser preview
- Real-time keyboard input from browser events
- Configurable frame count and port

**How to run:**
```bash
cargo run --example space_shooter_live
# Then open http://localhost:8080 in your browser

# Custom port:
cargo run --example space_shooter_live -- --port 9090

# Headless mode (no browser, deterministic):
cargo run --example space_shooter_live -- --headless
```

---

## 3. Platformer Demo

**File:** `examples/platformer_demo.rs`

A full-featured platformer simulation exercising every major 2D subsystem.
Features a player character with physics, collectible coins, moving platforms,
enemy AI, tweened animations, particles, and audio — all integrated into a
120-frame deterministic run.

**Subsystems demonstrated:**
- Scene tree with hierarchical node structure
- CharacterBody2D physics with move_and_slide
- RigidBody2D for dynamic objects
- StaticBody2D for terrain and platforms
- Input actions (move_left, move_right, jump)
- Tween animations
- CPU particles (dust, coin sparkle)
- Audio (jump, coin collect, background music)
- Camera2D with viewport following

**How to run:**
```bash
cargo run --example platformer_demo
```

**Output:** Renders to `output/platformer_frame.ppm` and prints a JSON summary
with player position, coins collected, and physics stats.

---

## 4. 2D Pipeline Demo

**File:** `examples/demo_2d.rs`

An end-to-end 2D pipeline demonstration. Loads a scene from a `.tscn` fixture
file, simulates 60 frames of physics with rigid and static bodies, then
renders the final frame with the software rasterizer.

**Subsystems demonstrated:**
- `.tscn` scene file loading
- Scene tree instantiation from packed scenes
- 2D physics world simulation (rigid + static bodies)
- Software 2D rendering pipeline
- Frame capture to PPM image

**How to run:**
```bash
cargo run --example demo_2d
```

**Output:** Renders to `output/demo_frame.ppm`.

---

## 5. GDScript Hello World

**File:** `examples/hello_gdscript.rs`

Minimal GDScript interpreter example. Parses and executes a short GDScript
program demonstrating variable assignment, arithmetic, string concatenation,
control flow, and `print` calls.

**Subsystems demonstrated:**
- GDScript tokenizer and parser
- Tree-walk interpreter execution
- Variable scoping and types
- Built-in function dispatch (print, str, len)
- Arithmetic and string operations

**How to run:**
```bash
cargo run --example hello_gdscript
```

**Output:** Prints GDScript execution results to stdout.

---

## 6. Project Loader

**File:** `examples/run_project.rs`

Loads a complete Godot project from disk — parses `project.godot`, loads the
main scene, instances all nodes, attaches GDScript files, runs lifecycle
callbacks (`_ready`, `_process`), and executes 60 frames of the main loop.

**Subsystems demonstrated:**
- Project file parsing (`project.godot`)
- Packed scene loading and instantiation
- GDScript attachment and lifecycle dispatch
- Main loop (idle + physics stepping)
- Notification system (_ready, _process, _physics_process)

**How to run:**
```bash
# With the sample project:
cargo run --example run_project

# With a custom Godot project:
cargo run --example run_project -- /path/to/your/project
```

---

## 7. Editor

**File:** `examples/editor.rs`

Launches the Patina Editor — a web-based scene editor served over HTTP.
Provides a scene tree viewer, node inspector, viewport renderer, and asset
browser.

**Subsystems demonstrated:**
- HTTP editor server
- Scene tree manipulation via REST API
- Property inspector with typed editors
- Viewport rendering (2D and 3D)
- Theme system
- VCS (git) integration

**How to run:**
```bash
cargo run --example editor
# Then open http://localhost:8082 in your browser
```

---

## 8. Benchmarks

**File:** `examples/benchmarks.rs`

Performance benchmark suite for core engine subsystems. Measures throughput
for math operations, physics stepping, scene tree manipulation, resource
loading, and rendering.

**How to run:**
```bash
# Run in release mode for accurate numbers:
cargo run --example benchmarks --release
```

**Output:** JSON benchmark results to stdout.

---

## Feature Coverage Matrix

Which engine subsystems each demo exercises:

| Example | Scene Tree | Physics | Input | Rendering | Audio | GDScript | Particles |
|---------|:----------:|:-------:|:-----:|:---------:|:-----:|:--------:|:---------:|
| Space Shooter | x | x | x | x | x | | x |
| Space Shooter Live | x | x | x | x | x | | x |
| Platformer Demo | x | x | x | x | x | | x |
| 2D Pipeline Demo | x | x | | x | | | |
| GDScript Hello | | | | | | x | |
| Project Loader | x | | | | | x | |
| Editor | x | | | x | | | |
| Benchmarks | x | x | | x | | | |

---

## Creating Your Own Example

Use the demos as templates for your own projects:

1. **Copy the closest example** to a new file in `examples/`
2. **Add it to `Cargo.toml`** under `[[example]]` if needed
3. **Modify the scene setup** — change node types, physics bodies, input bindings
4. **Run with** `cargo run --example your_example`

### Minimal Starter Template

```rust
use gdcore::math::Vector2;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;

fn main() {
    println!("=== My Patina Game ===");

    // Create scene tree
    let mut tree = SceneTree::new();
    let root = tree.root();

    // Add nodes
    let player = tree.add_child(root, Node::new("Player"));

    // Game loop
    for frame in 0..60 {
        let delta = 1.0 / 60.0;
        tree.process(delta);
        tree.physics_process(delta);
    }

    println!("Done! {} frames simulated.", 60);
}
```

---

## See Also

- [Migration Guide](migration-guide.md) — Porting Godot projects to Patina
- [GDScript Compatibility](GDSCRIPT_COMPATIBILITY.md) — Supported GDScript features
- [Node Type Table](migration-guide.md#node-type-compatibility-table) — All supported node types
