# CRATE_BOUNDARIES.md - Crate Relationship Rules

This document defines the crate structure of the Patina Engine, each crate's responsibilities, the dependency rules governing inter-crate relationships, and the invariants that must be maintained.

---

## Crate Categories

The engine workspace is organized into four categories of crates, each with distinct roles and dependency privileges.

---

## Core Crates

Core crates provide foundational types and abstractions used throughout the engine. They form the bottom of the dependency graph.

### gdcore

**Responsibility**: Low-level engine primitives shared by all crates.

- Engine-wide IDs and handles
- Math types (Vector2, Vector3, Rect2, Transform2D, Transform3D, Color, Basis, Quaternion, AABB, Plane, Projection)
- String types (String, StringName, NodePath)
- Allocation and model helpers
- Error types and diagnostic infrastructure
- Configuration and project settings primitives
- Type registration infrastructure
- Logging/tracing setup

**May depend on**: External Rust crates only (no engine crates)

**Must not depend on**: Any other `gd*` crate

### gdvariant

**Responsibility**: The Variant type system.

- Variant enum covering all supported types
- Type conversion rules and coercion logic
- Typed value containers
- Serialization and deserialization helpers
- Variant call mechanics
- Utility functions operating on Variants

**May depend on**: `gdcore`

**Must not depend on**: `gdobject`, `gdresource`, `gdscene`, or any runtime/higher crate

### gdobject

**Responsibility**: The object model, class hierarchy, and signal system.

- Object base type
- Class registration and ClassDB equivalent
- Inheritance metadata
- Property system (get/set, property info, property list)
- Signal declaration, connection, emission
- Notification dispatch
- Reference counting and destructor lifecycle
- Method binding infrastructure

**May depend on**: `gdcore`, `gdvariant`

**Must not depend on**: `gdresource`, `gdscene`, or any runtime/higher crate

---

## Scene and Resource Crates

These crates build on core crates to provide scene management and resource handling.

### gdresource

**Responsibility**: Resource loading, saving, caching, and identity.

- Resource base type
- Resource UID and path management
- Resource loader registry and format handlers
- Resource saver registry
- Resource cache
- .tres and .tscn file parsing
- Binary resource format support
- Import pipeline (subset)

**May depend on**: `gdcore`, `gdvariant`, `gdobject`

**Must not depend on**: `gdscene`, or any runtime/higher crate

### gdscene

**Responsibility**: Node hierarchy, SceneTree, and scene lifecycle.

- Node base class and tree structure
- SceneTree implementation
- MainLoop integration
- Enter/ready/process/exit lifecycle
- Parent-child management and tree traversal
- PackedScene loading and instancing
- Group membership
- Node2D, Control, and derived node types
- Scene-level notifications
- `%UniqueName` NodePath resolution within scene-owner scope (`get_node_by_unique_name`)

**May depend on**: `gdcore`, `gdvariant`, `gdobject`, `gdresource`

**Must not depend on**: Any runtime service crate (gdserver2d, gdrender2d, etc.) -- node types that need rendering or physics use trait-based abstractions

---

## Runtime Service Crates

These crates implement engine subsystems (rendering, physics, audio, platform). They interact with the scene layer through defined interfaces.

### gdserver2d

**Responsibility**: Abstract 2D rendering server API surface.

- RenderingServer 2D API
- Canvas item management
- Viewport management
- 2D draw commands and batching
- Z-index and draw ordering
- Server-side state management

**May depend on**: `gdcore`, `gdvariant`

**Must not depend on**: `gdobject`, `gdscene`, `gdrender2d`, `gdphysics2d`, or higher crates

### gdrender2d

**Responsibility**: 2D rendering backend implementation.

- GPU/software rendering backends
- Sprite rendering
- Primitive drawing
- Texture management
- Shader basics (2D subset)
- Render snapshot capture for testing

**May depend on**: `gdcore`, `gdserver2d`

**Must not depend on**: `gdscene`, `gdobject`, `gdphysics2d`, or higher crates

### gdphysics2d

**Responsibility**: 2D physics simulation.

- PhysicsServer2D API surface
- Shape types and collision detection
- Body types (static, kinematic, rigid)
- Space and area management
- Deterministic simulation mode
- Physics trace capture for testing

**May depend on**: `gdcore`, `gdvariant`

**Must not depend on**: `gdobject`, `gdscene`, `gdserver2d`, `gdrender2d`, or higher crates

### gdaudio

**Responsibility**: Audio runtime.

- AudioServer API surface
- Audio bus management
- Stream playback
- Basic mixer abstractions
- Audio effect basics

**May depend on**: `gdcore`

**Must not depend on**: `gdvariant`, `gdobject`, `gdscene`, rendering crates, physics crates, or higher crates

### gdplatform

**Responsibility**: Platform abstraction layer.

- DisplayServer API surface
- Window creation and management
- Input event handling
- Timing and frame synchronization
- OS integration

**May depend on**: `gdcore`

**Must not depend on**: `gdvariant`, `gdobject`, `gdscene`, rendering crates, or higher crates

---

## Higher-Level Crates

These crates integrate multiple subsystems and are near the top of the dependency graph.

### gdscript-interop

**Responsibility**: Scripting runtime compatibility layer.

- GDScript-compatible API surface
- Script resource loading
- Method call dispatch
- Property access from scripts
- Signal connection from scripts

**May depend on**: `gdcore`, `gdvariant`, `gdobject`

**Must not depend on**: `gdresource`, `gdscene`, `gdeditor`

### gdeditor

**Responsibility**: Editor-facing layers (later phase).

- Editor API surface
- Inspector integration
- Scene editor hooks
- Import pipeline UI
- Tool mode support

**May depend on**: `gdcore`, `gdobject`, `gdscene` (current); additional runtime crates as editor features are added

**Must not depend on**: Nothing above it (top of the graph)

---

## Cross-Cutting Support

These are not engine crates but supporting tooling and test infrastructure.

### tools/oracle

- Upstream behavior capture tools
- Runs inside upstream Godot, not inside Patina
- No dependency on engine crates

### tools/api-extract

- Contract extraction from API definitions
- Generates support matrix and contract docs
- May read engine crate source for analysis

### tests/compat

- Parity tests comparing Patina output against golden data
- Depends on engine crates as needed for test execution
- May depend on any engine crate

---

## Dependency Rules

### Rule 1: No Circular Dependencies

The crate dependency graph must be a strict DAG (directed acyclic graph). If crate A depends on crate B, then crate B must not depend on crate A, either directly or transitively.

### Rule 2: Lower Crates Never Depend on Higher Crates

The dependency direction is strictly bottom-up:

```
gdcore  (bottom — depends on nothing)
  |
gdvariant ——————————— gdaudio     gdplatform
  |                   (gdcore)    (gdcore)
gdobject
  |          gdserver2d     gdphysics2d
gdresource   (gdcore,       (gdcore,
  |           gdvariant)     gdvariant)
gdscene            |
  |            gdrender2d
gdscript-interop   (gdcore, gdserver2d)
(gdcore, gdvariant, gdobject)
  |
gdeditor  (top — gdcore, gdobject, gdscene)
```

### Rule 3: Runtime Services are Peers, Not Dependencies

Runtime service crates (gdserver2d, gdrender2d, gdphysics2d, gdaudio, gdplatform) must not depend on each other. They are parallel subsystems that interact only through the core/object layer or through abstract interfaces.

**Exception**: `gdrender2d` depends on `gdserver2d` (it implements the server's abstract API).

### Rule 4: Scene Does Not Depend on Services

`gdscene` must not directly depend on rendering, physics, audio, or platform crates. Node types that need subsystem interaction use trait-based abstractions or server handles obtained through dependency injection at runtime.

### Rule 5: New Dependencies Require Review

Adding a new inter-crate dependency requires:
1. Verification that no cycle is introduced.
2. Confirmation that the dependency direction follows the rules above.
3. Documentation of why the dependency is necessary.
4. Update of the dependency graph in ARCHITECTURE_MAP.md.

### Rule 6: External Crate Dependencies

- External Rust crates (from crates.io) are permitted at any level.
- Prefer widely-used, well-maintained crates.
- Pin versions in Cargo.toml.
- License must be compatible (MIT, Apache-2.0, BSD, zlib preferred).
- Each external dependency should be justified (not added speculatively).

---

## Enforcement

- **CI check**: A CI job verifies the dependency graph has no cycles and all inter-crate dependencies follow the rules above.
- **Code review**: PRs that add new inter-crate dependencies are flagged for architectural review.
- **Cargo workspace**: The workspace Cargo.toml is the single source of truth for crate membership and shared dependency versions.
