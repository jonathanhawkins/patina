# Editor Parity — Visual Fidelity (source-driven 1:1 chrome)

Execution map for the **visual fidelity** lane. Unlike the functional lanes
(which implement behaviors), this lane enumerates Godot's **permanent visual
structure** from `upstream/godot/editor/` and requires a served-DOM test per
element. See `prd/EDITOR_PARITY_VISUAL_FIDELITY_METHODOLOGY.md` for the procedure.

A criterion is satisfied only when a test boots the editor and asserts the served
`/editor` HTML renders the widget (id + verbatim Godot label + icon). "Looks the
same" by eye is never sufficient.

## Format

Each item is `` N. `key-slug` Description `` then an `Acceptance:` line naming
the served-DOM test as `(test: \`<test_name>\`)`.

## Now — FileSystem dock (worked example, source: `docks/filesystem_dock.cpp`)

1. `fs-vf-favorites-header` Render the always-present "Favorites:" section header with a star icon at the top of the FileSystem dock, shown even when zero directories are favorited (Godot creates `favorites_item` with `TTRC("Favorites:")` + the Favorites icon unconditionally).
   Acceptance: the served dock shows a collapsible `★ Favorites:` header and an (initially empty) favorites list region (test: `filesystem_dock_renders_favorites_section`)

2. `fs-vf-toolbar-chrome` Render the FileSystem dock's permanent toolbars: history prev/next buttons, the current-path line edit, the split-mode toggle, the "Filter Files" search box, and the sort button (Godot's `toolbar_hbc` + `toolbar2_hbc`).
   Acceptance: the served dock shows nav prev/next, a path field, a split toggle, a "Filter Files" box, and a sort button (test: `filesystem_dock_renders_toolbar_chrome`)

3. `fs-vf-chrome-wired` Wire the FileSystem dock chrome so it behaves: the filter box narrows the tree (keeping ancestors of matches), the sort button reorders, and selection drives the path field + nav history.
   Acceptance: the served editor JS sets up the dock and the filter/sort/path/nav helpers are present and invoked (test: `filesystem_dock_chrome_is_wired`)

## Next — remaining surfaces (enumerate from source, then implement)

Each item below is a source-driven enumeration + implementation task. Acceptance
is a new served-DOM test (to be authored) asserting the permanent widgets of that
surface, named in the per-surface criteria when the enumeration is done.

4. `scene-tree-vf-chrome` Enumerate the Scene tree dock's permanent chrome from `docks/scene_tree_dock.cpp` + `gui/scene_tree_editor.cpp` (add/instantiate/filter toolbar buttons, the node-filter box, the per-row visibility/lock/script/warning affordances) and render any missing element with a served-DOM test.

5. `inspector-vf-chrome` Enumerate the Inspector dock's permanent chrome from `docks/inspector_dock.cpp` + `inspector/editor_inspector.cpp` (resource toolbar new/open/save/tools, object header, "Filter Properties" box, history/forward/back) and render any missing element with a served-DOM test.

6. `signals-groups-vf-chrome` Enumerate the Node dock (Signals/Groups tabs) permanent chrome from `docks/node_dock.cpp`, `connections_dialog.cpp`, `groups_editor.cpp` and render any missing element with a served-DOM test.

7. `import-dock-vf-chrome` Enumerate the Import dock permanent chrome from `docks/import_dock.cpp` (preset option, "Reimport" button, import-as field) and render any missing element with a served-DOM test.

8. `bottom-panels-vf-chrome` Enumerate the bottom panels' permanent tab strip + per-panel toolbars (Output/Debugger/Audio/Animation/Shader) from the corresponding `editor/` sources and render any missing element with a served-DOM test.

9. `top-bar-vf-chrome` Enumerate the top menu bar + run toolbar permanent chrome from `editor_node.cpp` (menu list, 2D/3D/Script/Game/AssetLib switch, play/pause/stop/play-scene/play-custom buttons, renderer dropdown) and render any missing element with a served-DOM test.

10. `viewport-vf-chrome` Enumerate the 2D/3D viewport toolbar permanent chrome from `plugins/canvas_item_editor_plugin.cpp` + `plugins/node_3d_editor_plugin.cpp` (select/move/rotate/scale tools, snap, lock/group, zoom controls, view menu) and render any missing element with a served-DOM test.
