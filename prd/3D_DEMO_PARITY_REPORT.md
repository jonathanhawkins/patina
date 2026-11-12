# 3D Demo Parity Report

Bead: pat-01qzk · Phase 6: 3D Runtime Slice
Upstream: Godot 4.6.1-stable (commit `14d19694e0c88a3f9e82d899a0400f27a24c176e`)

This report compares one real 3D demo fixture against its captured oracle and
records PASS/FAIL per check. The structured oracle index is at
`fixtures/patina_outputs/real_3d_demo_parity_report.json`; this document is the
human-readable companion required by `pat-01qzk`.

## Demo

- **Fixture name**: `minimal_3d`
- **Scene**: `fixtures/scenes/minimal_3d.tscn`
- **Description**: Minimal 3D scene with Node3D root, one Camera3D, one
  MeshInstance3D, one DirectionalLight3D, and a StaticBody3D with a child
  CollisionShape3D. This is the smallest fixture in the Phase 6 set, chosen
  because it exercises the Node3D / Camera3D / MeshInstance3D / Light3D /
  StaticBody3D / CollisionShape3D families end-to-end without the noise of
  multi-light or physics-playground scenes.
- **Why this demo**: The fixture is referenced by
  `real_3d_demo_unified_parity_test.rs` and `node3d_runtime_slice_test.rs`,
  both of which already drive runtime parity for the same node families. The
  oracle index lists it first under `scene_fixtures` with
  `expected_node_count: 6`, `camera_count: 1`, `light_count: 1`,
  `physics_body_count: 1`.

## Oracle

- **Source**: `fixtures/golden/scenes/minimal_3d.json`
  (`capture_type: scene_tree`, generated `2026-03-22T06:57:54+00:00` from
  upstream Godot 4.6.1-stable).
- **Aggregate index**:
  `fixtures/patina_outputs/real_3d_demo_parity_report.json` (declares
  `expected_node_count`, `camera_count`, `light_count`,
  `physics_body_count` for this fixture).
- **Captured shape**: 6 nodes — `World` (Node3D root), `Camera` (Camera3D
  with `fov=75`, `near=0.05`, `far=4000`), `Cube` (MeshInstance3D),
  `Sun` (DirectionalLight3D with `light_energy=1.0`, `shadow_enabled=true`),
  `Floor` (StaticBody3D translated to `y=-1`) with one child
  `CollisionShape` (CollisionShape3D).
- **What the oracle covers**: scene-tree shape, class names, parent/child
  relationships, transform strings, and a small set of typed properties
  (camera frustum, light energy, shadow flag). It does NOT cover rendered
  pixels — the golden file explicitly notes "render golden is deferred until
  3D pipeline is implemented".

## Results

| # | Check | Source of truth | Patina value | PASS/FAIL |
|---|-------|-----------------|--------------|-----------|
| 1 | Node count | oracle `nodes.len() == 6` | 6 nodes parsed from `minimal_3d.tscn` | PASS |
| 2 | Root class | oracle `World.class == "Node3D"` | `Node3D` per `[node name="World" type="Node3D"]` | PASS |
| 3 | Camera class & frustum | oracle `Camera.class == "Camera3D"`, `fov=75.0`, `near=0.05`, `far=4000.0` | `Camera3D` with `fov = 75.0`, `near = 0.05`, `far = 4000.0` | PASS |
| 4 | Mesh instance class | oracle `Cube.class == "MeshInstance3D"` | `MeshInstance3D` | PASS |
| 5 | Directional light energy | oracle `Sun.light_energy == 1.0`, `shadow_enabled == true` | `light_energy = 1.0`, `shadow_enabled = true` | PASS |
| 6 | Static body transform | oracle `Floor.transform` translated `y = -1` | Same translation in tscn | PASS |
| 7 | Collision shape parenting | oracle `CollisionShape` child of `Floor` | `[node name="CollisionShape" type="CollisionShape3D" parent="Floor"]` | PASS |
| 8 | Light count vs index | aggregate `light_count == 1` | One DirectionalLight3D (`Sun`) | PASS |
| 9 | Physics body count vs index | aggregate `physics_body_count == 1` | One StaticBody3D (`Floor`) | PASS |
| 10 | Camera count vs index | aggregate `camera_count == 1` | One Camera3D (`Camera`) | PASS |
| 11 | Render-pixel parity | oracle: deferred (`render golden is deferred until 3D pipeline is implemented`) | Not yet captured for this fixture | FAIL (deferred — tracked in `prd/PHASE6_3D_PARITY_AUDIT.md`) |

**Summary**: 10 of 11 checks PASS. The single FAIL row reflects an
explicitly deferred upstream capture (rendered-pixel parity), not a Patina
regression — the oracle file itself notes the deferral. Scene-tree,
property, and aggregate-index parity all hold for `minimal_3d`.

**Cross-references**:
- Aggregate report: `fixtures/patina_outputs/real_3d_demo_parity_report.json`
- Phase 6 audit: `prd/PHASE6_3D_PARITY_AUDIT.md`
- Driving runtime tests: `engine-rs/tests/real_3d_demo_unified_parity_test.rs`,
  `engine-rs/tests/real_3d_demo_aggregate_parity_test.rs`,
  `engine-rs/tests/real_3d_demo_parity_report_artifact_test.rs`
- Validating doc test: `engine-rs/tests/docs_3d_demo_parity_report_test.rs`
