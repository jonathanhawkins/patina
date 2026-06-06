# Editor Parity — Top bar scene tabs, run controls, and editor mode switching

Lane source: `prd/EDITOR_PARITY_BEADS.md` lane 9 — "Top bar parity: scene
tabs, run controls, and editor mode switching" (scene tabs,
run/play/pause/stop, 2D/3D/Script/Game/AssetLib modes).

This execution map enumerates the concrete beads required for top-bar parity
with Godot: the open-scene tab strip and its switching/close/unsaved
behavior, the tab context menu and reordering, opening new scenes, the run
controls (play project, play current/custom scene, pause, stop) and their
running state, and the editor main-screen mode switcher.

## Format

Each bead is listed as `` N. `key-slug` Description `` followed by an
`Acceptance:` line naming the test the planner wires into criteria-driven
analysis: `(test: \`<test_name>\`)`.

## Now

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
