# Phase 6 3D Parity Audit

Date: 2026-03-29
Target upstream: Godot `4.6.1-stable`
Patina phase: `Phase 6 - 3D Runtime Slice`

## Purpose

This document turns the Phase 6 3D milestone from a broad roadmap item into a
systematic parity audit.

It answers four questions:

1. What 3D behavior does upstream Godot 4.6.1 expose?
2. What does Patina currently implement and measure?
3. Where do Patina docs overclaim relative to measured evidence?
4. Which remaining gaps should become beads without duplicating existing work?

## Audit Rules

Use this workflow for all future Phase 6 parity work.

1. Scope only the 3D runtime slice.
2. Treat `upstream/godot` at `4.6.1-stable` as the source for class surface and behavior contracts.
3. Treat `godot-docs` semantics as supporting evidence, not as proof of Patina parity.
4. Mark every audited item as one of:
   - `Measured`
   - `Implemented, not yet measured`
   - `Deferred`
   - `Missing`
5. Do not create a new bead if an existing bead already covers the same class family or acceptance outcome.
6. Prefer one bead per measurable behavior cluster, not one bead per upstream file.

## Sources To Compare

### Upstream Godot

Primary upstream code areas:

- `upstream/godot/scene/3d/`
- `upstream/godot/scene/3d/physics/`
- `upstream/godot/scene/resources/3d/`
- `upstream/godot/servers/rendering/`
- `upstream/godot/servers/physics_3d/`

Representative upstream files for the initial slice:

- `scene/3d/node_3d.*`
- `scene/3d/camera_3d.*`
- `scene/3d/mesh_instance_3d.*`
- `scene/3d/light_3d.*`
- `scene/3d/physics/rigid_body_3d.*`
- `scene/3d/physics/static_body_3d.*`
- `scene/3d/physics/character_body_3d.*`
- `scene/3d/physics/area_3d.*`
- `scene/3d/physics/ray_cast_3d.*`
- `scene/resources/3d/world_3d.*`

### Patina

Primary local crates:

- `engine-rs/crates/gdserver3d/`
- `engine-rs/crates/gdrender3d/`
- `engine-rs/crates/gdphysics3d/`
- `engine-rs/crates/gdscene/` for Node3D / scene-tree bindings

Primary local evidence:

- `engine-rs/tests/real_3d_demo_unified_parity_test.rs`
- `engine-rs/tests/real_3d_demo_aggregate_parity_test.rs`
- `engine-rs/tests/real_3d_demo_parity_report_artifact_test.rs`
- `engine-rs/tests/render_3d_parity_test.rs`
- `engine-rs/tests/physics3d_trace_comparison_test.rs`
- `engine-rs/tests/representative_3d_fixtures_test.rs`
- `docs/3D_DEMO_PARITY_REPORT.md`
- `docs/3D_ARCHITECTURE_SPEC.md`
- `docs/migration-guide.md`
- `COMPAT_DASHBOARD.md`
- `COMPAT_MATRIX.md`

## Crate Boundary Classification

Each 3D-related crate is mapped to the audited class families it owns.
The validation test `ci_3d_crate_boundary_audit_test.rs` guards this mapping.

### `gdserver3d` — 3D Server Abstractions

Responsible for server-side 3D data models, resource types, and rendering
primitives. Does not own scene-tree nodes or physics simulation.

| Module | Audited Families | Status |
|--------|-----------------|--------|
| `server.rs` | RenderingServer3D trait boundary | Measured |
| `light.rs` | DirectionalLight3D, OmniLight3D, SpotLight3D (data) | Measured |
| `mesh.rs`, `primitive_mesh.rs` | MeshInstance3D (server-side mesh data) | Measured |
| `material.rs`, `shader.rs` | StandardMaterial3D, ShaderMaterial3D | Partially measured |
| `environment.rs`, `sky.rs` | WorldEnvironment, Sky | Implemented, not measured |
| `reflection_probe.rs` | ReflectionProbe | Implemented, not measured |
| `fog_volume.rs` | FogVolume | Implemented, not measured |
| `csg.rs` | CSGCombiner3D, CSGBox3D, CSGSphere3D, CSGCylinder3D | Implemented, not measured |
| `navigation.rs` | NavigationRegion3D (mesh baking model) | Partial / deferred |
| `multimesh.rs` | MultiMeshInstance3D | Implemented, not measured |
| `particles3d.rs` | Particle systems | Deferred |
| `gi.rs`, `occluder.rs` | GI probes, occluders | Deferred |

### `gdrender3d` — 3D Software and GPU Rendering

Responsible for the actual rendering pipeline: software rasterizer,
depth buffer, shadow maps, and wgpu integration.

| Module | Audited Families | Status |
|--------|-----------------|--------|
| `renderer.rs` | Software renderer (deterministic path) | Measured |
| `rasterizer.rs` | Triangle rasterization | Measured |
| `depth_buffer.rs` | Depth testing | Measured |
| `shadow_map.rs` | Shadow rendering | Partially measured |
| `shader.rs` | Shader pipeline | Partially measured |
| `wgpu_pipeline.rs` | GPU rendering path | Deferred for parity |

### `gdphysics3d` — 3D Physics Simulation

Responsible for rigid body simulation, collision detection, queries,
and character movement.

| Module | Audited Families | Status |
|--------|-----------------|--------|
| `world.rs` | PhysicsWorld3D (deterministic stepping) | Measured |
| `body.rs` | RigidBody3D, StaticBody3D | Measured |
| `character.rs` | CharacterBody3D | Measured |
| `collision.rs`, `shape.rs` | CollisionShape3D | Measured |
| `query.rs` | PhysicsRayQuery3D, PhysicsShapeQuery3D | Measured |
| `area3d.rs` | Area3D | Implemented, not measured |
| `joint.rs` | PhysicsJoints (Pin/Hinge/Slider data) | Implemented, not measured |

### `gdscene` — Scene-Tree 3D Bindings

Provides Node3D, Camera3D, and scene-tree integration for 3D nodes.
This crate bridges server-side data into the scene tree.

| Module | Audited Families | Status |
|--------|-----------------|--------|
| `node3d.rs` | Node3D transform chain | Measured |
| `camera3d.rs` | Camera3D extraction/projection | Measured |
| `skeleton3d.rs` | Skeleton3D | Implemented, not measured |
| `render_server_3d.rs` | RenderServer3D adapter | Measured |
| `physics_server.rs`, `physics_server_3d.rs` | Physics server bindings | Measured |
| `particle3d.rs` | 3D particles (scene integration) | Deferred |

### Boundary Rules

1. `gdserver3d` owns data models. `gdrender3d` owns rendering. `gdphysics3d` owns simulation.
2. `gdscene` bridges server data into the scene tree — it does not own simulation or rendering.
3. Parity claims must cite the owning crate, not just the scene-tree binding.
4. Deferred modules exist in the crate structure but are not part of the Phase 6 parity claim.

## Current Patina 3D Read

### What is clearly real and measured

The 3D slice is not hypothetical. Patina has dedicated crates and meaningful
tests for:

- 3D render server and software rendering
- 3D scene fixtures and aggregate reports
- deterministic 3D physics stepping
- camera, light, transform, and scene-tree integration

Concrete local evidence includes:

- `render_3d_parity_test.rs` for deterministic framebuffer behavior
- `real_3d_demo_unified_parity_test.rs` for end-to-end transform/render/physics
- `docs/3D_DEMO_PARITY_REPORT.md` for the bounded Phase 6 runtime claim

### What the milestone report actually claims

The first real 3D demo report explicitly says:

- it is a Phase 6 runtime slice
- it is not a claim of full Godot 3D parity
- aggregate scene-tree parity is still partial
- render and physics dimensions are still skipped in the aggregate parity view

That is the right level of claim for the current evidence.

## Upstream 3D Surface Snapshot

Initial upstream class counts seen in the pinned tree:

- `scene/3d/*.h`: 55 headers
- `scene/3d/physics/*.h`: 17 headers

That upstream surface includes many classes that are broader than the current
Patina Phase 6 slice, including:

- `ReflectionProbe`
- `FogVolume`
- `Decal`
- `AudioStreamPlayer3D`
- `MultiMeshInstance3D`
- `NavigationRegion3D`
- `VehicleBody3D`
- `SoftBody3D`
- physics joints
- multiple skeletal / IK / modifier families
- GI and probe systems
- particle families

This means the audit must separate:

- the measured Phase 6 runtime slice
- broader 3D systems that exist upstream
- deferred or partially supported 3D features

## Claim Mismatch: Docs vs Measured Evidence

The strongest immediate audit issue is documentation drift.

`docs/3D_DEMO_PARITY_REPORT.md` makes a bounded slice claim.

`docs/migration-guide.md` currently labels many 3D node and physics families as
`Full`, including:

- `MeshInstance3D`
- `MultiMeshInstance3D`
- `Sprite3D`
- `Camera3D`
- `Skeleton3D`
- `BoneAttachment3D`
- many 3D shapes and body families
- several CSG families
- `AudioStreamPlayer3D`

Some of those may be implemented. The issue is that the document currently
states `Full` more broadly than the measured report can defend.

This is the first parity gap to fix:

- not necessarily code
- but parity claim hygiene and evidence mapping

## Initial Phase 6 Classification

This is the first audit pass, not the final matrix.

### First Matrix Rows

| Upstream Family | Upstream Path | Patina Area | Current Status | Evidence | Gap Type | Existing Bead | Action |
|-----------------|---------------|-------------|----------------|----------|----------|---------------|--------|
| `Node3D` transform chain | `scene/3d/node_3d.*` | `gdscene::node3d`, 3D fixture tests | Measured | `real_3d_demo_unified_parity_test.rs`, `node3d_transform_propagation_parity_test.rs`, `docs/3D_DEMO_PARITY_REPORT.md` | none | none needed | reuse current evidence |
| `Camera3D` basic extraction / projection | `scene/3d/camera_3d.*` | `gdscene::node3d`, `gdserver3d`, `gdrender3d` | Measured for slice | `real_3d_demo_unified_parity_test.rs`, `render_3d_parity_test.rs`, `transform3d_camera_light_contract_test.rs` | missing breadth | closed normalization beads | add breadth only if claim expands |
| `MeshInstance3D` basic render path | `scene/3d/mesh_instance_3d.*` | `gdscene`, `gdserver3d`, `gdrender3d` | Measured for basic slice | `render_3d_parity_test.rs`, `real_3d_demo_unified_parity_test.rs` | docs-overclaim | none specific | narrow docs from broad `Full` to slice-backed wording |
| `RigidBody3D` deterministic stepping | `scene/3d/physics/rigid_body_3d.*` | `gdphysics3d` | Measured for covered traces | `physics3d_trace_comparison_test.rs`, `physics3d_single_body_golden_test.rs`, `docs/3D_DEMO_PARITY_REPORT.md` | missing breadth | none needed | reuse current evidence |
| `StaticBody3D` fixture collision slice | `scene/3d/physics/static_body_3d.*` | `gdphysics3d` | Measured for covered fixtures | `physics3d_scene_hooks_deterministic_test.rs`, `real_3d_demo_unified_parity_test.rs` | missing breadth | none needed | reuse current evidence |
| `CharacterBody3D` movement | `scene/3d/physics/character_body_3d.*` | `gdphysics3d` | Measured narrowly | `characterbody3d_move_and_slide_test.rs`, `characterbody3d_move_and_slide_3d_test.rs` | missing breadth | closed `CharacterBody3D` bead | reuse; only add if new behavior gap found |
| `Area3D` overlap and signals | `scene/3d/physics/area_3d.*` | `gdphysics3d` | Implemented, not yet clearly tied into Phase 6 slice docs | `gdphysics3d/src/area3d.rs`, closed `Area3D` bead | docs-overclaim | closed `Area3D with overlap detection and signal emission` | add doc/evidence mapping, not new impl bead |
| `ReflectionProbe` | `scene/3d/reflection_probe.*` | `gdserver3d`, scene fixtures | Implemented, not yet measured as parity claim | closed `ReflectionProbe` bead, `fixtures/scenes/csg_composition.tscn` | missing-test | closed `ReflectionProbe for local cubemap reflections` | add measurement or narrow docs |
| `FogVolume` | `scene/3d/fog_volume.*` | `gdserver3d`, editor viewport support, fixtures | Implemented, not yet measured as parity claim | `gdserver3d/src/fog_volume.rs`, `fixtures/scenes/foggy_terrain_3d.tscn` | missing-test | closed `FogVolume node for volumetric fog regions` | add measurement or narrow docs |
| `Decal` | `scene/3d/decal.*` | scene/docs only | Implemented, not yet measured as parity claim | closed `Decal node for projected texture decals` | missing-test | closed `Decal` bead | add measurement or narrow docs |
| `NavigationRegion3D` mesh baking | `scene/3d/navigation_*`, resources | `gdserver3d/src/navigation.rs`, docs | Partial / deferred at runtime | migration guide limitation section | none if docs stay narrow | closed `NavigationRegion3D with 3D navigation mesh baking` | keep as partial, do not open parity bead unless runtime pathfinding enters scope |
| `VehicleBody3D` | `scene/3d/physics/vehicle_body_3d.*` | none in slice | Deferred | limitation documented in `docs/migration-guide.md` | deferred | none | no bead unless scope changes |
| `SoftBody3D` | `scene/3d/physics/soft_body_3d.*` | none in slice | Deferred | limitation documented in `docs/migration-guide.md` | deferred | none | no bead unless scope changes |
| physics joints | `scene/3d/physics/joints/*` | none in slice | Deferred | limitation documented in `docs/migration-guide.md` | deferred | closed historical joint bead exists | no new bead unless scope changes |

### Render / Environment Lane Notes

This lane is where the current docs are most likely to overstate support.

#### `WorldEnvironment` / `Environment3D`

- Upstream source family: `scene/3d/world_environment.*`, `scene/resources/3d/world_3d.*`
- Patina evidence:
  - `engine-rs/crates/gdserver3d/src/environment.rs`
  - `engine-rs/tests/viewport_3d_environment_preview_test.rs`
  - `engine-rs/tests/sky_resource_panoramic_procedural_test.rs`
- Current classification: `Implemented, not yet measured as full parity`
- Reason:
  - Patina has typed environment resources, background modes, ambient source, fog, and tone mapping structures.
  - Tests exist for editor-style environment preview behavior and resource roundtrip.
  - That is good implementation evidence, but not yet a direct scene/runtime parity proof against Godot for broad `WorldEnvironment` behavior.

#### `Sky`

- Upstream source family: `scene/resources/3d/sky_*`
- Patina evidence:
  - `engine-rs/tests/sky_resource_panoramic_procedural_test.rs`
  - `engine-rs/tests/viewport_3d_environment_preview_test.rs`
- Current classification: `Implemented, not yet measured as runtime parity`
- Reason:
  - Patina clearly implements `Procedural`, `Panoramic`, and `Physical` sky material handling and tests roundtrip plus preview rendering.
  - The evidence is currently stronger for resource semantics and editor preview than for scene/runtime parity.

#### `ReflectionProbe`

- Upstream source family: `scene/3d/reflection_probe.*`
- Patina evidence:
  - `engine-rs/crates/gdserver3d/src/reflection_probe.rs`
  - `fixtures/scenes/csg_composition.tscn`
  - closed tracker bead for ReflectionProbe
- Current classification: `Implemented, not yet measured`
- Reason:
  - Patina has the data model and tests for default values and influence bounds.
  - This is not yet the same as measured reflection parity in rendered scenes.

#### `FogVolume`

- Upstream source family: `scene/3d/fog_volume.*`
- Patina evidence:
  - `engine-rs/crates/gdserver3d/src/fog_volume.rs`
  - `engine-rs/tests/viewport_3d_environment_preview_test.rs`
  - `fixtures/scenes/foggy_terrain_3d.tscn`
- Current classification: `Implemented, partially measured`
- Reason:
  - Patina has a fog volume model and editor-preview oriented test coverage.
  - The evidence is enough to say the feature exists, but not enough to claim broad runtime parity for volumetric fog regions in-scene.

#### `Decal`

- Upstream source family: `scene/3d/decal.*`
- Patina evidence:
  - `engine-rs/crates/gdscene/src/decal.rs`
  - closed tracker bead for Decal
- Current classification: `Implemented, not yet measured`
- Reason:
  - Patina appears to have a dedicated Decal3D model and registry with substantial local tests.
  - There is still no explicit parity fixture or report tying decal behavior to upstream Godot output.

#### Resulting Non-Duplicate Tasks

Do not open new implementation beads for these features yet.

Open measurement or docs-alignment beads instead:

1. `Phase 6 parity: measure Environment3D / WorldEnvironment behavior against scene fixtures`
2. `Phase 6 parity: add ReflectionProbe / FogVolume / Decal fixture-backed evidence or narrow support claims`
3. `Phase 6 audit: downgrade 3D migration-guide statuses where only implementation evidence exists`

### Materials / Render Server Lane Notes

#### `RenderingServer3D` trait boundary

- Upstream source family: `servers/rendering/rendering_server.*`
- Patina evidence:
  - `engine-rs/crates/gdserver3d/src/server.rs`
  - `engine-rs/crates/gdrender3d/src/renderer.rs`
  - `engine-rs/crates/gdrender3d/src/wgpu_pipeline.rs`
  - `engine-rs/tests/render_3d_parity_test.rs`
- Current classification: `Measured for bounded Phase 6 slice`
- Reason:
  - Patina has a clear 3D rendering server abstraction with instance, material, light, probe, and viewport operations.
  - The software renderer is exercised directly by deterministic framebuffer tests.
  - The GPU path exists, but Phase 6 parity should still be grounded in the software path because that is what the docs describe as the deterministic validation route.

#### `Material3D` / `StandardMaterial3D`

- Upstream source family: `scene/resources/material.*`, `StandardMaterial3D`
- Patina evidence:
  - `engine-rs/crates/gdserver3d/src/material.rs`
  - `engine-rs/tests/render_3d_pipeline_shader_material_test.rs`
- Current classification: `Implemented, partially measured`
- Reason:
  - Patina has base material structs, texture slots, and conversion from property bags.
  - Tests demonstrate albedo, shading modes, and shader overrides in the render path.
  - That is meaningful slice evidence, but still weaker than broad Godot `StandardMaterial3D` parity across all surface features.

#### `ShaderMaterial3D` / shader override path

- Upstream source family: shader material / spatial shader pipeline
- Patina evidence:
  - `engine-rs/tests/render_3d_pipeline_shader_material_test.rs`
  - `engine-rs/crates/gdrender3d/src/shader.rs`
  - `engine-rs/crates/gdrender3d/src/wgpu_pipeline.rs`
- Current classification: `Measured for narrow slice`
- Reason:
  - Patina clearly supports shader-material override semantics for the tested slice.
  - The measured claim should remain narrow: custom shader color override, unshaded behavior, transform influence, and shading mode differences.

#### `Mesh3D` / primitive render path

- Upstream source family: `MeshInstance3D`, primitive mesh resources
- Patina evidence:
  - `engine-rs/crates/gdserver3d/src/mesh.rs`
  - `engine-rs/tests/render_3d_parity_test.rs`
  - `engine-rs/tests/render_3d_parity_hooks_test.rs`
- Current classification: `Measured for primitive/fixture slice`
- Reason:
  - The current evidence supports cubes, spheres, planes, visible/deterministic rendering, and material-driven pixel changes.
  - That is enough for the current slice, not for all Godot mesh/resource semantics.

#### `Shadow`-related light behavior

- Upstream source family: `light_3d.*`, rendering shadow behavior
- Patina evidence:
  - `engine-rs/tests/light3d_shadow_hint_alignment_test.rs`
  - multiple light/shadow-related tests in the 3D suite
- Current classification: `Implemented, partly measured`
- Reason:
  - Property defaults and hints are tested.
  - True rendered shadow parity still appears narrower than the property-level tests alone would imply.

#### Resulting Non-Duplicate Tasks

Do not open new crate/bootstrap beads here.

Prefer these parity tasks:

1. `Phase 6 parity: reconcile StandardMaterial3D support claims with measured shader/material coverage`
2. `Phase 6 parity: classify which render-server features are deterministic test-backed versus implementation-only`
3. `Phase 6 parity: separate shadow-property coverage from rendered shadow parity`

### Physics / Query Lane Notes

This lane needs the clearest separation between:

- physics behavior that is already measured in simulation tests
- low-level API surface that exists but is not yet proven as scene/runtime parity
- deferred runtime systems that should not create duplicate implementation beads

#### `RigidBody3D`

- Upstream source family: `scene/3d/physics/rigid_body_3d.*`
- Patina evidence:
  - `engine-rs/tests/rigidbody3d_forces_torques_contacts_test.rs`
  - `engine-rs/tests/physics3d_trace_comparison_test.rs`
  - `engine-rs/tests/physics3d_single_body_golden_test.rs`
- Current classification: `Measured for bounded runtime slice`
- Reason:
  - Patina has direct coverage for ClassDB registration, forces, torques,
    contacts, damping, sleeping, freeze modes, and deterministic stepping.
  - That is stronger than a property-only claim and is enough for the bounded
    Phase 6 slice.
  - It is still not the same as broad parity for every Godot `RigidBody3D`
    behavior such as joints, CCD edge cases, or advanced solver interactions.

#### `StaticBody3D`

- Upstream source family: `scene/3d/physics/static_body_3d.*`
- Patina evidence:
  - `engine-rs/tests/physics3d_scene_hooks_deterministic_test.rs`
  - `engine-rs/tests/real_3d_demo_unified_parity_test.rs`
  - `engine-rs/tests/node3d_runtime_slice_test.rs`
- Current classification: `Measured for fixture-backed slice`
- Reason:
  - Static collision participation is exercised through deterministic 3D
    physics scenes and runtime-slice integration.
  - No new implementation bead is justified unless a specific missing static
    collision behavior is identified.

#### `CharacterBody3D`

- Upstream source family: `scene/3d/physics/character_body_3d.*`
- Patina evidence:
  - `engine-rs/tests/characterbody3d_move_and_slide_3d_test.rs`
  - closed tracker bead `CharacterBody3D move_and_slide for 3D`
- Current classification: `Measured narrowly`
- Reason:
  - Patina covers move-and-slide, move-and-collide, floor/wall/ceiling
    classification, collision filtering, and tunneling-prevention cases.
  - This is good bounded coverage, but not a blanket parity claim for all Godot
    `CharacterBody3D` movement nuances.

#### `Area3D`

- Upstream source family: `scene/3d/physics/area_3d.*`
- Patina evidence:
  - `engine-rs/crates/gdphysics3d/src/area3d.rs`
  - `engine-rs/tests/node3d_runtime_slice_test.rs`
  - `docs/migration-guide.md`
- Current classification: `Implemented, not yet cleanly measured as runtime parity`
- Reason:
  - Patina clearly has low-level area overlap storage and enter/exit event
    generation for bodies and areas.
  - The current visible runtime wiring and tests are much weaker than the 2D
    `Area2D` path; this audit did not find matching 3D MainLoop or scene-signal
    parity tests for `body_entered` / `area_entered`.
  - The migration guide currently marks `Area3D` as `Full`, which is stronger
    than the measured evidence identified so far.

#### `PhysicsRayQueryParameters3D` / `PhysicsShapeQueryParameters3D`

- Upstream source family: direct-space-state query parameter objects
- Patina evidence:
  - `engine-rs/crates/gdphysics3d/src/query.rs`
  - `engine-rs/tests/physics_ray_shape_query3d_test.rs`
- Current classification: `Measured for low-level query objects`
- Reason:
  - Patina has meaningful coverage for ray hit/miss behavior, exclusion lists,
    collision masks, closest-hit selection, shape overlap depth, and world
    integration.
  - This is solid evidence for the parameter/query-object layer itself.
  - It should not be inflated into node-level parity for `RayCast3D` or
    `ShapeCast3D`.

#### `RayCast3D` / `ShapeCast3D`

- Upstream source family: `scene/3d/physics/ray_cast_3d.*`,
  `scene/3d/physics/shape_cast_3d.*`
- Patina evidence:
  - low-level query support only via `gdphysics3d::query`
- Current classification: `Missing or deferred at node/runtime layer`
- Reason:
  - This audit found low-level query objects, but did not find corresponding
    `RayCast3D` / `ShapeCast3D` node registration, defaults, scene integration,
    or parity tests.
  - The correct next bead here is a classification/measurement bead, not a
    duplicate of the existing low-level query work.

#### physics joints

- Upstream source family: `scene/3d/physics/joint_3d.*` and concrete joint nodes
- Patina evidence:
  - `engine-rs/crates/gdphysics3d/src/joint.rs`
  - `docs/migration-guide.md`
- Current classification: `Implemented data types, deferred runtime parity`
- Reason:
  - Patina does contain `PinJoint3D`, `HingeJoint3D`, and `SliderJoint3D`
    structs with unit coverage for their parameter semantics.
  - The migration guide still says there are no physics joints, which means the
    runtime/node/system integration is not being claimed.
  - The correct action is to classify this as deferred runtime parity and fix
    docs wording, not reopen generic “implement joints” beads.

#### `SoftBody3D` / `VehicleBody3D` / `SpringArm3D`

- Upstream source family: `soft_body_3d.*`, `vehicle_body_3d.*`,
  `spring_arm_3d.*`
- Patina evidence:
  - `docs/migration-guide.md` limitation section only
- Current classification:
  - `SoftBody3D`: `Deferred`
  - `VehicleBody3D`: `Deferred`
  - `SpringArm3D`: `Missing / not in current slice`
- Reason:
  - These are outside the bounded Phase 6 runtime claim.
  - No new implementation beads should be created unless project scope changes
    or an explicit post-Phase-6 milestone pulls them in.

#### Resulting Non-Duplicate Tasks

Do not open new rigid-body or character-body implementation beads.

Prefer these parity tasks:

1. `Phase 6 parity: reconcile Area3D support claims with actual runtime signal coverage`
2. `Phase 6 parity: separate low-level query parameter support from RayCast3D and ShapeCast3D node parity`
3. `Phase 6 audit: classify joint support as data-model-only versus integrated runtime behavior`
4. `Phase 6 docs: keep SoftBody3D, VehicleBody3D, and SpringArm3D explicitly outside the measured slice`

### Measured

These are supported by direct tests or Phase 6 report evidence:

- `Node3D` transform propagation
- `Camera3D` extraction and projection basics
- mesh rendering through the software renderer
- basic 3D lighting path
- deterministic `RigidBody3D` / `StaticBody3D` slice
- representative 3D fixture loading
- aggregate 3D report artifacts
- low-level 3D ray and shape query objects

### Implemented, not yet cleanly measured at parity level

These appear in local code or docs, but need explicit parity evidence mapped:

- `MultiMeshInstance3D`
- `ReflectionProbe`
- `FogVolume`
- `Decal`
- `NavigationRegion3D`
- `Area3D` runtime signal behavior
- physics-joint runtime integration
- broader material and shader behavior
- broader scene-tree classification for 3D nodes

### Deferred or explicitly limited

These are already called out as limited or absent and should not be represented
as parity gaps for the bounded Phase 6 slice unless project scope changes:

- runtime `NavigationAgent3D`
- `SoftBody3D`
- `VehicleBody3D`
- `SpringArm3D`
- broad GPU / post-processing parity
- full gameplay-behavior parity for 3D projects

## 3D Fixture Corpus Definition

The following table maps each audited 3D class family to its representative
fixture(s) in the corpus. This is the authoritative corpus definition for the
Phase 6 runtime slice.

The validation test `representative_3d_fixtures_test.rs` guards this mapping:
every audited family marked `Measured` must appear in at least one corpus
fixture, and every corpus fixture must parse and load successfully.

### Corpus Fixtures

| Fixture | Primary Class Families Covered |
|---------|-------------------------------|
| `minimal_3d.tscn` | Node3D, Camera3D, MeshInstance3D, DirectionalLight3D |
| `indoor_3d.tscn` | Node3D, Camera3D, MeshInstance3D, OmniLight3D, StaticBody3D, CollisionShape3D |
| `physics_3d_playground.tscn` | RigidBody3D, StaticBody3D, CollisionShape3D, MeshInstance3D, Camera3D, DirectionalLight3D |
| `multi_light_3d.tscn` | DirectionalLight3D, OmniLight3D, SpotLight3D, MeshInstance3D, StaticBody3D |
| `hierarchy_3d.tscn` | Node3D transform hierarchy |
| `outdoor_3d.tscn` | RigidBody3D, MeshInstance3D, OmniLight3D |
| `vehicle_3d.tscn` | RigidBody3D, MeshInstance3D, Camera3D |
| `spotlight_gallery_3d.tscn` | SpotLight3D, MeshInstance3D |
| `animated_scene_3d.tscn` | Skeleton3D, AnimationPlayer, MeshInstance3D |
| `physics_playground_extended.tscn` | RigidBody3D, StaticBody3D, CollisionShape3D |
| `foggy_terrain_3d.tscn` | FogVolume, WorldEnvironment, DirectionalLight3D, OmniLight3D, RigidBody3D |
| `csg_composition.tscn` | CSGCombiner3D, CSGBox3D, CSGSphere3D, CSGCylinder3D, ReflectionProbe |

### Audited Family Coverage Map

| Audited Family | Status | Corpus Fixture(s) |
|----------------|--------|--------------------|
| Node3D transform chain | Measured | minimal_3d, indoor_3d, hierarchy_3d |
| Camera3D | Measured | minimal_3d, indoor_3d, physics_3d_playground |
| MeshInstance3D | Measured | indoor_3d, multi_light_3d, outdoor_3d |
| DirectionalLight3D | Measured | minimal_3d, physics_3d_playground, multi_light_3d, foggy_terrain_3d |
| OmniLight3D | Measured | indoor_3d, multi_light_3d, foggy_terrain_3d |
| SpotLight3D | Measured | multi_light_3d, spotlight_gallery_3d |
| RigidBody3D | Measured | physics_3d_playground, outdoor_3d, vehicle_3d |
| StaticBody3D | Measured | indoor_3d, physics_3d_playground, multi_light_3d |
| CollisionShape3D | Measured | indoor_3d, physics_3d_playground |
| Skeleton3D / AnimationPlayer | Measured | animated_scene_3d |
| FogVolume | Measured | foggy_terrain_3d |
| WorldEnvironment | Measured | foggy_terrain_3d |
| CSG families | Measured | csg_composition |
| ReflectionProbe | Measured | csg_composition |
| CharacterBody3D | Implemented, not yet in corpus | — (2D fixture exists; 3D fixture deferred) |
| Area3D | Implemented, not yet in corpus | — (runtime signal coverage gap noted above) |
| Decal | Implemented, not yet in corpus | — (data model only, no scene fixture yet) |
| NavigationRegion3D | Partial / deferred | — (outside bounded Phase 6 slice) |

### Corpus Rules

1. Every `Measured` family must have at least one corpus fixture containing that class.
2. Every corpus fixture must have a matching golden JSON in `fixtures/golden/scenes/`.
3. The test `fixture_corpus_maps_to_audited_families` validates rule 1.
4. Families classified as `Implemented, not yet in corpus` or `Deferred` do not
   block the corpus gate — they are tracked as future work items.

## Crate Boundary Classification (pat-hx666)

This section maps each Patina 3D crate/module to its role in the Phase 6
runtime slice.  The validation test `crate_boundary_3d_audit_test.rs` guards
this mapping: every crate listed here must exist, and the module counts must
stay within the declared bounds.

### Phase 6 3D Crates

| Crate | Role in Slice | Key Modules | Status |
|-------|---------------|-------------|--------|
| `gdscene` | 3D scene-tree nodes and lifecycle | `node3d`, `camera3d`, `skeleton3d`, `particle3d`, `decal`, `lod`, `physics_server_3d`, `render_server_3d`, `collision` | Measured — transform, camera, skeleton, lifecycle |
| `gdserver3d` | Abstract 3D rendering server surface | `server`, `mesh`, `material`, `light`, `shader`, `sky`, `environment`, `fog_volume`, `reflection_probe`, `csg`, `gi`, `navigation`, `particles3d`, `multimesh`, `occluder`, `primitive_mesh`, `projection`, `viewport`, `instance` | Measured — server trait, mesh, material, light; partially measured — fog, reflection, CSG |
| `gdrender3d` | Software 3D renderer implementation | `renderer`, `rasterizer`, `shader`, `shadow_map`, `depth_buffer`, `compare`, `test_adapter` | Measured — deterministic framebuffer, shadow maps |
| `gdphysics3d` | 3D physics simulation | `body`, `character`, `shape`, `collision`, `world`, `area3d`, `query`, `joint` | Measured — rigid/static/character body, query; partially measured — area3d, joint |

### Supporting Crates (not 3D-specific but exercised by the 3D slice)

| Crate | Contribution to 3D Slice |
|-------|-------------------------|
| `gdcore` | `math3d` (Transform3D, Basis, Quaternion, Projection), `compare3d` |
| `gdobject` | ClassDB registration for 3D node types |
| `gdresource` | Scene/resource loading for `.tscn` fixtures with 3D nodes |
| `gdvariant` | Variant serialization for 3D property types (Vector3, Transform3D, etc.) |

### Boundary Rules

1. Each crate in the Phase 6 3D slice must appear in the table above.
2. `gdscene`, `gdserver3d`, `gdrender3d`, and `gdphysics3d` are the four
   primary 3D crates.  All other 3D behavior routes through them.
3. No 3D runtime behavior should live outside these four crates and their
   supporting dependencies.
4. The test `crate_boundary_3d_audit_test.rs` validates that these crates
   exist and expose the expected module surface.

## Existing Beads To Reuse

Do not create duplicates for these active Phase 6 beads:

- `pat-hx666` Define and bootstrap the first 3D crate set
- `pat-zaafu` Plan the first 3D fixture corpus and oracle capture flow
- `pat-on9xe` Add 3D render and physics comparison tooling
- `pat-57aw6` Produce the first real 3D demo parity report

Do not recreate already-closed 3D work where the title and acceptance match
existing evidence. Examples already closed in the tracker include:

- `ReflectionProbe for local cubemap reflections`
- `Decal node for projected texture decals`
- `NavigationRegion3D with 3D navigation mesh baking`
- `CharacterBody3D move_and_slide for 3D`
- `CollisionShape3D with all 3D shape types`
- `3D viewport environment preview`
- multiple Camera3D / Transform3D / Light3D normalization tasks

Any new bead must answer:

1. Why does the existing bead set not already cover it?
2. What exact class family or measurable behavior is still missing?
3. What command, fixture, or report proves it done?

## Bead Candidates From This Audit

These are the first non-duplicative candidate beads.

### Candidate 1

Title:
`Phase 6 audit: reconcile 3D support claims with measured evidence`

Acceptance:

- every 3D row in `docs/migration-guide.md` is reclassified as `Measured`,
  `Implemented, not yet measured`, `Deferred`, or `Missing`
- each `Measured` row cites at least one concrete test or report file
- `COMPAT_MATRIX.md`, `COMPAT_DASHBOARD.md`, and migration docs no longer make
  stronger 3D claims than the Phase 6 report supports

### Candidate 2

Title:
`Phase 6 audit: classify upstream 3D class surface against Patina slice`

Acceptance:

- upstream `scene/3d` and `scene/3d/physics` class families are mapped into a
  checked-in matrix
- each family is labeled `Measured`, `Implemented, not yet measured`,
  `Deferred`, or `Missing`
- the matrix cites both upstream source area and Patina evidence location

### Candidate 3

Title:
`Phase 6 parity: isolate the persistent 3D scene-tree node gap`

Acceptance:

- the consistent 1-node gap noted in `docs/3D_DEMO_PARITY_REPORT.md` is traced
  to a specific class or scene-tree behavior
- one or more focused tests or follow-up beads are produced from that cause

### Candidate 4

Title:
`Phase 6 parity: separate supported 3D runtime slice from deferred 3D systems`

Acceptance:

- deferred systems such as joints, `SoftBody3D`, `VehicleBody3D`, and advanced
  3D runtime navigation are explicitly classified in docs
- no Phase 6 doc implies parity for those systems without evidence

### Candidate 5

Title:
`Phase 6 parity: reconcile Area3D and 3D query-node claims with measured runtime coverage`

Acceptance:

- `Area3D`, `PhysicsRayQueryParameters3D`, `PhysicsShapeQueryParameters3D`,
  `RayCast3D`, and `ShapeCast3D` are each classified separately in docs and the
  audit matrix
- low-level query parameter support is not mislabeled as node parity
- any `Full` claim for `Area3D` is backed by explicit runtime signal tests or
  downgraded to match current evidence

## Instructions For Continuing This Audit

Follow this order:

1. Build the matrix from upstream class families, not from Patina docs.
2. For each family, map Patina evidence before making any claim.
3. Reconcile docs before creating new implementation beads.
4. Only after the matrix exists, open new beads for `Missing` or
   `Implemented, not yet measured` items with explicit evidence requirements.

Recommended row format:

| Upstream Family | Upstream Path | Patina Area | Current Status | Evidence | Gap Type | Existing Bead | Action |
|-----------------|---------------|-------------|----------------|----------|----------|---------------|--------|

Where:

- `Gap Type` is one of `docs-overclaim`, `missing-test`, `missing-impl`, `deferred`
- `Existing Bead` must be filled before any new bead is proposed
- `Action` should be `reuse`, `narrow docs`, `add measurement`, or `new bead`

## Comparison Tooling for Audited Dimensions

The Phase 6 audit identifies three measurable parity dimensions:

1. **Render** — framebuffer pixel comparison (`RenderCompareResult3D`)
2. **Physics** — deterministic trace comparison (`compare_physics_traces`)
3. **Scene tree** — structural node/class comparison (`compare_scene_trees`)

### Tooling location

| Dimension | Comparison function | Type | Crate |
|-----------|-------------------|------|-------|
| Render | pixel match ratio | `RenderCompareResult3D` | `gdcore::compare3d` |
| Physics | trace entry-by-entry | `compare_physics_traces()` | `gdcore::compare3d` |
| Scene tree | path + class matching | `compare_scene_trees()` | `gdcore::compare3d` |
| Per-fixture report | all three dimensions | `FixtureParityReport3D` | `gdcore::compare3d` |
| Aggregate report | across fixtures | `AggregateParityReport3D` | `gdcore::compare3d` |
| Batch report | subsystem scores | `BatchComparisonReport` | `gdcore::comparison_tooling` |

### Verdict thresholds

Each dimension produces a `DimensionVerdict`:

- **Pass**: match ratio >= 0.95 (physics/render) or exact match (scene tree)
- **Partial**: match ratio >= 0.70 (physics/render) or >= 0.80 (scene tree)
- **Fail**: below the partial threshold

Overall fixture verdict is Pass only if all non-skipped dimensions pass.

### Command path

Run the end-to-end comparison tests:

```bash
cargo nextest run -p patina-engine render_physics_comparison_tooling_test
cargo nextest run -p patina-engine comparison_tooling_3d_test
```

### End-to-end coverage

The test `render_physics_comparison_tooling_test` exercises the full pipeline:

1. Loads real `.tscn` fixtures (`minimal_3d`, `hierarchy_3d`, `indoor_3d`,
   `multi_light_3d`, `physics_3d_playground`)
2. Builds Patina scene trees and compares against golden oracle JSON
3. Compares physics golden traces via `compare_physics_traces`
4. Renders via `RenderServer3DAdapter` and derives render metrics
5. Produces `FixtureParityReport3D` per fixture with verdicts per dimension
6. Aggregates into `AggregateParityReport3D` with pass/partial/fail counts
7. Validates JSON and text output formats

This is the primary tooling path for validating the Phase 6 3D runtime slice.

## 3D Fixture Corpus — Audit Family Coverage

This section maps the checked-in 3D fixtures to the audit's class family
classification. A fixture "covers" a family if at least one golden scene
contains a node of that class.

### Measured Families — Fixture Coverage

| Family | Fixture(s) |
|--------|-----------|
| `Node3D` | all 3D fixtures |
| `Camera3D` | all 3D fixtures |
| `MeshInstance3D` | all 3D fixtures |
| `DirectionalLight3D` | minimal_3d, hierarchy_3d, outdoor_3d, multi_light_3d, foggy_terrain_3d, physics_3d_playground, vehicle_3d, animated_scene_3d |
| `OmniLight3D` | indoor_3d, outdoor_3d, multi_light_3d, foggy_terrain_3d, animated_scene_3d |
| `SpotLight3D` | spotlight_gallery_3d |
| `StaticBody3D` | minimal_3d, indoor_3d, multi_light_3d, spotlight_gallery_3d, outdoor_3d, foggy_terrain_3d, physics_3d_playground, vehicle_3d, animated_scene_3d |
| `RigidBody3D` | outdoor_3d, foggy_terrain_3d, physics_3d_playground, vehicle_3d |
| `CollisionShape3D` | minimal_3d, indoor_3d, multi_light_3d, spotlight_gallery_3d, outdoor_3d, foggy_terrain_3d, physics_3d_playground, vehicle_3d, animated_scene_3d |

### Implemented, Not Yet Measured — Fixture Coverage

| Family | Fixture(s) | Notes |
|--------|-----------|-------|
| `WorldEnvironment` | foggy_terrain_3d | single fixture; environment behavior not yet compared to upstream |
| `FogVolume` | foggy_terrain_3d | single fixture; volumetric fog not yet a parity claim |
| `Skeleton3D` | animated_scene_3d | single fixture; skeletal parity not yet measured |
| `AnimationPlayer` | animated_scene_3d | present in fixture but animation playback parity not measured |

### Measured Families — No Dedicated Fixture (covered by unit tests)

| Family | Evidence | Notes |
|--------|----------|-------|
| `CharacterBody3D` | `characterbody3d_move_and_slide_test.rs` | unit/integration test, not scene golden |
| `PhysicsRayQuery3D` / `PhysicsShapeQuery3D` | `physics_ray_shape_query3d_test.rs` | query-object tests, not scene golden |

### Not Yet In Any Fixture

| Family | Status | Notes |
|--------|--------|-------|
| `Area3D` | Implemented, not yet measured | needs fixture with overlap/signal behavior |
| `MultiMeshInstance3D` | Implemented, not yet measured | needs fixture demonstrating batch rendering |
| `ReflectionProbe` | Implemented, not yet measured | needs fixture with reflection behavior |
| `NavigationRegion3D` | Partial/deferred | navigation_integration fixture is 2D only |
| `CSGBox3D` / CSG family | Implemented, not yet measured | csg_composition has only Node3D root |

### Deferred — No Fixture Expected

| Family | Reason |
|--------|--------|
| `VehicleBody3D` | outside Phase 6 slice |
| `SoftBody3D` | outside Phase 6 slice |
| `SpringArm3D` | outside Phase 6 slice |
| physics joints | data model only, runtime deferred |

### Corpus Guard

The test `phase6_3d_fixture_corpus_audit_test` validates that:

1. All measured families have at least one fixture with that class
2. The fixture list stays in sync with this mapping
3. New fixtures are automatically detected if they contain 3D classes

## Immediate Next Step

The next useful implementation step is not another milestone bead.

It is to expand this file into a class-family matrix covering:

- transform / camera / light
- mesh / material / render server
- rigid/static/character/area/query physics
- environment / fog / reflection / decal
- navigation / CSG / particles / audio-3D

Once that matrix exists, new beads can be created systematically without
duplicating closed or active 3D work.
