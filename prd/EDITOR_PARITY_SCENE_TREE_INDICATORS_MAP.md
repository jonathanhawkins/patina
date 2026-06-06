# Editor Parity — Scene Tree indicators, badges, and selection state

Execution map for lane 2 of the editor-parity backlog
(`prd/EDITOR_PARITY_BEADS.md`). This lane covers the per-node visual
affordances in the Scene Tree dock: visibility and lock toggles, the
instancing / script / group / signal / unique-name markers, configuration
warnings, and the selection-state highlighting that keeps the dock in sync
with the viewport and inspector.

Source (Godot reference behaviour): `SceneTreeEditor` / `SceneTreeDock`
visual indicators, lock/visibility buttons, instancing/script/group/signal/
unique-name markers, and multi-selection state.

## Format

Each bead is listed as `` N. `key-slug` Description `` followed by an
`Acceptance:` line whose proof names the test the planner wires into
criteria-driven analysis, in the form `(test: \`<test_name>\`)`.

## Now

1. `scene-tree-visibility-toggle` Render and toggle the per-node visibility (eye) indicator for CanvasItem and Node3D rows, persisting the `visible` property and propagating to descendants
   Acceptance: clicking the visibility icon flips `Node.visible` and the row icon reflects hidden/visible/inherited-hidden state (test: `scene_tree_visibility_toggle_reflects_state`)

2. `scene-tree-lock-indicator` Render the lock indicator and support locking/unlocking a node so it cannot be picked in the viewport, with the lock badge shown on the row
   Acceptance: toggling lock sets the `_edit_lock_` meta and the lock badge appears/disappears, and a locked node is unselectable in the viewport (test: `scene_tree_lock_indicator_blocks_viewport_pick`)

3. `scene-tree-group-children-indicator` Render the "group" (children-selection-lock) indicator and prevent child selection in the viewport when a parent is grouped
   Acceptance: toggling the group button sets `_edit_group_` meta, shows the group badge, and clicking a grouped child in the viewport selects the grouped ancestor (test: `scene_tree_group_indicator_locks_child_selection`)

4. `scene-tree-script-badge` Render the script-attached badge with the correct script icon and open the script in the editor when activated
   Acceptance: a node with an attached script shows the script badge and activating it routes an open-script request for that node's script path (test: `scene_tree_script_badge_opens_script`)

5. `scene-tree-instance-badge` Render the scene-instance badge for instanced scenes and open the instanced scene when the badge is activated
   Acceptance: an instanced-scene node shows the instance badge and activating it routes an open-scene request for the instance's source path (test: `scene_tree_instance_badge_opens_source_scene`)

6. `scene-tree-signal-connection-indicator` Render the signal-connection indicator when a node has at least one connected signal, updating live as connections are added or removed
   Acceptance: connecting a signal to/from a node toggles the signal badge on its row in real time (test: `scene_tree_signal_indicator_tracks_connections`)

## Next

7. `scene-tree-unique-name-indicator` Render the unique-name-in-owner (`%`) indicator and keep it consistent with the node's `unique_name_in_owner` flag, surfacing collisions
   Acceptance: enabling "Access as Unique Name" shows the `%` badge and a duplicate unique name within the same owner is flagged as a warning (test: `scene_tree_unique_name_indicator_reflects_flag`)

8. `scene-tree-configuration-warning` Render the per-node configuration-warning triangle when `Node._get_configuration_warnings` returns messages and expose the warning text on hover/activation
   Acceptance: a node returning configuration warnings shows the warning triangle and its tooltip lists the warning strings (test: `scene_tree_configuration_warning_surfaces_messages`)

## Later

9. `scene-tree-selection-state-sync` Keep multi-selection highlighting in the dock synchronized with viewport and inspector selection, including range/additive selection semantics
   Acceptance: selecting nodes in the viewport or via shift/ctrl in the dock yields an identical selection set across dock, viewport, and inspector (test: `scene_tree_selection_state_stays_synced`)
