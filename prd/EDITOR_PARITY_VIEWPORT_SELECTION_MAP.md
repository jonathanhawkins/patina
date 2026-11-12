# Editor Parity — Viewport selection modes, zoom/pan, and viewport controls

Lane source: `prd/EDITOR_PARITY_BEADS.md` lane 6 — "Viewport parity: selection
modes, zoom/pan, and viewport controls" (2D viewport toolbar modes, zoom, pan,
framing, overlap selection, locked-node behavior).

This execution map enumerates the concrete beads required for 2D viewport
selection/navigation parity with Godot: click and box selection, cycling
through overlapping nodes, pan and zoom navigation, frame-selection,
locked/grouped-node selection behavior, and the viewport mode toolbar.

## Format

Each bead is listed as `` N. `key-slug` Description `` followed by an
`Acceptance:` line naming the test the planner wires into criteria-driven
analysis: `(test: \`<test_name>\`)`.

## Now

1. `viewport-select-click` Click-select picks the topmost node under the cursor and updates the selection
   Acceptance: Clicking in the viewport selects the topmost selectable node at that point, Shift/Ctrl-click toggles a node in/out of the selection, and clicking empty space clears it (test: `viewport_select_click`)

2. `viewport-select-box` Rubber-band box selection selects all nodes intersecting the dragged rectangle
   Acceptance: Dragging a selection rectangle selects every selectable node whose bounds intersect it, and modifier keys add to the existing selection (test: `viewport_select_box`)

3. `viewport-select-overlap-cycle` Repeated click / modifier cycles selection through stacked overlapping nodes
   Acceptance: Clicking repeatedly (or via the overlap modifier) at a point with stacked nodes cycles the selection through each overlapping node in z-order (test: `viewport_select_overlap_cycle`)

4. `viewport-pan` The viewport can be panned via middle-drag or space-drag
   Acceptance: Middle-mouse drag (or space+drag) pans the viewport by the cursor delta without changing the selection, and the scroll offset updates accordingly (test: `viewport_pan`)

5. `viewport-zoom` Wheel zoom and zoom controls scale the view about the cursor with reset-to-100%
   Acceptance: Mouse-wheel zoom scales the view toward the cursor within min/max bounds, the zoom buttons step in/out, and reset restores 100% zoom (test: `viewport_zoom`)

6. `viewport-frame-selection` Frame-selection centers and fits the selected node(s) in the viewport
   Acceptance: Invoking frame-selection (F) centers the current selection and adjusts zoom to fit its bounds; with no selection it frames the scene origin (test: `viewport_frame_selection`)

7. `viewport-locked-node-behavior` Locked and grouped nodes follow Godot selection rules in the viewport
   Acceptance: A locked node is not selectable by viewport click, and clicking a child of a grouped node selects the group root instead of the child (test: `viewport_locked_grouped_selection`)

8. `viewport-mode-toolbar` The viewport toolbar exposes select/pan/ruler modes and a zoom indicator
   Acceptance: The toolbar provides select, pan, and ruler tool modes with the active mode reflected in button state, and shows a live zoom-percentage indicator (test: `viewport_mode_toolbar`)
