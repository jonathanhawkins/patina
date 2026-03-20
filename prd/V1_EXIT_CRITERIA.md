# V1 Exit Criteria — Subsystem Checklists

Each subsystem lists specific, measurable criteria for "done" at the v1 milestone.
Status: ✅ done | 🔶 partial | ❌ not started

---

## Core Types (`gdcore`) — ✅ Done

- [x] `StringName`, `NodePath`, `GString` implemented with correct semantics
- [x] `RID` type implemented with monotonic allocator
- [x] `Error` enum covers the Godot OK/Error table
- [x] Core math types (`Vector2`, `Vector3`, `Rect2`, `Transform2D`, `Color`, `Basis`, `AABB`) pass oracle goldens
- [x] All core type tests green under `cargo test --workspace`

---

## Variant (`gdvariant`) — ✅ Done

- [x] All 28 Godot 4 Variant types represented
- [x] Encode/decode roundtrip for every variant type
- [x] `VariantType` enum matches upstream numbering
- [x] Type coercion rules match oracle
- [x] No `unsafe` without `// SAFETY:` comment
- [x] Oracle parity ≥ 98% on variant golden suite

---

## Object Model (`gdobject`) — 🔶 Partial

- [x] `GodotObject` trait with `get_class()`, `is_class()`, `get_instance_id()`
- [x] `ClassDB` stub: `class_exists()`, `get_parent_class()`, `get_class_list()`
- [x] Property get/set via `Variant`
- [x] Signal connect/emit/disconnect lifecycle
- [ ] Full `ClassDB` property and method enumeration (measurable against oracle output)
- [ ] `Object.notification()` dispatch with correct ordering
- [ ] Weak reference (`WeakRef`) behavior matches oracle
- [ ] `Object.free()` + use-after-free guard

**Exit gate:** ClassDB queries for representative classes, inheritance chains, and property lists all pass oracle comparison (see pat-h6a).

---

## Resources (`gdresource`) — 🔶 Partial

- [x] `Resource` base type with `resource_path`, `resource_name`
- [x] `.tres` text resource loader (basic key/value)
- [x] `.res` binary resource loader (basic)
- [ ] Resource UID registry (tracks `uid://` references)
- [ ] Sub-resource inline loading (nested resources in `.tres`)
- [ ] External resource reference resolution across multiple files
- [ ] Roundtrip: load → inspect → re-save produces byte-for-byte or semantically-equivalent output
- [ ] Oracle comparison for at least one fixture resource

**Exit gate:** One representative resource fixture loads, serializes back, and matches oracle-captured metadata without manual intervention.

---

## Scenes (`gdscene`) — 🔶 Partial

- [x] `.tscn` parser handles nodes, properties, sub-resources, and connections
- [x] `SceneTree` instantiation from parsed scene
- [x] `Node` hierarchy attach/detach
- [x] `_ready` / `_process` / `_physics_process` lifecycle hooks
- [ ] Instance inheritance (scenes that `[ext_resource]` another scene)
- [ ] `PackedScene` save/restore roundtrip
- [ ] Scene-level signal connections wired during instantiation
- [ ] Oracle golden comparison for non-trivial scene tree

**Exit gate:** A `demo_2d` scene loads, runs one frame, and produces oracle-matching output for node tree, signals, and 2D draw calls.

---

## Scripting (`gdscript-interop`) — 🔶 Partial

- [x] GDScript token skeleton (lexer exists)
- [ ] GDScript parser produces stable AST for representative scripts
- [ ] `@onready` variable resolution after `_ready`
- [ ] `func` dispatch via object method table
- [ ] `signal` declaration and `emit_signal` callable from script
- [ ] At least one script-driven fixture executes and matches oracle

**Exit gate:** A simple GDScript file (property, signal, one method) compiles and runs under Patina with oracle-matching behavior.

---

## Physics (`gdphysics2d`) — 🔶 Partial

- [x] AABB overlap and separation tests
- [x] Deterministic physics tick with fixed delta
- [x] Golden trace for one physics fixture
- [ ] `PhysicsServer2D` API surface: `body_create`, `body_set_state`, `body_get_state`
- [ ] Collision layers and masks respected
- [ ] `KinematicBody2D` `move_and_collide` baseline behavior
- [ ] Oracle comparison for one multi-body deterministic trace

**Exit gate:** Multi-body deterministic trace matches upstream oracle within documented numeric tolerance, checked in CI.

---

## Rendering (`gdrender2d`) — 🔶 Partial

- [x] 2D canvas item draw calls captured
- [x] Scene-driven golden rendering fixture
- [ ] Texture atlas sampling matches upstream pixel output (within tolerance)
- [ ] `CanvasItem` z-index ordering respected
- [ ] Visibility (`visible = false`) suppresses draw calls
- [ ] Camera2D transform applied correctly to render output
- [ ] Pixel diff against upstream golden ≤ 0.5% error rate

**Exit gate:** At least one scene renders a golden image that passes automated pixel-diff against a Godot-captured reference, checked in CI.

---

## Platform / Window / Input (`gdplatform`) — ❌ Not Started

- [ ] Window creation abstraction (backed by `winit`)
- [ ] Input event delivery: keyboard, mouse, gamepad stubs
- [ ] `OS` singleton: `get_ticks_msec()`, `get_name()`
- [ ] `Time` singleton: `get_ticks_usec()`
- [ ] Headless mode (no window, for CI) supported

**Exit gate:** `demo_2d` runs to completion in headless mode on CI without panicking.

---

## Overall v1 Gate

All subsystems above must reach their individual exit gates **and**:

- Oracle parity ≥ 98% across all supported scene fixtures (currently 90.5% — 7/9 scenes at 100%, remaining gaps in physics_playground and test_scripts)
- Zero known panics in headless mode on the `demo_2d` example
- CI green on `cargo test --workspace` including golden comparisons
- `THIRDPARTY_STRATEGY.md` reviewed and up to date before new subsystem imports
