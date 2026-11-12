# Editor Parity — Viewport transform gizmos and pivot workflows

Lane source: `prd/EDITOR_PARITY_BEADS.md` lane 7 — "Viewport parity: transform
gizmos and pivot workflows" (move/rotate/scale gizmos, origin marker,
local/global toggle).

This execution map enumerates the concrete beads required for 2D viewport
transform-gizmo parity with Godot: the move/rotate/scale gizmos and their
handles, the origin/pivot marker and pivot-relative transforms, the
local/global orientation toggle, snapping during gizmo drags, multi-node
gizmo transforms about a shared pivot, and undo/redo + dirty integration.

## Format

Each bead is listed as `` N. `key-slug` Description `` followed by an
`Acceptance:` line naming the test the planner wires into criteria-driven
analysis: `(test: \`<test_name>\`)`.

## Now

1. `viewport-gizmo-move` Move gizmo drags the selected node's position along axis handles or freely from the center
   Acceptance: Dragging a move-gizmo axis handle translates the selection along that axis only, and the center handle translates freely, committing the new position (test: `viewport_gizmo_move`)

2. `viewport-gizmo-rotate` Rotate gizmo rotates the selection around its pivot by dragging the rotation handle
   Acceptance: Dragging the rotate handle changes the selection's rotation about its pivot and the angle delta tracks the cursor, committing the new rotation (test: `viewport_gizmo_rotate`)

3. `viewport-gizmo-scale` Scale gizmo scales the selection via axis and uniform handles
   Acceptance: Dragging an axis scale handle scales the selection along that axis and the uniform handle scales both axes proportionally, committing the new scale (test: `viewport_gizmo_scale`)

4. `viewport-gizmo-origin-marker` The origin/pivot marker renders at the node's transform origin and can be moved to set the pivot
   Acceptance: The pivot marker is drawn at the node's origin, and dragging it (pivot-edit mode) relocates the transform pivot without moving the node's visual position (test: `viewport_gizmo_origin_marker`)

5. `viewport-gizmo-pivot-relative` Rotate and scale operate about the current pivot, not the node center, when a custom pivot is set
   Acceptance: With a custom pivot set, rotate and scale transforms are computed about that pivot point so the node orbits/scales around it (test: `viewport_gizmo_pivot_relative_transform`)

6. `viewport-gizmo-local-global-toggle` A local/global toggle switches gizmo handle orientation between the node's local axes and world axes
   Acceptance: Toggling local/global reorients the gizmo handles between the node's rotated local frame and the global frame, and transforms apply in the selected frame (test: `viewport_gizmo_local_global_toggle`)

7. `viewport-gizmo-snap-during-drag` Gizmo drags honor active translate/rotate/scale snap settings
   Acceptance: With snapping enabled, move drags snap to the grid step, rotate drags snap to the angle step, and scale drags snap to the scale step during the gizmo operation (test: `viewport_gizmo_snap_during_drag`)

8. `viewport-gizmo-multi-node-and-undo` Gizmo transforms apply to all selected nodes about a shared pivot and push a single undo entry
   Acceptance: With multiple nodes selected, a gizmo transform applies to every selection about the shared pivot and is recorded as one reversible undo/redo step that marks the scene dirty (test: `viewport_gizmo_multi_node_undo`)
