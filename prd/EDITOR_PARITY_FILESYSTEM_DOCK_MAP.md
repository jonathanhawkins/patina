# Editor Parity — FileSystem dock browser, file ops, and resource drag-drop integration

Per-lane execution map for the FileSystem dock. Source lane: `FileSystem dock
parity: browser, file ops, and resource drag-drop integration` in
`prd/EDITOR_PARITY_BEADS.md` (lane 15). Source surface: file browser,
move/rename/delete, drag to inspector/scene tree, previews, import hooks.

Scope: the dock's directory tree + file list, the file-management operations
and their dependency-safe path updates, drag-and-drop into the rest of the
editor, thumbnail previews, search/favorites, and the import pipeline hooks.
Resource property editing itself lives in the Inspector lanes.

## Format

Each bead is `` N. `key-slug` Description `` followed by an
`Acceptance:` line naming the test in the form `(test: \`<test_name>\`)` the
planner wires into criteria-driven analysis.

## Now

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
