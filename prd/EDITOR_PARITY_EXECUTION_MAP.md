# Editor Parity — Canonical Execution Map

Aggregated from the 18 per-lane execution maps (see `prd/EDITOR_PARITY_BOOTSTRAP_MAP.md`), in lane order. Each lane's beads are preserved verbatim under the `## Now` priority band; no lane declared `## Next`/`## Later` beads, so all work is Now-priority.

Exit criteria with one checkbox per acceptance test live in `prd/EDITOR_PARITY_EXIT.md`.

## Now

### Scene Tree node operations and hierarchy workflows

1. `scene-tree-ops-add-child-node` Add Child Node action inserts a new node of the chosen type as a child of the selected node and selects it
   Acceptance: Adding a child node to a selected parent produces a child of the requested type, focuses it in the tree, and marks the scene dirty (test: `scene_tree_add_child_node`)

2. `scene-tree-ops-instance-child-scene` Instance Child Scene action embeds an external scene as an instanced child with the instance badge
   Acceptance: Instancing a `.tscn` as a child creates an instanced node rooted at the selected parent and records its source path for the instance indicator (test: `scene_tree_instance_child_scene`)

3. `scene-tree-ops-rename-node` Rename via double-click / F2 updates the node name with collision-safe uniquification among siblings
   Acceptance: Renaming a node commits the new name, rejects empty names, and auto-suffixes a duplicate sibling name to keep names unique (test: `scene_tree_rename_node_unique`)

4. `scene-tree-ops-delete-node` Delete (Del) removes the selected node(s) and their subtrees, with multi-selection support
   Acceptance: Deleting one or more selected nodes removes their full subtrees from the scene and clears the selection without orphaning descendants (test: `scene_tree_delete_node_subtree`)

5. `scene-tree-ops-duplicate-node` Duplicate (Ctrl+D) clones the selected node and its subtree as a sibling with a uniquified name
   Acceptance: Duplicating a node produces a deep copy of its subtree inserted as the next sibling, with a unique name and copied properties (test: `scene_tree_duplicate_node`)

6. `scene-tree-ops-reparent-node` Reparent (drag-drop or Reparent dialog) moves a node under a new parent while preserving its subtree
   Acceptance: Reparenting a node moves it and its descendants under the target parent, rejects reparenting a node into its own descendant, and preserves global transform when requested (test: `scene_tree_reparent_node`)

7. `scene-tree-ops-reorder-siblings` Move Up / Move Down reorders a node among its siblings
   Acceptance: Move Up / Move Down changes the node's index among siblings, is a no-op at the boundaries, and updates child ordering deterministically (test: `scene_tree_reorder_siblings`)

8. `scene-tree-ops-cut-copy-paste` Cut / Copy / Paste node clipboard operations move or clone subtrees across the hierarchy
   Acceptance: Copy then Paste inserts a clone of the copied subtree under the paste target, Cut then Paste moves the original, and paste uniquifies names (test: `scene_tree_cut_copy_paste_node`)

9. `scene-tree-ops-change-type` Change Type converts a node to a compatible type, preserving children and shared properties
   Acceptance: Changing a node's type replaces it with the chosen type in place, retains its children and name, and carries over properties common to both types (test: `scene_tree_change_node_type`)

10. `scene-tree-ops-editable-children` Editable Children / Make Local toggles ownership and exposes instanced subtree nodes for editing
    Acceptance: Toggling Editable Children reveals an instance's internal nodes as editable, and Make Local converts an instanced node into an owned local subtree (test: `scene_tree_editable_children_make_local`)

11. `scene-tree-ops-group-membership` Context-menu group editing adds and removes the selected node from named groups
    Acceptance: Adding a node to a group records the membership and the group badge, and removing it clears the membership; group lists stay consistent across nodes (test: `scene_tree_group_membership`)

### Scene Tree indicators, badges, and selection state

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

### Inspector resource toolbar, history, and object navigation

1. `inspector-object-header` Render the inspected-object header with class icon, object/resource name, and class label matching Godot's EditorInspector header row.
   Acceptance: inspecting a node shows a header line with its class name and resource path; clearing the inspector hides it (test: `inspector_object_header_renders_class_and_name`)

2. `inspector-history-back-forward` Implement back/forward navigation buttons that move through the stack of previously inspected objects, enabling/disabling at the ends of the stack.
   Acceptance: inspecting object A then B then pressing Back re-selects A and enables Forward; Back at the start is disabled (test: `inspector_history_back_forward_navigates_stack`)

3. `inspector-history-dropdown` Add the history dropdown menu listing recently inspected objects, selecting an entry re-inspects that object.
   Acceptance: after inspecting three objects the dropdown lists all three most-recent-first and selecting one inspects it (test: `inspector_history_dropdown_lists_and_selects`)

4. `inspector-resource-toolbar-actions` Add the resource action toolbar (edit, load, clear, save-as, copy, paste) wired to the inspected Resource, disabling actions that do not apply to the current object.
   Acceptance: inspecting a Resource exposes load/clear/copy/paste actions and clear empties the slot; inspecting a plain Node hides resource-only actions (test: `inspector_resource_toolbar_actions_apply`)

## Next

5. `inspector-subresource-breadcrumb` Render a sub-resource breadcrumb so editing an embedded resource shows the path back to the owning object and clicking a crumb navigates up.
   Acceptance: editing a sub-resource of a node shows a breadcrumb with the parent object and clicking the parent crumb re-inspects it (test: `inspector_subresource_breadcrumb_navigates_up`)

6. `inspector-pin-lock` Implement the pin/lock toggle that keeps the current object inspected even as the scene-tree selection changes.
   Acceptance: pinning the inspector keeps object A shown after selecting object B in the scene tree; unpinning resumes following selection (test: `inspector_pin_lock_holds_selection`)

7. `inspector-copy-paste-resource` Implement copy/paste of a resource reference across the resource toolbar so a copied resource can be pasted into a compatible property slot.
   Acceptance: copying a resource from one object and pasting into a compatible slot on another assigns the same resource; incompatible slots reject the paste (test: `inspector_copy_paste_resource_round_trip`)

## Later

8. `inspector-history-persists-per-scene` Persist the inspected-object history per open scene so switching scene tabs restores that scene's navigation stack.
   Acceptance: building history in scene A, switching to scene B, then back to A restores A's back/forward stack (test: `inspector_history_persists_per_scene`)

### Inspector core property editing and interaction

1. `inspector-props-typed-editors` Property editors render and commit per type (numeric, string, bool, enum, Vector2/3, Color, NodePath, resource)
   Acceptance: Each supported property type renders its matching editor widget and committing an edit writes the typed value back to the object (test: `inspector_typed_property_editors`)

2. `inspector-props-drag-to-adjust` Numeric fields support click-drag scrubbing to adjust the value with step granularity
   Acceptance: Dragging horizontally on a numeric editor changes the value by the field's step, respects min/max range, and commits on release (test: `inspector_numeric_drag_to_adjust`)

3. `inspector-props-inline-expression` Numeric fields accept inline math expressions that evaluate on commit
   Acceptance: Entering an expression like `2*PI` or `1+1` into a numeric field evaluates to the computed number on commit, and an invalid expression is rejected without mutating the value (test: `inspector_numeric_inline_expression`)

4. `inspector-props-revert-default` A revert button appears when a property differs from its default and resets it on click
   Acceptance: A property whose value differs from the object default shows a revert affordance that, when activated, restores the default value and clears the override (test: `inspector_revert_to_default`)

5. `inspector-props-linked-components` Vector/Rect editors offer a proportional lock so editing one component scales the others by ratio
   Acceptance: With the proportional lock enabled, editing one component of a multi-component property scales the remaining components by the original ratio; with it disabled, only the edited component changes (test: `inspector_linked_proportional_components`)

6. `inspector-props-copy-paste-value` Copy/Paste property value transfers a value between compatible properties
   Acceptance: Copying a property value and pasting it onto a type-compatible property writes the value, and pasting onto an incompatible type is rejected (test: `inspector_copy_paste_property_value`)

7. `inspector-props-copy-property-path` Copy Property Path yields the scripting path for the selected property
   Acceptance: The copy-property-path action produces the property's resolvable path string (e.g. `position:x`) for the selected sub-property (test: `inspector_copy_property_path`)

8. `inspector-props-multi-edit` Editing a property with multiple objects selected applies the change to all of them
   Acceptance: With several nodes selected, committing a shared property applies the new value to every selected node in a single undo step (test: `inspector_multi_node_property_edit`)

9. `inspector-props-key-animation` A key affordance lets a property value be inserted as an animation track keyframe from the inspector
   Acceptance: When an AnimationPlayer track context is active, the inspector key affordance inserts a keyframe for the property at the current time with the editor's value (test: `inspector_key_property_to_animation`)

10. `inspector-props-undo-redo-dirty` Property edits push a reversible undo/redo entry and mark the scene dirty
    Acceptance: Committing a property change records an undo entry that restores the prior value on undo and reapplies it on redo, and the owning scene is marked modified (test: `inspector_property_undo_redo_dirty`)

### Inspector advanced property organization and exported script fields

1. `inspector-property-categories` Render class-based category separators (with class icon and name) that group inherited properties by the class that declares them, matching Godot's EditorInspector category rows.
   Acceptance: inspecting a node shows category headers for each declaring class in inheritance order with the class icon (test: `inspector_property_categories_group_by_class`)

2. `inspector-export-groups` Implement `@export_group` / `@export_subgroup` collapsing so prefixed properties nest under collapsible group and subgroup headers.
   Acceptance: a script with an export group and subgroup renders nested collapsible headers and toggling a group hides its members (test: `inspector_export_groups_nest_and_collapse`)

3. `inspector-export-hint-widgets` Map `PropertyHint` values from `@export` annotations to the correct editor widget (range slider, enum dropdown, file/dir picker, multiline text, flags).
   Acceptance: exported fields with range, enum, file, and multiline hints each render their hint-specific widget rather than the default editor (test: `inspector_export_hints_select_widget`)

4. `inspector-subresource-inline-edit` Allow a resource-typed property to expand inline so its sub-properties are editable without leaving the parent object.
   Acceptance: expanding an embedded resource property shows its editable sub-properties inline and edits persist to the sub-resource (test: `inspector_subresource_inline_edit_persists`)

## Next

5. `inspector-favorites-pinning` Implement property favorites so a user can pin properties to a Favorites section that stays at the top across objects of the same class.
   Acceptance: favoriting a property moves it into a top Favorites section and re-inspecting a same-class object keeps it favorited (test: `inspector_favorites_pin_to_top`)

6. `inspector-export-category-separator` Support `@export_category` to insert a labeled category divider in script-declared property order.
   Acceptance: a script using `@export_category` renders a labeled divider at the declared position separating the following exports (test: `inspector_export_category_divider_renders`)

7. `inspector-typed-collection-export` Render typed array and dictionary exports with add/remove/reorder controls and per-element editors driven by the element type hint.
   Acceptance: a typed array export shows add/remove/reorder controls and elements use the element-type editor; a typed dictionary edits keys and values (test: `inspector_typed_collection_export_edits`)

## Later

8. `inspector-usage-flags-visibility` Honor property usage flags (storage, editor, read-only, no-editor) so script and engine properties show, hide, or lock in the inspector accordingly.
   Acceptance: a read-only-flagged export renders disabled, a no-editor-flagged property is hidden, and a storage-only property does not appear (test: `inspector_usage_flags_control_visibility`)

### Viewport selection modes, zoom/pan, and viewport controls

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

### Viewport transform gizmos and pivot workflows

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

### Viewport snapping, guides, rulers, grid, and canvas overlays

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

### Top bar scene tabs, run controls, and editor mode switching

1. `top-bar-scene-tabs` Open scenes appear as tabs in the top bar and clicking a tab makes it the active scene
   Acceptance: Each open scene renders a tab labeled with its name, and selecting a tab switches the active scene and editor context to it (test: `top_bar_scene_tabs_switch`)

2. `top-bar-tab-unsaved-and-close` Tabs show an unsaved-changes marker and a close affordance with a save prompt when dirty
   Acceptance: A scene with unsaved edits shows a modified marker on its tab; closing a dirty tab prompts to save/discard/cancel and closing a clean tab removes it immediately (test: `top_bar_tab_unsaved_and_close`)

3. `top-bar-tab-context-menu` Right-clicking a scene tab offers close/close-others/close-all and path actions
   Acceptance: The tab context menu exposes Close, Close Other Tabs, Close All, Copy Scene Path, and Show in FileSystem, each acting on the targeted tab (test: `top_bar_tab_context_menu`)

4. `top-bar-tab-reorder` Scene tabs can be reordered by dragging
   Acceptance: Dragging a tab to a new position reorders the tab strip and preserves the active selection (test: `top_bar_tab_reorder`)

5. `top-bar-open-new-scene` The new-scene control adds a fresh untitled scene tab and focuses it
   Acceptance: Activating the new-scene control creates an untitled scene, adds its tab, and makes it the active scene (test: `top_bar_open_new_scene`)

6. `top-bar-run-controls` Run controls play the project, play the current scene, pause, and stop
   Acceptance: Play-project launches the project main scene, play-scene launches the current scene, pause toggles the paused state, and stop terminates the running instance (test: `top_bar_run_controls`)

7. `top-bar-play-custom-scene` A play-custom-scene control runs a chosen scene other than the main scene
   Acceptance: Selecting play-custom-scene runs the specified scene and records it as the last-played custom scene for quick replay (test: `top_bar_play_custom_scene`)

8. `top-bar-mode-switch` The main-screen mode switcher toggles between 2D, 3D, Script, and AssetLib views
   Acceptance: Selecting a mode button switches the central editor view to 2D, 3D, Script, or AssetLib and reflects the active mode in the button state (test: `top_bar_editor_mode_switch`)

### Menu scene/project/debug/editor/help actions

1. `menus-scene-actions` Implement the Scene menu (New Scene, New Inherited Scene, Open Scene, Save, Save As, Save All, Close Scene, Revert Scene, Quit) wired to the scene-document lifecycle.
   Acceptance: each Scene menu action dispatches its document command and Save/Save As persist the active scene to disk (test: `menus_scene_actions_dispatch`)

2. `menus-project-actions` Implement the Project menu (Project Settings, Version Control, Export, Reload Current Project, Quit to Project List) wired to the corresponding editor subsystems.
   Acceptance: opening Project Settings and Export from the Project menu raises their dialogs and Reload Current Project triggers a project reload (test: `menus_project_actions_open_subsystems`)

3. `menus-debug-toggles` Implement the Debug menu toggles (Visible Collision Shapes, Visible Navigation, Visible Paths, Synchronize Scene Changes, Synchronize Script Changes) as persisted checkable items applied to play sessions.
   Acceptance: toggling a Debug menu item flips its checked state, persists, and the flag is passed to the next play session (test: `menus_debug_toggles_persist_and_apply`)

4. `menus-help-actions` Implement the Help menu (Search Help, Online Docs, Report a Bug, About) routing to the help search dialog, external links, and the about dialog.
   Acceptance: Search Help opens the help search, an external-link item resolves its target URL, and About opens the about dialog (test: `menus_help_actions_route_targets`)

## Next

5. `menus-editor-actions` Implement the Editor menu (Editor Settings, Command Palette, Editor Layout, Toggle Fullscreen, Manage Editor Features) wired to editor-level commands.
   Acceptance: Editor Settings and Command Palette open from the Editor menu and Toggle Fullscreen flips the window state (test: `menus_editor_actions_dispatch`)

6. `menus-open-recent` Implement Scene > Open Recent with a maintained most-recent-first list that opens the selected scene and persists across sessions.
   Acceptance: opening scenes populates Open Recent most-recent-first, selecting an entry opens it, and the list persists after reload (test: `menus_open_recent_tracks_and_opens`)

7. `menus-action-shortcuts` Bind menu actions to their keyboard shortcuts and display the accelerator text in the menu item label.
   Acceptance: a menu action's shortcut both fires the action when pressed and is shown as accelerator text on the item (test: `menus_action_shortcuts_bind_and_display`)

## Later

8. `menus-action-enablement` Enable/disable and check/uncheck menu items based on editor state (e.g. Save disabled with no unsaved changes, Revert disabled for an unsaved scene).
   Acceptance: Save is disabled when the active scene has no changes and becomes enabled after an edit; Revert is disabled for a never-saved scene (test: `menus_action_enablement_reflects_state`)

### Create Node dialog for 2D workflows

1. `create-node-type-tree` The dialog shows the node-type inheritance tree rooted at the requested base type
   Acceptance: Opening Create Node renders the class tree rooted at the base type (e.g. Node) with inheritance nesting, expandable to leaf types (test: `create_node_type_tree`)

2. `create-node-incremental-search` Typing in the search box incrementally filters the tree and highlights matches
   Acceptance: Entering search text filters the tree to matching type names (substring/fuzzy), keeps ancestors visible for context, and highlights the matched span (test: `create_node_incremental_search`)

3. `create-node-best-match-confirm` Enter confirms the best-matching type and double-click / Create button instantiates the selected type
   Acceptance: Pressing Enter selects the top-ranked search match, and double-clicking a type or activating Create instantiates the selected type and closes the dialog (test: `create_node_best_match_confirm`)

4. `create-node-favorites` Types can be favorited and appear in a Favorites section at the top of the dialog
   Acceptance: Toggling favorite on a type adds it to the persistent Favorites section, toggling off removes it, and favorites survive reopening the dialog (test: `create_node_favorites`)

5. `create-node-recent` Recently created types appear in a Recent section ordered by last use
   Acceptance: Creating a node records its type in the Recent section, ordered most-recent-first and capped to a recent-history limit (test: `create_node_recent`)

6. `create-node-description-panel` Selecting a type shows its description/help text in the dialog
   Acceptance: Selecting a type populates a description panel with the type's summary/help text, and unknown/empty descriptions render a graceful placeholder (test: `create_node_description_panel`)

7. `create-node-child-of-selection` The created node is inserted as a child of the currently selected scene node (or as root when empty)
   Acceptance: Confirming creation inserts the new node as a child of the active selection, focuses it in the scene tree, and makes it the scene root when the scene is empty (test: `create_node_child_of_selection`)

8. `create-node-2d-catalog` The dialog exposes the common 2D node catalog and helper nodes
   Acceptance: The type tree includes the core 2D nodes (Node2D, Sprite2D, AnimatedSprite2D, CollisionShape2D, Area2D, CharacterBody2D, Camera2D) and they instantiate correctly (test: `create_node_2d_catalog`)

### Bottom panels (output, debugger, monitors, audio buses, shader editor)

1. `bottom-panel-bar` Implement the bottom panel bar that lists registered panels (Output, Debugger, Audio, Shader, …) and shows/hides the active one, with an expand-to-fill toggle.
   Acceptance: clicking a panel button shows that panel and hides others, clicking the active button collapses the dock, and expand fills the editor height (test: `bottom_panel_bar_toggles_active_panel`)

2. `bottom-output-console` Implement the Output console capturing game/editor stdout, errors, and warnings with clear, and per-severity filtering.
   Acceptance: running prints appear in Output, error/warning lines are styled by severity, the severity filter hides matching lines, and Clear empties the log (test: `bottom_output_console_captures_and_filters`)

3. `bottom-debugger-stack-vars` Implement the Debugger stack-frame list and variable inspector that populate on a breakpoint/error and let the user step and inspect locals.
   Acceptance: hitting a breakpoint populates the stack frames and selecting a frame shows its local variables in the inspector (test: `bottom_debugger_stack_and_variables_populate`)

4. `bottom-debugger-errors-tab` Implement the Debugger Errors tab listing runtime errors/warnings with message, source, and a jump-to-source action.
   Acceptance: a runtime error appears in the Errors tab with its source location and activating it navigates to that line (test: `bottom_debugger_errors_tab_lists_and_navigates`)

## Next

5. `bottom-monitors` Implement the performance Monitors panel graphing FPS, memory, draw calls, and physics over time during a play session.
   Acceptance: during play the Monitors panel plots the selected metrics over time and selecting a monitor updates the graph (test: `bottom_monitors_plot_metrics`)

6. `bottom-audio-buses` Implement the Audio bus layout editor (add/remove bus, volume sliders, mute/solo/bypass, effect chain, send routing) with save/load of the layout resource.
   Acceptance: adding a bus, adjusting its volume, toggling mute, and adding an effect update the bus layout and the layout saves to and loads from a resource (test: `bottom_audio_buses_edit_and_persist`)

## Later

7. `bottom-shader-editor-shell` Implement the bottom Shader editor shell that opens when a shader resource is edited, with tabbed open shaders and a compile/error status line.
   Acceptance: editing a shader resource opens it in the bottom shader panel as a tab and a compile error is surfaced in the status line (test: `bottom_shader_editor_shell_opens_and_reports`)

8. `bottom-debugger-profiler` Implement the Debugger profiler tab measuring per-frame function time with start/stop capture and a sortable cost breakdown.
   Acceptance: starting the profiler during play captures per-frame function costs and the breakdown is sortable by total/self time (test: `bottom_debugger_profiler_captures_costs`)

### Script editor core editing features

1. `script-core-syntax-highlight` GDScript syntax highlighting colors keywords, strings, comments, numbers, and types
   Acceptance: Opening a GDScript file applies highlighting spans for keywords, string/number literals, comments, and known types, and updates spans as text changes (test: `script_core_syntax_highlight`)

2. `script-core-autocomplete` Context-aware autocompletion suggests identifiers, members, and keywords and inserts the chosen entry
   Acceptance: Triggering completion offers ranked candidates for the current context (locals, members after `.`, keywords), and accepting one inserts it at the caret (test: `script_core_autocomplete`)

3. `script-core-auto-indent` Pressing Enter auto-indents the new line based on the previous line and block openers
   Acceptance: Newlines inherit the previous line's indentation and add one level after a block opener (e.g. line ending in `:`), respecting the configured tab/space indent style (test: `script_core_auto_indent`)

4. `script-core-bracket-autoclose-match` Brackets/quotes auto-close and matching pairs are highlighted
   Acceptance: Typing an opening bracket or quote inserts the closing counterpart with the caret between them, and the editor highlights the matching pair around the caret (test: `script_core_bracket_autoclose_match`)

5. `script-core-comment-toggle` Toggle-comment comments or uncomments the selected lines
   Acceptance: The comment-toggle action prefixes uncommented selected lines with the line-comment token and removes it from already-commented lines, preserving indentation (test: `script_core_comment_toggle`)

6. `script-core-line-operations` Duplicate-line, move-line-up/down, and delete-line operate on the caret line or selection
   Acceptance: Duplicate-line copies the current line/selection, move-line-up/down reorders it, and delete-line removes it, each updating the caret position consistently (test: `script_core_line_operations`)

7. `script-core-indent-unindent` Tab/Shift+Tab indent and unindent the selected lines by one level
   Acceptance: Indent adds one indent level to each selected line and unindent removes one (no-op at column zero), using the configured indent width (test: `script_core_indent_unindent`)

8. `script-core-code-folding` Foldable regions (functions, indented blocks) can be collapsed and expanded
   Acceptance: Foldable regions show a gutter affordance, folding hides the region body and shows a placeholder, and unfolding restores it; folds survive edits outside the region (test: `script_core_code_folding`)

9. `script-core-multi-caret` Multiple carets / column selection apply edits at all caret positions simultaneously
   Acceptance: Adding carets (e.g. add-caret or select-next-occurrence) and typing inserts the same text at every caret, and Escape collapses back to a single caret (test: `script_core_multi_caret`)

10. `script-core-save-and-trim` Saving writes the buffer and normalizes trailing whitespace / final newline per editor settings
    Acceptance: Save persists the script, and when whitespace normalization is enabled it trims trailing whitespace and ensures a single final newline; undo restores the pre-save buffer state (test: `script_core_save_and_trim`)

### Script editor search, navigation, debugging, and script panel

1. `script-nav-find-replace` In-file find and replace supports case/whole-word/regex options and replace-all
   Acceptance: Find highlights and cycles matches honoring case/whole-word/regex toggles, and replace / replace-all substitutes matches and reports the replacement count (test: `script_nav_find_replace`)

2. `script-nav-find-in-files` Find-in-files searches across project scripts and lists results with navigation
   Acceptance: A project-wide search returns matches grouped by file with line context, and activating a result opens that file at the matched line (test: `script_nav_find_in_files`)

3. `script-nav-goto-line` Go-to-line jumps the caret to a requested line number
   Acceptance: The go-to-line action moves the caret to the requested line, clamps out-of-range input to the valid range, and centers the line in view (test: `script_nav_goto_line`)

4. `script-nav-function-list` A function/member list (outline) lets the user jump to a symbol in the current script
   Acceptance: The outline lists the script's functions/members in document order and selecting an entry moves the caret to that symbol's definition (test: `script_nav_function_list`)

5. `script-nav-goto-definition` Goto-definition (Ctrl+click / shortcut) navigates to the definition of the symbol under the caret
   Acceptance: Invoking goto-definition on a symbol opens the defining script (or scrolls within the current one) at the definition line, and is a no-op for unresolved symbols (test: `script_nav_goto_definition`)

6. `script-nav-bookmarks` Bookmarks can be toggled on lines and navigated next/previous
   Acceptance: Toggling a bookmark marks the line in the gutter, and next/previous-bookmark cycles the caret through bookmarks across the current script (test: `script_nav_bookmarks`)

7. `script-nav-breakpoints-debugger` Breakpoints can be toggled and integrate with the debugger to pause execution
   Acceptance: Toggling a breakpoint marks the line and registers it with the debugger so a running project halts at that line, exposing the stack/locals; clearing it resumes normal execution (test: `script_nav_breakpoints_debugger`)

8. `script-nav-open-scripts-panel` The open-scripts panel lists loaded scripts and switches the active script on selection
   Acceptance: The script panel lists currently open scripts, selecting one makes it the active editor buffer, and closing an entry removes it from the panel (test: `script_nav_open_scripts_panel`)

### FileSystem dock browser, file ops, and resource drag-drop integration

1. `fs-dock-browser` Implement the FileSystem dock with a directory tree and a file list of the selected folder, navigable and reflecting the project's `res://` filesystem.
   Acceptance: selecting a folder in the tree lists its files and subfolders and the view tracks the on-disk `res://` contents (test: `fs_dock_browser_lists_directory_contents`)

2. `fs-dock-file-ops` Implement file operations (new folder, rename, move, duplicate, delete) via context menu and shortcuts, with dependency-safe path remapping on rename/move.
   Acceptance: renaming/moving a resource updates references in dependent scenes/resources and delete/duplicate/new-folder affect the filesystem correctly (test: `fs_dock_file_ops_remap_dependencies`)

3. `fs-dock-drag-to-scene-tree` Support dragging a scene/resource from the dock onto the Scene tree to instance it (scenes) or assign it (resources).
   Acceptance: dragging a `.tscn` onto a node instances it as a child and dragging a resource onto a compatible node assigns it (test: `fs_dock_drag_to_scene_tree_instances`)

4. `fs-dock-drag-to-inspector` Support dragging a resource from the dock onto a compatible Inspector property slot to assign it.
   Acceptance: dragging a resource onto a matching-typed property assigns it and an incompatible slot rejects the drop (test: `fs_dock_drag_to_inspector_assigns_property`)

## Next

5. `fs-dock-thumbnails` Generate and display thumbnail previews for textures and scene/resource files in the file list, refreshing when the source changes.
   Acceptance: texture and scene files show generated thumbnails and editing a source file refreshes its thumbnail (test: `fs_dock_thumbnails_render_and_refresh`)

6. `fs-dock-search-filter` Implement the dock search/filter box that narrows the file list to name matches across the current scope.
   Acceptance: typing in the filter narrows the listing to matching files and clearing it restores the full view (test: `fs_dock_search_filter_narrows_listing`)

7. `fs-dock-import-hooks` Wire the import pipeline so importable assets re-import on change and expose import settings, marking stale imports.
   Acceptance: changing an imported asset triggers a re-import and the dock reflects import status; import settings are editable per asset (test: `fs_dock_import_hooks_reimport_on_change`)

## Later

8. `fs-dock-favorites-context` Implement favorite directories and the file context menu (open, show in file manager, edit dependencies, view owners).
   Acceptance: favoriting a directory pins it to a Favorites section and the context menu's open/edit-dependencies/view-owners actions operate on the selected file (test: `fs_dock_favorites_and_context_menu`)

### Signals dock browsing, connection dialog, and connection management

1. `signals-dock-tree` Render the Signals tab as a tree of the selected node's signals grouped by declaring class, with signal signatures and existing connections nested under each signal.
   Acceptance: selecting a node lists its signals grouped by class with signatures, and signals with connections show them as children (test: `signals_dock_tree_lists_signals_and_connections`)

2. `signals-connect-dialog` Implement the Connect-a-Signal dialog to pick a target node and target method from the scene, creating the connection on confirm.
   Acceptance: connecting a signal to a target node+method via the dialog creates a persisted connection shown under the signal (test: `signals_connect_dialog_creates_connection`)

3. `signals-connection-flags-binds` Support the dialog's advanced options — extra bound arguments and the deferred/one-shot connect flags — and persist them on the connection.
   Acceptance: a connection made with bound args and deferred/one-shot flags persists those settings and they are visible when editing the connection (test: `signals_connection_flags_and_binds_persist`)

4. `signals-disconnect-edit` Support editing and disconnecting an existing connection from the signal tree.
   Acceptance: editing a connection updates its target/binds/flags and disconnect removes it from the node and the tree (test: `signals_disconnect_and_edit_connection`)

## Next

5. `signals-navigate-to-method` Implement go-to-method navigation that opens the target script at the connected receiver method.
   Acceptance: activating a connection navigates the script editor to the receiver method definition (test: `signals_navigate_to_connected_method`)

6. `signals-autocreate-receiver` Auto-generate a receiver method stub in the target node's script when connecting to a method that does not yet exist.
   Acceptance: confirming a connection to a non-existent method appends a correctly-signatured method stub to the target script (test: `signals_autocreate_receiver_method_stub`)

## Later

7. `signals-docs-tooltips` Surface signal documentation/descriptions as tooltips and a details area in the signal tree.
   Acceptance: hovering or selecting a signal shows its documentation text sourced from the class reference (test: `signals_docs_tooltips_render`)

8. `signals-filter-search` Implement a filter box that narrows the signal tree to name matches.
   Acceptance: typing in the filter narrows the signal tree to matching signals and clearing restores the full tree (test: `signals_filter_search_narrows_tree`)

### Animation editor (AnimationPlayer, timeline, tracks, AnimationTree)

1. `anim-player-panel` The AnimationPlayer panel lists animations and supports create/rename/duplicate/delete
   Acceptance: The panel lists the player's animations, and create/rename/duplicate/delete operate on the AnimationLibrary and update the selection (test: `anim_player_panel_manage`)

2. `anim-timeline-playhead` The timeline shows a time ruler and a draggable playhead with zoom and snap
   Acceptance: Dragging the playhead scrubs the animation time, the ruler reflects the current zoom, and snapping aligns the playhead to the configured time step (test: `anim_timeline_playhead`)

3. `anim-keyframe-edit` Keyframes can be inserted, selected, moved, and deleted on tracks
   Acceptance: Inserting a key adds it at the playhead on the active track, dragging moves it in time, multi-select moves a group, and delete removes the selection (test: `anim_keyframe_edit`)

4. `anim-track-types` The editor supports the core track types (property/value, transform, method-call, audio, animation)
   Acceptance: Each supported track type can be added to an animation, renders its type-appropriate row, and stores keys in the correct typed form (test: `anim_track_types`)

5. `anim-track-add-remove` Tracks can be added (including via inspector keying) and removed
   Acceptance: Adding a track for a node/property creates the track, keying a property from the inspector targets it, and removing a track deletes it with its keys (test: `anim_track_add_remove`)

6. `anim-bezier-editing` Bezier (value-curve) tracks expose draggable handles for easing
   Acceptance: A bezier track renders an editable curve, dragging in/out handles reshapes the interpolation, and the evaluated value follows the curve between keys (test: `anim_bezier_editing`)

7. `anim-interpolation-loop-modes` Per-key interpolation modes and animation loop mode are configurable
   Acceptance: Setting a key's interpolation (nearest/linear/cubic) changes how values are sampled between keys, and toggling loop mode wraps playback at the animation boundaries (test: `anim_interpolation_loop_modes`)

8. `anim-onion-skinning` Onion skinning previews neighboring frames around the playhead
   Acceptance: Enabling onion skinning renders ghosted past/future frames with configurable count, and disabling it clears the ghosts (test: `anim_onion_skinning`)

9. `anim-playback-controls` Playback controls drive play/pause/stop, loop, speed, and autoplay-on-load
   Acceptance: Play/pause/stop control the running animation, the speed scalar adjusts playback rate, and the autoplay flag marks the animation to start on scene load (test: `anim_playback_controls`)

10. `anim-tree-graph` The AnimationTree editor edits state-machine / blend-tree nodes and transitions
    Acceptance: The AnimationTree graph lets nodes (state machine, blend space, blend nodes) be added, connected, and parameterized, and transitions between states are created and removed (test: `anim_tree_graph_edit`)

### Editor systems (project settings, editor settings, VCS, export, variant coverage)

1. `systems-project-settings` Implement the Project Settings dialog (general settings tree, add/override/revert a setting) reading and writing `project.godot`.
   Acceptance: changing, adding, and reverting a project setting persists to `project.godot` and reloads on reopen (test: `systems_project_settings_persist`)

2. `systems-input-map` Implement the Project Settings Input Map tab (add/remove actions, bind key/mouse/joypad events, set deadzone) persisted to project settings.
   Acceptance: adding an action and binding an input event persists to the input map and the action resolves at runtime (test: `systems_input_map_actions_persist`)

3. `systems-autoloads` Implement autoload/singleton management (add, rename, reorder, enable, remove) writing the autoload section of project settings.
   Acceptance: adding an autoload registers a global singleton, reordering changes load order, and removal clears it (test: `systems_autoloads_register_globals`)

4. `systems-editor-settings` Implement the Editor Settings dialog covering interface, text editor, and shortcut preferences persisted to the editor settings store.
   Acceptance: changing an editor setting and a shortcut persists across editor restarts and applies immediately (test: `systems_editor_settings_persist_and_apply`)

## Next

5. `systems-export-presets` Implement export presets (create/edit/delete a preset, select platform, set resources/features, export to a target).
   Acceptance: creating a preset and exporting produces a target artifact and the preset persists in `export_presets.cfg` (test: `systems_export_presets_create_and_export`)

6. `systems-vcs-integration` Implement version-control integration (repo status, stage/unstage, commit, branch list, diff view) via the VCS interface.
   Acceptance: staging changes and committing creates a commit and the status/diff views reflect the working tree (test: `systems_vcs_stage_commit_and_diff`)

## Later

7. `systems-plugin-management` Implement editor plugin management (enable/disable plugins from Project Settings) loading and unloading the plugin at runtime.
   Acceptance: enabling a plugin loads it and registers its editor contributions; disabling unloads it (test: `systems_plugin_enable_disable`)

8. `systems-variant-coverage` Ensure Variant type coverage across settings/export serialization so every supported Variant type round-trips through the settings and preset stores.
   Acceptance: each supported Variant type set as a project/editor setting serializes and deserializes without loss (test: `systems_variant_coverage_round_trips`)
