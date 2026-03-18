# ARCHITECTURE_MAP.md - Godot Subsystem to Rust Crate Mapping

This document maps upstream Godot C++ subsystems to their corresponding Rust crate implementations in the Patina Engine.

---

## Mapping Overview

| Godot Subsystem | Godot Source Area | Rust Crate | Phase |
|----------------|-------------------|------------|-------|
| Object system | `core/object/` | `gdobject` | 3 |
| Variant types | `core/variant/` | `gdvariant` | 3 |
| Core primitives | `core/` (config, error, math, string, templates) | `gdcore` | 3 |
| Node / SceneTree | `scene/main/` | `gdscene` | 3-4 |
| Resources | `core/io/resource*`, `scene/resources/` | `gdresource` | 3-4 |
| RenderingServer (2D) | `servers/rendering/`, `drivers/` | `gdserver2d` / `gdrender2d` | 4 |
| PhysicsServer (2D) | `servers/physics_2d/` | `gdphysics2d` | 4 |
| DisplayServer / Input | `servers/display_server*`, `core/input/` | `gdplatform` | 7 |
| AudioServer | `servers/audio/`, `drivers/` | `gdaudio` | 5 |
| GDScript interop | `modules/gdscript/` | `gdscript-interop` | 5+ |
| Editor | `editor/` | `gdeditor` | 8+ |

---

## Core Crates

### gdcore

**Maps to**: `core/` (excluding object/ and variant/)

**Responsibility**: Low-level engine primitives shared by all other crates.

- Engine-wide IDs and handles
- Math types (Vector2, Vector3, Rect2, Transform2D, Transform3D, Color, etc.)
- String types (String, StringName, NodePath)
- Allocation and model helpers
- Error types and diagnostics
- Configuration and project settings
- Type registration infrastructure

**Dependencies**: None (leaf crate)

### gdvariant

**Maps to**: `core/variant/`

**Responsibility**: The Variant type system and value containers.

- Variant enum with all supported types
- Type conversion rules and coercion
- Typed value containers
- Serialization and deserialization helpers
- Variant call mechanics
- Utility functions operating on Variants

**Dependencies**: `gdcore`

### gdobject

**Maps to**: `core/object/`

**Responsibility**: The object model, class hierarchy, and signal system.

- Object base type and class registration
- Inheritance metadata and class database (ClassDB equivalent)
- Property system (get/set, property info, property list)
- Signal declaration, connection, and emission
- Notification dispatch
- Reference counting and destructor lifecycle
- Method binding infrastructure

**Dependencies**: `gdcore`, `gdvariant`

---

## Scene Crates

### gdscene

**Maps to**: `scene/main/`, `scene/2d/`, `scene/gui/` (subset)

**Responsibility**: Node hierarchy, SceneTree, and scene lifecycle.

- Node base class and tree structure
- SceneTree implementation (subset)
- MainLoop integration
- Enter/ready/process/exit lifecycle
- Parent-child management
- PackedScene loading and instancing
- Group membership
- Node2D, Control, and derived node types (subset)
- Scene-level notifications

**Dependencies**: `gdcore`, `gdvariant`, `gdobject`, `gdresource`

### gdresource

**Maps to**: `core/io/resource*`, `scene/resources/`

**Responsibility**: Resource loading, saving, caching, and identity.

- Resource base type
- Resource UID and path management
- Resource loader registry and format handlers
- Resource saver registry
- Resource cache
- .tres and .tscn parsing
- Binary resource format support
- Import pipeline (subset for v1)

**Dependencies**: `gdcore`, `gdvariant`, `gdobject`

---

## Runtime Service Crates

### gdserver2d

**Maps to**: `servers/rendering/` (2D surface)

**Responsibility**: Abstract 2D server-facing runtime surface.

- RenderingServer 2D API surface
- Canvas item management
- Viewport management
- 2D draw commands and batching
- Z-index and draw ordering
- Server-side state management

**Dependencies**: `gdcore`, `gdvariant`

### gdrender2d

**Maps to**: `drivers/` (2D rendering backend)

**Responsibility**: 2D rendering backend implementation.

- GPU/software rendering backends
- Sprite rendering
- Primitive drawing (lines, rects, polygons)
- Texture management
- Shader basics (2D subset)
- Render snapshot capture for testing
- Render diff adapter for oracle comparison

**Dependencies**: `gdcore`, `gdserver2d`

### gdphysics2d

**Maps to**: `servers/physics_2d/`

**Responsibility**: 2D physics simulation.

- PhysicsServer2D API surface
- Shape types (circle, rect, segment, capsule, polygon)
- Collision detection
- Static, kinematic, and rigid body support
- Space and area management
- Deterministic simulation mode for testing
- Physics trace capture for oracle comparison

**Dependencies**: `gdcore`, `gdvariant`

### gdaudio

**Maps to**: `servers/audio/`, `drivers/audio/`

**Responsibility**: Audio runtime and playback.

- AudioServer API surface
- Audio bus management
- Stream playback (WAV, OGG subset)
- Basic mixer abstractions
- Audio effect basics
- Spatial audio (later)

**Dependencies**: `gdcore`

### gdplatform

**Maps to**: `servers/display_server*`, `platform/`, `core/input/`

**Responsibility**: Platform abstraction layer.

- DisplayServer API surface
- Window creation and management
- Input event handling (keyboard, mouse, gamepad)
- Timing and frame synchronization
- OS integration (file dialogs, clipboard, etc.)
- Platform-specific backends (Linux/X11/Wayland, macOS, Windows)

**Dependencies**: `gdcore`

---

## Higher-Level Crates

### gdscript-interop

**Maps to**: `modules/gdscript/` (compatibility layer)

**Responsibility**: Scripting and runtime interop layer.

- GDScript-compatible API surface
- Script resource loading
- Method call dispatch from scripts
- Property access from scripts
- Signal connection from scripts
- Not a full GDScript interpreter in v1; focused on interop contracts

**Dependencies**: `gdcore`, `gdvariant`, `gdobject`

### gdeditor

**Maps to**: `editor/`

**Responsibility**: Editor-facing layers (later phase).

- Editor API surface
- Inspector integration
- Scene editor hooks
- Import pipeline UI
- Tool mode support
- Plugin system

**Dependencies**: `gdcore`, `gdobject`, `gdscene`

---

## Cross-Cutting Support

### tools/oracle

**Responsibility**: Upstream behavior capture and golden output generation.

- Scene tree dumper
- Property dumper
- Signal/notification tracer
- Resource roundtrip tool
- Render snapshot capture
- Physics trace capture
- Golden output format definition

### tools/api-extract

**Responsibility**: Contract extraction from upstream API definitions.

- Parse Godot API definitions (extension_api.json, ClassDB)
- Normalize types across Godot and Rust
- Generate support matrix
- Produce crate-boundary contract docs
- Identify impossible or awkward API surfaces

### tests/compat

**Responsibility**: Parity tests against upstream oracle outputs.

- Golden test runner
- Render diff comparison
- Physics trace comparison
- Signal ordering comparison
- Property value comparison

---

## Dependency Graph

```
                         +---------+
                         | gdcore  |
                         +---------+
                        /     |     \
                       v      v      v
               +-----------+  |  +-----------+
               | gdvariant |  |  | gdaudio   |  gdplatform
               +-----------+  |  +-----------+  (gdcore only)
                /      \      |
               v        v     v
         +----------+  +-----------+
         | gdobject |  | gdserver2d|  gdphysics2d
         +----------+  | gdvariant |  (gdcore, gdvariant)
          /   |         +-----------+
         v    v               |
  +--------+ +----------+     v
  |gdscene | |gdresource|  +-----------+
  +--------+ +----------+  | gdrender2d|
       |      (gdobject)    +-----------+
       v
  +-----------+
  | gdscript- |  gdeditor
  | interop   |  (gdcore, gdobject, gdscene)
  +-----------+
  (gdcore, gdvariant, gdobject)

  Actual Cargo.toml dependencies — no circular deps permitted.
```

---

## Mapping Principles

1. **Behavior, not structure**: Crate boundaries are drawn around behavioral contracts, not upstream file organization.
2. **Strict layering**: Lower crates never depend on higher crates. The dependency graph is a DAG.
3. **Server/client split**: Server crates (gdserver2d) define abstract APIs; implementation crates (gdrender2d) provide backends.
4. **Testability**: Every crate must be testable in isolation with mock dependencies where needed.
5. **Incremental delivery**: Core crates ship first; higher-level crates build on proven foundations.
