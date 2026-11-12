# Editor Parity — Menu scene/project/debug/editor/help actions

Per-lane execution map for the editor's top menu bar actions. Source lane:
`Menu parity: scene/project/debug/editor/help actions` in
`prd/EDITOR_PARITY_BEADS.md` (lane 10). Source surface: the Scene, Project,
Debug, Editor, and Help menus and the shared menu behaviors (recent files,
shortcuts, enable/disable state).

Scope: the menu-bar dropdowns and the actions they fire. The scene-tab strip
and run controls live in `EDITOR_PARITY_TOP_BAR_MAP.md`; this lane is the
menus themselves and their command dispatch.

## Format

Each bead is `` N. `key-slug` Description `` followed by an
`Acceptance:` line naming the test in the form `(test: \`<test_name>\`)` the
planner wires into criteria-driven analysis.

## Now

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
