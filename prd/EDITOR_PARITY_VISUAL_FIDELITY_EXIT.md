# Editor Parity — Visual Fidelity Exit Criteria

Source-driven 1:1 chrome parity. Each criterion is satisfied only by a served-DOM
test that boots the editor and asserts the widget/label/icon is present. The
planner ticks `- [ ]` → `- [x]` automatically as each `(test: ...)` passes.
See `prd/EDITOR_PARITY_VISUAL_FIDELITY_METHODOLOGY.md`.

## FileSystem dock (source: `upstream/godot/editor/docks/filesystem_dock.cpp`)

- [ ] `fs-vf-favorites-header` The FileSystem dock renders the always-present `★ Favorites:` section header (shown even with zero favorites), matching Godot's unconditional `favorites_item`. (test: `filesystem_dock_renders_favorites_section`)
- [ ] `fs-vf-toolbar-chrome` The FileSystem dock renders its permanent toolbars — nav prev/next, path field, split-mode toggle, "Filter Files" box, sort button. (test: `filesystem_dock_renders_toolbar_chrome`)
- [ ] `fs-vf-chrome-wired` The FileSystem dock chrome is wired in the served JS — filter narrowing, sort, and path/nav-history helpers are present and invoked at boot. (test: `filesystem_dock_chrome_is_wired`)

## Remaining surfaces (enumerate from source, then add their DOM tests here)

These are tracked as execution-map items (`prd/EDITOR_PARITY_VISUAL_FIDELITY_MAP.md`,
items 4–10). Each becomes a checkbox with its own `(test: ...)` once the surface
is enumerated from `upstream/godot/editor/` and its served-DOM test is authored.

- [ ] `scene-tree-vf-chrome` Scene tree dock permanent chrome enumerated + rendered with a served-DOM test.
- [ ] `inspector-vf-chrome` Inspector dock permanent chrome enumerated + rendered with a served-DOM test.
- [ ] `signals-groups-vf-chrome` Node (Signals/Groups) dock permanent chrome enumerated + rendered with a served-DOM test.
- [ ] `import-dock-vf-chrome` Import dock permanent chrome enumerated + rendered with a served-DOM test.
- [ ] `bottom-panels-vf-chrome` Bottom panels tab strip + per-panel toolbars enumerated + rendered with a served-DOM test.
- [ ] `top-bar-vf-chrome` Top menu bar + run toolbar permanent chrome enumerated + rendered with a served-DOM test.
- [ ] `viewport-vf-chrome` 2D/3D viewport toolbar permanent chrome enumerated + rendered with a served-DOM test.
