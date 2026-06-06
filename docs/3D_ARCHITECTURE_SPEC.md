# Patina Engine — 3D Architecture Spec

> Initial specification for 3D subsystem expansion.
> Bead: pat-k22f | Phase 5 deliverable from PORT_GODOT_TO_RUST_PLAN.md

## Status

**Phase**: Bootstrapped (3D crates created, boundary tests green)
**Baseline**: 2D vertical slice is green; all runtime parity exits pass.
**3D Crates**: `gdserver3d`, `gdrender3d`, `gdphysics3d` — all workspace members, compiling, tested.

## Existing 3D Foundations

The engine already has significant 3D groundwork in place:

### Math Layer (`gdcore::math3d`)

All core 3D math types are implemented with full operator support:

| Type | Description | Key Operations |
|------|-------------|----------------|
| `Vector3` | 3D vector (f32) | dot, cross, lerp, normalize, distance |
| `Quaternion` | Unit quaternion | slerp, from_euler, to_euler, from_axis_angle, xform |
| `Basis` | 3x3 rotation/scale matrix | euler/quat conversion, xform, rotated, inverse |
| `Transform3D` | Affine transform (Basis + origin) | looking_at, translated, rotated, scaled, composition |
| `Aabb` | Axis-aligned bounding box | contains_point, intersects, merge, get_center |
| `Plane` | Infinite plane (normal + d) | — |

### Type System (`gdvariant`)

`Variant` enum already includes all 3D types: `Vector3`, `Basis`, `Transform3D`, `Quaternion`, `Aabb`, `Plane`.

### Scene Layer (`gdscene::node3d`)

Property helpers exist for `Node3D`, `Camera3D`, `MeshInstance3D`, and `Light3D` — position/rotation/scale getters/setters, global transform computation via parent chain walk, camera projection parameters, and light properties.

## Proposed 3D Crate Map

Following the 2D pattern (`gdserver2d` / `gdrender2d` / `gdphysics2d`), the 3D expansion introduces three new crates:

```
                    ┌─────────────┐
                    │   gdcore    │  (math3d types already here)
                    └──────┬──────┘
                           │
                    ┌──────┴──────┐
                    │  gdvariant  │  (3D Variant types already here)
                    └──────┬──────┘
                           │
              ┌────────────┼────────────┐
              │            │            │
       ┌──────┴──────┐    │     ┌──────┴──────┐
       │  gdobject   │    │     │ gdresource  │
       └──────┬──────┘    │     └──────┬──────┘
              │           │            │
              └─────┬─────┘            │
                    │                  │
              ┌─────┴─────┐           │
              │  gdscene  │ ←─────────┘
              └─────┬─────┘
                    │
         ┌──────────┼──────────┐
         │          │          │
  ┌──────┴───┐ ┌───┴────┐ ┌───┴────────┐
  │gdserver3d│ │gdphys3d│ │(existing2D)│
  └──────┬───┘ └────────┘ └────────────┘
         │
  ┌──────┴───┐
  │gdrender3d│
  └──────────┘
```

### New Crates

| Crate | Responsibility | Dependencies |
|-------|----------------|--------------|
| `gdserver3d` | 3D rendering server abstraction (draw commands, materials, meshes, lights) | `gdcore`, `gdvariant` |
| `gdrender3d` | 3D rendering backend (initial: software rasterizer or wgpu) | `gdcore`, `gdserver3d` |
| `gdphysics3d` | 3D physics (rigid bodies, collision shapes, raycasting) | `gdcore`, `gdvariant` |

### Design Principles

1. **Mirror 2D structure**: Each 3D crate follows the same patterns as its 2D counterpart. `gdserver3d` defines traits; `gdrender3d` implements them.

2. **Shared foundation**: `gdcore`, `gdvariant`, `gdobject`, `gdresource`, and `gdscene` are dimension-agnostic. No changes needed for 3D support — the math types and property system are already in place.

3. **Independent 2D/3D**: The 2D and 3D rendering/physics stacks are separate crates with no cross-dependency. A game can use either or both.

4. **Scene tree is unified**: `gdscene` handles both 2D and 3D nodes through the same `Node` struct and property system. Node class determines behavior, not the scene tree itself.

## Crate-Boundary Review

### Current Boundaries (13 crates)

| Layer | Crates | Boundary Rule |
|-------|--------|---------------|
| **Primitives** | `gdcore` | No internal deps. Math, IDs, errors only. |
| **Types** | `gdvariant` | Depends only on `gdcore`. Dynamic type system. |
| **Objects** | `gdobject` | `gdcore` + `gdvariant`. Object model, signals, ClassDB. |
| **Resources** | `gdresource` | + `gdobject`. Parsing, caching, loaders. |
| **Scripting** | `gdscript-interop` | + `gdobject`. Script execution. |
| **Scene** | `gdscene` | Hub crate. Depends on objects, resources, physics, platform. |
| **2D Render** | `gdserver2d`, `gdrender2d` | Server defines traits; render implements. |
| **2D Physics** | `gdphysics2d` | `gdcore` + `gdvariant` only. |
| **Audio** | `gdaudio` | `gdcore` only. |
| **Platform** | `gdplatform` | Windowing/input. Optional render dep. |
| **Editor** | `gdeditor` | Top-level. Depends on scene, render, platform. |
| **Runner** | `patina-runner` | CLI entry point. |

### Boundary Violations to Watch

1. **`gdscene` as hub**: Currently depends on `gdphysics2d` directly. When adding `gdphysics3d`, this coupling grows. Consider a `PhysicsServer` trait in `gdcore` that both 2D/3D implement, with `gdscene` depending only on the trait.

2. **Render coupling in `gdplatform`**: The optional `gdrender2d` dependency should remain optional. When `gdrender3d` exists, `gdplatform` should not depend on either directly — use a `RenderBackend` trait instead.

3. **`gdeditor` fan-in**: Currently depends on `gdrender2d`. For 3D editor support, it should depend on server traits (`gdserver2d`/`gdserver3d`) rather than concrete backends.

### Recommended Pre-3D Refactors

1. **Extract `PhysicsServer` trait** from `gdphysics2d` into `gdcore` so `gdscene` doesn't need direct 2D physics dependency.
2. **Extract `RenderServer` trait** from `gdserver2d` into `gdcore` for the same reason.
3. **Keep `gdscene` dimension-agnostic** — it should never import 2D or 3D server crates directly.

## 3D Node Classes (Initial Subset)

Phase 6 targets these Godot node classes:

| Class | Category | Priority |
|-------|----------|----------|
| `Node3D` | Transform | P0 — base class |
| `Camera3D` | View | P0 — required for rendering |
| `MeshInstance3D` | Rendering | P0 — geometry display |
| `DirectionalLight3D` | Lighting | P1 — basic lighting |
| `OmniLight3D` | Lighting | P1 — point lights |
| `StaticBody3D` | Physics | P1 — collision geometry |
| `RigidBody3D` | Physics | P1 — dynamic bodies |
| `CollisionShape3D` | Physics | P1 — shape attachment |
| `CharacterBody3D` | Physics | P2 — gameplay movement |
| `Area3D` | Physics | P2 — trigger volumes |
| `RayCast3D` | Physics | P2 — ray queries |
| `SpotLight3D` | Lighting | P2 — focused lights |

Property helpers for `Node3D`, `Camera3D`, `MeshInstance3D`, and `Light3D` already exist in `gdscene::node3d`.

## 3D Fixture Plan

Following the oracle methodology used for 2D:

1. **Minimal 3D scene**: Single `Node3D` with transform — verifies property round-trip.
2. **Camera + Mesh**: `Camera3D` looking at a `MeshInstance3D` — verifies transform composition.
3. **Lit scene**: Camera + Mesh + `DirectionalLight3D` — verifies light property loading.
4. **Physics scene**: `RigidBody3D` + `CollisionShape3D` + `StaticBody3D` — verifies physics property loading.
5. **Hierarchy scene**: Nested `Node3D` chain — verifies global transform accumulation.

Each fixture will have a corresponding `_tree.json` oracle from upstream Godot for lifecycle trace comparison.

## Render Abstraction Decision

**Recommendation**: Start with a headless/software 3D render server (no GPU dependency) for parity testing, following the same pattern as `gdrender2d`'s `FrameBuffer` approach. GPU acceleration (via `wgpu`) can be added as an alternative backend behind the `gdserver3d` trait boundary.

This ensures:
- CI runs without GPU requirements
- Parity testing is deterministic
- GPU backend development doesn't block scene/physics work

## Exit Criteria for This Spec

- [x] This document exists and covers crate map, boundaries, node subset, fixtures, and render strategy
- [x] Crate boundary validation tests exist and pass (56 tests across crate_boundary_3d_review_test + crate_set_3d_bootstrap_test)
- [x] No code changes to existing crates required at this stage
