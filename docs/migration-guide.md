# Patina Engine Migration Guide

A practical guide for users adopting Patina runtime milestones.
Each section maps to a project phase from `prd/PORT_GODOT_TO_RUST_PLAN.md`
and covers what changed, what you can use, and how to migrate.

---

## Overview

Patina is a Rust-native runtime that aims for behavior-compatibility with
Godot. It is **not** a drop-in replacement. Migration happens in stages
aligned with the project's milestone phases:

| Milestone | Phase | What You Get |
|-----------|-------|--------------|
| Headless Runtime | 3 | Scene loading, object model, signals, resources |
| 2D Vertical Slice | 4 | Sprites, transforms, input, 2D physics, rendering |
| Broader Runtime | 5 | Audio, extended input, richer resource types |
| 3D Runtime Slice | 6 | 3D nodes, cameras, lights, 3D physics |
| Platform Layer | 7 | Windowing, desktop targets, packaging |
| Editor Support | 8 | Inspector, import pipeline, editor APIs |
| Stable Release | 9 | Benchmarks, fuzz tests, migration guide (this doc) |

---

## Milestone 1: Headless Runtime (Phase 3)

### Crates Available

- **`gdcore`** -- Core math types (`Vector2`, `Vector3`, `Quaternion`, `Basis`,
  `Transform2D`, `Transform3D`, `Rect2`, `Color`, `NodePath`, `StringName`)
- **`gdvariant`** -- Variant type system matching Godot's dynamic typing
- **`gdobject`** -- Object model with signals, notifications, class registration
- **`gdresource`** -- `.tres` resource loading and saving
- **`gdscene`** -- `.tscn` packed scene loading and instancing

### What You Can Do

- Load `.tscn` and `.tres` files created in Godot
- Build a scene tree with nodes, connect signals, receive notifications
- Run headless (no window, no rendering) for servers, tests, or CI

### Migration Steps

1. **Add dependencies** in your `Cargo.toml`:
   ```toml
   [dependencies]
   gdcore = { path = "engine-rs/crates/gdcore" }
   gdvariant = { path = "engine-rs/crates/gdvariant" }
   gdobject = { path = "engine-rs/crates/gdobject" }
   gdresource = { path = "engine-rs/crates/gdresource" }
   gdscene = { path = "engine-rs/crates/gdscene" }
   ```

2. **Load a scene**:
   ```rust
   use gdscene::packed_scene::PackedScene;

   let scene = PackedScene::load("res://main.tscn")?;
   let root = scene.instantiate();
   ```

3. **Connect signals** (Godot equivalent: `node.connect("signal", callable)`):
   ```rust
   use gdobject::signal::Signal;

   // Signals use string names, matching Godot's API
   node.connect("ready", callback);
   ```

4. **Key differences from Godot**:
   - No GDScript -- logic is written in Rust
   - `Variant` is an enum, not a pointer-based type
   - `NodePath` and `StringName` are value types with interning
   - Resource paths use `res://` prefix, same as Godot

---

## Milestone 2: 2D Vertical Slice (Phase 4)

### Crates Available

- **`gdserver2d`** -- 2D rendering server (draw commands, layers, sprites)
- **`gdrender2d`** -- Software 2D renderer with frame buffer output
- **`gdphysics2d`** -- 2D physics (circles, rectangles, capsules, collision)
- **`gdplatform`** -- Input handling, timing, windowed or headless backends

### What You Can Do

- Render 2D sprites and shapes to a window or headless frame buffer
- Handle keyboard, mouse, and gamepad input
- Run 2D physics simulations with collision detection
- Build simple 2D games or prototypes

### Migration Steps

1. **Add 2D dependencies**:
   ```toml
   [dependencies]
   gdserver2d = { path = "engine-rs/crates/gdserver2d" }
   gdrender2d = { path = "engine-rs/crates/gdrender2d" }
   gdphysics2d = { path = "engine-rs/crates/gdphysics2d" }
   gdplatform = { path = "engine-rs/crates/gdplatform" }
   ```

2. **Set up input** (Godot equivalent: `Input.is_action_pressed("move_left")`):
   ```rust
   use gdplatform::input::{InputMap, InputState, Key, ActionBinding};

   let mut map = InputMap::new();
   map.add_action("move_left", ActionBinding::Key(Key::Left));

   let mut state = InputState::new();
   // In your frame loop:
   if state.is_action_pressed("move_left") {
       // handle movement
   }
   ```

3. **2D physics** (Godot equivalent: `CharacterBody2D.move_and_slide()`):
   ```rust
   use gdphysics2d::world::PhysicsWorld2D;
   use gdphysics2d::body::PhysicsBody2D;
   use gdphysics2d::shape::Shape2D;

   let mut world = PhysicsWorld2D::new();
   // Bodies are added to the world and stepped each frame
   world.step(delta);
   ```

4. **Key differences from Godot**:
   - No scene-tree-integrated physics nodes yet -- use the physics world directly
   - Input uses `InputMap` + `InputState` rather than the global `Input` singleton
   - Rendering is software-based initially (no GPU shaders)
   - `InputSnapshot` provides a frozen read-only view of input state per frame

---

## Milestone 3: Broader Runtime and 3D Prep (Phase 5)

### Crates Available

- **`gdaudio`** -- Audio bus system, playback control
- **`gdscript-interop`** -- GDScript variable and export interop layer

### What You Can Do

- Play audio with bus routing
- Handle extended input (touch, multi-gamepad)
- Load more complex resource types

### Migration Steps

1. **Audio setup** (Godot equivalent: `AudioStreamPlayer.play()`):
   ```rust
   use gdaudio::{AudioBus, AudioServer};

   let mut server = AudioServer::new();
   let bus = server.add_bus("Master");
   // Playback control through the audio server
   ```

2. **Key differences from Godot**:
   - Audio is managed through an explicit server, not node-based
   - Bus routing is programmatic rather than configured in the editor

---

## Milestone 4: 3D Runtime Slice (Phase 6)

### Crates Available

- **`gdserver3d`** -- 3D rendering server (meshes, materials, lights, cameras)
- **`gdrender3d`** -- Software 3D renderer with depth buffer
- **`gdphysics3d`** -- 3D physics (spheres, boxes, capsules, raycasting)

### What You Can Do

- Render 3D meshes with perspective projection
- Set up directional, point, and spot lights
- Run 3D physics simulations with collision and raycasting
- Build simple 3D scenes

### Migration Steps

1. **Add 3D dependencies**:
   ```toml
   [dependencies]
   gdserver3d = { path = "engine-rs/crates/gdserver3d" }
   gdrender3d = { path = "engine-rs/crates/gdrender3d" }
   gdphysics3d = { path = "engine-rs/crates/gdphysics3d" }
   ```

2. **3D physics** (Godot equivalent: `RigidBody3D` with `CollisionShape3D`):
   ```rust
   use gdphysics3d::world::PhysicsWorld3D;
   use gdphysics3d::body::{PhysicsBody3D, BodyType3D, BodyId3D};
   use gdphysics3d::shape::Shape3D;

   let mut world = PhysicsWorld3D::new();
   let body = PhysicsBody3D::new(
       BodyId3D(0),
       BodyType3D::Rigid,
       position,
       Shape3D::Sphere { radius: 1.0 },
       1.0, // mass
   );
   world.add_body(body);
   world.step(1.0 / 60.0);
   ```

3. **Raycasting** (Godot equivalent: `PhysicsDirectSpaceState3D.intersect_ray()`):
   ```rust
   let hit = world.raycast(origin, direction);
   if let Some(h) = hit {
       println!("Hit body {:?} at {:?}", h.body_id, h.point);
   }
   ```

4. **Key differences from Godot**:
   - 3D rendering is software wireframe initially (no GPU pipeline)
   - Physics uses direct world API, not node-tree integration
   - Materials are data structs, not shader-based

---

## Milestone 5: Platform Layer (Phase 7)

### Crates Available

- **`gdplatform`** (extended) -- Desktop targets, export configs, OS detection

### What You Can Do

- Build for Linux (x86_64, aarch64), macOS (x86_64, Apple Silicon), Windows (x86_64, aarch64)
- Query platform capabilities at runtime
- Package your game with export configurations

### Migration Steps

1. **Check platform support**:
   ```rust
   use gdplatform::platform_targets::{
       current_target, supports_capability, PlatformCapability,
   };

   let target = current_target().expect("unsupported platform");
   println!("Running on: {}", target.name);

   if supports_capability(PlatformCapability::GpuRendering) {
       // Use GPU path
   }
   ```

2. **Export configuration** (Godot equivalent: Export dialog presets):
   ```rust
   use gdplatform::export::ExportConfig;

   let config = ExportConfig::new("x86_64-unknown-linux-gnu", "MyGame")
       .with_resource("res://")
       .with_icon("icon.png");
   ```

3. **Supported targets**:

   | Target | Triple | CI Tested |
   |--------|--------|-----------|
   | Linux x86_64 | `x86_64-unknown-linux-gnu` | Yes |
   | Linux aarch64 | `aarch64-unknown-linux-gnu` | No |
   | macOS x86_64 | `x86_64-apple-darwin` | Yes |
   | macOS aarch64 | `aarch64-apple-darwin` | Yes |
   | Windows x86_64 | `x86_64-pc-windows-msvc` | Yes |
   | Windows aarch64 | `aarch64-pc-windows-msvc` | No |
   | Web (WASM) | `wasm32-unknown-unknown` | No |

---

## Milestone 6: Editor Support (Phase 8)

### Crates Available

- **`gdeditor`** -- Editor server, inspector, scene tree viewer

### What You Can Do

- Run a local editor server for scene inspection
- View and edit node properties through a web UI

### Migration Steps

- Editor features are under active development
- The editor is gated behind runtime parity milestones
- See `docs/EDITOR_SETTINGS.md` for current configuration

---

## Porting a Godot 4 Project Step-by-Step

This section walks through porting an existing Godot 4 project to Patina.

### Step 1: Assess Compatibility

Before porting, check which Godot features your project uses against the
compatibility table below.

#### Node Type Compatibility Table

**Legend**: Full = works as in Godot | Partial = subset of properties/methods | Stub = recognized but minimal logic | -- = not supported

##### Core & Base Types

| Godot Type | Status | Patina Crate | Notes |
|------------|--------|--------------|-------|
| `Object` | Full | `gdobject` | Signals, notifications, class info |
| `Node` | Full | `gdscene` | Scene tree lifecycle, groups, paths |
| `Resource` | Full | `gdresource` | `.tres` load/save, UID registry |
| `CanvasItem` | Full | `gdscene` | Base for all 2D drawable nodes |

##### 2D Nodes

| Godot Type | Status | Patina Crate | Notes |
|------------|--------|--------------|-------|
| `Node2D` | Full | `gdscene` | Position, rotation, scale, z-index |
| `Sprite2D` | Full | `gdscene` | Texture, offset, flip, region |
| `AnimatedSprite2D` | Full | `gdscene` | SpriteFrames, animation playback |
| `Camera2D` | Full | `gdscene` | Zoom, offset, smoothing, limits |
| `TileMap` | Partial | `gdscene` | Tile placement; no runtime autotiling |
| `Marker2D` | Full | `gdscene` | Position marker |
| `Line2D` | Full | `gdscene` | Line drawing with width/color |
| `Polygon2D` | Full | `gdscene` | Polygon shape rendering |
| `Parallax2D` | Full | `gdscene` | Parallax scrolling |
| `Path2D` | Full | `gdscene` | Curve/spline paths |
| `PathFollow2D` | Full | `gdscene` | Follows Path2D curves |
| `RemoteTransform2D` | Full | `gdscene` | Transform sync |
| `VisibleOnScreenNotifier2D` | Full | `gdscene` | Visibility callbacks |

##### 2D Physics

| Godot Type | Status | Patina Crate | Notes |
|------------|--------|--------------|-------|
| `RigidBody2D` | Full | `gdphysics2d` | Dynamic bodies with forces/impulses |
| `StaticBody2D` | Full | `gdphysics2d` | Immovable collision bodies |
| `CharacterBody2D` | Full | `gdphysics2d` | `move_and_slide()` kinematic controller |
| `Area2D` | Full | `gdphysics2d` | Overlap detection, signals |
| `CollisionShape2D` | Full | `gdphysics2d` | Circle, rectangle, capsule shapes |
| `CollisionPolygon2D` | Full | `gdphysics2d` | Polygon collision boundaries |
| `RayCast2D` | Full | `gdphysics2d` | Raycast queries |
| `CircleShape2D` | Full | `gdphysics2d` | Circle collision primitive |
| `RectangleShape2D` | Full | `gdphysics2d` | Rectangle collision primitive |
| `CapsuleShape2D` | Full | `gdphysics2d` | Capsule collision primitive |

##### 2D Effects & Lighting

| Godot Type | Status | Patina Crate | Notes |
|------------|--------|--------------|-------|
| `PointLight2D` | Full | `gdscene` | Point light source |
| `DirectionalLight2D` | Full | `gdscene` | Directional light |
| `CPUParticles2D` | Partial | `gdscene` | Emission, gravity, velocity; no sub-emitters |
| `GPUParticles2D` | Stub | `gdscene` | Recognized, falls back to CPU |

##### 3D Nodes

| Godot Type | Status | Patina Crate | Notes |
|------------|--------|--------------|-------|
| `Node3D` | Full | `gdscene` | Full Transform3D propagation |
| `MeshInstance3D` | Full | `gdscene` | Mesh rendering with materials |
| `MultiMeshInstance3D` | Full | `gdscene` | Instanced mesh rendering |
| `Sprite3D` | Full | `gdscene` | Billboard sprites in 3D |
| `Camera3D` | Full | `gdscene` | Perspective/orthogonal projection |
| `Skeleton3D` | Full | `gdscene` | Bone transforms, skeletal animation |
| `BoneAttachment3D` | Full | `gdscene` | Attachment to skeleton bones |
| `Decal` | Partial | `gdscene` | Decal projection (software) |
| `ReflectionProbe` | Partial | `gdscene` | Local cubemap capture |
| `NavigationRegion3D` | Partial | `gdscene` | Mesh baking; no runtime pathfinding |
| `Occluder3D` | Stub | `gdscene` | Recognized, no runtime culling |
| `FogVolume` | Partial | `gdscene` | Volumetric fog regions |

##### 3D Physics

| Godot Type | Status | Patina Crate | Notes |
|------------|--------|--------------|-------|
| `RigidBody3D` | Full | `gdphysics3d` | Dynamic bodies, forces, contacts |
| `StaticBody3D` | Full | `gdphysics3d` | Immovable collision bodies |
| `CharacterBody3D` | Full | `gdphysics3d` | `move_and_slide()` 3D controller |
| `Area3D` | Full | `gdphysics3d` | Overlap detection, signals |
| `CollisionShape3D` | Full | `gdphysics3d` | All primitive shapes below |
| `BoxShape3D` | Full | `gdphysics3d` | AABB collision |
| `SphereShape3D` | Full | `gdphysics3d` | Sphere collision |
| `CapsuleShape3D` | Full | `gdphysics3d` | Capsule collision |
| `CylinderShape3D` | Full | `gdphysics3d` | Cylinder collision |
| `ConvexPolygonShape3D` | Full | `gdphysics3d` | Convex hull collision |
| `ConcavePolygonShape3D` | Full | `gdphysics3d` | Trimesh collision |
| `HeightMapShape3D` | Full | `gdphysics3d` | Terrain heightmap |

##### 3D Lighting

| Godot Type | Status | Patina Crate | Notes |
|------------|--------|--------------|-------|
| `DirectionalLight3D` | Full | `gdscene` | Sun/directional with shadow hints |
| `OmniLight3D` | Full | `gdscene` | Point light with shadow cubemap |
| `SpotLight3D` | Full | `gdscene` | Spotlight with cone/attenuation |
| `WorldEnvironment` | Partial | `gdscene` | Sky, ambient; no post-processing |
| `Sky` | Partial | `gdscene` | Panoramic and procedural sky |

##### 3D CSG (Constructive Solid Geometry)

| Godot Type | Status | Patina Crate | Notes |
|------------|--------|--------------|-------|
| `CSGBox3D` | Full | `gdscene` | CSG box boolean operations |
| `CSGSphere3D` | Full | `gdscene` | CSG sphere |
| `CSGCylinder3D` | Full | `gdscene` | CSG cylinder/cone |
| `CSGMesh3D` | Full | `gdscene` | CSG from arbitrary mesh |
| `CSGPolygon3D` | Full | `gdscene` | CSG polygon extrusion |
| `CSGCombiner3D` | Full | `gdscene` | CSG root combiner |

##### 3D Mesh Primitives

| Godot Type | Status | Patina Crate | Notes |
|------------|--------|--------------|-------|
| `BoxMesh` | Full | `gdscene` | Cube/box primitive |
| `SphereMesh` | Full | `gdscene` | Sphere primitive |
| `CapsuleMesh` | Full | `gdscene` | Capsule primitive |
| `CylinderMesh` | Full | `gdscene` | Cylinder/cone primitive |
| `PlaneMesh` | Full | `gdscene` | Plane primitive |
| `QuadMesh` | Full | `gdscene` | Quad primitive |

##### UI / Control Nodes

| Godot Type | Status | Patina Crate | Notes |
|------------|--------|--------------|-------|
| `Control` | Full | `gdscene` | Base UI node, anchors, margins |
| `CanvasLayer` | Full | `gdscene` | Render layer for UI overlays |
| `Label` | Full | `gdscene` | Text display |
| `Button` | Full | `gdscene` | Click/press interaction |
| `Panel` | Full | `gdscene` | Basic panel background |
| `PanelContainer` | Full | `gdscene` | Panel with child margins |
| `MarginContainer` | Full | `gdscene` | Margin/padding layout |
| `VBoxContainer` | Full | `gdscene` | Vertical layout |
| `HBoxContainer` | Full | `gdscene` | Horizontal layout |
| `TabContainer` | Partial | `gdscene` | Tabbed UI; basic switching |
| `Tree` | Partial | `gdscene` | Tree view; no custom draw |
| `ItemList` | Partial | `gdscene` | Scrollable list |
| `TextEdit` | Partial | `gdscene` | Multi-line text; no code completion |
| `LineEdit` | Partial | `gdscene` | Single-line text input |
| `RichTextLabel` | Partial | `gdscene` | BBCode subset |
| `TextureRect` | Full | `gdscene` | Image/texture display |
| `NinePatchRect` | Full | `gdscene` | 9-patch scalable texture |
| `ProgressBar` | Full | `gdscene` | Progress indicator |
| `Slider` / `HSlider` / `VSlider` | Full | `gdscene` | Slider controls |
| `SpinBox` | Full | `gdscene` | Numeric spinner |
| `CheckBox` | Full | `gdscene` | Checkbox toggle |
| `CheckButton` | Full | `gdscene` | Toggle button |
| `OptionButton` | Partial | `gdscene` | Dropdown; basic functionality |

##### Audio

| Godot Type | Status | Patina Crate | Notes |
|------------|--------|--------------|-------|
| `AudioStreamPlayer` | Full | `gdaudio` | Non-positional playback |
| `AudioStreamPlayer2D` | Full | `gdaudio` | 2D positional audio |
| `AudioStreamPlayer3D` | Full | `gdaudio` | 3D positional/spatial audio |

##### Animation

| Godot Type | Status | Patina Crate | Notes |
|------------|--------|--------------|-------|
| `AnimationPlayer` | Partial | `gdscene` | Keyframe playback; no blend trees |
| `Tween` | Stub | `gdscene` | Basic tween; no chaining |

##### Not Yet Supported

The following Godot node types are **not** currently implemented:

| Category | Types |
|----------|-------|
| Networking | `MultiplayerSpawner`, `MultiplayerSynchronizer`, `HTTPRequest` |
| Navigation | `NavigationAgent2D/3D` (runtime pathfinding) |
| Advanced rendering | `SubViewport`, `CanvasGroup`, `BackBufferCopy` |
| Mobile-specific | Touch input nodes, `XRCamera3D`, `XRController3D` |
| Shader pipeline | Custom `ShaderMaterial` (GPU), `VisualShader` |
| GDExtension | `GDExtension` nodes, C# interop |

#### Quick Compatibility Checklist

```
Supported (port directly):
  ✓ Scene tree with 100+ node types (see table above)
  ✓ .tscn and .tres file loading (format=3)
  ✓ Signals and notifications
  ✓ 2D physics (RigidBody2D, StaticBody2D, CharacterBody2D, Area2D)
  ✓ 3D physics (RigidBody3D, StaticBody3D, CharacterBody3D)
  ✓ Keyboard, mouse, and gamepad input
  ✓ Audio playback with bus routing and spatial audio
  ✓ Resource caching and UID registry
  ✓ Full UI/Control layout container set
  ✓ 3D lighting (directional, omni, spot) with shadows
  ✓ CSG boolean operations
  ✓ Skeletal animation with bone attachments

Partially supported (may need adaptation):
  ~ GDScript (must be rewritten in Rust)
  ~ Shaders (software rendering only, no custom shaders)
  ~ Animation (keyframe playback, no blend trees or state machines)
  ~ Some UI widgets (Tree, TextEdit, RichTextLabel) have limited features

Not yet supported (requires workarounds or deferral):
  ✗ Networking/multiplayer
  ✗ Runtime navigation/pathfinding
  ✗ Mobile platforms (iOS, Android)
  ✗ GDExtension/C# interop
  ✗ GPU rendering pipeline
  ✗ XR/VR support
```

### Step 2: Create a Rust Project

```bash
cargo init my-game
cd my-game

# Add Patina dependencies
cat >> Cargo.toml << 'EOF'

[dependencies]
gdcore = { path = "../patina/engine-rs/crates/gdcore" }
gdvariant = { path = "../patina/engine-rs/crates/gdvariant" }
gdobject = { path = "../patina/engine-rs/crates/gdobject" }
gdresource = { path = "../patina/engine-rs/crates/gdresource" }
gdscene = { path = "../patina/engine-rs/crates/gdscene" }
gdplatform = { path = "../patina/engine-rs/crates/gdplatform" }
gdphysics2d = { path = "../patina/engine-rs/crates/gdphysics2d" }
gdrender2d = { path = "../patina/engine-rs/crates/gdrender2d" }
gdserver2d = { path = "../patina/engine-rs/crates/gdserver2d" }
EOF
```

### Step 3: Copy Scene Files

Copy your `.tscn` and `.tres` files from the Godot project:

```bash
cp -r godot-project/*.tscn my-game/scenes/
cp -r godot-project/*.tres my-game/resources/
cp -r godot-project/assets/ my-game/assets/
```

Patina reads the same `.tscn`/`.tres` format as Godot 4.x (`format=3`).

### Step 4: Rewrite GDScript in Rust

This is the main porting effort. Each GDScript file becomes Rust code that
operates on the scene tree.

**GDScript (before)**:
```gdscript
extends CharacterBody2D

var speed = 200.0

func _physics_process(delta):
    var input_dir = Input.get_vector("left", "right", "up", "down")
    velocity = input_dir * speed
    move_and_slide()
```

**Rust (after)**:
```rust
use gdcore::math::Vector2;
use gdplatform::input::InputState;

const SPEED: f32 = 200.0;

fn physics_process(input: &InputState, delta: f64) -> Vector2 {
    let dir = Vector2::new(
        input.get_axis("left", "right"),
        input.get_axis("up", "down"),
    );
    dir.normalized() * SPEED * delta as f32
}
```

### Step 5: Run and Iterate

```bash
cargo run
```

Use the test suite to verify parity with Godot behavior:

```bash
cargo test
```

Common issues during porting:
- **Missing node type**: Check the concept mapping table below for the
  Patina equivalent. If the type is not supported, file a feature request.
- **Property name mismatch**: Patina uses the same property names as Godot.
  If a property is not recognized, check the oracle outputs for the
  canonical name.
- **Signal not firing**: Verify the signal name matches Godot's. Use
  `--nocapture` to see signal trace output.

---

## Known Limitations and Workarounds

This section documents known behavioral differences between Patina and
Godot 4.6 that may affect ported projects, along with recommended workarounds.

### Rendering

| Limitation | Impact | Workaround |
|------------|--------|------------|
| Software rendering only — no GPU shader pipeline | Visual fidelity is lower than Godot; custom shaders don't work | Use built-in material properties (albedo, emission, metallic). For shader-dependent effects, pre-bake textures in Godot and load as static assets. |
| No post-processing stack (glow, DOF, SSAO, SSR) | Scenes relying on post-processing will look flat | Bake ambient occlusion into lightmap textures. Use vertex colors or pre-lit textures for depth cues. |
| No GPU particles | `GPUParticles2D`/`GPUParticles3D` fall back to CPU | Use `CPUParticles2D`/`CPUParticles3D` directly. Reduce particle counts for performance. |
| No `SubViewport` or render-to-texture | Minimap, picture-in-picture, or portal effects won't work | Render secondary views as separate scenes and composite in UI. |

### GDScript

| Limitation | Impact | Workaround |
|------------|--------|------------|
| No GDScript interpreter | All game logic must be written in Rust | Port scripts method-by-method. See the GDScript-to-Rust example in "Step 4" above. |
| No `@onready` or `@export` annotations | Node references and editor-visible properties need manual setup | Use `NodePath` resolution at `_ready()` time. Define configuration as Rust structs loaded from `.tres` resources. |
| No `await` / coroutines | Asynchronous GDScript patterns don't translate directly | Use Rust `async`/channels, or split long operations across frames using the process callback. |
| No `match` pattern-matching on Variant types | GDScript `match` on dictionaries/arrays won't work | Use Rust `match` on `Variant` enum variants. Type patterns map to `Variant::Int`, `Variant::String`, etc. |

### Physics

| Limitation | Impact | Workaround |
|------------|--------|------------|
| No `NavigationAgent2D`/`3D` runtime pathfinding | A* and navigation mesh queries aren't available at runtime | Implement custom A* using `gdcore::math` types, or pre-compute paths offline. `NavigationRegion3D` mesh baking works for defining walkable areas. |
| No physics joints (HingeJoint3D, SliderJoint3D, etc.) | Constrained bodies like doors, ragdolls, or vehicles need alternatives | Simulate constraints manually with force application, or simplify the design to use kinematic animation. |
| No SoftBody3D | Cloth, jelly, or deformable mesh simulation isn't available | Use skeletal animation driven by `AnimationPlayer` to approximate soft-body motion. |
| No `VehicleBody3D` | Built-in vehicle physics controller not available | Implement wheel physics manually using `RigidBody3D` with custom force application per-frame. |

### Animation

| Limitation | Impact | Workaround |
|------------|--------|------------|
| No blend trees or `AnimationTree` | Complex animation blending (walk/run/idle transitions) won't work | Cross-fade between animations manually using `AnimationPlayer` with interpolated weights in your process loop. |
| No Tween chaining | `Tween.chain()` / `Tween.parallel()` don't work | Sequence tweens manually using timers or frame counters in your process callback. |
| No bezier curve tracks | Animation curves default to linear interpolation | Pre-bake bezier curves as keyframe sequences with enough samples for smooth playback. |

### Audio

| Limitation | Impact | Workaround |
|------------|--------|------------|
| No audio effects (reverb, chorus, EQ, etc.) | Bus effects configured in Godot won't apply | Pre-process audio files with effects baked in. Use separate audio files for different environments (e.g., cave reverb). |
| No audio stream types beyond WAV/OGG | MIDI, MP3, or procedural audio aren't supported | Convert all audio to WAV or OGG Vorbis before importing. |

### UI / Control

| Limitation | Impact | Workaround |
|------------|--------|------------|
| `RichTextLabel` supports a BBCode subset only | Complex formatting (tables, images in text) may not render | Use multiple `Label` + `TextureRect` nodes in a container for complex layouts instead of inline BBCode. |
| `Tree` and `ItemList` have no custom cell rendering | Custom icons, progress bars in cells won't render | Build custom list UIs from `VBoxContainer` + per-row `HBoxContainer` scenes. |
| No `PopupMenu` / `FileDialog` / `AcceptDialog` built-in | Native dialogs from Godot aren't available | Build dialog UI from `Panel` + `VBoxContainer` + `Button` combinations. The editor uses this pattern internally. |
| No theme resource hot-reload | Theme changes require restart | Set theme properties at startup. Use the editor's theme editor for iterating on designs. |

### Platform & Export

| Limitation | Impact | Workaround |
|------------|--------|------------|
| No mobile platforms (iOS, Android) | Mobile games can't be exported | Target desktop first. Mobile support is planned for a future milestone. |
| No web export with full feature set | WASM target compiles but has limited platform integration | Use the `wasm32-unknown-unknown` target for headless/logic-only web builds. Full web rendering is not yet available. |
| No XR/VR support | VR headset rendering and controller tracking aren't available | No workaround — XR is not on the current roadmap. |
| No GDExtension / C# interop | Existing GDExtension plugins or C# scripts won't load | Port plugin logic to Rust. For complex plugins, consider wrapping as a separate process communicating via IPC. |

### Resource System

| Limitation | Impact | Workaround |
|------------|--------|------------|
| No encrypted resources | `ResourceLoader.load()` with encryption flags won't work | Use OS-level encryption or a custom resource wrapper. |
| No remote resource loading (HTTP) | `load("http://...")` isn't supported | Download resources to local storage first, then load via `res://` paths. |
| No `.import` file processing at runtime | Godot import settings (texture compression, audio resampling) are ignored | Pre-process assets to their final format before bundling. |

### Networking

| Limitation | Impact | Workaround |
|------------|--------|------------|
| No `MultiplayerPeer` / ENet / WebSocket transport | Multiplayer games won't have built-in networking | Use Rust networking crates (`tokio`, `quinn`, `tungstenite`) directly. The `gdplatform::network` module provides type stubs for API compatibility. |
| No `HTTPRequest` node | HTTP calls from the scene tree aren't available | Use `reqwest` or `ureq` Rust crates for HTTP calls in your game logic. |

### Debugging & Profiling

| Limitation | Impact | Workaround |
|------------|--------|------------|
| No Godot debugger protocol | Godot's remote debugger won't connect | Use Rust debugging tools (`gdb`, `lldb`, `rust-analyzer`). The Patina editor provides inspector and scene tree views via HTTP. |
| No visual profiler | Frame time, draw call, and physics step visualizations aren't available | Use `tracing` crate instrumentation. Run `cargo test` benchmarks for performance data. See `docs/BENCHMARK_BASELINES.md`. |

---

## General Migration Advice

### Godot Concept Mapping

| Godot Concept | Patina Equivalent | Notes |
|---------------|-------------------|-------|
| `Node` | `gdobject` node types | Same lifecycle (ready, process, etc.) |
| `Signal` | `gdobject::signal::Signal` | String-based, same connect/emit pattern |
| `Variant` | `gdvariant::Variant` | Enum-based instead of pointer-based |
| `PackedScene` | `gdscene::packed_scene::PackedScene` | Loads `.tscn` files |
| `Resource` | `gdresource` types | Loads `.tres` files |
| `Input` | `gdplatform::input::InputState` | Explicit state instead of global singleton |
| `InputMap` | `gdplatform::input::InputMap` | Same action-binding concept |
| `Vector2/3` | `gdcore::math::Vector2/Vector3` | Same API surface |
| `Transform2D/3D` | `gdcore::math::Transform2D/Transform3D` | Same `xform()` method |
| `NodePath` | `gdcore::node_path::NodePath` | Same path syntax |
| `StringName` | `gdcore::string_name::StringName` | Interned, same semantics |
| `PhysicsServer2D` | `gdphysics2d::world::PhysicsWorld2D` | Direct API, not singleton |
| `PhysicsServer3D` | `gdphysics3d::world::PhysicsWorld3D` | Direct API, not singleton |
| `RenderingServer` | `gdserver2d/gdserver3d` | Trait-based servers |
| `AudioServer` | `gdaudio::AudioServer` | Explicit bus management |
| `OS` | `gdplatform::os` | Platform detection, ticks |
| `DisplayServer` | `gdplatform::display::DisplayServer` | VSync, display management |

### What Is Not Yet Supported

- GDScript execution (logic must be written in Rust)
- GPU shader pipeline (software rendering only, no custom shaders)
- Animation blend trees and state machines (keyframe playback works)
- Runtime navigation/pathfinding (mesh baking works)
- Networking/multiplayer
- Mobile platforms (iOS, Android)
- XR/VR support
- GDExtension/C# interop
- Plugin/addon system

See the full [Node Type Compatibility Table](#node-type-compatibility-table)
above for per-type details.

### Testing Your Migration

Run the full test suite to verify your setup:

```bash
cd engine-rs
cargo test --workspace
```

Run specific subsystem tests:

```bash
# Core math and types
cargo test -p gdcore

# Scene loading
cargo test -p gdscene

# 2D physics
cargo test -p gdphysics2d

# 3D physics
cargo test -p gdphysics3d

# Platform targets
cargo test -p gdplatform

# Integration tests
cargo test --test platform_targets_validation_test
cargo test --test fuzz_property_runtime_test
```

---

## Version Compatibility

### Live Oracle Pin: Godot 4.6.1-stable

Patina's parity tests, golden fixtures, and oracle outputs are all validated
against **Godot 4.6.1-stable** (`14d19694e0c88a3f9e82d899a0400f27a24c176e`).
This is the upstream version that defines "correct behavior" for all runtime
contracts. The pinned version is recorded in `tools/oracle/common.py` and
enforced by CI.

| Item | Version | Notes |
|------|---------|-------|
| Upstream oracle pin | 4.6.1-stable | Defines all parity contracts |
| GDExtension lab | godot-rust 0.2 | Compatible with Godot 4.2–4.6 |
| Scene format | `format=3` | Godot 4.x `.tscn` / `.tres` |
| Minimum Rust | 1.75.0 | 2021 edition |

### Historical: Godot 4.5.1-stable

Prior to the 4.6.1 repin (2026-03-20), the oracle was pinned to
**Godot 4.5.1-stable** (`f62fdbde15035c5576dad93e586201f4d41ef0cb`).
Numbers, golden traces, and fixture data from the 4.5.1 era are preserved
in historical sections of docs (see `docs/BENCHMARK_BASELINES.md`) but are
**not** used for current parity validation. Any reference to 4.5.1 in test
files or docs should be treated as historical context, not a live contract.

### What Changed in the 4.5.1 → 4.6.1 Repin

- Oracle outputs regenerated from 4.6.1 runtime
- Golden physics traces re-captured with 4.6.1 deterministic stepping
- Benchmark baselines formally recorded for the first time
- GDExtension lab probes updated for 4.6.1 API surface
- CI repin-validation pipeline added to gate future version advances

---

## Getting Help

- Check `prd/PORT_GODOT_TO_RUST_PLAN.md` for the full phase plan
- See `docs/3D_ARCHITECTURE_SPEC.md` for 3D subsystem details
- See `docs/BENCHMARK_BASELINES.md` for performance data
- File issues in the project tracker
