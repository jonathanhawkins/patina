# First Real 3D Demo Parity Report

Bead: `pat-57aw6` (originally `pat-zwc7p`, updated for audit alignment)

Audit source: `prd/PHASE6_3D_PARITY_AUDIT.md`

This report records the measurable Patina 3D runtime slice against real
fixture scenes, aligned with the Phase 6 parity audit. The goal is not to
claim full Godot 3D parity. The goal is to show that the phase-6 slice is
real, testable, and anchored in reproducible evidence across transforms,
rendering, and physics — and that every 3D family is classified as measured,
implemented-not-yet-measured, or deferred.

## Scope

- Transform coverage through `Node3D`, camera extraction, and hierarchy propagation
- Render coverage through `RenderServer3DAdapter`, `SoftwareRenderer3D`, and `ParityReport3D`
- Physics coverage through deterministic `PhysicsWorld3D` stepping and golden traces
- Fixture-backed validation using checked-in `.tscn` scenes and golden JSON oracles

## Fixture Corpus

The parity slice is exercised against 12 representative 3D fixtures:

- `minimal_3d.tscn` — basic camera + mesh + light + static body
- `hierarchy_3d.tscn` — nested Node3D transform chain
- `indoor_3d.tscn` — multi-light, multi-mesh, collision hierarchy
- `multi_light_3d.tscn` — key/fill/rim/accent lighting setup
- `physics_3d_playground.tscn` — mixed rigid bodies, static platform, ramp

Plus 7 additional fixtures for broader coverage:

- `animated_scene_3d.tscn` — Skeleton3D + AnimationPlayer
- `csg_composition.tscn` — CSG boolean families + ReflectionProbe
- `foggy_terrain_3d.tscn` — FogVolume + WorldEnvironment
- `outdoor_3d.tscn` — terrain, vegetation, rigid bodies
- `physics_playground_extended.tscn` — extended rigid/static body scenes
- `spotlight_gallery_3d.tscn` — SpotLight3D gallery
- `vehicle_3d.tscn` — vehicle chassis + wheels

The corpus definition and class-family mapping are maintained in
`prd/PHASE6_3D_PARITY_AUDIT.md` under the "3D Fixture Corpus Definition" section.

The structured report artifact (`fixtures/patina_outputs/real_3d_demo_parity_report.json`)
inventories the 5 core scenes with node/camera/light/physics-body counts and
references 3 physics golden traces.

## Physics Golden Traces

Three physics trace goldens anchor deterministic simulation:

- `gravity_fall_30frames.json` — single rigid body freefall (30 frames)
- `multi_body_3d_20frames.json` — Ball/Cube/HeavyBlock from physics_3d_playground (30 entries)
- `rigid_sphere_bounce_3d_20frames.json` — rigid sphere bounce trajectory (20 frames)

## Measured Evidence

The current report is backed by these integration suites:

- `cargo test -p patina-engine --test real_3d_demo_unified_parity_test`
- `cargo test -p patina-engine --test real_3d_demo_aggregate_parity_test`
- `cargo test -p patina-engine --test real_3d_demo_parity_report_artifact_test`
- `cargo test -p patina-engine --test demo_3d_report_doc_test`

Representative single-test commands for focused verification:

- `cargo test -p patina-engine --test demo_3d_parity_report_test minimal_3d_scene_has_expected_structure -- --exact`
- `cargo test -p patina-engine --test demo_3d_parity_report_test physics_3d_freefall_matches_golden_trace -- --exact`

Those suites verify:

- All 10 3D scene fixtures load and produce valid scene trees
- Scene tree parity against oracle outputs (per-fixture and aggregate)
- Camera, light, and physics body classification from golden scene data
- Physics stepping is deterministic and comparable against golden trajectories
- The structured report artifact has correct metadata, inventory counts, and evidence pointers
- All referenced golden files exist and have correct frame counts

## Audit-Aligned 3D Family Classification

Per `prd/PHASE6_3D_PARITY_AUDIT.md`, every 3D family is classified into one of
three tiers. The structured artifact at
`fixtures/patina_outputs/real_3d_demo_parity_report.json` contains the
machine-readable version of this classification.

### Measured (fixture-backed or test-backed parity evidence)

- `Node3D` — transform propagation and hierarchy
- `Camera3D` — extraction and projection
- `MeshInstance3D` — basic render path through software renderer
- `DirectionalLight3D` — shadow mapping and light energy
- `OmniLight3D` — point lighting in fixture scenes
- `SpotLight3D` — spot lighting with range/angle
- `CollisionShape3D` — collision shapes in all physics fixtures
- `RigidBody3D` — deterministic stepping, forces, torques, contacts
- `StaticBody3D` — fixture collision participation
- `CharacterBody3D` — move-and-slide, floor/wall/ceiling classification
- `PhysicsRayQuery3D` — ray hit/miss, exclusion, collision masks
- `PhysicsShapeQuery3D` — shape overlap depth, world integration
- `RenderingServer3D` — software renderer, instance/material/light ops
- `ShaderMaterial3D` — custom shader override, unshaded behavior

### Implemented, not yet measured at parity level

- `Area3D` — overlap storage exists, no runtime signal parity test
- `ReflectionProbe` — data model tested, no rendered reflection parity
- `FogVolume` — editor-preview tests exist, no scene runtime parity
- `Decal` — model and registry exist, no upstream parity fixture
- `NavigationRegion3D` — mesh baking model, runtime pathfinding deferred
- `MultiMeshInstance3D` — implementation exists, no parity measurement
- `StandardMaterial3D` — albedo/shading tested, not broad property parity
- `WorldEnvironment` — typed resources and preview tests, not runtime parity
- `Sky` — procedural/panoramic/physical handling, resource semantics only
- `PhysicsJoints` — data types exist (Pin/Hinge/Slider), no runtime integration
- `Skeleton3D` — appears in animated_scene_3d fixture, no dedicated test
- `AnimationPlayer` — appears in animated_scene_3d fixture, no animation parity
- `CSGCombiner3D` / `CSGBox3D` / `CSGSphere3D` / `CSGCylinder3D` — appear in csg_composition fixture, no CSG boolean parity

### Deferred or explicitly limited

- `VehicleBody3D` — outside bounded Phase 6 slice
- `SoftBody3D` — outside bounded Phase 6 slice
- `SpringArm3D` — not in current slice
- `RayCast3D` / `ShapeCast3D` — low-level query support only, no node parity
- `NavigationAgent3D` — runtime pathfinding outside Phase 6 scope
- `GPUPostProcessing` — broad GPU/post-processing parity deferred

## Current Read

- Transform path: fixture-backed and measurable across 10 fixtures
- Render path: functional for camera-plus-mesh scenes and exposed through `ParityReport3D`
- Physics path: deterministic for the covered rigid-body slice with 3 golden traces
- Scene tree parity: 85.7%--94.4% match ratio per fixture (1 node gap, consistent)
- Reporting path: reproducible through checked-in tests and committed JSON artifact

## Limits

- This milestone does not claim full 3D feature parity
- Complex material, animation, and broad gameplay behavior remain outside this report
- Scene tree match is partial (~90%) due to consistent 1-node gap per fixture
- Physics and render dimensions in the aggregate parity view are still skipped (scene-tree only)

## Exit-Criteria Mapping

Phase 6 calls for:

- representative 3D fixtures run
- performance and correctness are measurable
- platform/runtime boundaries remain clean

This report satisfies that deliverable by tying the first real 3D demo claim to
fixture execution, golden comparison, and adapter-level metrics instead of
aspirational milestone text. The structured artifact at
`fixtures/patina_outputs/real_3d_demo_parity_report.json` provides machine-readable
evidence with rerunnable test commands.
