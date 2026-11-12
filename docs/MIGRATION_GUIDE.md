# Patina Engine — Migration Guide for Godot Users

> Phase 9 deliverable: help Godot users understand how to work with Patina.

## Overview

Patina is a Rust-native engine that aims for Godot 4 compatibility at the scene, resource, and scripting layers. This guide covers what works today, what differs from upstream Godot, and how to migrate existing projects.

## Scene Compatibility

### Supported Scene Formats

| Format | Status | Notes |
|--------|--------|-------|
| `.tscn` (text scenes) | Supported | Full parser for Godot 4 format |
| `.tres` (text resources) | Supported | Key/value and sub-resource loading |
| `.res` (binary resources) | Supported | Basic binary resource loading |
| `.scn` (binary scenes) | Not yet | Planned for future release |

### Loading Existing Scenes

Patina can load `.tscn` files created by Godot 4 directly:

```rust
use gdscene::scene_tree::SceneTree;
use gdobject::class_db;

// Register classes (done once at startup)
class_db::register_default_classes();
class_db::register_3d_classes();

// Load a scene
let mut tree = SceneTree::new();
tree.load_scene("res://scenes/my_scene.tscn").unwrap();
```

### Node Types

#### Fully Supported (2D)
- `Node`, `Node2D`, `Sprite2D`, `Camera2D`
- `CharacterBody2D`, `RigidBody2D`, `StaticBody2D`
- `CollisionShape2D`, `Area2D`
- `Timer`, `AnimationPlayer`
- `Control`, `Label`, `Button`, `Panel`, `Container` (UI nodes)

#### Fully Supported (3D)
- `Node3D`, `Camera3D`, `MeshInstance3D`
- `DirectionalLight3D`, `OmniLight3D`, `SpotLight3D`
- `RigidBody3D`, `StaticBody3D`, `CharacterBody3D`
- `CollisionShape3D`

#### Stub/Partial
- `AudioStreamPlayer` — API surface exists, no audio backend yet
- `NavigationAgent2D/3D` — stub implementation
- `TileMap` — partial, editor support in progress

## Property System

Patina's property system mirrors Godot's `Variant` type system:

| Godot Type | Patina Type | Notes |
|-----------|-------------|-------|
| `int` | `Variant::Int(i64)` | Same range |
| `float` | `Variant::Float(f64)` | Same precision |
| `String` | `Variant::String(String)` | UTF-8 |
| `Vector2` | `Variant::Vector2(Vector2)` | f32 components |
| `Vector3` | `Variant::Vector3(Vector3)` | f32 components |
| `Color` | `Variant::Color(Color)` | RGBA f32 |
| `NodePath` | `Variant::NodePath(NodePath)` | Full path resolution |
| `Array` | `Variant::Array(Vec<Variant>)` | Heterogeneous |
| `Dictionary` | `Variant::Dictionary(...)` | Key-value pairs |

All 28 Godot 4 variant types are represented.

## Key Differences from Godot

### 1. No GDScript Runtime (Yet)

Patina parses GDScript for interop analysis but does not execute it. Game logic should be written in Rust using the Patina API. GDScript interop is for migration tooling — extracting signals, properties, and class structure from existing `.gd` files.

### 2. Headless-First Design

Patina runs headless by default (no window). This is ideal for CI, testing, and server-side game logic. To create a window:

```rust
use gdplatform::window::WindowConfig;

let config = WindowConfig::default()
    .with_title("My Game")
    .with_size(1280, 720);
```

### 3. ClassDB is Static

Godot's ClassDB is populated at engine startup. In Patina, you register classes explicitly:

```rust
use gdobject::class_db;

class_db::register_default_classes(); // Node, Resource, etc.
class_db::register_3d_classes();      // Node3D, Camera3D, etc.
```

This makes the class hierarchy fully inspectable and testable.

### 4. Resource Paths

Patina supports `res://` paths relative to the project root, just like Godot. UID references (`uid://`) are also supported for resource deduplication.

### 5. Signals

Signal connect/emit/disconnect works the same way conceptually, but connections are made programmatically in Rust rather than through the Godot editor's signal dialog.

## Migration Workflow

### Step 1: Validate Your Scenes

Run your `.tscn` files through Patina's scene loader to check compatibility:

```bash
cd engine-rs
cargo test --test oracle_regression_test
```

### Step 2: Check Property Parity

Compare your scene's properties against Godot oracle output:

```bash
# Generate oracle output from Godot (requires Godot 4 installed)
# Then compare:
cargo test --test oracle_parity_test
```

### Step 3: Port Game Logic

Replace GDScript with Rust. Use the GDScript interop layer to extract your class structure:

```rust
use gdscript_interop::parser::parse_gdscript;

let script = std::fs::read_to_string("player.gd").unwrap();
let parsed = parse_gdscript(&script);
// Inspect signals, properties, methods
```

### Step 4: Test with Golden Fixtures

Patina includes a golden fixture system for regression testing. Add your scenes to `fixtures/scenes/` and generate golden outputs to lock in expected behavior.

## Current Parity Status

- **Oracle parity**: ~81% across all scene fixtures (2D near 100%, 3D ~60-75%)
- **ClassDB coverage**: Full for supported node types
- **Physics**: Deterministic tick with golden trace comparison
- **Rendering**: 2D canvas items with pixel-diff validation; 3D render path in progress

See `COMPAT_MATRIX.md` for detailed per-scene parity numbers.

## Getting Help

- Check `docs/contributor-onboarding.md` for development setup
- Run `cargo test --workspace` to verify your environment
- File issues in the project repository
