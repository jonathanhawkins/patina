# Editor Parity — Viewport snapping, guides, rulers, grid, and canvas overlays

Per-lane execution map for the 2D viewport's snapping system and non-content
overlays. Source lane: `Viewport parity: snapping, guides, rulers, grid, and
canvas overlays` in `prd/EDITOR_PARITY_BEADS.md` (lane 8). Source surface:
snap config, smart snap, guides, rulers, origin, navigation/y-sort/viewport
overlays.

Scope: the alignment and reference layers drawn over the 2D canvas — grid and
its snapping, smart/relative snapping, draggable guides, rulers, the origin
axes, and the debug-style canvas overlays (visibility rect, navigation,
y-sort). Selection, gizmos, and camera controls are out of scope (see
`EDITOR_PARITY_VIEWPORT_SELECTION_MAP.md` and
`EDITOR_PARITY_VIEWPORT_GIZMOS_MAP.md`).

## Format

Each bead is `` N. `key-slug` Description `` followed by an
`Acceptance:` line naming the test in the form `(test: \`<test_name>\`)` the
planner wires into criteria-driven analysis.

## Now

1. `viewport-grid-display` Draw the configurable 2D grid (step, offset, primary-line interval) and toggle its visibility from the View menu.
   Acceptance: enabling the grid draws lines at the configured step and offset and toggling visibility hides them (test: `viewport_grid_display_respects_step_offset`)

2. `viewport-grid-snapping` Snap dragged/created nodes to the grid when snap is enabled, honoring the grid step and offset.
   Acceptance: with grid snap on, dragging a node lands its position on the nearest grid intersection; with snap off it moves freely (test: `viewport_grid_snapping_quantizes_position`)

3. `viewport-rulers` Render horizontal and vertical rulers showing canvas coordinates that track zoom and pan.
   Acceptance: the rulers display canvas-space tick labels that rescale on zoom and shift on pan (test: `viewport_rulers_track_zoom_and_pan`)

4. `viewport-guides` Support dragging horizontal and vertical guides out of the rulers, moving them, and snapping nodes to them.
   Acceptance: dragging from a ruler creates a guide, the guide can be repositioned, and node drag snaps to it when snapping is on (test: `viewport_guides_create_move_and_snap`)

## Next

5. `viewport-smart-snap` Implement smart/relative snapping to other nodes' edges, centers, and anchors with snap-line feedback.
   Acceptance: with smart snap on, dragging a node aligns to a sibling's edge/center and a snap guide line is shown at the match (test: `viewport_smart_snap_aligns_to_siblings`)

6. `viewport-origin-axes` Draw the world origin axes (X/Y reference lines) and toggle them from the View menu.
   Acceptance: the origin axes render through (0,0) in the distinct axis colors and can be toggled off (test: `viewport_origin_axes_render_at_zero`)

7. `viewport-snap-config` Provide the snap configuration dialog for grid step/offset, rotation snap step, and scale snap step, persisting values across sessions.
   Acceptance: changing grid step, rotation step, and scale step in the snap dialog updates snapping behavior and the values persist after reload (test: `viewport_snap_config_persists_and_applies`)

## Later

8. `viewport-canvas-debug-overlays` Render toggleable canvas overlays for the visibility rect, navigation polygons, and y-sort ordering preview.
   Acceptance: enabling the visibility-rect, navigation, and y-sort overlays each draws its respective overlay and toggling off removes it (test: `viewport_canvas_debug_overlays_toggle`)
