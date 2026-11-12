# Editor Parity — Visual Fidelity Exit Criteria

Source-driven 1:1 chrome parity. Each criterion is satisfied only by a served-DOM
test that boots the editor and asserts the widget/label/icon is present. The
planner ticks `- [ ]` → `- [x]` automatically as each `(test: ...)` passes.
See `prd/EDITOR_PARITY_VISUAL_FIDELITY_METHODOLOGY.md`.

## FileSystem dock (source: `upstream/godot/editor/docks/filesystem_dock.cpp`)

- [x] `fs-vf-favorites-header` The FileSystem dock renders the always-present `★ Favorites:` section header (shown even with zero favorites), matching Godot's unconditional `favorites_item`. (test: `filesystem_dock_renders_favorites_section`)
- [x] `fs-vf-toolbar-chrome` The FileSystem dock renders its permanent toolbars — nav prev/next, path field, split-mode toggle, "Filter Files" box, sort button. (test: `filesystem_dock_renders_toolbar_chrome`)
- [x] `fs-vf-chrome-wired` The FileSystem dock chrome is wired in the served JS — filter narrowing, sort, and path/nav-history helpers are present and invoked at boot. (test: `filesystem_dock_chrome_is_wired`)

## Remaining surfaces (enumerate from source, then add their DOM tests here)

These are tracked as execution-map items (`prd/EDITOR_PARITY_VISUAL_FIDELITY_MAP.md`,
items 4–10). Each becomes a checkbox with its own `(test: ...)` once the surface
is enumerated from `upstream/godot/editor/` and its served-DOM test is authored.

- [x] `scene-tree-vf-chrome` Scene tree dock permanent chrome enumerated + rendered with a served-DOM test. (test: `scene_tree_dock_renders_toolbar_chrome` — closed pat-l695a)
- [x] `inspector-vf-chrome` Inspector dock permanent chrome enumerated + rendered with a served-DOM test. (test: `inspector_dock_renders_open_docs_button` — closed pat-5b63c)
- [x] `signals-groups-vf-chrome` Node (Signals/Groups) dock permanent chrome enumerated + rendered with a served-DOM test. (test: `node_dock_shell_is_present` — closed pat-ig21c)
- [ ] `import-dock-vf-chrome` Import dock permanent chrome enumerated + rendered with a served-DOM test.
- [x] `bottom-panels-vf-chrome` Bottom panels tab strip + per-panel toolbars enumerated + rendered with a served-DOM test. (test: `bottom_panel_renders_tab_strip` — closed pat-8y71p)
- [ ] `top-bar-vf-chrome` Top menu bar + run toolbar permanent chrome enumerated + rendered with a served-DOM test.
- [x] `viewport-vf-chrome` 2D/3D viewport toolbar permanent chrome enumerated + rendered with a served-DOM test. (test: `viewport_toolbars_2d_and_3d_present` — closed pat-9bh6y)
