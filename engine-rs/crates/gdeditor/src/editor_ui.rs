//! Embedded HTML/JS/CSS for the Patina editor web UI.
//!
//! This module exports a single constant containing the full editor
//! interface as self-contained HTML. No external dependencies are used —
//! everything is vanilla HTML, CSS, and JavaScript.

/// The complete editor UI as a self-contained HTML page.
pub const EDITOR_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Patina Editor</title>
<style>
*, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }
:root {
  /* Godot 4 dark theme — NEUTRAL grays (R=G=B), sky-blue accent only.
     Values sampled directly from the Godot editor reference. */
  --bg: #1b1b1b; --panel: #282828; --border: #141414;
  --text: #d4d4d4; --text-dim: #717171; --accent: #579eff;
  --selected: #45474d; --hover: #333333; --error: #e0584e;
  --icon-node: #cfcfcf; --icon-node2d: #8ebbff; --icon-sprite2d: #8eef97;
  --icon-camera2d: #c88eff; --icon-control: #8eef97; --icon-label: #8eef97;
  --icon-button: #8eef97; --icon-node3d: #fc7f7f;
  --vec-x: #ec6a5e; --vec-y: #8bd450;
}
body.light {
  --bg: #e4e4e4; --panel: #f0f0f0; --border: #c8c8c8;
  --text: #1c1c1c; --text-dim: #5f5f5f; --accent: #2f74d0;
  --selected: #cdddf2; --hover: #e2e2e2; --error: #d03030;
}
body {
  background: var(--bg); color: var(--text); font-family: 'SF Mono', 'Cascadia Code', 'Consolas', monospace;
  font-size: 13px; line-height: 1.4; height: 100vh; display: flex; flex-direction: column; overflow: hidden;
}
button {
  background: var(--panel); color: var(--text); border: 1px solid var(--border);
  padding: 4px 10px; cursor: pointer; font: inherit; font-size: 12px; border-radius: 3px;
}
button:hover { background: var(--hover); border-color: var(--accent); }
button:active { background: var(--selected); }
input, select {
  background: var(--bg); color: var(--text); border: 1px solid var(--border);
  padding: 3px 6px; font: inherit; font-size: 12px; border-radius: 2px; outline: none;
}
input:focus, select:focus { border-color: var(--accent); }
input[type="checkbox"] { accent-color: var(--accent); width: 14px; height: 14px; }
input[type="color"] { padding: 1px 2px; height: 24px; width: 48px; cursor: pointer; }

/* Menu bar */
#menu-bar {
  display: flex; align-items: center; gap: 0; padding: 0 6px;
  background: var(--panel); border-bottom: 1px solid var(--border); flex-shrink: 0;
  font-size: 12px; height: 34px;
}
#menu-bar .menu-bar-brand { color: var(--accent); font-weight: bold; font-size: 13px; padding: 0 12px 0 4px; }
.menu-spacer { flex: 1 1 auto; min-width: 8px; }
.renderer-indicator {
  color: #6cab4d; font-size: 11px; padding: 4px 6px 4px 12px; margin-left: 6px;
  border-left: 1px solid var(--border); white-space: nowrap;
}
/* pat-j8sfr: the renderer indicator is now a <select> dropdown; keep it looking
   like the inline indicator (transparent, just the left divider). */
select.renderer-indicator {
  background: transparent; border: none; border-left: 1px solid var(--border);
  border-radius: 0; cursor: pointer; outline: none; -webkit-appearance: none; appearance: none;
}
select.renderer-indicator:hover { color: #8fd06a; }
select.renderer-indicator option { color: var(--text); background: var(--panel); }
.menu-item { position: relative; }
.menu-item > button {
  background: none; border: none; color: var(--text); padding: 4px 10px; cursor: pointer;
  font: inherit; font-size: 12px; border-radius: 2px;
}
.menu-item > button:hover, .menu-item > button.open { background: var(--hover); color: var(--accent); }
.menu-dropdown {
  display: none; position: absolute; top: 100%; left: 0; z-index: 300;
  background: var(--panel); border: 1px solid var(--border); border-radius: 3px;
  min-width: 200px; padding: 4px 0; margin-top: 1px; box-shadow: 0 6px 16px rgba(0,0,0,0.6);
}
.menu-dropdown.open { display: block; }
.menu-dropdown .menu-action {
  padding: 4px 14px; cursor: pointer; white-space: nowrap; display: flex;
  justify-content: space-between; align-items: center; font-size: 12px;
}
.menu-dropdown .menu-action:hover { background: var(--hover); color: var(--accent); }
.menu-dropdown .menu-action .shortcut { color: var(--text-dim); font-size: 11px; margin-left: 24px; }
.menu-dropdown .menu-sep { height: 1px; background: var(--border); margin: 4px 0; }
.menu-dropdown .menu-action.disabled { color: var(--text-dim); cursor: default; opacity: 0.5; }

/* Toolbar — Godot's canvas/contextual toolbar above the viewport */
#toolbar {
  display: flex; align-items: center; gap: 5px; padding: 4px 8px;
  background: var(--bg); border-bottom: 1px solid var(--border); flex-shrink: 0;
}
#toolbar .sep { width: 1px; height: 18px; background: var(--border); margin: 0 3px; }
.add-menu { position: relative; }
.add-menu-dropdown {
  display: none; position: absolute; top: 100%; left: 0; z-index: 100;
  background: var(--panel); border: 1px solid var(--border); border-radius: 3px;
  min-width: 160px; padding: 4px 0; margin-top: 2px; box-shadow: 0 4px 12px rgba(0,0,0,0.5);
}
.add-menu-dropdown.open { display: block; }
.add-menu-dropdown div {
  padding: 4px 12px; cursor: pointer; white-space: nowrap;
}
.add-menu-dropdown div:hover { background: var(--hover); color: var(--accent); }

/* Tool mode buttons */
.tool-btn {
  padding: 4px 8px; font-size: 12px; min-width: 28px; text-align: center;
}
.tool-btn.active { background: var(--selected); border-color: var(--accent); color: var(--accent); }
#scene-file-indicator { color: var(--text-dim); font-size: 11px; margin-left: auto; }
#scene-file-indicator .modified { color: var(--accent); }

/* Main layout */
#main { display: flex; flex: 1; overflow: hidden; }

/* Center area (viewport + bottom panel) */
#center-area { flex: 1; display: flex; flex-direction: column; overflow: hidden; }

/* Scene tree panel */
#scene-panel {
  background: var(--panel);
  display: flex; flex-direction: column; flex: 1; min-height: 80px; overflow: hidden;
}
#scene-panel .panel-header {
  padding: 6px 10px; font-weight: bold; font-size: 11px; text-transform: uppercase;
  color: var(--text-dim); border-bottom: 1px solid var(--border); letter-spacing: 0.5px;
}
/* pat-l695a: scene-tree dock permanent toolbar (Godot filter_hbc parity) */
#scene-tree-toolbar {
  display: flex; align-items: center; gap: 3px; padding: 3px 6px; flex-shrink: 0;
  border-bottom: 1px solid var(--border);
}
.st-tool-btn {
  background: transparent; border: 1px solid transparent; border-radius: 3px;
  color: var(--text-dim); font-size: 13px; line-height: 1; padding: 2px 5px;
  cursor: pointer; flex-shrink: 0;
}
.st-tool-btn:hover:not(:disabled) { background: var(--hover); color: var(--text); border-color: var(--border); }
.st-tool-btn:disabled { opacity: 0.35; cursor: default; }
#scene-search {
  flex: 1; min-width: 0; margin: 0; padding: 4px 8px; font-size: 12px; border-radius: 3px;
  background: var(--bg); color: var(--text); border: 1px solid var(--border);
}
#scene-search:focus { border-color: var(--accent); }
#scene-search::placeholder { color: var(--text-dim); }
#scene-tree { flex: 1; overflow: auto; padding: 4px 0; }

/* Godot parity: SceneTreeDock "Create Root Node:" panel, shown in the Scene
   dock in place of the tree while the edited scene has no root node. */
#create-root-panel { display: flex; flex-direction: column; padding: 6px 8px; gap: 4px; }
.create-root-header { font-size: 12px; font-weight: 600; color: var(--text-dim); padding: 2px 2px 6px; }
.create-root-btn {
  display: flex; align-items: center; gap: 8px; width: 100%;
  padding: 6px 8px; text-align: left; font-size: 13px;
  background: transparent; color: var(--text); border: none; border-radius: 4px; cursor: pointer;
}
.create-root-btn:hover { background: var(--hover, rgba(255,255,255,0.07)); }
.create-root-icon { display: inline-flex; width: 16px; justify-content: center; }

/* Import dock (Godot parity): tabbed with Scene */
#import-dock { flex: 1; overflow: auto; display: flex; flex-direction: column; padding: 8px; }
#import-empty { color: var(--text-dim); font-size: 12px; padding: 8px 4px; line-height: 1.5; }
.import-row { display: flex; align-items: center; gap: 8px; padding: 4px 0; font-size: 12px; }
.import-label { color: var(--text-dim); min-width: 72px; }
.import-select, .import-btn {
  flex: 1; background: var(--bg); border: 1px solid var(--border); border-radius: 3px;
  color: var(--text); font-size: 12px; padding: 3px 6px; cursor: pointer;
}
.import-btn:hover { border-color: var(--accent); color: var(--accent); }
#import-options { flex: 1; min-height: 40px; }
/* pat-h5s93: editable import option rows */
.import-opt-row .import-label { min-width: 110px; }
.import-input {
  flex: 1; background: var(--bg); border: 1px solid var(--border); border-radius: 3px;
  color: var(--text); font-size: 12px; padding: 3px 6px;
}
.import-input:focus { border-color: var(--accent); outline: none; }
.import-opt-row input[type="checkbox"] { margin-left: auto; }
.import-reimport { flex: none; align-self: stretch; margin-top: 8px; background: var(--panel); }
.import-reimport:disabled { opacity: 0.5; cursor: default; }
.tree-node { user-select: none; }
.tree-row {
  display: flex; align-items: center; padding: 2px 8px; cursor: pointer;
  white-space: nowrap; gap: 4px; position: relative; border: 1px solid transparent;
}
.tree-row:hover { background: var(--hover); }
.tree-row.selected { background: var(--selected); color: var(--text); }
.tree-row.drag-over-above { border-top: 2px solid var(--accent); }
.tree-row.drag-over-inside { background: rgba(87,158,255,0.15); border: 1px dashed var(--accent); }
.tree-row.drag-over-below { border-bottom: 2px solid var(--accent); }
.tree-row.hidden-node { opacity: 0.4; }
.tree-toggle { width: 14px; text-align: center; font-size: 10px; color: var(--text-dim); flex-shrink: 0; cursor: pointer; }
.tree-icon { font-size: 12px; flex-shrink: 0; width: 16px; text-align: center; line-height: 1; }
.tree-name { flex: 1; overflow: hidden; text-overflow: ellipsis; }
.tree-visibility {
  width: 18px; text-align: center; font-size: 13px; cursor: pointer; flex-shrink: 0;
  opacity: 0.4; transition: opacity 0.15s;
}
.tree-row:hover .tree-visibility { opacity: 0.8; }
.tree-visibility:hover { opacity: 1 !important; }
.tree-visibility.vis-hidden { opacity: 0.7; color: var(--error); }
.tree-lock {
  width: 18px; text-align: center; font-size: 12px; cursor: pointer; flex-shrink: 0;
  opacity: 0.4; transition: opacity 0.15s;
}
.tree-row:hover .tree-lock { opacity: 0.8; }
.tree-lock:hover { opacity: 1 !important; }
.tree-lock.locked { opacity: 0.7; color: var(--accent); }
.tree-badge { font-size: 10px; flex-shrink: 0; margin-left: 2px; }
.tree-badge.unique-name { color: #c88eff; font-size: 9px; font-weight: bold; }
.tree-children { display: none; }
.tree-children.expanded { display: block; }

/* Context menu */
#context-menu {
  display: none; position: fixed; z-index: 200;
  background: var(--panel); border: 1px solid var(--border); border-radius: 4px;
  min-width: 180px; padding: 4px 0; box-shadow: 0 6px 16px rgba(0,0,0,0.6);
}
#context-menu.open { display: block; }
.ctx-item {
  padding: 5px 14px; cursor: pointer; white-space: nowrap; display: flex;
  justify-content: space-between; align-items: center; font-size: 12px;
}
.ctx-item:hover { background: var(--hover); color: var(--accent); }
.ctx-shortcut { color: var(--text-dim); font-size: 11px; margin-left: 20px; }
.ctx-separator { height: 1px; background: var(--border); margin: 4px 0; }

/* Viewport */
#viewport-panel { flex: 1; display: flex; flex-direction: column; background: var(--bg); overflow: hidden; }
/* Godot has no "Viewport" title bar — the canvas sits directly under the scene tab. */
#viewport-panel > .panel-header { display: none; }
#viewport-container { flex: 1; display: flex; align-items: center; justify-content: center; overflow: hidden; padding: 0; position: relative; }
/* pat-9yq5q: 2D viewport toolbar (Godot CanvasItemEditor main_menu_hbox) */
#vp2d-toolbar {
  position: absolute; top: 4px; left: 4px; right: 4px; z-index: 8;
  display: flex; align-items: center; gap: 2px; padding: 3px 5px;
  background: var(--panel); border: 1px solid var(--border); border-radius: 4px;
  box-shadow: 0 2px 6px rgba(0,0,0,0.4); font-size: 12px;
}
.vp2d-btn {
  background: transparent; border: 1px solid transparent; border-radius: 3px;
  color: var(--text-dim); font-size: 12px; line-height: 1; padding: 3px 5px;
  cursor: pointer; flex-shrink: 0; min-width: 22px; display: inline-flex; align-items: center; justify-content: center; gap: 2px;
}
.vp2d-btn:hover { background: var(--hover); color: var(--text); border-color: var(--border); }
.vp2d-btn.active { background: var(--selected); color: var(--accent); border-color: var(--accent); }
.vp2d-sep { width: 1px; height: 18px; background: var(--border); margin: 0 3px; flex-shrink: 0; }
.vp2d-spacer { flex: 1; }
#vp2d-zoom { display: flex; align-items: center; gap: 2px; flex-shrink: 0; }
#vp2d-zoom button { background: none; border: none; color: var(--text); cursor: pointer; padding: 0 3px; font-size: 13px; }
#vp2d-zoom #vp2d-zoom-reset { color: var(--text-dim); font-size: 10px; }
#vp2d-zoom #vp2d-zoom-label { min-width: 38px; text-align: center; font-size: 11px; color: var(--text-dim); }
.vp2d-dropdown {
  position: fixed; background: var(--panel); border: 1px solid var(--border); border-radius: 3px;
  padding: 4px 0; z-index: 700; box-shadow: 0 4px 12px rgba(0,0,0,0.5); min-width: 180px; font-size: 12px;
}
.vp2d-dropdown .vp2d-mi { padding: 4px 12px; cursor: pointer; white-space: nowrap; display: flex; align-items: center; gap: 6px; }
.vp2d-dropdown .vp2d-mi:hover { background: var(--hover); }
.vp2d-dropdown .vp2d-mi .vp2d-check { width: 12px; color: var(--accent); }
.vp2d-dropdown .vp2d-mi-sep { height: 1px; background: var(--border); margin: 4px 0; }
/* Central script editor (pat-qtmbv): Godot-style toolbar + script list + relocated #script-panel. */
#script-editor-panel { flex: 1; display: flex; flex-direction: column; overflow: hidden; }
#script-toolbar {
  display: flex; align-items: center; gap: 2px; padding: 3px 6px; min-height: 30px;
  background: var(--bg); border-bottom: 1px solid var(--border); flex-shrink: 0;
}
.script-menu-btn {
  background: transparent; border: none; color: var(--text); font-size: 12px;
  padding: 4px 8px; border-radius: 3px; cursor: pointer;
}
.script-menu-btn:hover, .script-menu-btn.open { background: var(--hover); }
#script-name-label { flex: 1; text-align: center; color: var(--text-dim); font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.script-tool-btn {
  background: transparent; border: none; color: var(--text-dim); font-size: 12px;
  padding: 4px 8px; border-radius: 3px; cursor: pointer; white-space: nowrap;
}
.script-tool-btn:hover:not(:disabled) { background: var(--hover); color: var(--text); }
.script-tool-btn:disabled { opacity: 0.35; cursor: default; }
.script-icon-btn { padding: 4px 7px; font-size: 13px; }
.script-tb-sep { width: 1px; height: 18px; background: var(--border); margin: 0 4px; flex-shrink: 0; }
#script-body { flex: 1; display: flex; min-height: 0; overflow: hidden; }
#script-list-col {
  width: 200px; min-width: 120px; flex-shrink: 0; display: flex; flex-direction: column;
  border-right: 1px solid var(--border); overflow: hidden;
}
.script-filter-input {
  margin: 4px; background: var(--bg); border: 1px solid var(--border); border-radius: 3px;
  color: var(--text); font-size: 11px; padding: 3px 6px; outline: none;
}
.script-filter-input:focus { border-color: var(--accent); }
.script-filter-input::placeholder { color: var(--text-dim); }
#script-list { flex: 1; overflow: auto; min-height: 40px; }
#script-members { flex: 1; overflow: auto; min-height: 40px; border-top: 1px solid var(--border); }
.script-list-item { padding: 4px 10px; font-size: 12px; cursor: pointer; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.script-list-item:hover { background: var(--hover); }
.script-list-item.active { background: var(--selected); color: var(--accent); }
/* pat-f64ui: members overview (method outline) */
#script-members-bar { display: flex; align-items: center; gap: 4px; }
#script-members-bar .script-filter-input { flex: 1; }
.script-member-item { padding: 3px 10px; font-size: 12px; cursor: pointer; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-family: monospace; color: var(--text-dim); }
.script-member-item:hover { background: var(--hover); color: var(--text); }
/* pat-nrwy2: in-editor Search Help dialog (Godot EditorHelpSearch) */
.help-search-overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.45); z-index: 950; display: flex; align-items: center; justify-content: center; }
.help-search-box { background: var(--panel); border: 1px solid var(--border); border-radius: 6px; width: 520px; max-width: 80%; max-height: 70%; display: flex; flex-direction: column; box-shadow: 0 8px 24px rgba(0,0,0,0.6); }
.help-search-head { padding: 8px 12px; font-weight: 600; border-bottom: 1px solid var(--border); }
.help-search-input { margin: 8px 12px 6px; padding: 5px 8px; font-size: 13px; background: var(--bg); color: var(--text); border: 1px solid var(--border); border-radius: 3px; outline: none; }
.help-search-input:focus { border-color: var(--accent); }
.help-search-results { flex: 1; overflow: auto; min-height: 80px; border-top: 1px solid var(--border); margin: 0 0 0; }
.help-search-item { padding: 4px 12px; font-size: 12px; cursor: pointer; }
.help-search-item:hover { background: var(--hover); }
.help-search-item.active { background: var(--selected); color: var(--accent); }
.help-search-detail { padding: 8px 12px; font-size: 12px; color: var(--text-dim); border-top: 1px solid var(--border); min-height: 36px; }
#script-main-host { flex: 1; position: relative; overflow: hidden; display: flex; flex-direction: column; }
#script-main-host > #script-panel { flex: 1; height: auto; }
/* Script toolbar dropdown (File/Search/Debug menus) */
.script-dropdown {
  position: fixed; background: var(--panel); border: 1px solid var(--border); border-radius: 3px;
  padding: 4px 0; z-index: 600; box-shadow: 0 4px 12px rgba(0,0,0,0.5); min-width: 210px;
}
.script-dropdown-item { display: flex; justify-content: space-between; gap: 16px; padding: 4px 12px; font-size: 12px; cursor: pointer; white-space: nowrap; }
.script-dropdown-item:hover { background: var(--hover); }
.script-dropdown-item .sd-shortcut { color: var(--text-dim); font-size: 11px; }
.script-dropdown-sep { height: 1px; background: var(--border); margin: 4px 0; }
/* Game view (pat-xwdlg) */
#game-panel { flex: 1; display: flex; flex-direction: column; overflow: hidden; }
/* Game view toolbar (Godot game_view_plugin.cpp parity) */
#game-toolbar {
  display: flex; align-items: center; gap: 2px; padding: 3px 6px; min-height: 30px;
  background: var(--bg); border-bottom: 1px solid var(--border); flex-shrink: 0; flex-wrap: wrap;
}
.game-tool-btn {
  display: inline-flex; align-items: center; gap: 4px; background: transparent; border: 1px solid transparent;
  color: var(--text-dim); font-size: 12px; padding: 3px 7px; border-radius: 3px; cursor: pointer; white-space: nowrap;
}
.game-tool-btn:hover:not(:disabled) { background: var(--hover); color: var(--text); }
.game-tool-btn.active { background: var(--selected); color: var(--accent); border-color: var(--accent); }
.game-tb-sep { width: 1px; height: 18px; background: var(--border); margin: 0 4px; flex-shrink: 0; }
.game-dot { width: 8px; height: 8px; border-radius: 50%; display: inline-block; }
.game-dot.dot-2d { background: var(--icon-node2d, #8ebbff); }
.game-dot.dot-3d { background: var(--icon-node3d, #fc7f7f); }
.game-status { font-size: 11px; color: var(--text-dim); }
#game-screen { flex: 1; position: relative; display: flex; align-items: center; justify-content: center; background: #000; overflow: hidden; }
#game-screen-img { max-width: 100%; max-height: 100%; image-rendering: pixelated; }
#game-idle { position: absolute; text-align: center; color: var(--text-dim); font-size: 13px; }
.game-idle-icon { font-size: 40px; color: var(--accent); margin-bottom: 8px; }
/* AssetLib browser (pat-4jaq7) */
#assetlib-panel { flex: 1; display: flex; flex-direction: column; overflow: hidden; }
#assetlib-toolbar { display: flex; gap: 8px; padding: 8px; border-bottom: 1px solid var(--border); align-items: center; flex-wrap: wrap; }
#assetlib-toolbar input, #assetlib-toolbar select { font-size: 12px; padding: 4px 6px; background: var(--panel); color: var(--text); border: 1px solid var(--border); border-radius: 3px; }
#assetlib-search { flex: 1; min-width: 160px; }
#assetlib-toolbar2 { display: flex; gap: 8px; padding: 6px 8px; border-bottom: 1px solid var(--border); align-items: center; flex-wrap: wrap; }
#assetlib-toolbar2 select { font-size: 12px; padding: 4px 6px; background: var(--panel); color: var(--text); border: 1px solid var(--border); border-radius: 3px; }
.assetlib-tbtn { font-size: 12px; padding: 4px 12px; }
/* Godot-style list view (scrolls); min-height:0 lets the flex child scroll. */
/* Godot-style 2-column grid of borderless asset items. */
#assetlib-grid { flex: 1; min-height: 0; overflow: auto; padding: 8px 10px; display: grid; grid-template-columns: repeat(auto-fill, minmax(270px, 1fr)); gap: 6px 18px; align-content: start; }
.asset-row { display: flex; align-items: flex-start; gap: 10px; padding: 6px; cursor: pointer; border-radius: 4px; }
.asset-row:hover { background: var(--hover); }
.asset-row-icon { width: 48px; height: 48px; display: flex; align-items: center; justify-content: center; font-size: 26px; background: linear-gradient(135deg,#2a2d35,#1b1d22); border-radius: 4px; flex-shrink: 0; }
.asset-row-body { flex: 1; min-width: 0; padding-top: 1px; }
.asset-row-title { font-size: 13px; font-weight: 600; color: var(--text); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.asset-row-cat { font-size: 11px; color: var(--text-dim); margin-top: 1px; }
.asset-row-author { font-size: 11px; color: var(--text-dim); margin-top: 2px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.assetlib-flabel { font-size: 12px; color: var(--text-dim); margin-left: 4px; }
#assetlib-toolbar2 .assetlib-flabel:first-child { margin-left: 0; }
#assetlib-pagination { display: flex; justify-content: center; align-items: center; gap: 4px; padding: 8px; border-top: 1px solid var(--border); flex-shrink: 0; }
.assetlib-page-btn { padding: 3px 9px; font-size: 11px; background: var(--panel); color: var(--text); border: 1px solid var(--border); border-radius: 3px; cursor: pointer; }
.assetlib-page-btn:hover:not(:disabled) { border-color: var(--accent); color: var(--accent); }
.assetlib-page-btn.active { background: var(--selected); color: #fff; border-color: var(--accent); }
.assetlib-page-btn:disabled { opacity: 0.4; cursor: default; }
.asset-card { border: 1px solid var(--border); border-radius: 4px; background: var(--panel); overflow: hidden; display: flex; flex-direction: column; }
.asset-card-thumb { height: 88px; display: flex; align-items: center; justify-content: center; font-size: 32px; background: linear-gradient(135deg, #2a2d35, #1b1d22); }
.asset-card-body { padding: 6px 8px; }
.asset-card-title { font-size: 12px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.asset-card-meta { font-size: 10px; color: var(--text-dim); margin-top: 2px; }
.asset-card-cat { display: inline-block; font-size: 9px; padding: 1px 6px; border-radius: 8px; background: rgba(255,255,255,0.08); margin-top: 5px; color: var(--text-dim); }
.assetlib-loading, .assetlib-empty { color: var(--text-dim); font-size: 12px; padding: 12px; grid-column: 1 / -1; }
#viewport-img {
  /* Fill the whole panel like Godot. The server renders at the panel's real
     size (reported via /api/viewport/resize), so the aspect matches and
     `contain` fills edge-to-edge instead of letterboxing a fixed square. */
  width: 100%; height: 100%; object-fit: contain; image-rendering: pixelated; background: #111;
}
#viewport-placeholder {
  color: var(--text-dim); font-size: 14px; text-align: center;
}
#viewport-placeholder .loading-spinner {
  display: inline-block; width: 24px; height: 24px; border: 2px solid var(--border);
  border-top-color: var(--accent); border-radius: 50%; animation: vp-spin 0.8s linear infinite;
  margin-bottom: 8px;
}
@keyframes vp-spin { to { transform: rotate(360deg); } }

/* Inspector panel */
#inspector-panel {
  width: 300px; min-width: 200px; background: var(--panel);
  border-left: 1px solid var(--border); display: flex; flex-direction: column; flex-shrink: 0;
}
/* Draggable splitter on the inspector's left edge. */
#inspector-divider { width: 5px; cursor: ew-resize; background: transparent; flex-shrink: 0; }
#inspector-divider:hover { background: var(--accent); opacity: 0.45; }
#inspector-panel .panel-header {
  padding: 6px 10px; font-weight: bold; font-size: 11px; text-transform: uppercase;
  color: var(--text-dim); border-bottom: 1px solid var(--border); letter-spacing: 0.5px;
}
#inspector { flex: 1; overflow: auto; padding: 8px; }
.insp-section { margin-bottom: 4px; }
.insp-section-header {
  font-weight: bold; font-size: 11px; text-transform: uppercase; color: var(--text-dim);
  padding: 6px 4px 4px 0; border-bottom: 1px solid var(--border); letter-spacing: 0.5px;
  cursor: pointer; user-select: none; display: flex; align-items: center; gap: 4px;
}
.insp-section-header:hover { color: var(--accent); }
.insp-section-toggle { font-size: 9px; width: 12px; text-align: center; transition: transform 0.15s; }
.insp-section-toggle.collapsed { transform: rotate(-90deg); }
.insp-section-body { padding: 4px 0; }
.insp-section-body.collapsed { display: none; }
/* Godot EditorInspector class-category bar (groups properties by declaring class) */
.insp-class-category {
  display: flex; align-items: center; justify-content: center; gap: 6px;
  padding: 5px 8px; margin: 8px 0 2px; background: #2f2f2f;
  border-top: 1px solid #3a3a3a; border-bottom: 1px solid #1f1f1f;
  font-size: 12px; font-weight: 600; color: var(--text); user-select: none;
}
.insp-class-category .tree-icon { font-size: 13px; }
/* Collapsible inspector subgroup (Transform, Visibility, Collision, …) */
.insp-subgroup {
  display: flex; align-items: center; gap: 5px; padding: 4px 2px;
  font-size: 12px; color: var(--text); cursor: pointer; user-select: none;
}
.insp-subgroup:hover { color: #fff; }
.insp-subgroup-arrow { font-size: 9px; width: 10px; text-align: center; color: var(--text-dim); transition: transform 0.15s; }
.insp-subgroup-arrow.collapsed { transform: rotate(-90deg); }
.insp-subgroup-body { padding-left: 10px; }
.insp-subgroup-body.collapsed { display: none; }
/* Godot object-selector header (class icon + node name) */
.insp-object-selector {
  display: flex; align-items: center; gap: 6px; padding: 5px 8px; margin-bottom: 6px;
  background: var(--bg); border: 1px solid var(--border); border-radius: 3px;
}
.insp-object-selector .tree-icon { font-size: 13px; flex-shrink: 0; }
.insp-object-name { flex: 1; font-size: 12px; color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; cursor: text; }
.insp-object-chevron { color: var(--text-dim); font-size: 8px; flex-shrink: 0; }
.insp-subgroup-header {
  font-size: 11px; color: var(--accent); font-weight: bold; padding: 4px 0 2px 8px;
  border-bottom: 1px solid var(--border); margin: 4px 0 2px;
}
.insp-fav-btn {
  font-size: 10px; cursor: pointer; opacity: 0; transition: opacity 0.15s;
  flex-shrink: 0; width: 12px; text-align: center;
}
.insp-row:hover .insp-fav-btn { opacity: 0.45; }
.insp-fav-btn:hover { opacity: 1 !important; }
.insp-fav-btn.active { opacity: 1 !important; color: #f0c040; }
/* Godot resource picker (texture, material, …) */
.res-picker { display: flex; gap: 4px; flex: 1; align-items: center; min-width: 0; }
.res-picker-main { flex: 1; display: flex; align-items: center; justify-content: space-between; gap: 4px; padding: 2px 6px; background: var(--bg); border: 1px solid var(--border); border-radius: 2px; cursor: pointer; font-size: 11px; min-width: 0; }
.res-picker-main:hover { border-color: var(--accent); }
.res-picker-label { color: var(--text-dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.res-picker-label.has-res { color: var(--text); }
.res-picker-chevron { color: var(--text-dim); font-size: 8px; flex-shrink: 0; }
.res-picker-load { cursor: pointer; color: var(--text-dim); font-size: 12px; flex-shrink: 0; }
.res-picker-load:hover { color: var(--accent); }
.res-picker-menu { position: fixed; z-index: 600; background: var(--panel); border: 1px solid var(--border); border-radius: 3px; box-shadow: 0 6px 16px rgba(0,0,0,0.6); padding: 4px 0; min-width: 170px; max-height: 60vh; overflow-y: auto; }
.res-picker-menu div { padding: 5px 12px; font-size: 12px; cursor: pointer; white-space: nowrap; }
.res-picker-menu div:hover { background: var(--hover); color: var(--accent); }
/* Godot collision Layer/Mask bit-flag grid */
/* Godot collision Layer/Mask: label on top, full-width grouped 4x2 blocks + ⋮ menu */
.insp-row-flags { flex-direction: column; align-items: stretch; gap: 3px; }
.insp-row-flags .insp-fav-btn { display: none; }
.insp-row-flags .insp-label { width: auto; }
.insp-row-flags .insp-value { width: 100%; }
.bitflag-wrap { display: flex; align-items: stretch; gap: 4px; flex: 1; min-width: 0; }
.bitflag-gridcol { flex: 1; min-width: 0; display: flex; flex-direction: column; }
.bitflag-grid { display: flex; gap: 5px; flex-wrap: wrap; }
.bitflag-block { display: grid; grid-template-columns: repeat(4, 1fr); gap: 2px; flex: 1 1 0; min-width: 0; }
.bitflag-cell { font-size: 9px; text-align: center; padding: 2px 0; background: #2f343c; border: 1px solid var(--border); border-radius: 2px; cursor: pointer; color: var(--text-dim); user-select: none; min-width: 0; line-height: 1.3; }
.bitflag-cell:hover { border-color: var(--accent); }
.bitflag-cell.on { background: var(--accent); color: #fff; border-color: var(--accent); }
.bitflag-expand { font-size: 11px; color: var(--text-dim); cursor: pointer; line-height: 1; padding: 2px; align-self: flex-end; }
.bitflag-expand:hover { color: var(--accent); }
.bitflag-menu { width: 14px; padding: 0; border: 1px solid transparent; background: transparent; color: var(--text-dim); cursor: pointer; font-size: 12px; border-radius: 3px; flex-shrink: 0; display: flex; align-items: center; justify-content: center; }
.bitflag-menu:hover { background: var(--hover); color: var(--text); border-color: var(--border); }
/* Godot inspector action toolbar */
.insp-toolbar { display: flex; align-items: center; gap: 1px; padding: 0 0 5px; margin-bottom: 5px; border-bottom: 1px solid var(--border); }
.insp-toolbar-spacer { flex: 1; }
.insp-tool-btn { width: 24px; height: 22px; padding: 0; border: 1px solid transparent; background: transparent; color: var(--text-dim); cursor: pointer; font-size: 12px; border-radius: 3px; display: inline-flex; align-items: center; justify-content: center; flex-shrink: 0; }
.insp-tool-btn:hover:not(:disabled) { background: var(--hover); color: var(--text); border-color: var(--border); }
.insp-tool-btn:disabled { opacity: 0.3; cursor: default; }
.insp-filter-row { display: flex; align-items: center; gap: 4px; margin: 4px 0; }
.insp-sub-resource-btn {
  font-size: 10px; cursor: pointer; color: var(--text-dim); padding: 1px 4px;
  border: 1px solid var(--border); border-radius: 2px; background: none;
}
.insp-sub-resource-btn:hover { color: var(--accent); border-color: var(--accent); }
.insp-row { display: flex; align-items: center; margin-bottom: 4px; gap: 6px; position: relative; }
.insp-label { width: 80px; font-size: 12px; color: var(--text-dim); flex-shrink: 0; overflow: hidden; text-overflow: ellipsis; }
.insp-value { flex: 1; display: flex; gap: 4px; align-items: center; }
.insp-value input[type="text"], .insp-value input[type="number"] { width: 100%; }
.insp-value select { width: 100%; }

/* Vector2 editor */
.vec2-editor { display: flex; gap: 2px; flex: 1; align-items: center; }
.vec2-field { display: flex; align-items: center; flex: 1; gap: 2px; }
.vec2-label { font-size: 10px; font-weight: bold; width: 12px; text-align: center; flex-shrink: 0; }
.vec2-label.x-label { color: var(--vec-x); }
.vec2-label.y-label { color: var(--vec-y); }
.vec2-input { flex: 1; min-width: 36px; }
.insp-unit { font-size: 10px; color: var(--text-dim); margin-left: 2px; flex-shrink: 0; }
.vec2-link { cursor: pointer; color: var(--text-dim); font-size: 12px; padding: 0 2px; flex-shrink: 0; user-select: none; }
.vec2-link.active { color: var(--accent); }

/* Color editor */
.color-editor { display: flex; gap: 4px; flex: 1; align-items: center; flex-wrap: wrap; }
.color-swatch { width: 28px; height: 24px; border: 1px solid var(--border); border-radius: 2px; cursor: pointer; flex-shrink: 0; }
.color-hex { width: 70px; font-size: 11px; }
.color-slider-group { display: flex; align-items: center; gap: 1px; }
.color-slider-label { font-size: 9px; color: var(--text-dim); width: 8px; }

/* Checkbox styling */
.bool-editor { display: flex; align-items: center; gap: 6px; }
.bool-editor input[type="checkbox"] {
  width: 16px; height: 16px; cursor: pointer; accent-color: var(--accent);
}
.bool-editor label { font-size: 12px; color: var(--text-dim); cursor: pointer; }

/* NodePath editor */
.nodepath-editor { display: flex; gap: 4px; flex: 1; align-items: center; }
.nodepath-input { flex: 1; }
.nodepath-pick { padding: 2px 6px; font-size: 11px; }

/* Property revert button */
.insp-revert {
  width: 16px; height: 16px; padding: 0; border: none; background: transparent;
  color: var(--text-dim); cursor: pointer; font-size: 12px; flex-shrink: 0;
  display: flex; align-items: center; justify-content: center; border-radius: 2px;
  opacity: 0; transition: opacity 0.15s;
}
.insp-row:hover .insp-revert { opacity: 1; }
.insp-revert:hover { color: var(--accent); background: var(--hover); }

.insp-value .vec-label { font-size: 11px; color: var(--text-dim); min-width: 10px; }
.insp-value .vec-input { flex: 1; min-width: 40px; }
.insp-readonly { color: var(--text-dim); font-style: italic; font-size: 12px; }
.insp-empty { color: var(--text-dim); font-style: italic; padding: 20px 0; text-align: center; }

/* Inspector toolbar */
.insp-history {
  display: flex; align-items: center; gap: 4px; padding: 4px 0; margin-bottom: 4px;
  border-bottom: 1px solid var(--border);
}
.insp-history button {
  width: 24px; height: 22px; padding: 0; border: 1px solid var(--border); border-radius: 2px;
  background: var(--bg); color: var(--text); cursor: pointer; font-size: 12px;
  display: flex; align-items: center; justify-content: center;
}
.insp-history button:hover:not(:disabled) { background: var(--hover); border-color: var(--accent); color: var(--accent); }
.insp-history button:disabled { opacity: 0.3; cursor: default; }
.resource-info {
  flex: 1; font-size: 11px; color: var(--text-dim); overflow: hidden;
  text-overflow: ellipsis; white-space: nowrap; padding: 0 4px;
}
.resource-info .resource-type { color: var(--accent); font-weight: bold; }
.resource-info .resource-path { color: var(--text-dim); }

/* Sub-resource breadcrumb navigation */
.insp-breadcrumb {
  display: flex; align-items: center; gap: 2px; padding: 3px 0; font-size: 11px;
  color: var(--text-dim); flex-wrap: wrap;
}
.insp-breadcrumb-item {
  cursor: pointer; color: var(--text-dim); padding: 1px 4px; border-radius: 2px;
  border: none; background: none; font: inherit; font-size: 11px;
}
.insp-breadcrumb-item:hover { color: var(--accent); background: var(--hover); }
.insp-breadcrumb-item.active { color: var(--text); font-weight: bold; cursor: default; }
.insp-breadcrumb-sep { color: var(--text-dim); opacity: 0.5; }

/* Bottom panel */
#bottom-panel {
  background: var(--panel); border-top: 1px solid var(--border);
  display: flex; flex-direction: column; flex-shrink: 0;
  min-height: 30px; transition: height 0.15s;
}
/* Godot EditorBottomPanel (editor/gui/editor_bottom_panel.cpp): item_vbox
   (content) fills the top and bottom_hbox (the tab strip) is docked at the very
   bottom edge. Collapsed => only the 30px tab strip is visible. The flex order
   keys put content above the tab strip without reordering the DOM. */
#bottom-panel.collapsed { height: 30px !important; }
#bottom-panel.collapsed #bottom-panel-content { display: none; }
#bottom-panel-content { order: 0; flex: 1; overflow: auto; padding: 6px 10px; font-size: 12px; }
.bottom-content-tab { display: none; }
.bottom-content-tab.active { display: block; }
#bottom-panel-header {
  order: 1; display: flex; align-items: center; gap: 2px; padding: 0 4px;
  border-top: 1px solid var(--border); flex-shrink: 0; height: 30px;
}
.bottom-tab {
  padding: 4px 12px; font-size: 12px; cursor: pointer; color: var(--text-dim);
  border: none; background: transparent; border-radius: 3px; font: inherit;
}
.bottom-tab:hover { color: var(--text); background: var(--hover); }
#bottom-panel:not(.collapsed) .bottom-tab.active { color: var(--text); background: var(--hover); }
/* Right-side cluster of the tab strip (Godot bottom_hbox): version label, then
   the Pin + Expand buttons. Pin/Expand only show while a panel is open
   (editor_bottom_panel.cpp:122-123). */
#bottom-strip-right { margin-left: auto; display: flex; align-items: center; gap: 6px; padding-right: 2px; }
#bottom-version { font-size: 11px; color: var(--text-dim); opacity: 0.65; user-select: none; }
.bottom-strip-btn {
  padding: 3px 7px; font-size: 12px; line-height: 1; cursor: pointer; color: var(--text-dim);
  background: transparent; border: none; border-radius: 3px;
}
.bottom-strip-btn:hover { color: var(--text); background: var(--hover); }
.bottom-strip-btn.active { color: var(--text); background: var(--hover); }
#bottom-panel.collapsed .bottom-strip-btn { display: none; }
/* AnimationTree state-machine view, nested in the ANIMATION bottom tab.
   Hidden until the tree button toggles `.open` (pat-v4o3a). */
#anim-tree-dialog { display: none; margin-bottom: 8px; border: 1px solid var(--border); border-radius: 4px; background: var(--panel); }
#anim-tree-dialog.open { display: block; }
#anim-tree-header { display: flex; align-items: center; justify-content: space-between; padding: 4px 8px; border-bottom: 1px solid var(--border); font-weight: 600; }
#anim-tree-header button { background: transparent; border: none; color: var(--text-dim); cursor: pointer; }
#anim-tree-header button:hover { color: var(--text); }
#anim-tree-body { padding: 6px 8px; }
.anim-tree-node { padding: 3px 8px; border-radius: 3px; font-size: 12px; }
.anim-tree-node.active { background: var(--accent); color: #000; }
/* Output panel (Godot EditorLog parity): log + filter box, right-side toolbar */
#output-panel-body { display: flex; height: 100%; gap: 4px; }
#output-left { flex: 1; display: flex; flex-direction: column; min-width: 0; }
#output-log { flex: 1; overflow: auto; font-family: monospace; font-size: 11px; line-height: 1.5; }
.output-search {
  margin-top: 4px; background: var(--bg); border: 1px solid var(--border); border-radius: 3px;
  color: var(--text); font-size: 11px; padding: 3px 6px; outline: none; flex-shrink: 0;
}
.output-search:focus { border-color: var(--accent); }
.output-search::placeholder { color: var(--text-dim); }
#output-toolbar {
  display: flex; flex-direction: column; align-items: stretch; gap: 3px; flex-shrink: 0;
  padding: 2px 4px; border-left: 1px solid var(--border); min-width: 56px;
}
.output-tool-row { display: flex; gap: 2px; justify-content: center; }
.output-tool-sep { height: 1px; background: var(--border); margin: 2px 2px; }
.output-tool-btn, .output-filter-btn {
  background: transparent; border: 1px solid transparent; border-radius: 3px;
  color: var(--text-dim); font-size: 13px; padding: 3px 5px; cursor: pointer; line-height: 1;
}
.output-tool-btn:hover, .output-filter-btn:hover { background: var(--hover); color: var(--text); }
.output-tool-btn.active { color: var(--accent); background: var(--selected); }
.output-filter-btn { display: flex; align-items: center; gap: 5px; justify-content: space-between; font-size: 11px; }
.output-filter-btn:not(.active) { opacity: 0.4; }
.of-icon { font-size: 11px; }
.ofi-error { color: var(--error); }
.ofi-warn { color: #e0c050; }
.ofi-editor { color: var(--accent); }
.of-count { color: var(--text-dim); min-width: 14px; text-align: right; }
.log-entry { padding: 1px 0; }
.log-entry .log-time { color: var(--text-dim); margin-right: 8px; }
.log-entry .log-msg { color: var(--text); }
.log-entry.log-warn .log-msg { color: #e0c050; }
.log-entry.log-error .log-msg { color: var(--error); }
.log-entry.log-hidden { display: none; }
/* pat-d6uah: occurrence-count badge on collapsed duplicate log lines */
.log-entry .log-count { display: inline-block; margin-left: 8px; padding: 0 6px; font-size: 10px; border-radius: 8px; background: var(--selected); color: var(--text-dim); }
#scene-info { line-height: 1.8; }
.scene-info-row { display: flex; gap: 8px; }
.scene-info-label { color: var(--text-dim); width: 120px; }

/* Audio bus layout editor (Godot EditorAudioBuses parity) */
#audio-panel { display: flex; flex-direction: column; height: 100%; }
#audio-toolbar {
  display: flex; align-items: center; gap: 8px; padding: 2px 2px 6px; flex-shrink: 0;
  border-bottom: 1px solid var(--border);
}
#audio-layout-label { color: var(--text-dim); font-size: 11px; }
#audio-layout-path { color: var(--text); font-size: 11px; }
.audio-tb-spacer { flex: 1; }
.audio-tb-btn {
  background: var(--panel); border: 1px solid var(--border); border-radius: 3px;
  color: var(--text); font-size: 11px; padding: 3px 10px; cursor: pointer; font: inherit;
}
.audio-tb-btn:hover { background: var(--hover); }
#audio-buses {
  display: flex; gap: 6px; padding: 8px 2px; overflow-x: auto; flex: 1; align-items: stretch;
}
.audio-empty { color: var(--text-dim); font-size: 12px; padding: 8px; }
.audio-bus {
  display: flex; flex-direction: column; align-items: stretch; flex-shrink: 0;
  width: 116px; background: var(--panel); border: 1px solid var(--border);
  border-radius: 4px; padding: 6px;
}
.audio-bus.master { border-color: var(--accent); }
.audio-bus-name {
  text-align: center; font-size: 11px; font-weight: 600; color: var(--text);
  padding: 2px 4px; border: 1px solid transparent; border-radius: 3px;
  background: transparent; width: 100%; box-sizing: border-box; outline: none; font: inherit;
}
input.audio-bus-name { background: var(--bg); border-color: var(--border); cursor: text; }
input.audio-bus-name:focus { border-color: var(--accent); }
.audio-bus-toggles { display: flex; justify-content: center; gap: 3px; margin: 5px 0; }
.abus-toggle {
  width: 22px; height: 18px; font-size: 10px; font-weight: 700; line-height: 1;
  border: 1px solid var(--border); border-radius: 3px; background: var(--bg);
  color: var(--text-dim); cursor: pointer; padding: 0;
}
.abus-toggle:hover { color: var(--text); }
.abus-toggle.solo.active { background: #e0c050; color: #000; border-color: #e0c050; }
.abus-toggle.mute.active { background: var(--error); color: #fff; border-color: var(--error); }
.abus-toggle.bypass.active { background: #6a8caf; color: #000; border-color: #6a8caf; }
.audio-bus-meter-row { display: flex; justify-content: center; gap: 5px; margin: 4px 0; }
.abus-db-scale { display: flex; flex-direction: column; justify-content: space-between; height: 130px; font-size: 8px; color: var(--text-dim); text-align: right; line-height: 1; padding: 1px 0; }
.abus-menu { font-size: 12px; }
.abus-fader {
  -webkit-appearance: slider-vertical; appearance: slider-vertical;
  writing-mode: vertical-lr; direction: rtl;
  width: 20px; height: 130px; cursor: pointer; accent-color: var(--accent);
}
.abus-meter {
  width: 12px; height: 130px; background: var(--bg); border: 1px solid var(--border);
  border-radius: 2px; position: relative; overflow: hidden;
}
.abus-meter-fill {
  position: absolute; left: 0; right: 0; bottom: 0;
  background: linear-gradient(to top, #3fb950 0%, #3fb950 55%, #e0c050 80%, var(--error) 100%);
}
.abus-db { text-align: center; font-size: 10px; color: var(--text-dim); margin: 3px 0; }
.abus-effects { display: flex; flex-direction: column; gap: 2px; margin: 2px 0; }
.abus-effect {
  display: flex; align-items: center; gap: 4px; font-size: 10px; color: var(--text);
  background: var(--bg); border: 1px solid var(--border); border-radius: 3px; padding: 1px 4px;
}
.abus-effect.disabled .abus-effect-name { opacity: 0.45; text-decoration: line-through; }
.abus-effect-name { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.abus-effect-del { cursor: pointer; color: var(--text-dim); border: none; background: transparent; font-size: 12px; padding: 0 2px; }
.abus-effect-del:hover { color: var(--error); }
.abus-add-effect, .abus-output {
  width: 100%; box-sizing: border-box; font-size: 10px; margin-top: 4px; font: inherit;
  background: var(--bg); border: 1px solid var(--border); border-radius: 3px; color: var(--text); padding: 2px;
}
.abus-output-label { font-size: 9px; color: var(--text-dim); text-align: center; margin-top: 4px; }

/* Resize handle for bottom panel */
#bottom-resize-handle {
  height: 4px; cursor: ns-resize; background: transparent; flex-shrink: 0;
}
#bottom-resize-handle:hover { background: var(--accent); opacity: 0.3; }


/* Animation timeline panel */
#animation-panel { display: flex; flex-direction: column; height: 100%; }
.anim-toolbar {
  display: flex; align-items: center; gap: 4px; padding: 4px 0;
  border-bottom: 1px solid var(--border); flex-shrink: 0;
}
.anim-toolbar select { min-width: 120px; }
.anim-toolbar .anim-sep { width: 1px; height: 18px; background: var(--border); margin: 0 4px; }
.anim-record { color: #888; }
.anim-record.active { color: #e05050; }
#anim-time-display { color: var(--text-dim); font-size: 11px; margin-left: auto; font-family: monospace; }
.anim-timeline-area { display: flex; flex: 1; overflow: hidden; min-height: 50px; }
.anim-tracks {
  width: 160px; min-width: 100px; border-right: 1px solid var(--border);
  overflow-y: auto; flex-shrink: 0;
}
.anim-empty { color: var(--text-dim); font-style: italic; padding: 8px; font-size: 11px; }
.anim-track-row {
  display: flex; align-items: center; padding: 3px 6px; font-size: 11px;
  border-bottom: 1px solid var(--border); height: 24px; gap: 4px;
}
.anim-track-node { color: var(--accent); }
.anim-track-prop { color: var(--text-dim); }
.anim-track-type-badge {
  display: inline-block; width: 14px; text-align: center; font-weight: bold;
  font-size: 10px; margin-right: 4px; flex-shrink: 0;
}
.anim-track-delete-btn {
  margin-left: auto; background: none; border: none; color: var(--text-dim);
  cursor: pointer; font-size: 10px; padding: 0 4px; opacity: 0; transition: opacity 0.15s;
}
.anim-track-row:hover .anim-track-delete-btn { opacity: 1; }
.anim-track-delete-btn:hover { color: #e05050; }
/* Add track dialog */
.dialog-overlay {
  position: fixed; top: 0; left: 0; width: 100%; height: 100%;
  background: rgba(0,0,0,0.5); z-index: 1000; display: flex;
  align-items: center; justify-content: center;
}
.dialog-box {
  background: var(--panel, #161616); border: 1px solid var(--border, #2a2a2a);
  border-radius: 6px; padding: 16px; min-width: 320px; max-width: 400px;
  box-shadow: 0 8px 32px rgba(0,0,0,0.6);
}
.dialog-title { font-size: 13px; font-weight: bold; margin-bottom: 12px; color: var(--text, #ccc); }
.dialog-field { margin-bottom: 10px; }
.dialog-field label { display: block; font-size: 11px; color: var(--text-dim, #888); margin-bottom: 4px; }
.dialog-field input {
  width: 100%; box-sizing: border-box; padding: 5px 8px; font-size: 12px;
  background: var(--bg, #0d0d0d); border: 1px solid var(--border, #2a2a2a);
  color: var(--text, #ccc); border-radius: 3px;
}
.dialog-field input:focus { border-color: var(--accent, #579eff); outline: none; }
.dialog-track-type-btns { display: flex; gap: 4px; }
.track-type-btn {
  flex: 1; padding: 5px 0; font-size: 11px; border: 1px solid var(--border, #2a2a2a);
  background: var(--bg, #0d0d0d); color: var(--text-dim, #888); cursor: pointer;
  border-radius: 3px; transition: all 0.15s;
}
.track-type-btn:hover { border-color: var(--text-dim, #888); }
.track-type-btn.selected { border-color: var(--accent, #579eff); color: var(--accent, #579eff); background: rgba(87,158,255,0.1); }
.dialog-hint { font-size: 10px; color: var(--text-dim, #888); font-style: italic; margin-bottom: 12px; }
.dialog-actions { display: flex; justify-content: flex-end; gap: 8px; }
.dialog-actions button {
  padding: 5px 14px; font-size: 11px; border: 1px solid var(--border, #2a2a2a);
  background: var(--bg, #0d0d0d); color: var(--text, #ccc); cursor: pointer;
  border-radius: 3px;
}
.dialog-actions button.primary { border-color: var(--accent, #579eff); color: var(--accent, #579eff); }
.dialog-actions button:hover { background: rgba(255,255,255,0.05); }
.anim-timeline { flex: 1; position: relative; overflow-x: auto; overflow-y: hidden; }
#anim-timeline-canvas { display: block; cursor: crosshair; }
.anim-playhead {
  position: absolute; top: 0; width: 2px; height: 100%;
  background: var(--accent); pointer-events: none; left: 0;
}
.anim-add-track-bar { padding: 4px 0; border-top: 1px solid var(--border); flex-shrink: 0; }
.anim-add-track-bar button { font-size: 11px; }
/* Recording mode indicator */
body.anim-recording #viewport-container { box-shadow: inset 0 0 0 2px #e05050; }

/* Curve editor popup */
.curve-editor-overlay {
  display: none; position: fixed; top: 0; left: 0; width: 100%; height: 100%;
  background: rgba(0,0,0,0.4); z-index: 500; align-items: center; justify-content: center;
}
.curve-editor-overlay.open { display: flex; }
.curve-editor-popup {
  background: var(--panel); border: 1px solid var(--border); border-radius: 6px;
  padding: 12px; width: 340px; box-shadow: 0 8px 24px rgba(0,0,0,0.6);
}
.curve-editor-popup h3 { margin: 0 0 8px; font-size: 13px; color: var(--accent); }
.curve-canvas-wrap { position: relative; width: 200px; height: 200px; margin: 0 auto 8px; }
#curve-editor-canvas {
  display: block; width: 200px; height: 200px; border: 1px solid var(--border);
  border-radius: 3px; cursor: crosshair; background: var(--bg);
}
.curve-presets { display: flex; flex-wrap: wrap; gap: 4px; margin-bottom: 8px; }
.curve-presets button { font-size: 10px; padding: 2px 6px; }
.curve-inputs { display: flex; gap: 6px; margin-bottom: 8px; align-items: center; }
.curve-inputs label { font-size: 11px; color: var(--text-dim); }
.curve-inputs input { width: 50px; font-size: 11px; }
.curve-actions { display: flex; gap: 6px; justify-content: flex-end; }

/* Status bar */
#statusbar {
  display: flex; align-items: center; gap: 16px; padding: 4px 10px;
  background: var(--panel); border-top: 1px solid var(--border); font-size: 11px;
  color: var(--text-dim); flex-shrink: 0;
}
#statusbar .accent { color: var(--accent); }

/* Left panel split: scene tree + filesystem */
#left-panel {
  width: 240px; min-width: 160px; display: flex; flex-direction: column;
  flex-shrink: 0; border-right: 1px solid var(--border);
}
#left-divider {
  height: 4px; cursor: ns-resize; background: var(--border); flex-shrink: 0;
}
#left-divider:hover { background: var(--accent); opacity: 0.5; }

/* FileSystem dock */
#filesystem-panel {
  background: var(--panel); display: flex; flex-direction: column; flex: 1; min-height: 80px; overflow: hidden;
}
#filesystem-panel .panel-header {
  padding: 6px 10px; font-weight: bold; font-size: 11px; text-transform: uppercase;
  color: var(--text-dim); border-bottom: 1px solid var(--border); letter-spacing: 0.5px;
  display: flex; justify-content: space-between; align-items: center;
}
#filesystem-panel .panel-header .fs-path { font-weight: normal; font-size: 10px; color: var(--accent); }
#fs-chrome { flex: 1; display: flex; flex-direction: column; min-height: 0; overflow: hidden; }
/* pat-96yv4: split box holds the tree and (in split modes) a separate file list.
   Godot's FileSystemDock toggles DISPLAY_MODE_TREE_ONLY / VSPLIT / HSPLIT. */
#fs-split-box { flex: 1; display: flex; flex-direction: column; min-height: 0; overflow: hidden; }
#fs-tree { flex: 1; overflow: auto; padding: 4px 0; }
#fs-file-list { display: none; overflow: auto; padding: 4px 0; }
/* VSPLIT: tree on top, file list below. */
#fs-chrome.fs-mode-vsplit #fs-tree { flex: 0 0 45%; border-bottom: 1px solid var(--border); }
#fs-chrome.fs-mode-vsplit #fs-file-list { display: block; flex: 1; }
/* HSPLIT: tree beside the file list. */
#fs-chrome.fs-mode-hsplit #fs-split-box { flex-direction: row; }
#fs-chrome.fs-mode-hsplit #fs-tree { flex: 0 0 50%; border-right: 1px solid var(--border); }
#fs-chrome.fs-mode-hsplit #fs-file-list { display: block; flex: 1; }
.fs-file-row { display: flex; align-items: center; gap: 6px; padding: 2px 8px; cursor: pointer; font-size: 12px; }
.fs-file-row:hover { background: var(--hover); }
.fs-file-row.selected { background: var(--accent); color: #fff; }
.fs-file-empty { padding: 6px 8px; font-size: 11px; color: var(--text-dim); }

/* FileSystem dock toolbars (Godot parity): nav + path + split, then filter + sort */
#fs-toolbar, #fs-toolbar2 {
  display: flex; align-items: center; gap: 3px; padding: 3px 4px; flex-shrink: 0;
  border-bottom: 1px solid var(--border);
}
.fs-tool-btn {
  background: transparent; border: 1px solid transparent; border-radius: 3px;
  color: var(--text-dim); font-size: 12px; line-height: 1; padding: 2px 6px;
  cursor: pointer; flex-shrink: 0;
}
.fs-tool-btn:hover:not(:disabled) { background: var(--hover); color: var(--text); border-color: var(--border); }
.fs-tool-btn.active { color: var(--accent); }
.fs-tool-btn:disabled { opacity: 0.35; cursor: default; }
.fs-path-input, .fs-filter-input {
  flex: 1; min-width: 0; background: var(--bg); border: 1px solid var(--border);
  border-radius: 3px; color: var(--text); font-size: 11px; padding: 3px 6px; outline: none;
}
.fs-path-input:focus, .fs-filter-input:focus { border-color: var(--accent); }
.fs-filter-input::placeholder { color: var(--text-dim); }

/* Always-present Favorites section (Godot shows this header even when empty) */
#fs-favorites { flex-shrink: 0; }
.fs-fav-header {
  display: flex; align-items: center; gap: 5px; padding: 3px 8px; cursor: pointer;
  font-size: 12px; color: var(--text); user-select: none;
}
.fs-fav-header:hover { background: var(--hover); }
.fs-fav-toggle { width: 12px; font-size: 9px; color: var(--text-dim); flex-shrink: 0; }
.fs-fav-star { color: var(--accent); font-size: 12px; flex-shrink: 0; }
.fs-fav-label { color: var(--text-dim); }
#fs-fav-list { padding-left: 6px; }
#fs-fav-list:empty { display: none; }
.fs-fav-item {
  display: flex; align-items: center; gap: 6px; padding: 3px 8px 3px 20px;
  font-size: 12px; color: var(--text); cursor: pointer; white-space: nowrap;
}
.fs-fav-item:hover { background: var(--hover); }
#fs-favorites.collapsed #fs-fav-list { display: none; }
#fs-favorites.collapsed .fs-fav-toggle { transform: none; }

/* Godot-style tabbed dock headers (Scene / FileSystem docks) */
#scene-panel > .panel-header, #filesystem-panel > .panel-header {
  padding: 0 6px; height: 28px; background: var(--bg); border-bottom: 1px solid var(--border);
  display: flex; align-items: flex-end; gap: 2px; text-transform: none;
  letter-spacing: 0; font-weight: normal;
}
.dock-tab {
  padding: 5px 12px; font-size: 11px; color: var(--text-dim); background: transparent;
  border-bottom: 2px solid transparent; white-space: nowrap; border-radius: 3px 3px 0 0;
}
.dock-tab.active { color: var(--text); background: var(--panel); border-bottom-color: var(--accent); }
#fs-preview {
  border-top: 1px solid var(--border); padding: 6px; max-height: 200px;
  overflow: auto; display: none; flex-shrink: 0; background: var(--bg);
}
#fs-preview.active { display: block; }

/* History dock (tabbed with FileSystem, bottom-left — Godot parity) */
#history-dock { flex: 1; flex-direction: column; overflow: hidden; min-height: 0; }
#history-filters {
  display: flex; gap: 16px; padding: 6px 10px; flex-shrink: 0;
  border-bottom: 1px solid var(--border); font-size: 11px; color: var(--text-dim);
}
#history-filters label { display: flex; align-items: center; gap: 5px; cursor: pointer; user-select: none; }
#history-filters input { cursor: pointer; }
#history-list { flex: 1; overflow: auto; padding: 2px 0; }
.history-entry {
  display: flex; align-items: center; gap: 6px; padding: 3px 10px; font-size: 12px;
  color: var(--text); cursor: pointer; white-space: nowrap; border-left: 2px solid transparent;
}
.history-entry:hover { background: var(--hover); }
.history-entry.current { background: var(--selected); border-left-color: var(--accent); }
.history-entry.future { color: var(--text-dim); font-style: italic; }
.history-entry .history-icon { width: 10px; font-size: 9px; flex-shrink: 0; color: var(--accent); }
.history-entry.beginning .history-label { color: var(--text-dim); }
#fs-preview .preview-header {
  font-size: 10px; color: var(--text-dim); text-transform: uppercase;
  margin-bottom: 4px; letter-spacing: 0.5px;
}
#fs-preview .preview-img {
  max-width: 100%; max-height: 140px; object-fit: contain;
  border-radius: 2px; background: repeating-conic-gradient(#333 0% 25%, #444 0% 50%) 50%/16px 16px;
}
#fs-preview .preview-code {
  font-family: monospace; font-size: 10px; white-space: pre; color: var(--text);
  overflow: hidden; line-height: 1.4; max-height: 120px;
}
#fs-preview .preview-info {
  font-size: 11px; color: var(--text); line-height: 1.6;
}
#fs-preview .preview-info span { color: var(--text-dim); }
.fs-node { user-select: none; }
.fs-row {
  display: flex; align-items: center; padding: 2px 8px; cursor: pointer;
  white-space: nowrap; gap: 4px; font-size: 12px;
}
.fs-row:hover { background: var(--hover); }
.fs-row.selected { background: var(--selected); color: var(--accent); }
.fs-row .fs-dragging { opacity: 0.5; }
/* Viewport drop indicator */
#viewport-container.viewport-drop-active { box-shadow: inset 0 0 0 3px var(--accent); }
/* Inspector property drop indicator */
.nodepath-editor.insp-drop-active { background: rgba(87,158,255,0.15); border: 1px dashed var(--accent); border-radius: 3px; }
.fs-toggle { width: 14px; text-align: center; font-size: 10px; color: var(--text-dim); flex-shrink: 0; cursor: pointer; }
.fs-icon { font-size: 12px; flex-shrink: 0; width: 16px; text-align: center; }
.fs-name { flex: 1; overflow: hidden; text-overflow: ellipsis; }

/* Scene tabs */
#scene-tabs {
  display: flex; align-items: center; background: #1a1a1a; border-bottom: 1px solid var(--border);
  flex-shrink: 0; min-height: 28px; padding: 0 4px; gap: 0; overflow-x: auto;
}
.scene-tab {
  display: flex; align-items: center; padding: 4px 14px; font-size: 12px; cursor: pointer;
  color: var(--text-dim); border: none; background: transparent; border-bottom: 2px solid transparent;
  font: inherit; white-space: nowrap; gap: 4px; flex-shrink: 0;
}
.scene-tab:hover { color: var(--text); background: var(--hover); }
.scene-tab.active { color: var(--text); background: var(--panel); border-bottom-color: var(--accent); }
.scene-tab .modified-indicator { color: var(--accent); font-size: 14px; }

/* Add node dialog */
/* Create New Node dialog — 1:1 with Godot's CreateDialog layout:
   Favorites/Recent split on the left, Search + Matches tree + Description
   on the right (editor/gui/create_dialog.cpp). */
#add-node-dialog {
  display: none; position: fixed; top: 0; left: 0; width: 100%; height: 100%;
  background: rgba(0,0,0,0.5); z-index: 300; align-items: center; justify-content: center;
}
#add-node-dialog.open { display: flex; }
#add-node-dialog-inner {
  background: var(--panel); border: 1px solid var(--border); border-radius: 6px;
  width: 900px; max-width: 80vw; height: 700px; max-height: 80vh;
  display: flex; flex-direction: column; box-shadow: 0 8px 32px rgba(0,0,0,0.6);
}
#add-node-dialog-header {
  padding: 10px 14px; font-weight: bold; font-size: 13px; border-bottom: 1px solid var(--border);
  display: flex; justify-content: space-between; align-items: center;
}
#add-node-dialog-header button {
  background: transparent; border: none; color: var(--text-dim); cursor: pointer; font-size: 16px; padding: 0 4px;
}
#add-node-dialog-header button:hover { color: var(--text); background: transparent; border: none; }
#add-node-body { flex: 1; display: flex; gap: 12px; padding: 10px 14px; min-height: 0; }
#add-node-left { width: 190px; flex-shrink: 0; display: flex; flex-direction: column; gap: 4px; min-height: 0; }
.add-node-pane-label { font-size: 11px; font-weight: bold; color: var(--text); padding: 2px 0; }
#add-node-favorites, #add-node-recent {
  flex: 1; overflow: auto; background: var(--bg); border: 1px solid var(--border);
  border-radius: 3px; min-height: 60px;
}
.add-node-side-item { display: flex; align-items: center; gap: 6px; padding: 3px 8px; cursor: pointer; font-size: 12px; color: var(--text); }
.add-node-side-item:hover { background: var(--hover); }
.add-node-side-item.selected { background: var(--selected); }
.add-node-side-item.drag-over { border-top: 2px solid var(--accent); }
#add-node-right { flex: 1; display: flex; flex-direction: column; gap: 4px; min-width: 0; min-height: 0; }
#add-node-search-row { display: flex; gap: 6px; align-items: center; }
#add-node-search-wrap { flex: 1; position: relative; }
#add-node-search {
  width: 100%; box-sizing: border-box; padding: 6px 26px 6px 10px; font-size: 12px; border-radius: 3px;
  background: var(--bg); color: var(--text); border: 1px solid var(--border);
}
#add-node-search:focus { border-color: var(--accent); outline: none; }
#add-node-search-clear {
  position: absolute; right: 4px; top: 50%; transform: translateY(-50%);
  background: transparent; border: none; color: var(--text-dim); cursor: pointer;
  font-size: 13px; padding: 0 4px; display: none;
}
#add-node-search-clear:hover { color: var(--text); }
#add-node-search-wrap.has-text #add-node-search-clear { display: block; }
#add-node-fav-btn {
  background: var(--bg); border: 1px solid var(--border); border-radius: 3px;
  color: var(--text-dim); cursor: pointer; font-size: 13px; padding: 4px 9px; flex-shrink: 0;
}
#add-node-fav-btn.pressed { color: #e0c050; }
#add-node-fav-btn:disabled { opacity: 0.4; cursor: default; }
.add-node-row { display: flex; align-items: center; gap: 4px; padding: 2px 6px; cursor: pointer; font-size: 12px; white-space: nowrap; color: var(--text); }
.add-node-row:hover { background: var(--hover); }
.add-node-row.selected { background: var(--selected); }
.add-node-row.disabled-class > .add-node-name { color: var(--text-dim); }
.add-node-row .add-node-twist { width: 14px; text-align: center; color: var(--text-dim); font-size: 9px; flex-shrink: 0; }
.add-node-row .node-type-icon { width: 16px; text-align: center; font-size: 12px; flex-shrink: 0; }
#add-node-description {
  height: 96px; flex-shrink: 0; overflow: auto; background: var(--bg);
  border: 1px solid var(--border); border-radius: 3px; padding: 8px 10px;
  font-size: 11px; color: var(--text-dim); line-height: 1.5;
}
#add-node-description .desc-class-chain { color: var(--text); margin-bottom: 4px; }
#add-node-description .desc-class-chain b { color: var(--accent); }
/* Open Scene picker dialog */
#open-scene-dialog { display: none; position: fixed; top: 0; left: 0; width: 100%; height: 100%; background: rgba(0,0,0,0.5); z-index: 320; align-items: center; justify-content: center; }
#open-scene-dialog.open { display: flex; }
#open-scene-inner { background: var(--panel); border: 1px solid var(--border); border-radius: 6px; width: 480px; max-height: 520px; display: flex; flex-direction: column; box-shadow: 0 8px 32px rgba(0,0,0,0.6); }
#open-scene-header { padding: 10px 14px; font-weight: bold; font-size: 13px; border-bottom: 1px solid var(--border); display: flex; justify-content: space-between; align-items: center; }
#open-scene-header button { background: transparent; border: none; color: var(--text-dim); cursor: pointer; font-size: 16px; padding: 0 4px; }
#open-scene-header button:hover { color: var(--text); background: transparent; }
#open-scene-filter { margin: 8px 12px 4px; padding: 6px 10px; font-size: 12px; border-radius: 3px; background: var(--bg); color: var(--text); border: 1px solid var(--border); }
#open-scene-filter:focus { border-color: var(--accent); }
#open-scene-list { flex: 1; overflow: auto; padding: 4px 6px; min-height: 200px; max-height: 360px; }
.open-scene-item { display: flex; align-items: center; gap: 8px; padding: 6px 8px; border-radius: 3px; cursor: pointer; }
.open-scene-item:hover { background: var(--hover); }
.open-scene-item .osi-icon { color: var(--icon-node2d); font-size: 13px; flex-shrink: 0; }
.open-scene-item .osi-name { color: var(--text); font-size: 12px; }
.open-scene-item .osi-path { color: var(--text-dim); font-size: 10px; margin-left: auto; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 250px; }
#open-scene-footer { padding: 8px 14px; border-top: 1px solid var(--border); display: flex; justify-content: flex-end; gap: 6px; }
/* Save Scene As dialog (Godot EditorFileDialog FILE_MODE_SAVE_FILE) */
#save-scene-dialog { display: none; position: fixed; top: 0; left: 0; width: 100%; height: 100%; background: rgba(0,0,0,0.5); z-index: 320; align-items: center; justify-content: center; }
#save-scene-dialog.open { display: flex; }
#save-scene-inner { background: var(--panel); border: 1px solid var(--border); border-radius: 6px; width: 480px; max-height: 560px; display: flex; flex-direction: column; box-shadow: 0 8px 32px rgba(0,0,0,0.6); }
#save-scene-header { padding: 10px 14px; font-weight: bold; font-size: 13px; border-bottom: 1px solid var(--border); display: flex; justify-content: space-between; align-items: center; }
#save-scene-header button { background: transparent; border: none; color: var(--text-dim); cursor: pointer; font-size: 16px; padding: 0 4px; }
#save-scene-header button:hover { color: var(--text); }
#save-scene-path { padding: 6px 14px; font-size: 11px; color: var(--text-dim); border-bottom: 1px solid var(--border); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
#save-scene-list { flex: 1; overflow: auto; padding: 4px 6px; min-height: 180px; max-height: 300px; }
.save-scene-item { display: flex; align-items: center; gap: 8px; padding: 6px 8px; border-radius: 3px; cursor: pointer; }
.save-scene-item:hover { background: var(--hover); }
.save-scene-item .ssi-icon { font-size: 13px; flex-shrink: 0; }
.save-scene-item.ssi-dir .ssi-icon { color: var(--icon-node); }
.save-scene-item.ssi-file .ssi-icon { color: var(--icon-node2d); }
.save-scene-item .ssi-name { color: var(--text); font-size: 12px; }
#save-scene-name-row { display: flex; align-items: center; gap: 8px; padding: 8px 14px 4px; }
#save-scene-name-row label { font-size: 12px; color: var(--text-dim); }
#save-scene-name { flex: 1; padding: 6px 10px; font-size: 12px; border-radius: 3px; background: var(--bg); color: var(--text); border: 1px solid var(--border); }
#save-scene-name:focus { border-color: var(--accent); }
#save-scene-error { padding: 0 14px; min-height: 14px; font-size: 11px; color: var(--danger, #e06c75); }
#save-scene-footer { padding: 8px 14px; border-top: 1px solid var(--border); display: flex; justify-content: flex-end; gap: 6px; }
#save-scene-confirm { border-color: var(--accent); color: var(--accent); }
/* Open Project dialog (pat-v89tw): root the editor at a chosen game project dir */
#open-project-dialog { display: none; position: fixed; top: 0; left: 0; width: 100%; height: 100%; background: rgba(0,0,0,0.5); z-index: 320; align-items: center; justify-content: center; }
#open-project-dialog.open { display: flex; }
#open-project-inner { background: var(--panel); border: 1px solid var(--border); border-radius: 6px; width: 480px; display: flex; flex-direction: column; box-shadow: 0 8px 32px rgba(0,0,0,0.6); }
#open-project-header { padding: 10px 14px; font-weight: bold; font-size: 13px; border-bottom: 1px solid var(--border); display: flex; justify-content: space-between; align-items: center; }
#open-project-header button { background: transparent; border: none; color: var(--text-dim); cursor: pointer; font-size: 16px; padding: 0 4px; }
#open-project-header button:hover { color: var(--text); background: transparent; }
#open-project-body { padding: 12px 14px; display: flex; flex-direction: column; gap: 8px; }
#open-project-hint { color: var(--text-dim); font-size: 11px; }
#open-project-path { padding: 6px 10px; font-size: 12px; border-radius: 3px; background: var(--bg); color: var(--text); border: 1px solid var(--border); }
#open-project-path:focus { border-color: var(--accent); }
#open-project-error { color: var(--danger, #e05252); font-size: 11px; min-height: 14px; }
#open-project-footer { padding: 8px 14px; border-top: 1px solid var(--border); display: flex; justify-content: flex-end; gap: 6px; }
#add-node-list {
  flex: 1; overflow: auto; padding: 2px 0; min-height: 120px;
  background: var(--bg); border: 1px solid var(--border); border-radius: 3px;
}
#add-node-dialog-footer {
  padding: 10px 14px; border-top: 1px solid var(--border); display: flex;
  justify-content: center; gap: 40px;
}
#add-node-dialog-footer button { min-width: 100px; }
#add-node-create:disabled { opacity: 0.4; cursor: default; }

/* Play buttons */
.play-buttons {
  display: flex; align-items: center; gap: 1px;
}
.play-btn {
  padding: 4px 7px; font-size: 13px; min-width: 28px; text-align: center; border-radius: 3px;
  background: transparent; border: 1px solid transparent;
}
.play-btn:hover { background: var(--hover); border-color: var(--border); }
.play-btn.play-main { color: #50c878; }
.play-btn.play-main:hover { background: rgba(80,200,120,0.1); }
.play-btn.pause-btn { color: #e0c050; }
.play-btn.pause-btn:hover { background: rgba(224,192,80,0.1); }
.play-btn.stop-btn { color: var(--error); }
.play-btn.stop-btn:hover { background: rgba(224,80,80,0.1); }
.play-btn.play-current { color: #8ebbff; }
.play-btn.play-current:hover { background: rgba(142,187,255,0.1); }

/* Scrollbar styling */
::-webkit-scrollbar { width: 6px; height: 6px; }
::-webkit-scrollbar-track { background: var(--bg); }
::-webkit-scrollbar-thumb { background: var(--border); border-radius: 3px; }
::-webkit-scrollbar-thumb:hover { background: #444; }
/* Inspector gets a wider, always-visible scrollbar so it's clear it scrolls. */
#inspector::-webkit-scrollbar { width: 11px; }
#inspector::-webkit-scrollbar-track { background: var(--panel); }
#inspector::-webkit-scrollbar-thumb { background: #4d4d4d; border-radius: 6px; border: 2px solid var(--panel); }
#inspector::-webkit-scrollbar-thumb:hover { background: #606060; }

/* Help dialog */
#help-dialog {
  display: none; position: fixed; top: 0; left: 0; width: 100%; height: 100%;
  background: rgba(0,0,0,0.6); z-index: 300; align-items: center; justify-content: center;
}
#help-dialog.open { display: flex; }
#help-dialog-inner {
  background: var(--panel); border: 1px solid var(--border); border-radius: 6px;
  width: 520px; max-height: 80vh; display: flex; flex-direction: column;
  box-shadow: 0 8px 32px rgba(0,0,0,0.5);
}
#help-dialog-header {
  padding: 10px 14px; font-weight: bold; font-size: 13px; border-bottom: 1px solid var(--border);
  display: flex; justify-content: space-between; align-items: center;
}
#help-dialog-header button {
  background: transparent; border: none; color: var(--text-dim); cursor: pointer; font-size: 16px; padding: 0 4px;
}
#help-dialog-header button:hover { color: var(--text); background: transparent; border: none; }
#help-dialog-body { overflow: auto; padding: 10px 14px; }
.help-category { margin-bottom: 12px; }
.help-category-title { font-weight: bold; font-size: 11px; text-transform: uppercase; color: var(--accent); margin-bottom: 4px; letter-spacing: 0.5px; }
.help-row { display: flex; justify-content: space-between; padding: 2px 0; font-size: 12px; }
.help-key { color: var(--text); font-family: monospace; background: var(--bg); padding: 1px 6px; border-radius: 2px; border: 1px solid var(--border); font-size: 11px; }
.help-desc { color: var(--text-dim); }

/* Confirmation dialog (Godot-style ConfirmationDialog) */
#confirm-dialog {
  display: none; position: fixed; top: 0; left: 0; width: 100%; height: 100%;
  background: rgba(0,0,0,0.5); z-index: 500; align-items: center; justify-content: center;
}
#confirm-dialog.open { display: flex; }
#confirm-dialog-inner {
  background: var(--panel); border: 1px solid var(--border); border-radius: 6px;
  min-width: 340px; max-width: 460px; display: flex; flex-direction: column;
  box-shadow: 0 10px 40px rgba(0,0,0,0.55);
}
#confirm-dialog-header {
  padding: 8px 12px; font-weight: bold; font-size: 12px; border-bottom: 1px solid var(--border);
  display: flex; justify-content: space-between; align-items: center; color: var(--text-dim);
  letter-spacing: 0.3px;
}
#confirm-dialog-header button {
  background: transparent; border: none; color: var(--text-dim); cursor: pointer; font-size: 16px; padding: 0 4px; line-height: 1;
}
#confirm-dialog-header button:hover { color: var(--text); background: transparent; border: none; }
#confirm-dialog-body {
  padding: 26px 22px; font-size: 13px; color: var(--text); text-align: center; line-height: 1.5; word-break: break-word;
}
#confirm-dialog-footer {
  padding: 12px; border-top: 1px solid var(--border); display: flex; justify-content: center; gap: 10px;
}
#confirm-dialog-footer button { padding: 5px 20px; font-size: 12px; min-width: 92px; }
#confirm-dialog-footer .confirm-primary { border-color: var(--accent); color: var(--accent); font-weight: bold; }
#confirm-dialog-footer .confirm-primary:hover { background: var(--selected); border-color: var(--accent); }
#confirm-dialog-footer .confirm-primary:focus { border-color: var(--accent); box-shadow: 0 0 0 1px var(--accent); }
#confirm-dialog-footer .confirm-primary.confirm-danger { border-color: var(--error); color: var(--error); }
#confirm-dialog-footer .confirm-primary.confirm-danger:hover { background: rgba(224,80,80,0.12); border-color: var(--error); }
#confirm-dialog-footer .confirm-primary.confirm-danger:focus { box-shadow: 0 0 0 1px var(--error); }

/* Inspector/Node tabs */
.right-panel-tabs {
  display: flex; border-bottom: 1px solid var(--border); flex-shrink: 0;
}
.right-panel-tab {
  padding: 5px 14px; font-size: 12px; cursor: pointer; color: var(--text-dim);
  border: none; background: transparent; border-bottom: 2px solid transparent;
  font: inherit;
}
.right-panel-tab:hover { color: var(--text); background: transparent; border-color: transparent; }
.right-panel-tab.active { color: var(--text); border-bottom-color: var(--accent); }
.right-panel-content { display: none; flex: 1; overflow: auto; }
.right-panel-content.active { display: flex; flex-direction: column; }

/* Signals panel */
.signal-row {
  display: flex; align-items: center; padding: 3px 8px; gap: 6px; font-size: 12px;
}
.signal-row:hover { background: var(--hover); }
.signal-icon { font-size: 12px; flex-shrink: 0; }
.signal-icon.connected { color: #50c878; }
.signal-icon.disconnected { color: var(--text-dim); }
.signal-name { flex: 1; }
.signal-connect-btn {
  padding: 2px 6px; font-size: 10px; opacity: 0; transition: opacity 0.15s;
}
.signal-row:hover .signal-connect-btn { opacity: 1; }
.signal-connection-row { cursor: context-menu; }
.signal-connection-row:hover { background: var(--hover); }
/* pat-9zrvd: Filter Signals box with a right-aligned Search icon */
.signal-search-wrap { position: relative; margin: 4px 8px; }
.signal-search {
  width: 100%; box-sizing: border-box; padding: 4px 24px 4px 8px; font-size: 12px;
  border-radius: 3px; background: var(--bg); color: var(--text); border: 1px solid var(--border);
}
.signal-search:focus { border-color: var(--accent); outline: none; }
.signal-search::placeholder { color: var(--text-dim); }
.signal-search-icon {
  position: absolute; right: 6px; top: 50%; transform: translateY(-50%);
  font-size: 11px; color: var(--text-dim); pointer-events: none;
}

/* Groups panel */
.groups-section { padding: 4px 8px; }
.group-tag {
  display: inline-flex; align-items: center; gap: 4px;
  background: var(--bg); border: 1px solid var(--border); border-radius: 3px;
  padding: 2px 8px; margin: 2px 4px 2px 0; font-size: 11px;
}
.group-tag .group-remove {
  cursor: pointer; color: var(--text-dim); font-size: 10px; padding: 0 2px;
}
.group-tag .group-remove:hover { color: var(--error); }
.group-add-row { display: flex; gap: 4px; margin-top: 4px; }
.group-add-row input { flex: 1; font-size: 11px; }
.group-add-row button { font-size: 11px; padding: 2px 8px; }
/* pat-3uujp: Groups tab (Godot GroupsEditor parity) */
.groups-toolbar { display: flex; align-items: center; gap: 4px; margin-bottom: 6px; }
.group-add-btn {
  background: transparent; border: 1px solid transparent; border-radius: 3px;
  color: var(--text-dim); font-size: 13px; line-height: 1; padding: 2px 6px; cursor: pointer; flex-shrink: 0;
}
.group-add-btn:hover { background: var(--hover); color: var(--text); border-color: var(--border); }
.group-filter-wrap { position: relative; flex: 1; }
.group-filter {
  width: 100%; box-sizing: border-box; padding: 4px 24px 4px 8px; font-size: 12px;
  border-radius: 3px; background: var(--bg); color: var(--text); border: 1px solid var(--border);
}
.group-filter:focus { border-color: var(--accent); outline: none; }
.group-filter::placeholder { color: var(--text-dim); }
.group-filter-icon { position: absolute; right: 6px; top: 50%; transform: translateY(-50%); font-size: 11px; color: var(--text-dim); pointer-events: none; }
.group-tree-header { font-size: 11px; font-weight: 600; color: var(--text-dim); text-transform: uppercase; padding: 4px 2px 2px; margin-top: 4px; }
.group-row { display: flex; align-items: center; gap: 6px; padding: 2px 4px; font-size: 12px; cursor: context-menu; }
.group-row:hover { background: var(--hover); }
.group-check { flex-shrink: 0; cursor: pointer; }
.group-row-name { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.group-copy-btn { background: transparent; border: none; color: var(--text-dim); cursor: pointer; font-size: 11px; opacity: 0; padding: 0 2px; }
.group-row:hover .group-copy-btn { opacity: 1; }
.group-copy-btn:hover { color: var(--text); }
.group-empty { font-size: 11px; color: var(--text-dim); padding: 2px 4px 4px; }

/* Connect dialog */
.connect-dialog-overlay {
  display: none; position: fixed; top: 0; left: 0; right: 0; bottom: 0;
  background: rgba(0,0,0,0.5); z-index: 300; align-items: center; justify-content: center;
}
.connect-dialog-overlay.open { display: flex; }
.connect-dialog {
  background: var(--panel); border: 1px solid var(--border); border-radius: 6px;
  padding: 16px; min-width: 320px; max-width: 400px; box-shadow: 0 8px 24px rgba(0,0,0,0.6);
}
.connect-dialog h3 { font-size: 13px; color: var(--accent); margin-bottom: 12px; }
.connect-dialog label { display: block; font-size: 11px; color: var(--text-dim); margin-bottom: 4px; }
.connect-dialog input, .connect-dialog select {
  width: 100%; margin-bottom: 10px; padding: 4px 8px;
}
.connect-dialog-buttons { display: flex; gap: 8px; justify-content: flex-end; margin-top: 8px; }

/* Script panel */
#script-panel { display: flex; flex-direction: column; height: 100%; }
.script-header {
  display: flex; align-items: center; gap: 6px; padding: 4px 8px;
  font-size: 11px; color: var(--text-dim); border-bottom: 1px solid var(--border); flex-shrink: 0;
}
.script-header .script-path { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.script-header .script-save-btn { padding: 2px 8px; font-size: 10px; border-radius: 2px; }
.script-header .script-save-btn:hover { border-color: var(--accent); color: var(--accent); }
.script-header .script-new-btn { padding: 2px 8px; font-size: 10px; border-radius: 2px; }
.script-header .script-new-btn:hover { border-color: var(--accent); color: var(--accent); }
.script-saved-indicator { color: #50c878; font-size: 10px; opacity: 0; transition: opacity 0.3s; }
.script-saved-indicator.visible { opacity: 1; }
.script-editor-wrap { position: relative; flex: 1; overflow: hidden; background: var(--bg); }
.script-line-numbers {
  position: absolute; top: 0; left: 0; width: 40px;
  font-family: 'SF Mono', 'Cascadia Code', 'Consolas', monospace;
  font-size: 12px; line-height: 1.6; color: var(--text-dim);
  text-align: right; padding: 4px 8px 4px 0; user-select: none;
  border-right: 1px solid var(--border); pointer-events: none; white-space: pre;
}
.script-highlight-layer {
  position: absolute; top: 0; left: 48px; right: 0;
  font-family: 'SF Mono', 'Cascadia Code', 'Consolas', monospace;
  font-size: 12px; line-height: 1.6; padding: 4px 8px; white-space: pre-wrap;
  word-wrap: break-word; pointer-events: none; color: transparent;
}
.script-textarea {
  position: absolute; top: 0; left: 48px; right: 0; bottom: 0;
  font-family: 'SF Mono', 'Cascadia Code', 'Consolas', monospace;
  font-size: 12px; line-height: 1.6; padding: 4px 8px;
  background: transparent; color: var(--text); border: none; outline: none;
  resize: none; white-space: pre; overflow: auto; tab-size: 4; caret-color: var(--accent);
}
.script-textarea::selection { background: rgba(87,158,255,0.3); }
.script-empty { color: var(--text-dim); font-style: italic; padding: 20px; text-align: center; }
/* pat-bc0tv: Shader Editor bottom-panel (Godot ShaderEditorPlugin parity).
   HSplitContainer: shader file list (left) + TextShaderEditor (right). */
#shader-panel { display: flex; flex-direction: row; height: 100%; overflow: hidden; }
/* files_split left: the ItemList of open shaders */
#shader-files { width: 200px; min-width: 90px; flex-shrink: 0; display: flex; flex-direction: column; background: var(--bg); border: 1px solid var(--border); border-radius: 3px; overflow: hidden; }
#shader-files.hidden { display: none; }
#shader-files-list { flex: 1; overflow-y: auto; padding: 2px; }
.shader-file-item { display: flex; align-items: center; gap: 6px; padding: 3px 6px; font-size: 12px; color: var(--text); cursor: pointer; border-radius: 3px; white-space: nowrap; overflow: hidden; }
.shader-file-item:hover { background: var(--hover); }
.shader-file-item.active { background: var(--selected); }
.shader-file-item .sfi-icon { flex-shrink: 0; color: #5b9bd5; font-size: 12px; width: 13px; text-align: center; }
.shader-file-item .sfi-name { flex: 1; overflow: hidden; text-overflow: ellipsis; }
.shader-file-item .sfi-err { flex-shrink: 0; color: var(--error); font-size: 11px; }
.shader-file-item .sfi-close { flex-shrink: 0; color: var(--text-dim); font-size: 13px; visibility: hidden; padding: 0 2px; }
.shader-file-item:hover .sfi-close { visibility: visible; }
.shader-file-item .sfi-close:hover { color: var(--error); }
.shader-files-empty { color: var(--text-dim); font-size: 11px; font-style: italic; padding: 8px 6px; }
#shader-split-handle { width: 5px; flex-shrink: 0; cursor: col-resize; background: transparent; }
#shader-split-handle:hover { background: var(--accent); opacity: 0.5; }
/* files_split right: the TextShaderEditor (menu bar + code + warnings + status) */
#shader-editor-main { flex: 1; display: flex; flex-direction: column; min-width: 0; margin-left: 4px; }
/* menu_bar_hbox: File / Edit / Search / Go To + spacer + Online Docs */
#shader-menubar { display: flex; align-items: center; gap: 1px; padding: 1px 2px; background: var(--panel); border: 1px solid var(--border); border-radius: 3px 3px 0 0; border-bottom: none; flex-shrink: 0; }
.shader-menu { position: relative; }
.shader-menu-btn, .shader-docs-btn { background: transparent; border: none; color: var(--text); font: inherit; font-size: 12px; padding: 3px 9px; border-radius: 3px; cursor: pointer; }
.shader-menu-btn:hover, .shader-menu.open .shader-menu-btn, .shader-docs-btn:hover { background: var(--hover); }
.shader-menu-spacer { flex: 1; }
.shader-docs-btn { color: var(--text-dim); }
.shader-menu-pop { display: none; position: absolute; top: 100%; left: 0; min-width: 210px; background: var(--panel); border: 1px solid var(--border); border-radius: 4px; box-shadow: 0 6px 18px rgba(0,0,0,0.5); padding: 3px 0; z-index: 60; }
.shader-menu.open > .shader-menu-pop { display: block; }
.shader-mi { display: flex; align-items: center; justify-content: space-between; gap: 24px; padding: 4px 12px; font-size: 12px; color: var(--text); cursor: pointer; white-space: nowrap; }
.shader-mi:hover { background: var(--accent); color: #06121f; }
.shader-mi.disabled { color: var(--text-dim); opacity: 0.45; pointer-events: none; }
.shader-mi-key { color: var(--text-dim); font-size: 11px; }
.shader-mi:hover .shader-mi-key { color: #06121f; }
.shader-mi-sep { height: 1px; background: var(--border); margin: 3px 6px; }
.shader-mi-sub { position: relative; }
.shader-submenu-pop { top: -4px; left: 100%; display: none; }
.shader-mi-sub:hover > .shader-submenu-pop { display: block; }
/* FindReplaceBar */
#shader-find-bar { display: none; align-items: center; gap: 4px; padding: 3px 4px; background: var(--panel); border: 1px solid var(--border); border-bottom: none; flex-shrink: 0; }
#shader-find-bar.open { display: flex; }
#shader-find-bar input { background: var(--bg); border: 1px solid var(--border); border-radius: 3px; color: var(--text); font-size: 12px; padding: 2px 6px; outline: none; }
#shader-find-bar input:focus { border-color: var(--accent); }
#shader-find-input, #shader-replace-input { width: 150px; }
.shader-find-btn { background: var(--bg); border: 1px solid var(--border); border-radius: 3px; color: var(--text); font-size: 11px; padding: 2px 7px; cursor: pointer; }
.shader-find-btn:hover { border-color: var(--accent); color: var(--accent); }
.shader-find-count { color: var(--text-dim); font-size: 11px; margin: 0 2px; }
/* code editor: gutter + highlighted textarea overlay */
#shader-code-area { flex: 1; display: flex; position: relative; background: var(--bg); border: 1px solid var(--border); overflow: hidden; min-height: 60px; }
#shader-gutter { flex-shrink: 0; overflow: hidden; padding: 6px 6px 6px 8px; text-align: right; color: var(--text-dim); background: var(--panel); border-right: 1px solid var(--border); font-family: monospace; font-size: 12px; line-height: 1.5; white-space: pre; user-select: none; }
#shader-gutter .sg-ln.bookmarked { color: #e0c050; }
#shader-gutter .sg-ln.errline { color: var(--error); font-weight: bold; }
#shader-code-wrap { flex: 1; position: relative; overflow: hidden; }
#shader-highlight, #shader-source {
  margin: 0; box-sizing: border-box; position: absolute; inset: 0; width: 100%; height: 100%;
  padding: 6px 8px; border: 0; font-family: monospace; font-size: 12px; line-height: 1.5;
  white-space: pre; overflow: auto; tab-size: 4;
}
#shader-highlight { pointer-events: none; overflow: hidden; color: var(--text); z-index: 1; }
#shader-source { background: transparent; color: transparent; caret-color: var(--text); resize: none; outline: none; z-index: 2; }
#shader-source::selection { background: rgba(87,158,255,0.30); }
#shader-code-area.no-shader #shader-gutter,
#shader-code-area.no-shader #shader-code-wrap { visibility: hidden; }
.shader-empty { display: none; position: absolute; inset: 0; align-items: center; justify-content: center; text-align: center; color: var(--text-dim); font-size: 12px; line-height: 1.7; padding: 20px; }
#shader-code-area.no-shader .shader-empty { display: flex; }
/* warnings_panel (RichTextLabel) */
#shader-warnings { display: none; max-height: 120px; overflow-y: auto; background: var(--panel); border: 1px solid var(--border); border-top: none; padding: 4px 8px; font-size: 12px; flex-shrink: 0; }
#shader-warnings.open { display: block; }
.shader-warn-row { display: flex; gap: 8px; padding: 2px 0; }
.shader-warn-row .sw-line { color: #e0c050; cursor: pointer; white-space: nowrap; text-decoration: underline; }
.shader-warn-row.error .sw-line { color: var(--error); }
.shader-warn-row .sw-msg { color: var(--text); }
/* CodeTextEditor status bar */
#shader-status { display: flex; align-items: center; gap: 10px; padding: 2px 8px; background: var(--panel); border: 1px solid var(--border); border-top: none; border-radius: 0 0 3px 3px; font-size: 11px; flex-shrink: 0; }
.shader-status-err { color: var(--error); cursor: pointer; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.shader-status-err.ok { color: #6ac06a; cursor: default; }
.shader-status-spacer { flex: 1; }
.shader-status-zoom { color: var(--text-dim); }
.shader-status-linecol { color: var(--text-dim); white-space: nowrap; }
/* Inline prompt dialog (Godot GotoLinePopup / ShaderCreateDialog parity) */
.shader-prompt-overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.45); z-index: 500; display: flex; align-items: center; justify-content: center; }
.shader-prompt-box { background: var(--panel); border: 1px solid var(--border); border-radius: 6px; box-shadow: 0 8px 32px rgba(0,0,0,0.6); padding: 16px; width: 380px; }
.shader-prompt-title { font-size: 13px; color: var(--text); margin-bottom: 10px; }
.shader-prompt-input { width: 100%; box-sizing: border-box; background: var(--bg); border: 1px solid var(--border); border-radius: 3px; color: var(--text); font-family: monospace; font-size: 12px; padding: 6px 8px; outline: none; }
.shader-prompt-input:focus { border-color: var(--accent); }
.shader-prompt-btns { display: flex; justify-content: flex-end; gap: 8px; margin-top: 12px; }
.shader-prompt-btns button { font-size: 12px; padding: 4px 14px; border-radius: 3px; border: 1px solid var(--border); background: var(--bg); color: var(--text); cursor: pointer; }
.shader-prompt-ok { border-color: var(--accent) !important; color: var(--accent) !important; }
.shader-prompt-btns button:hover { background: var(--hover); }
/* GDShader token colors (ShaderHighlightKind parity) */
.sh-keyword { color: #ff7085; }
.sh-type { color: #4ec9b0; }
.sh-qualifier { color: #c586c0; }
.sh-builtin { color: #dcdcaa; }
.sh-number { color: #d19a66; }
.sh-bool { color: #569cd6; }
.sh-comment { color: #6a9955; font-style: italic; }
.sh-string { color: #ce9178; }
.sh-op { color: #cccccc; }
.sh-punct { color: #bbbbbb; }
/* GDScript syntax highlighting */
.gd-keyword { color: #569cd6; }
.gd-string { color: #6a9955; }
.gd-comment { color: #6a6a6a; font-style: italic; }
.gd-number { color: #d19a66; }
.gd-builtin { color: #dcdcaa; }
.gd-nodepath { color: #c586c0; }
.gd-annotation { color: #4ec9b0; }
.gd-constant { color: #c586c0; }
.gd-classname { color: #4fc1ff; }
.gd-typehint { color: #4ec9b0; }
.gd-arrow { color: #4ec9b0; }

/* Command palette (pat-ugb0p) */
#command-palette {
  display: none; position: fixed; top: 0; left: 0; width: 100%; height: 100%;
  background: rgba(0,0,0,0.4); z-index: 400; align-items: flex-start; justify-content: center;
  padding-top: 15vh;
}
#command-palette.open { display: flex; }
#command-palette-inner {
  background: var(--panel); border: 1px solid var(--border); border-radius: 6px;
  width: 480px; max-height: 400px; box-shadow: 0 8px 32px rgba(0,0,0,0.7);
  display: flex; flex-direction: column; overflow: hidden;
}
#cmd-search {
  width: 100%; padding: 10px 14px; font-size: 14px; border: none; border-bottom: 1px solid var(--border);
  background: var(--panel); color: var(--text); outline: none; font-family: inherit;
}
#cmd-search::placeholder { color: var(--text-dim); }
#cmd-results {
  flex: 1; overflow-y: auto; padding: 4px 0; max-height: 340px;
}
.cmd-item {
  display: flex; align-items: center; padding: 6px 14px; cursor: pointer; gap: 10px;
}
.cmd-item:hover, .cmd-item.active { background: var(--selected); }
.cmd-item.active { border-left: 2px solid var(--accent); }
.cmd-label { flex: 1; color: var(--text); font-size: 13px; }
.cmd-label mark { background: transparent; color: var(--accent); font-weight: bold; }
.cmd-cat { color: var(--text-dim); font-size: 11px; }
.cmd-empty { padding: 16px; text-align: center; color: var(--text-dim); font-size: 13px; }

/* Settings dialog */
#settings-dialog {
  display: none; position: fixed; top: 0; left: 0; width: 100%; height: 100%;
  background: rgba(0,0,0,0.5); z-index: 300; align-items: center; justify-content: center;
}
#settings-dialog.open { display: flex; }
#settings-dialog-inner {
  background: var(--panel); border: 1px solid var(--border); border-radius: 6px;
  width: 380px; padding: 16px; box-shadow: 0 8px 32px rgba(0,0,0,0.6);
}
#settings-dialog-inner h3 { font-size: 14px; color: var(--accent); margin-bottom: 12px; }
.settings-row { display: flex; align-items: center; margin-bottom: 8px; gap: 8px; }
.settings-label { width: 120px; font-size: 12px; color: var(--text-dim); }
.settings-value { flex: 1; }
/* Project settings category tabs (pat-c4zlm) */
.pset-cat { color: var(--text-dim); transition: background 0.15s, color 0.15s; }
.pset-cat:hover { background: var(--hover); color: var(--text); }
.pset-cat.active { background: var(--hover); color: var(--accent); border-left: 2px solid var(--accent); }
/* pat-db37w: Input Map editor */
.input-action-row .settings-value { display: flex; flex-wrap: wrap; align-items: center; gap: 4px; }
.input-key-chip { display: inline-flex; align-items: center; gap: 3px; background: var(--bg); border: 1px solid var(--border); border-radius: 3px; padding: 1px 5px; font-size: 10px; }
.input-key-chip .input-key-x { cursor: pointer; color: var(--text-dim); }
.input-key-chip .input-key-x:hover { color: var(--error); }
.input-key-add { width: 64px; font-size: 10px; padding: 1px 4px; }
.input-action-del { cursor: pointer; color: var(--text-dim); font-size: 11px; margin-left: 4px; }
.input-action-del:hover { color: var(--error); }
/* Editor settings tabs (pat-sbdts) */
.eset-tab { transition: color 0.15s, border-color 0.15s; }
.eset-tab:hover { color: var(--text) !important; }
.eset-tab.active { color: var(--accent) !important; border-bottom-color: var(--accent) !important; }
.eset-section .settings-row { margin-bottom: 10px; }
.eset-kb-row { display: flex; align-items: center; padding: 5px 0; border-bottom: 1px solid var(--border); gap: 8px; font-size: 11px; }
.eset-kb-row .kb-action { width: 120px; color: var(--text); }
.eset-kb-row .kb-desc { flex: 1; color: var(--text-dim); }
.eset-kb-row .kb-keys { padding: 2px 8px; background: var(--bg); border: 1px solid var(--border); border-radius: 3px; font-family: monospace; font-size: 11px; cursor: pointer; color: var(--accent); }
.eset-kb-row .kb-keys:hover { border-color: var(--accent); }
.eset-plugin-row { display: flex; align-items: center; padding: 8px 0; border-bottom: 1px solid var(--border); gap: 8px; font-size: 11px; }
.eset-plugin-row .plugin-name { font-weight: bold; color: var(--text); }
.eset-plugin-row .plugin-desc { flex: 1; color: var(--text-dim); }
#box-select-overlay { position: absolute; border: 2px dashed var(--accent); background: rgba(87,158,255,0.08); pointer-events: none; z-index: 15; display: none; }
/* 3D view menu (Godot's top-left Perspective dropdown) */
#vp3d-view-menu { position: absolute; top: 6px; left: 8px; z-index: 8; font-size: 12px; }
#vp3d-view-btn { background: rgba(28,31,38,0.82); color: var(--text); border: 1px solid var(--border); border-radius: 3px; padding: 3px 10px; cursor: pointer; font: inherit; }
#vp3d-view-btn:hover { background: var(--hover); }
#vp3d-view-dropdown { position: absolute; top: 26px; left: 0; min-width: 132px; background: var(--panel); border: 1px solid var(--border); border-radius: 4px; box-shadow: 0 4px 12px rgba(0,0,0,0.4); padding: 3px 0; }
.vp3d-mi { padding: 4px 12px; cursor: pointer; color: var(--text); white-space: nowrap; display: flex; gap: 6px; align-items: center; }
.vp3d-mi:hover { background: var(--accent); color: #000; }
.vp3d-mi .vp3d-check { width: 11px; display: inline-block; color: var(--accent); font-size: 11px; }
.vp3d-mi:hover .vp3d-check { color: #000; }
.vp3d-sep { height: 1px; background: var(--border); margin: 3px 0; }
/* 3D spatial editor toolbar (Godot Node3DEditor main_menu_hbox parity). */
#vp3d-toolbar {
  display: flex; align-items: center; gap: 2px; padding: 4px 8px; flex-wrap: wrap;
  background: var(--bg); border-bottom: 1px solid var(--border); flex-shrink: 0; min-height: 30px;
}
.vp3d-tb-btn {
  background: transparent; border: 1px solid transparent; color: var(--text);
  font-size: 14px; line-height: 1; min-width: 26px; height: 24px; padding: 2px 5px;
  border-radius: 3px; cursor: pointer; display: inline-flex; align-items: center; justify-content: center;
}
.vp3d-tb-btn:hover { background: var(--hover); }
.vp3d-tb-btn.active { background: var(--selected); border-color: var(--accent); color: var(--accent); }
.vp3d-tb-sep { width: 1px; height: 18px; background: var(--border); margin: 0 4px; flex-shrink: 0; }
.vp3d-tb-menu { position: relative; }
.vp3d-tb-menubtn {
  background: transparent; border: none; color: var(--text); font-size: 12px;
  padding: 4px 8px; border-radius: 3px; cursor: pointer; font: inherit;
}
.vp3d-tb-menubtn:hover, .vp3d-tb-menubtn.open { background: var(--hover); }
.vp3d-tb-dropdown {
  position: absolute; top: 100%; left: 0; z-index: 100; margin-top: 2px;
  background: var(--panel); border: 1px solid var(--border); border-radius: 4px;
  box-shadow: 0 4px 12px rgba(0,0,0,0.5); padding: 3px 0; min-width: 190px;
}
.vp3d-tb-dropdown .vp3d-mi { font-size: 12px; }
.vp3d-mi.vp3d-disabled { opacity: 0.4; cursor: default; }
.vp3d-mi.vp3d-disabled:hover { background: transparent; color: var(--text); }
.vp3d-shortcut { margin-left: auto; color: var(--text-dim); font-size: 11px; padding-left: 18px; }
.vp3d-mi:hover .vp3d-shortcut { color: #000; }
/* Small popups shared by the toolbar (Configure Snap, Transform Dialog, etc.). */
.vp3d-popup {
  position: absolute; z-index: 200; background: var(--panel); border: 1px solid var(--border);
  border-radius: 5px; box-shadow: 0 6px 18px rgba(0,0,0,0.55); padding: 12px 14px; min-width: 220px;
  color: var(--text); font-size: 12px;
}
.vp3d-popup h4 { margin: 0 0 8px; font-size: 12px; color: var(--text); font-weight: 600; }
.vp3d-popup label { display: flex; align-items: center; justify-content: space-between; gap: 10px; margin: 5px 0; }
.vp3d-popup input[type=number] {
  width: 90px; background: var(--bg); border: 1px solid var(--border); color: var(--text);
  border-radius: 3px; padding: 3px 5px; font: inherit;
}
.vp3d-popup .vp3d-popup-actions { display: flex; justify-content: flex-end; gap: 6px; margin-top: 10px; }
.vp3d-popup button { background: var(--bg); border: 1px solid var(--border); color: var(--text); border-radius: 3px; padding: 4px 12px; cursor: pointer; font: inherit; }
.vp3d-popup button.primary { background: var(--accent); border-color: var(--accent); color: #000; }
.vp3d-popup button:hover { background: var(--hover); }
.vp3d-popup button.primary:hover { filter: brightness(1.1); }
.gizmo-indicator { position: absolute; pointer-events: none; z-index: 14; font-size: 10px; padding: 1px 4px; border-radius: 2px; color: #fff; }
.gizmo-indicator.x-axis { background: var(--vec-x); }
.gizmo-indicator.y-axis { background: var(--vec-y); }
.gizmo-indicator.rotate { background: #c88eff; }
.gizmo-indicator.scale { background: #e0c050; }
#snap-indicator { position: absolute; top: 4px; left: 50%; transform: translateX(-50%); background: var(--panel); border: 1px solid var(--border); padding: 2px 8px; font-size: 10px; color: var(--text-dim); border-radius: 3px; z-index: 10; display: none; }
#snap-indicator.active { display: block; color: var(--accent); }
#script-find-bar { display: none; padding: 4px 8px; background: var(--panel); border-bottom: 1px solid var(--border); gap: 4px; align-items: center; flex-shrink: 0; font-size: 11px; }
#script-find-bar.open { display: flex; flex-wrap: wrap; }
#script-find-bar input { flex: 1; min-width: 100px; padding: 2px 6px; font-size: 11px; }
#script-find-bar button { padding: 2px 8px; font-size: 10px; }
#script-find-bar .find-count { color: var(--text-dim); font-size: 10px; min-width: 40px; }
#script-goto-bar { display: none; padding: 4px 8px; background: var(--panel); border-bottom: 1px solid var(--border); gap: 4px; align-items: center; flex-shrink: 0; }
#script-goto-bar.open { display: flex; }
#script-goto-bar input { width: 80px; padding: 2px 6px; font-size: 11px; }
.script-line-numbers .breakpoint { color: var(--error); font-weight: bold; }
.script-line-numbers .error-line { background: rgba(224,80,80,0.2); }
.script-minimap { position: absolute; top: 0; right: 0; width: 60px; height: 100%; background: rgba(13,13,13,0.8); border-left: 1px solid var(--border); overflow: hidden; cursor: pointer; z-index: 5; }
.script-minimap canvas { width: 100%; height: 100%; }
.fold-gutter { position: absolute; top: 0; left: 32px; width: 14px; cursor: pointer; z-index: 3; }
.fold-marker { font-size: 9px; color: var(--text-dim); text-align: center; line-height: 1.6; user-select: none; }
.fold-marker:hover { color: var(--accent); }
.signal-count-badge { background: var(--accent); color: var(--bg); font-size: 9px; padding: 0 5px; border-radius: 8px; font-weight: bold; margin-left: 4px; }
.signal-filter { padding: 4px 8px; font-size: 11px; border-bottom: 1px solid var(--border); }
.signal-filter input { width: 100%; padding: 2px 6px; font-size: 11px; }
.signal-connection-detail { font-size: 10px; color: var(--text-dim); padding-left: 28px; }
.anim-track-drag { cursor: grab; color: var(--text-dim); font-size: 10px; padding: 0 4px; }
.anim-track-drag:active { cursor: grabbing; }
/* Debugger panel (Godot ScriptEditorDebugger parity): sub-tabs + sub-panels */
#debugger-panel { display: flex; flex-direction: column; height: 100%; overflow: hidden; }
#debugger-subtabs {
  display: flex; align-items: flex-end; gap: 2px; flex-shrink: 0; flex-wrap: wrap;
  border-bottom: 1px solid var(--border); padding: 0 4px;
}
.dbg-subtab {
  padding: 4px 10px; font-size: 11px; color: var(--text-dim); cursor: pointer;
  border-bottom: 2px solid transparent; white-space: nowrap; border-radius: 3px 3px 0 0;
}
.dbg-subtab:hover { color: var(--text); background: var(--hover); }
.dbg-subtab.active { color: var(--text); background: var(--panel); border-bottom-color: var(--accent); }
#debugger-subcontent { flex: 1; min-height: 0; overflow: hidden; }
.dbg-subpanel { display: none; height: 100%; overflow: auto; padding: 6px 8px; flex-direction: column; }
.dbg-subpanel.active { display: flex; }
/* Godot debugger: Stack Frames / Breakpoints are centered header bars atop
   inset bordered boxes — the boxes (darker bg + border) divide the columns. */
.debug-section-label {
  background: var(--bg); border: 1px solid var(--border); border-bottom: none;
  padding: 5px 8px; text-align: center; color: var(--text); font-size: 12px;
  border-radius: 3px 3px 0 0; flex-shrink: 0;
}
#debug-split .debug-col .debug-section-label ~ .debug-section-label { margin-top: 6px; }
.dbg-subtabs-spacer { flex: 1; }
#bottom-panel.expanded { height: 60vh !important; flex: none !important; }
/* Godot puts Thread + Filter Stack Variables + the debug toolbar on one row. */
#debug-headrow { display: flex; align-items: center; gap: 8px; margin-bottom: 6px; flex-shrink: 0; }
#debug-headrow .debug-thread-row { flex-shrink: 0; }
#debug-headrow .debug-filter-input { flex: 1; min-width: 80px; }
#debug-headrow .debug-status { flex-shrink: 0; }
#debug-toolbar { display: flex; gap: 3px; align-items: center; flex-shrink: 0; }
.dbg-tool {
  background: transparent; border: 1px solid transparent; border-radius: 3px;
  color: var(--text-dim); font-size: 13px; padding: 2px 6px; cursor: pointer; line-height: 1;
}
/* Render glyph-icons as monochrome TEXT (theme color), not colored emoji —
   Godot's toolbar icons are all monochrome. Applies across the toolbars. */
.dbg-tool, .game-tool-btn, .output-tool-btn, .output-filter-btn, .fs-tool-btn,
.script-tool-btn, .dbg-text-btn {
  font-variant-emoji: text;
}
.dbg-tool:hover:not(:disabled) { background: var(--hover); color: var(--text); }
.dbg-tool.active { color: var(--accent); }
.dbg-tb-sep { width: 1px; height: 16px; background: var(--border); margin: 0 3px; }
.dbg-tb-spacer { flex: 1; }
#debug-toolbar .debug-status { font-size: 11px; color: var(--text-dim); }
#debug-toolbar .debug-status.paused { color: var(--accent); }
#debug-toolbar .debug-status.running { color: #50c878; }
#debug-split { flex: 1; min-height: 0; display: flex; gap: 8px; }
#debug-split .debug-col { flex: 1; min-width: 0; display: flex; flex-direction: column; overflow: hidden; }
/* Content areas are inset panels (darker bg + border) — the visible boxes/dividers. */
#debug-stack-frames, #debug-variables, #debug-globals, #debug-breakpoints-list {
  background: var(--bg); border: 1px solid var(--border); flex: 1; min-height: 40px;
  overflow: auto; border-radius: 0 0 3px 3px;
}
.debug-thread-row { display: flex; align-items: center; gap: 6px; }
.debug-thread-label { font-size: 11px; color: var(--text-dim); }
#debug-thread { flex: 1; background: var(--bg); border: 1px solid var(--border); color: var(--text); font-size: 11px; border-radius: 3px; padding: 2px 4px; }
.debug-filter-input { background: var(--bg); border: 1px solid var(--border); border-radius: 3px; color: var(--text); font-size: 11px; padding: 3px 6px; outline: none; }
.debug-filter-input:focus { border-color: var(--accent); }
.debug-filter-input::placeholder { color: var(--text-dim); }
#debug-errors-toolbar { display: flex; gap: 6px; align-items: center; margin-bottom: 6px; flex-shrink: 0; }
.dbg-text-btn { background: transparent; border: 1px solid var(--border); border-radius: 3px; color: var(--text); font-size: 11px; padding: 3px 8px; cursor: pointer; }
.dbg-text-btn:hover:not(:disabled) { background: var(--hover); border-color: var(--accent); }
.dbg-text-btn:disabled { opacity: 0.45; cursor: default; }
/* Misc debugger tab (Godot ScriptEditorDebugger misc GridContainer + buttons). */
#dbg-misc { padding: 6px 4px; }
#dbg-misc-grid { display: grid; grid-template-columns: auto 1fr; gap: 4px 8px; align-items: center; }
.dbg-misc-label { font-size: 11px; color: var(--text); white-space: nowrap; }
.dbg-misc-input { font-size: 11px; padding: 3px 6px; background: var(--bg); color: var(--text); border: 1px solid var(--border); border-radius: 3px; width: 100%; box-sizing: border-box; }
.dbg-misc-input:read-only { color: var(--text-dim); }
.dbg-misc-lehb { display: flex; gap: 6px; align-items: center; }
.dbg-misc-lehb .dbg-misc-input { flex: 1; }
.dbg-misc-sep { height: 1px; background: var(--border); margin: 8px 0; }
.dbg-misc-buttons { display: flex; gap: 6px; }
#debug-errors-list { flex: 1; overflow: auto; }
/* Profiler sub-tab (Godot EditorProfiler parity): toolbar + tree | graph split */
#profiler-panel { display: flex; flex-direction: column; height: 100%; overflow: hidden; }
#profiler-toolbar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; padding: 2px; margin-bottom: 6px; flex-shrink: 0; }
.profiler-tb-group { display: flex; align-items: center; gap: 4px; }
.profiler-tb-label { font-size: 11px; color: var(--text-dim); }
.profiler-tb-spacer { flex: 1; }
.profiler-check { display: inline-flex; align-items: center; gap: 4px; font-size: 11px; color: var(--text); cursor: pointer; }
.profiler-select { background: var(--bg); border: 1px solid var(--border); color: var(--text); font-size: 11px; border-radius: 3px; padding: 2px 4px; }
.profiler-frame-input { width: 72px; background: var(--bg); border: 1px solid var(--border); color: var(--text); font-size: 11px; border-radius: 3px; padding: 2px 4px; }
#profiler-split { flex: 1; min-height: 0; display: flex; gap: 8px; }
#profiler-tree { width: 320px; min-width: 180px; flex-shrink: 0; display: flex; flex-direction: column; background: var(--bg); border: 1px solid var(--border); border-radius: 3px; overflow: hidden; }
#profiler-tree-head { display: flex; gap: 6px; padding: 4px 6px; border-bottom: 1px solid var(--border); font-weight: bold; flex-shrink: 0; }
#profiler-func-breakdown { flex: 1; overflow: auto; }
.pcol { font-size: 11px; color: var(--text-dim); }
.pcol-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.pcol-time { width: 75px; text-align: right; flex-shrink: 0; }
.pcol-calls { width: 50px; text-align: right; flex-shrink: 0; }
.profiler-row { display: flex; gap: 6px; padding: 2px 6px; }
.profiler-row:hover { background: var(--hover); }
.profiler-row .pcol { color: var(--text); }
.profiler-row .pcol-name { display: flex; align-items: center; gap: 5px; }
.profiler-swatch { width: 8px; height: 8px; border-radius: 2px; flex-shrink: 0; }
.profiler-row .pcheck { width: 12px; height: 12px; margin: 0; flex-shrink: 0; accent-color: var(--accent); cursor: pointer; }
#profiler-graph-wrap { flex: 1; min-width: 0; background: var(--bg); border: 1px solid var(--border); border-radius: 3px; overflow: hidden; display: flex; }
#profiler-graph-wrap canvas { width: 100%; height: 100%; }
/* Monitors sub-tab (Godot EditorPerformanceProfiler): Monitor|Value tree + graph */
#monitors-panel { display: flex; flex-direction: column; height: 100%; overflow: hidden; }
#monitors-split { flex: 1; min-height: 0; display: flex; gap: 8px; }
#monitor-tree { width: 340px; min-width: 200px; flex-shrink: 0; display: flex; flex-direction: column; background: var(--bg); border: 1px solid var(--border); border-radius: 3px; overflow: hidden; }
#monitor-tree-head { display: flex; gap: 6px; padding: 4px 6px; border-bottom: 1px solid var(--border); font-weight: bold; flex-shrink: 0; }
#monitor-tree-body { flex: 1; overflow: auto; }
.mcol { font-size: 11px; color: var(--text-dim); }
.mcol-name { flex: 1; min-width: 0; }
.mcol-value { width: 100px; text-align: right; flex-shrink: 0; }
.monitor-cat { padding: 4px 6px 2px; font-size: 11px; font-weight: bold; color: var(--text); }
.monitor-row { display: flex; gap: 6px; padding: 2px 6px 2px 18px; }
.monitor-row:hover { background: var(--hover); }
.monitor-row .mcol { color: var(--text); }
.monitor-row .mcol-name { display: flex; align-items: center; gap: 6px; }
.monitor-row .mcheck { width: 12px; height: 12px; margin: 0; flex-shrink: 0; cursor: pointer; }
.monitor-row .mcol-value { color: var(--text-dim); font-variant-numeric: tabular-nums; }
#monitor-graph-wrap { flex: 1; min-width: 0; position: relative; background: var(--bg); border: 1px solid var(--border); border-radius: 3px; overflow: hidden; display: flex; }
#monitor-graph-wrap canvas { width: 100%; height: 100%; }
#monitor-graph-empty { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; text-align: center; color: var(--text-dim); font-size: 12px; font-style: italic; padding: 16px; }
/* Network Profiler sub-tab (Godot EditorNetworkProfiler): toolbar + RPC/replication trees */
#net-profiler-panel { display: flex; flex-direction: column; height: 100%; min-height: 0; overflow: hidden; }
#net-profiler-toolbar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; padding: 2px; margin-bottom: 6px; flex-shrink: 0; }
.net-tb-spacer { flex: 1; min-width: 12px; }
.net-bw-group { display: inline-flex; align-items: center; gap: 6px; }
.net-bw-label { font-size: 11px; color: var(--text-dim); }
.net-bw-field { display: inline-flex; align-items: center; gap: 4px; min-width: 120px; justify-content: flex-end; background: var(--bg); border: 1px solid var(--border); border-radius: 3px; padding: 2px 6px; }
.net-bw-val { font-size: 11px; color: var(--text); font-variant-numeric: tabular-nums; }
.net-bw-val.net-bw-dim { opacity: 0.5; }
.net-bw-arrow { font-size: 11px; color: var(--text-dim); }
.net-bw-mid { width: 30px; flex-shrink: 0; }
#net-profiler-split { flex: 1; min-height: 0; display: flex; gap: 8px; }
.net-tree { flex: 1; min-width: 140px; display: flex; flex-direction: column; background: var(--bg); border: 1px solid var(--border); border-radius: 3px; overflow: hidden; }
.net-tree-head { display: flex; gap: 6px; padding: 4px 6px; border-bottom: 1px solid var(--border); font-weight: bold; flex-shrink: 0; }
.net-tree-body { flex: 1; overflow: auto; }
.ncol { font-size: 11px; color: var(--text-dim); }
.ncol-node, .ncol-root, .ncol-sync, .ncol-config { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.ncol-num { width: 96px; text-align: right; flex-shrink: 0; }
.ncol-count, .ncol-size { width: 64px; }
.net-row { display: flex; gap: 6px; padding: 2px 6px; }
.net-row:hover { background: var(--hover); }
.net-row .ncol { color: var(--text); }
.net-row .ncol-num { font-variant-numeric: tabular-nums; }
.net-cell-icon { display: inline-flex; align-items: center; gap: 5px; min-width: 0; }
.net-cell-icon .net-ic { flex-shrink: 0; color: var(--text-dim); }
.net-cell-text { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.net-empty { padding: 8px; color: var(--text-dim); font-style: italic; font-size: 11px; }
/* Video RAM sub-tab (Godot ScriptEditorDebugger vmem): header + Resource Path|Type|Format|Usage table */
.dbg-subpanel[data-dbgtab="video-ram"] { display: none; }
.dbg-subpanel[data-dbgtab="video-ram"].active { display: flex; }
#dbg-vram-header { display: flex; align-items: center; gap: 6px; padding: 2px; margin-bottom: 6px; flex-shrink: 0; }
.dbg-vram-title { font-size: 11px; font-weight: bold; color: var(--text); }
.dbg-vram-notice { color: var(--text-dim); cursor: help; font-size: 13px; }
.dbg-vram-spacer { flex: 1; }
.dbg-vram-totallabel { font-size: 11px; color: var(--text-dim); }
.dbg-vram-totalinput { width: 110px; background: var(--bg); border: 1px solid var(--border); color: var(--text); font-size: 11px; border-radius: 3px; padding: 2px 6px; text-align: right; }
#dbg-vram-table { flex: 1; min-height: 0; display: flex; flex-direction: column; background: var(--bg); border: 1px solid var(--border); border-radius: 3px; overflow: hidden; }
#dbg-vram-head { display: flex; gap: 8px; padding: 4px 8px; border-bottom: 1px solid var(--border); font-weight: bold; flex-shrink: 0; }
#dbg-vram-list { flex: 1; overflow: auto; }
.vrcol { font-size: 11px; color: var(--text-dim); }
.vrcol-path { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.vrcol-type { width: 100px; flex-shrink: 0; }
.vrcol-format { width: 90px; flex-shrink: 0; }
.vrcol-usage { width: 80px; text-align: right; flex-shrink: 0; }
.vram-row { display: flex; gap: 8px; padding: 2px 8px; font-family: monospace; }
.vram-row:hover { background: var(--hover); }
.vram-row .vrcol { color: var(--text); }
.vram-row .vrcol-type, .vram-row .vrcol-format { color: var(--text-dim); }
.debug-stack-frame { padding: 2px 4px; font-size: 11px; font-family: monospace; cursor: pointer; }
.debug-stack-frame:hover { background: var(--hover); }
.debug-stack-frame.current { color: var(--accent); background: var(--selected); }
.debug-breakpoint-item { padding: 2px 4px; font-size: 11px; font-family: monospace; display: flex; align-items: center; gap: 4px; }
.debug-breakpoint-item:hover { background: var(--hover); }
.debug-breakpoint-item .bp-enabled { width: 12px; height: 12px; accent-color: var(--error); }
.debug-breakpoint-item .bp-location { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.debug-breakpoint-item .bp-remove { cursor: pointer; color: var(--text-dim); font-size: 10px; }
.debug-breakpoint-item .bp-remove:hover { color: var(--error); }
.debug-var-item { padding: 1px 4px; font-size: 11px; font-family: monospace; display: flex; gap: 6px; }
.debug-var-item:hover { background: var(--hover); }
.debug-var-item .var-name { color: var(--accent); min-width: 80px; }
.debug-var-item .var-type { color: var(--text-dim); font-size: 10px; min-width: 50px; }
.debug-var-item .var-value { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
#debug-variables { max-height: 200px; overflow-y: auto; }
#debug-breakpoints-list { max-height: 120px; overflow-y: auto; }
#monitors-panel { padding: 8px; }
.monitor-graph { width: 100%; height: 80px; background: var(--bg); border: 1px solid var(--border); border-radius: 3px; margin-bottom: 8px; }
.monitor-stats { display: flex; gap: 16px; font-size: 11px; color: var(--text-dim); margin-bottom: 8px; }
.monitor-stats .stat-value { color: var(--text); font-weight: bold; }
.mode-buttons { display: flex; gap: 3px; align-items: center; }
.mode-btn { padding: 5px 14px; font-size: 12px; border: 1px solid transparent; background: transparent; color: #c2c2c2; cursor: pointer; font: inherit; border-radius: 3px; }
.mode-btn:hover { background: var(--hover); color: var(--text); }
.mode-btn.active { background: rgba(87,158,255,0.13); color: var(--accent); }
.scene-tab-close { opacity: 0; margin-left: 4px; font-size: 10px; cursor: pointer; color: var(--text-dim); padding: 0 2px; border-radius: 2px; }
.scene-tab:hover .scene-tab-close { opacity: 0.7; }
.scene-tab-close:hover { opacity: 1 !important; color: var(--error); background: var(--hover); }
</style>
</head>
<body>

<!-- Menu bar -->
<div id="menu-bar">
  <span class="menu-bar-brand">Patina</span>
  <div class="menu-item" data-menu="scene">
    <button>Scene</button>
    <div class="menu-dropdown">
      <div class="menu-action" data-action="scene-new">New Scene<span class="shortcut">Ctrl+N</span></div>
      <div class="menu-action" data-action="scene-open">Open Scene<span class="shortcut">Ctrl+O</span></div>
      <div class="menu-action" data-action="scene-save">Save Scene<span class="shortcut">Ctrl+S</span></div>
      <div class="menu-action" data-action="scene-save-as">Save Scene As...<span class="shortcut">Ctrl+Shift+S</span></div>
      <div class="menu-sep"></div>
      <div class="menu-action" data-action="scene-close">Close Scene</div>
      <div class="menu-sep"></div>
      <div class="menu-action" data-action="scene-quit">Quit<span class="shortcut">Ctrl+Q</span></div>
    </div>
  </div>
  <div class="menu-item" data-menu="edit">
    <button>Edit</button>
    <div class="menu-dropdown">
      <div class="menu-action" data-action="edit-undo">Undo<span class="shortcut">Ctrl+Z</span></div>
      <div class="menu-action" data-action="edit-redo">Redo<span class="shortcut">Ctrl+Shift+Z</span></div>
      <div class="menu-sep"></div>
      <div class="menu-action" data-action="edit-cut">Cut<span class="shortcut">Ctrl+X</span></div>
      <div class="menu-action" data-action="edit-copy">Copy<span class="shortcut">Ctrl+C</span></div>
      <div class="menu-action" data-action="edit-paste">Paste<span class="shortcut">Ctrl+V</span></div>
      <div class="menu-sep"></div>
      <div class="menu-action" data-action="edit-select-all">Select All<span class="shortcut">Ctrl+A</span></div>
      <div class="menu-action" data-action="edit-deselect">Deselect All</div>
    </div>
  </div>
  <div class="menu-item" data-menu="project">
    <button>Project</button>
    <div class="menu-dropdown">
      <div class="menu-action" data-action="project-open">Open Project...</div>
      <div class="menu-sep"></div>
      <div class="menu-action" data-action="project-settings">Project Settings...</div>
      <div class="menu-sep"></div>
      <div class="menu-action" data-action="project-export">Export...</div>
      <div class="menu-action" data-action="project-install-android">Install Android Build Template</div>
      <div class="menu-sep"></div>
      <div class="menu-action" data-action="project-refresh">Refresh Filesystem</div>
    </div>
  </div>
  <div class="menu-item" data-menu="debug">
    <button>Debug</button>
    <div class="menu-dropdown">
      <div class="menu-action" data-action="debug-run">Run Project<span class="shortcut">F5</span></div>
      <div class="menu-action" data-action="debug-run-current">Run Current Scene<span class="shortcut">F6</span></div>
      <div class="menu-action" data-action="debug-pause">Pause<span class="shortcut">F7</span></div>
      <div class="menu-action" data-action="debug-stop">Stop<span class="shortcut">F8</span></div>
      <div class="menu-sep"></div>
      <div class="menu-action" data-action="debug-step">Step</div>
      <div class="menu-action" data-action="debug-break">Break</div>
      <div class="menu-sep"></div>
      <div class="menu-action" data-action="debug-keep-running">Deploy with Remote Debug</div>
    </div>
  </div>
  <div class="menu-item" data-menu="editor">
    <button>Editor</button>
    <div class="menu-dropdown">
      <div class="menu-action" data-action="editor-settings">Editor Settings...<span class="shortcut"></span></div>
      <div class="menu-sep"></div>
      <div class="menu-action" data-action="editor-layout-save">Save Editor Layout</div>
      <div class="menu-action" data-action="editor-layout-default">Reset to Default Layout</div>
      <div class="menu-sep"></div>
      <div class="menu-action" data-action="editor-toggle-fullscreen">Toggle Fullscreen<span class="shortcut">F11</span></div>
      <div class="menu-action" data-action="editor-toggle-console">Toggle Output<span class="shortcut">Ctrl+`</span></div>
    </div>
  </div>
  <div class="menu-item" data-menu="help">
    <button>Help</button>
    <div class="menu-dropdown">
      <div class="menu-action" data-action="help-docs">Online Documentation</div>
      <div class="menu-action" data-action="help-issues">Report a Bug</div>
      <div class="menu-sep"></div>
      <div class="menu-action" data-action="help-about">About Patina Engine</div>
    </div>
  </div>
  <!-- Workspace switcher (centered) — Godot's main-screen tabs -->
  <div class="menu-spacer"></div>
  <div class="mode-buttons">
    <button class="mode-btn active" data-mode="2d">2D</button>
    <button class="mode-btn" data-mode="3d">3D</button>
    <button class="mode-btn" data-mode="script">Script</button>
    <button class="mode-btn" data-mode="game">Game</button>
    <button class="mode-btn" data-mode="assetlib">AssetLib</button>
  </div>
  <div class="menu-spacer"></div>
  <!-- Play controls + renderer indicator (right) -->
  <div class="play-buttons">
    <button class="play-btn play-main" id="btn-play" title="Play (F5)">&#9654;</button>
    <button class="play-btn pause-btn" id="btn-pause" title="Pause (F7)">&#9208;</button>
    <button class="play-btn stop-btn" id="btn-stop" title="Stop (F8)">&#9209;</button>
    <button class="play-btn play-current" id="btn-play-current" title="Play Current Scene (F6)">&#9654;&#9998;</button>
    <!-- pat-12qzs: Play Custom Scene (Godot editor_run_bar.cpp play_custom_scene_button,
         PlayCustom icon, "Play a custom scene."). -->
    <button class="play-btn play-custom" id="btn-play-custom" title="Play a custom scene.">&#9654;&#9776;</button>
    <!-- pat-uqj6w: Movie Maker mode toggle (Godot editor_run_bar.cpp
         write_movie_button, MainMovie icon). -->
    <button class="play-btn movie-maker-btn" id="btn-movie-maker" title="Enable Movie Maker mode.&#10;The project will run at stable FPS and the visual and audio output will be recorded to a video file.">&#127916;</button>
  </div>
  <!-- pat-j8sfr: renderer selector dropdown (Godot editor_node.cpp renderer
       OptionButton: Forward+ / Mobile / Compatibility), not static text. -->
  <select class="renderer-indicator" id="renderer-select" title="Rendering method">
    <option value="forward_plus">Forward+</option>
    <option value="mobile">Mobile</option>
    <option value="gl_compatibility">Compatibility</option>
  </select>
</div>
<div id="toolbar">
  <button class="tool-btn active" data-tool="select" title="Select (Q)">Q</button>
  <button class="tool-btn" data-tool="move" title="Move (W)">W</button>
  <button class="tool-btn" data-tool="rotate" title="Rotate (E)">E</button>
  <button class="tool-btn" data-tool="scale" title="Scale (R)">R</button>
  <div class="sep"></div>
  <button id="btn-add" title="Add Node">+ Add Node</button>
  <button id="btn-delete" title="Delete Node (Del)">&#10005; Delete</button>
  <div class="sep"></div>
  <button id="btn-undo" title="Undo (Ctrl+Z)">&#8630; Undo</button>
  <button id="btn-redo" title="Redo (Ctrl+Y)">&#8631; Redo</button>
  <div class="sep"></div>
  <button id="btn-save" title="Save Scene (Ctrl+S)">&#128190; Save</button>
  <button id="btn-load" title="Load Scene">&#128194; Load</button>
  <div class="sep"></div>
  <button id="btn-settings" title="Editor Settings">&#9881; Settings</button>
  <span id="scene-file-indicator"></span>
</div>

<!-- Context menu -->
<div id="context-menu">
  <div class="ctx-item" data-action="rename">Rename<span class="ctx-shortcut">F2</span></div>
  <div class="ctx-item" data-action="copy">Copy<span class="ctx-shortcut">Ctrl+C</span></div>
  <div class="ctx-item" data-action="paste">Paste<span class="ctx-shortcut">Ctrl+V</span></div>
  <div class="ctx-item" data-action="cut">Cut<span class="ctx-shortcut">Ctrl+X</span></div>
  <div class="ctx-separator"></div>
  <div class="ctx-item" data-action="duplicate">Duplicate<span class="ctx-shortcut">Ctrl+D</span></div>
  <div class="ctx-item" data-action="delete">Delete<span class="ctx-shortcut">Del</span></div>
  <div class="ctx-separator"></div>
  <div class="ctx-item" data-action="add-child">Add Child Node</div>
  <div class="ctx-item" data-action="instance-scene">Instance Child Scene</div>
  <div class="ctx-separator"></div>
  <div class="ctx-item" data-action="move-up">Move Up</div>
  <div class="ctx-item" data-action="move-down">Move Down</div>
  <div class="ctx-separator"></div>
  <div class="ctx-item" data-action="attach-script">Attach Script</div>
</div>

<!-- Add Node Dialog — 1:1 with Godot's Create New Node (CreateDialog) -->
<div id="add-node-dialog">
  <div id="add-node-dialog-inner">
    <div id="add-node-dialog-header">
      <span>Create New Node</span>
      <button id="add-node-close" title="Close">&times;</button>
    </div>
    <div id="add-node-body">
      <div id="add-node-left">
        <div class="add-node-pane-label">Favorites:</div>
        <div id="add-node-favorites"></div>
        <div class="add-node-pane-label">Recent:</div>
        <div id="add-node-recent"></div>
      </div>
      <div id="add-node-right">
        <div class="add-node-pane-label">Search:</div>
        <div id="add-node-search-row">
          <div id="add-node-search-wrap">
            <input type="text" id="add-node-search" autocomplete="off" spellcheck="false">
            <button id="add-node-search-clear" title="Clear" tabindex="-1">&times;</button>
          </div>
          <button id="add-node-fav-btn" title="(Un)favorite selected item." disabled>&#9733;</button>
        </div>
        <div class="add-node-pane-label">Matches:</div>
        <div id="add-node-list"></div>
        <div class="add-node-pane-label">Description:</div>
        <div id="add-node-description">Select a node type to see its description.</div>
      </div>
    </div>
    <div id="add-node-dialog-footer">
      <button id="add-node-cancel">Cancel</button>
      <button id="add-node-create" style="border-color:var(--accent);color:var(--accent)">Create</button>
    </div>
  </div>
</div>

<!-- Open Scene Dialog (scene picker) -->
<div id="open-scene-dialog">
  <div id="open-scene-inner">
    <div id="open-scene-header">
      <span>Open Scene</span>
      <button id="open-scene-close" title="Close">&times;</button>
    </div>
    <input type="text" id="open-scene-filter" placeholder="Filter scenes..." autocomplete="off">
    <div id="open-scene-list"></div>
    <div id="open-scene-footer">
      <button id="open-scene-cancel">Cancel</button>
    </div>
  </div>
</div>

<!-- Save Scene As dialog (Godot EditorFileDialog FILE_MODE_SAVE_FILE) -->
<div id="save-scene-dialog">
  <div id="save-scene-inner">
    <div id="save-scene-header">
      <span>Save Scene As...</span>
      <button id="save-scene-close" title="Close">&times;</button>
    </div>
    <div id="save-scene-path" title="Current folder"></div>
    <div id="save-scene-list"></div>
    <div id="save-scene-name-row">
      <label for="save-scene-name">File:</label>
      <input type="text" id="save-scene-name" placeholder="scene.tscn" autocomplete="off" spellcheck="false">
    </div>
    <div id="save-scene-error"></div>
    <div id="save-scene-footer">
      <button id="save-scene-cancel">Cancel</button>
      <button id="save-scene-confirm">Save</button>
    </div>
  </div>
</div>

<!-- Open Project dialog (pat-v89tw) -->
<div id="open-project-dialog">
  <div id="open-project-inner">
    <div id="open-project-header">
      <span>Open Project</span>
      <button id="open-project-close" title="Close">&times;</button>
    </div>
    <div id="open-project-body">
      <div id="open-project-hint">Enter the path to a Godot project directory (the folder containing <code>project.godot</code>). The FileSystem dock will re-root at the project's res:// tree.</div>
      <input type="text" id="open-project-path" placeholder="/path/to/your/project" autocomplete="off">
      <div id="open-project-error"></div>
    </div>
    <div id="open-project-footer">
      <button id="open-project-cancel">Cancel</button>
      <button id="open-project-confirm">Open</button>
    </div>
  </div>
</div>

<!-- Help Dialog -->
<div id="help-dialog">
  <div id="help-dialog-inner">
    <div id="help-dialog-header">
      <span>Keyboard Shortcuts</span>
      <button id="help-close" title="Close">&times;</button>
    </div>
    <div id="help-dialog-body">
      <div class="help-category">
        <div class="help-category-title">File</div>
        <div class="help-row"><span class="help-desc">Save scene</span><span class="help-key">Ctrl+S</span></div>
        <div class="help-row"><span class="help-desc">Command palette</span><span class="help-key">Ctrl+Shift+P</span></div>
        <div class="help-row"><span class="help-desc">Search nodes</span><span class="help-key">Ctrl+F</span></div>
      </div>
      <div class="help-category">
        <div class="help-category-title">Edit</div>
        <div class="help-row"><span class="help-desc">Undo</span><span class="help-key">Ctrl+Z</span></div>
        <div class="help-row"><span class="help-desc">Redo</span><span class="help-key">Ctrl+Y / Ctrl+Shift+Z</span></div>
        <div class="help-row"><span class="help-desc">Duplicate node</span><span class="help-key">Ctrl+D</span></div>
        <div class="help-row"><span class="help-desc">Copy</span><span class="help-key">Ctrl+C</span></div>
        <div class="help-row"><span class="help-desc">Paste</span><span class="help-key">Ctrl+V</span></div>
        <div class="help-row"><span class="help-desc">Cut</span><span class="help-key">Ctrl+X</span></div>
        <div class="help-row"><span class="help-desc">Delete node</span><span class="help-key">Delete</span></div>
        <div class="help-row"><span class="help-desc">Rename node</span><span class="help-key">F2</span></div>
      </div>
      <div class="help-category">
        <div class="help-category-title">View</div>
        <div class="help-row"><span class="help-desc">Zoom in</span><span class="help-key">Ctrl+ +</span></div>
        <div class="help-row"><span class="help-desc">Zoom out</span><span class="help-key">Ctrl+ -</span></div>
        <div class="help-row"><span class="help-desc">Reset zoom &amp; pan</span><span class="help-key">Ctrl+0</span></div>
        <div class="help-row"><span class="help-desc">Pan viewport</span><span class="help-key">Middle-click drag / Shift+drag</span></div>
      </div>
      <div class="help-category">
        <div class="help-category-title">Tools</div>
        <div class="help-row"><span class="help-desc">Select tool</span><span class="help-key">Q</span></div>
        <div class="help-row"><span class="help-desc">Move tool</span><span class="help-key">W</span></div>
        <div class="help-row"><span class="help-desc">Rotate tool</span><span class="help-key">E</span></div>
        <div class="help-row"><span class="help-desc">Scale tool</span><span class="help-key">R</span></div>
      </div>
      <div class="help-category">
        <div class="help-category-title">Runtime</div>
        <div class="help-row"><span class="help-desc">Play</span><span class="help-key">F5</span></div>
        <div class="help-row"><span class="help-desc">Play current scene</span><span class="help-key">F6</span></div>
        <div class="help-row"><span class="help-desc">Pause</span><span class="help-key">F7</span></div>
        <div class="help-row"><span class="help-desc">Stop</span><span class="help-key">F8</span></div>
      </div>
      <div class="help-category">
        <div class="help-category-title">Help</div>
        <div class="help-row"><span class="help-desc">Show this dialog</span><span class="help-key">F1 / ?</span></div>
      </div>
    </div>
  </div>
</div>

<!-- Confirmation Dialog (Godot-style ConfirmationDialog) -->
<div id="confirm-dialog">
  <div id="confirm-dialog-inner">
    <div id="confirm-dialog-header">
      <span id="confirm-dialog-title">Please Confirm...</span>
      <button id="confirm-dialog-x" title="Close">&times;</button>
    </div>
    <div id="confirm-dialog-body"></div>
    <div id="confirm-dialog-footer">
      <button id="confirm-dialog-cancel">Cancel</button>
      <button id="confirm-dialog-ok" class="confirm-primary">OK</button>
    </div>
  </div>
</div>

<!-- Main area -->
<div id="main">
  <!-- Left panel: Scene tree + FileSystem -->
  <div id="left-panel">
    <div id="scene-panel">
      <div class="panel-header">
        <span class="dock-tab active" id="scene-dock-tab">Scene</span>
        <span class="dock-tab" id="import-dock-tab">Import</span>
      </div>
      <!-- pat-l695a: the permanent scene-tree dock toolbar Godot always builds
           in its constructor (scene_tree_dock.cpp filter_hbc): Add Child Node,
           Instantiate Child Scene, the Filter Nodes box, Attach/Detach/Extend
           Script, and the Extra scene options menu. -->
      <div id="scene-tree-toolbar">
        <button id="st-add-child" class="st-tool-btn" title="Add/Create a New Node.">&#65291;</button>
        <button id="st-instantiate" class="st-tool-btn" title="Instantiate a scene file as a Node. Creates an inherited scene if no root node exists.">&#9094;</button>
        <input type="text" id="scene-search" class="st-filter" placeholder="Filter Nodes" autocomplete="off"
          title="Filter nodes by entering a part of their name, type (if prefixed with &quot;type:&quot; or &quot;t:&quot;) or group (if prefixed with &quot;group:&quot; or &quot;g:&quot;). Filtering is case-insensitive.">
        <button id="st-attach-script" class="st-tool-btn" title="Attach a new or existing script to the selected node.">&#128206;</button>
        <button id="st-detach-script" class="st-tool-btn" title="Detach the script from the selected node.">&#9986;</button>
        <button id="st-extend-script" class="st-tool-btn" title="Extend the script of the selected node.">&#10530;</button>
        <button id="st-tree-menu" class="st-tool-btn" title="Extra scene options.">&#8942;</button>
      </div>
      <div id="scene-tree"></div>
      <!-- Import dock (Godot parity): tabbed with Scene; shows import settings for
           the selected resource. Empty-state until a file is selected. -->
      <div id="import-dock" style="display:none">
        <div id="import-empty">Select a resource file in the FileSystem dock to import it.</div>
        <div id="import-content" style="display:none">
          <div class="import-row"><span class="import-label">Import As:</span><select id="import-as" class="import-select"></select></div>
          <div class="import-row"><span class="import-label">Preset</span><button id="import-preset" class="import-btn">Preset</button></div>
          <div id="import-options"></div>
          <button id="import-reimport" class="import-btn import-reimport">Reimport</button>
        </div>
      </div>
    </div>
    <div id="left-divider"></div>
    <div id="filesystem-panel">
      <div class="panel-header">
        <span class="dock-tab active" id="fs-dock-tab">FileSystem</span>
        <span class="dock-tab" id="history-dock-tab">History</span>
      </div>
      <!-- FileSystem chrome (Godot parity): nav + path + split toggle, then a
           filter box + sort button, then the always-present Favorites section. -->
      <div id="fs-chrome">
        <div id="fs-toolbar">
          <button id="fs-nav-prev" class="fs-tool-btn" title="Go to previous selected folder/file." disabled>&#8249;</button>
          <button id="fs-nav-next" class="fs-tool-btn" title="Go to next selected folder/file." disabled>&#8250;</button>
          <input type="text" id="fs-path" class="fs-path-input" value="res://" title="Path" spellcheck="false" autocomplete="off">
          <button id="fs-split-toggle" class="fs-tool-btn" title="Change Split Mode">&#9707;</button>
        </div>
        <div id="fs-toolbar2">
          <input type="text" id="fs-filter" class="fs-filter-input" placeholder="Filter Files" autocomplete="off">
          <button id="fs-sort" class="fs-tool-btn" title="Sort Files">&#8693;</button>
        </div>
        <div id="fs-favorites">
          <div class="fs-fav-header" id="fs-fav-header">
            <span class="fs-fav-toggle" id="fs-fav-toggle">▼</span>
            <span class="fs-fav-star">★</span>
            <span class="fs-fav-label">Favorites:</span>
          </div>
          <div id="fs-fav-list"></div>
        </div>
        <div id="fs-split-box">
          <div id="fs-tree"></div>
          <div id="fs-file-list"></div>
        </div>
        <div id="fs-preview"></div>
      </div>
      <div id="history-dock" style="display:none">
        <div id="history-filters">
          <label><input type="checkbox" id="history-scene" checked> Scene</label>
          <label><input type="checkbox" id="history-global" checked> Global</label>
        </div>
        <div id="history-list"></div>
      </div>
    </div>
  </div>

  <!-- Center: viewport + bottom panel -->
  <div id="center-area">
    <!-- Scene tabs -->
    <div id="scene-tabs">
      <div class="scene-tab active" id="scene-tab-current"><span class="scene-tab-name">Untitled</span><span class="scene-tab-close" title="Close tab">&times;</span></div>
    </div>
    <!-- Viewport -->
    <div id="viewport-panel">
      <div class="panel-header">Viewport</div>
      <!-- 3D spatial editor toolbar (Godot Node3DEditor main_menu_hbox parity,
           node_3d_editor_plugin.cpp): transform tools, list-select, lock/group,
           ruler, local-space + snap, sun/environment preview + settings, and the
           Transform/View menus. Shown only in 3D mode (toggled by editorMode). -->
      <div id="vp3d-toolbar" style="display:none;">
        <button class="vp3d-tb-btn vp3d-tool active" data-vptool="transform" id="vp3d-tool-transform" title="Transform Mode (Q)">&#10021;</button>
        <button class="vp3d-tb-btn vp3d-tool" data-vptool="move" id="vp3d-tool-move" title="Move Mode (W)">&#10011;</button>
        <button class="vp3d-tb-btn vp3d-tool" data-vptool="rotate" id="vp3d-tool-rotate" title="Rotate Mode (E)">&#10227;</button>
        <button class="vp3d-tb-btn vp3d-tool" data-vptool="scale" id="vp3d-tool-scale" title="Scale Mode (R)">&#10530;</button>
        <button class="vp3d-tb-btn vp3d-tool" data-vptool="select" id="vp3d-tool-select" title="Select Mode (V)">&#11034;</button>
        <span class="vp3d-tb-sep"></span>
        <button class="vp3d-tb-btn" id="vp3d-list-select" title="Show list of selectable nodes at position clicked.">&#9776;</button>
        <button class="vp3d-tb-btn" id="vp3d-lock" title="Lock selected node, preventing selection and movement.">&#128274;</button>
        <button class="vp3d-tb-btn" id="vp3d-unlock" title="Unlock selected node, allowing selection and movement.">&#128275;</button>
        <button class="vp3d-tb-btn" id="vp3d-group" title="Groups the selected node with its children. This selects the parent when any child node is clicked in 2D and 3D view.">&#8862;</button>
        <button class="vp3d-tb-btn" id="vp3d-ungroup" title="Ungroups the selected node from its children. Child nodes will be individual items in 2D and 3D view.">&#8863;</button>
        <button class="vp3d-tb-btn" id="vp3d-ruler" title="Ruler Mode (M)&#10;LMB+Drag: Measure the distance between two points in 3D space.">&#128207;</button>
        <span class="vp3d-tb-sep"></span>
        <button class="vp3d-tb-btn" data-flag="local_space" id="vp3d-local-space" title="Use Local Space (T)">&#9672;</button>
        <button class="vp3d-tb-btn" data-flag="snap" id="vp3d-snap" title="Use Snap (Y)">&#9638;</button>
        <span class="vp3d-tb-sep"></span>
        <button class="vp3d-tb-btn active" data-flag="sun" id="vp3d-sun" title="Toggle preview sunlight.&#10;If a DirectionalLight3D node is added to the scene, preview sunlight is disabled.">&#9728;</button>
        <button class="vp3d-tb-btn active" data-flag="environment" id="vp3d-environ" title="Toggle preview environment.&#10;If a WorldEnvironment node is added to the scene, preview environment is disabled.">&#127760;</button>
        <button class="vp3d-tb-btn" id="vp3d-sun-environ-settings" title="Edit Sun and Environment settings.">&#8942;</button>
        <span class="vp3d-tb-sep"></span>
        <div class="vp3d-tb-menu">
          <button class="vp3d-tb-menubtn" id="vp3d-transform-menu-btn">Transform</button>
          <div class="vp3d-tb-dropdown" id="vp3d-transform-dropdown" style="display:none;">
            <div class="vp3d-mi" data-act="snap-floor">Snap Object to Floor<span class="vp3d-shortcut">PgDn</span></div>
            <div class="vp3d-mi" data-act="transform-dialog">Transform Dialog...</div>
            <div class="vp3d-sep"></div>
            <div class="vp3d-mi" data-act="configure-snap">Configure Snap...</div>
          </div>
        </div>
        <div class="vp3d-tb-menu">
          <button class="vp3d-tb-menubtn" id="vp3d-viewmenu-btn">View</button>
          <div class="vp3d-tb-dropdown" id="vp3d-viewmenu-dropdown" style="display:none;">
            <div class="vp3d-mi" data-act="view-1"><span class="vp3d-check">&#10003;</span>1 Viewport</div>
            <div class="vp3d-mi vp3d-disabled" title="Patina uses a single viewport"><span class="vp3d-check"></span>2 Viewports</div>
            <div class="vp3d-mi vp3d-disabled" title="Patina uses a single viewport"><span class="vp3d-check"></span>3 Viewports</div>
            <div class="vp3d-mi vp3d-disabled" title="Patina uses a single viewport"><span class="vp3d-check"></span>4 Viewports</div>
            <div class="vp3d-sep"></div>
            <div class="vp3d-mi" data-act="view-origin"><span class="vp3d-check">&#10003;</span>View Origin</div>
            <div class="vp3d-mi" data-act="view-grid"><span class="vp3d-check">&#10003;</span>View Grid</div>
            <div class="vp3d-sep"></div>
            <div class="vp3d-mi" data-act="view-settings">Settings...</div>
          </div>
        </div>
      </div>
      <div id="viewport-container">
        <div id="viewport-placeholder"><div class="loading-spinner"></div><div>Loading viewport...</div></div>
        <canvas id="viewport-overlay-canvas" style="position:absolute;top:0;left:0;width:100%;height:100%;pointer-events:none;z-index:5;"></canvas>
        <div id="box-select-overlay"></div>
        <div id="snap-indicator">Snap: 8px</div>
        <!-- pat-9yq5q: 2D viewport toolbar (Godot CanvasItemEditor main_menu_hbox,
             canvas_item_editor_plugin.cpp:5702-6040), in Godot order. Shown in 2D
             mode. The transform tools (Q/W/E/R) live in the main editor toolbar. -->
        <div id="vp2d-toolbar">
          <button class="vp2d-btn vp2d-tool" id="vp2d-list-select" title="Show list of selectable nodes at position clicked.">&#9776;</button>
          <button class="vp2d-btn vp2d-tool" id="vp2d-pivot" title="Click to change object's rotation pivot.">&#10043;</button>
          <span class="vp2d-sep"></span>
          <button class="vp2d-btn vp2d-tool" id="vp2d-pan" title="Pan Mode (G). You can also use the Pan View shortcut (Space by default) to pan in any mode.">&#9995;</button>
          <button class="vp2d-btn vp2d-tool" id="vp2d-ruler" title="Ruler Mode (M)">&#128207;</button>
          <span class="vp2d-sep"></span>
          <button class="vp2d-btn vp2d-toggle" id="vp2d-local-space" title="Use Local Space (T)">&#9707;</button>
          <button class="vp2d-btn vp2d-toggle" id="vp2d-smart-snap" title="Toggle smart snapping. (Shift+S)">&#9737;</button>
          <button class="vp2d-btn vp2d-toggle" id="vp2d-grid-snap" title="Toggle grid snapping. (Shift+G)">&#9638;</button>
          <button class="vp2d-btn vp2d-menu-btn" id="vp2d-snap-config" title="Snapping Options">&#9881;&#9662;</button>
          <span class="vp2d-sep"></span>
          <button class="vp2d-btn" id="vp2d-lock" title="Lock the selected object in place (can't be moved).">&#128274;</button>
          <button class="vp2d-btn" id="vp2d-unlock" title="Unlock the selected object (can be moved).">&#128275;</button>
          <button class="vp2d-btn" id="vp2d-group" title="Makes sure the object's children are not selectable.">&#9783;</button>
          <button class="vp2d-btn" id="vp2d-ungroup" title="Restores the object's children's ability to be selected.">&#9784;</button>
          <span class="vp2d-sep"></span>
          <button class="vp2d-btn vp2d-menu-btn" id="vp2d-skeleton" title="Skeleton Options">&#9760;&#9662;</button>
          <span class="vp2d-sep"></span>
          <button class="vp2d-btn vp2d-menu-btn" id="vp2d-view" title="View">View&#9662;</button>
          <span class="vp2d-spacer"></span>
          <div id="vp2d-zoom">
            <button id="vp2d-zoom-out" title="Zoom Out">&minus;</button>
            <span id="vp2d-zoom-label">100%</span>
            <button id="vp2d-zoom-in" title="Zoom In">+</button>
            <button id="vp2d-zoom-reset" title="Zoom Reset">Reset</button>
          </div>
        </div>
        <!-- 3D view menu (Godot's top-left "Perspective" dropdown): projection
             toggle + standard view presets. Shown only in 3D mode. -->
        <div id="vp3d-view-menu" style="display:none;">
          <button id="vp3d-view-btn" title="View / projection">Perspective</button>
          <div id="vp3d-view-dropdown" style="display:none;">
            <div class="vp3d-mi" data-proj="false"><span class="vp3d-check">&#10003;</span>Perspective</div>
            <div class="vp3d-mi" data-proj="true"><span class="vp3d-check"></span>Orthogonal</div>
            <div class="vp3d-sep"></div>
            <div class="vp3d-mi" data-view="top"><span class="vp3d-check"></span>Top</div>
            <div class="vp3d-mi" data-view="bottom"><span class="vp3d-check"></span>Bottom</div>
            <div class="vp3d-mi" data-view="front"><span class="vp3d-check"></span>Front</div>
            <div class="vp3d-mi" data-view="rear"><span class="vp3d-check"></span>Rear</div>
            <div class="vp3d-mi" data-view="left"><span class="vp3d-check"></span>Left</div>
            <div class="vp3d-mi" data-view="right"><span class="vp3d-check"></span>Right</div>
          </div>
        </div>
      </div>
    </div>

    <!-- Script editor (central) — shown in Script mode (pat-qtmbv). The bottom
         #script-panel editor is relocated into #script-main-host while in
         script mode, so the full code editor reuses the existing machinery. -->
    <div id="script-editor-panel" style="display:none">
      <!-- Script editor toolbar (Godot menu_hb parity, script_editor_plugin.cpp:4210-4458):
           File/Search/Debug menus, centered script name, Online Docs, Search Help,
           history arrows, and the make-floating toggle. -->
      <div id="script-toolbar">
        <button class="script-menu-btn" id="script-file-menu">File</button>
        <button class="script-menu-btn" id="script-search-menu">Search</button>
        <button class="script-menu-btn" id="script-debug-menu">Debug</button>
        <span id="script-name-label"></span>
        <button class="script-tool-btn" id="script-online-docs" title="Open Godot online documentation in a browser.">&#127760; Online Docs</button>
        <button class="script-tool-btn" id="script-search-help" title="Search the reference documentation.">&#128269; Search Help</button>
        <span class="script-tb-sep"></span>
        <button class="script-tool-btn script-icon-btn" id="script-back" title="Go to previous edited document." disabled>&#8249;</button>
        <button class="script-tool-btn script-icon-btn" id="script-forward" title="Go to next edited document." disabled>&#8250;</button>
        <span class="script-tb-sep"></span>
        <button class="script-tool-btn script-icon-btn" id="script-make-floating" title="Make the script editor floating.">&#9707;</button>
      </div>
      <div id="script-body">
        <div id="script-list-col">
          <input type="text" id="script-filter" class="script-filter-input" placeholder="Filter Scripts" autocomplete="off">
          <div id="script-list"></div>
          <div id="script-members-bar">
            <input type="text" id="script-method-filter" class="script-filter-input" placeholder="Filter Methods" autocomplete="off">
            <button id="script-members-sort" class="script-tool-btn script-icon-btn" title="Sort alphabetically">&#8593;A</button>
          </div>
          <div id="script-members"></div>
        </div>
        <div id="script-main-host"></div>
      </div>
    </div>

    <!-- Game view (central) — shown in Game mode (pat-xwdlg): embeds the running
         game render, with a Play prompt + controls when stopped. -->
    <div id="game-panel" style="display:none">
      <!-- Game view toolbar (Godot parity, game_view_plugin.cpp:1233-1370):
           process controls | input/2D/3D | selection tools | mute | camera/embed.
           Play/Pause/Stop live in the top-bar run controls (as in Godot). -->
      <div id="game-toolbar">
        <button class="game-tool-btn" id="game-suspend" title="Force pause at SceneTree level. Stops all processing, but you can still interact with the project.">&#9208;</button>
        <button class="game-tool-btn" id="game-next-frame" title="Next Frame">&#9197;</button>
        <button class="game-tool-btn" id="game-speed" title="Change the game speed.">1.0&times;</button>
        <button class="game-tool-btn" id="game-reset-speed" title="Reset the game speed.">&#8635;</button>
        <span class="game-tb-sep"></span>
        <button class="game-tool-btn game-toggle active" id="game-input" title="Allow game input.">Input</button>
        <button class="game-tool-btn game-toggle" id="game-node-2d" title="Disable game input and allow to select Node2Ds, Controls, and manipulate the 2D camera."><span class="game-dot dot-2d"></span>2D</button>
        <button class="game-tool-btn game-toggle" id="game-node-3d" title="Disable game input and allow to select Node3Ds and manipulate the 3D camera."><span class="game-dot dot-3d"></span>3D</button>
        <span class="game-tb-sep"></span>
        <button class="game-tool-btn game-select active" id="game-select-single" title="Select Mode">&#8598;</button>
        <button class="game-tool-btn game-select" id="game-select-list" title="Show list of selectable nodes at position clicked.">&#9776;</button>
        <button class="game-tool-btn" id="game-hide-selection" title="Toggle Selection Visibility">&#128065;</button>
        <button class="game-tool-btn" id="game-selection-options" title="Selection Options">&#8942;</button>
        <span class="game-tb-sep"></span>
        <button class="game-tool-btn" id="game-mute" title="Mute the game audio.">&#128266;</button>
        <span class="game-tb-sep"></span>
        <button class="game-tool-btn" id="game-camera-override" title="Override the in-game camera.">&#128247;</button>
        <button class="game-tool-btn" id="game-camera-menu" title="Camera Options">&#8942;</button>
        <button class="game-tool-btn" id="game-embed-options" title="Embedding Options">&#9635;</button>
      </div>
      <div id="game-screen">
        <img id="game-screen-img" alt="game" style="display:none" />
        <div id="game-idle">
          <div class="game-idle-icon">&#9654;</div>
          <div id="game-status">Press play to start the game.</div>
        </div>
      </div>
    </div>

    <!-- AssetLib browser (central) — shown in AssetLib mode (pat-4jaq7). -->
    <div id="assetlib-panel" style="display:none">
      <div class="panel-header">Asset Library</div>
      <div id="assetlib-toolbar">
        <input id="assetlib-search" type="text" placeholder="Search assets..." />
        <button id="assetlib-import" class="assetlib-tbtn">Import&hellip;</button>
        <button id="assetlib-plugins" class="assetlib-tbtn">Plugins</button>
      </div>
      <div id="assetlib-toolbar2">
        <span class="assetlib-flabel">Sort:</span>
        <select id="assetlib-sort">
          <option value="recent">Recently Updated</option>
          <option value="name">Name</option>
          <option value="rating">Rating</option>
        </select>
        <span class="assetlib-flabel">Category:</span>
        <select id="assetlib-category"><option value="">All</option></select>
        <span class="assetlib-flabel">Site:</span>
        <select id="assetlib-site"><option value="patina">Patina (Official)</option></select>
        <span class="assetlib-flabel">Support:</span>
        <select id="assetlib-support">
          <option value="">All</option>
          <option value="official">Official</option>
          <option value="community">Community</option>
          <option value="testing">Testing</option>
        </select>
      </div>
      <div id="assetlib-grid"><div class="assetlib-loading">Loading assets&hellip;</div></div>
      <div id="assetlib-pagination" style="display:none"></div>
    </div>

    <!-- Bottom panel -->
    <div id="bottom-resize-handle"></div>
    <div id="bottom-panel" class="collapsed" style="height: 240px;">
      <div id="bottom-panel-header">
        <button class="bottom-tab active" data-tab="output">Output</button>
        <button class="bottom-tab" data-tab="debugger">Debugger</button>
        <button class="bottom-tab" data-tab="audio">Audio</button>
        <button class="bottom-tab" data-tab="animation">Animation</button>
        <button class="bottom-tab" data-tab="scene-info">Scene Info</button>
        <button class="bottom-tab" data-tab="script" id="script-tab-btn">Script</button>
        <!-- pat-bc0tv: Shader Editor bottom-panel tab (Godot shader_editor_plugin.cpp
             adds "Shader Editor" to the bottom panel). -->
        <button class="bottom-tab" data-tab="shader" id="shader-tab-btn">Shader Editor</button>
        <div id="bottom-strip-right">
          <span id="bottom-version" title="Patina Engine">Patina</span>
          <button id="bottom-pin" class="bottom-strip-btn" title="Pin Bottom Panel Switching">&#128204;</button>
          <button id="bottom-expand" class="bottom-strip-btn" title="Expand Bottom Panel (Shift+F12)">&#10530;</button>
        </div>
      </div>
      <div id="bottom-panel-content">
        <div class="bottom-content-tab active" data-tab="output">
          <!-- Output panel (Godot EditorLog parity, editor_log.cpp:505-617):
               left = log + "Filter Messages" box; right = Clear/Copy, Collapse/
               Show-Search toggles, and Standard/Errors/Warnings/Editor filters. -->
          <div id="output-panel-body">
            <div id="output-left">
              <div id="output-log"></div>
              <input type="text" id="output-search" class="output-search" placeholder="Filter Messages" autocomplete="off">
            </div>
            <div id="output-toolbar">
              <div class="output-tool-row">
                <button class="output-tool-btn" id="output-clear" title="Clear Output">&#128465;</button>
                <button class="output-tool-btn" id="output-copy" title="Copy Selection">&#128203;</button>
              </div>
              <div class="output-tool-sep"></div>
              <div class="output-tool-row">
                <button class="output-tool-btn" id="output-collapse" title="Collapse duplicate messages into one log entry. Shows number of occurrences.">&#8803;</button>
                <button class="output-tool-btn active" id="output-show-search" title="Focus Search/Filter Bar">&#128269;</button>
              </div>
              <div class="output-tool-sep"></div>
              <button class="output-filter-btn active" id="output-filter-std" title="Toggle visibility of standard output messages."><span class="of-icon ofi-std">&#128172;</span><span class="of-count" id="output-count-std">0</span></button>
              <button class="output-filter-btn active" id="output-filter-error" title="Toggle visibility of errors."><span class="of-icon ofi-error">&#9679;</span><span class="of-count" id="output-count-error">0</span></button>
              <button class="output-filter-btn active" id="output-filter-warning" title="Toggle visibility of warnings."><span class="of-icon ofi-warn">&#9650;</span><span class="of-count" id="output-count-warning">0</span></button>
              <button class="output-filter-btn active" id="output-filter-editor" title="Toggle visibility of editor messages."><span class="of-icon ofi-editor">&#9998;</span><span class="of-count" id="output-count-editor">0</span></button>
            </div>
          </div>
        </div>
        <div class="bottom-content-tab" data-tab="audio">
          <!-- Audio bus layout editor (Godot EditorAudioBuses parity):
               toolbar with Add Bus / Load / Save As / Load Default / Create over
               a horizontal strip of bus channels (Master + user buses). Each
               channel has name, Solo/Mute/Bypass, a vertical fader + VU meter,
               a dB readout, an effect chain, an Add Effect dropdown, and (for
               non-master buses) an output routing dropdown. -->
          <div id="audio-panel">
            <div id="audio-toolbar">
              <span id="audio-layout-label">Layout</span>
              <span id="audio-layout-path">default_bus_layout.tres</span>
              <span class="audio-tb-spacer"></span>
              <button class="audio-tb-btn" id="audio-add-bus" title="Add a new Audio Bus to this layout.">Add Bus</button>
              <button class="audio-tb-btn" id="audio-load" title="Load an existing Bus Layout.">Load</button>
              <button class="audio-tb-btn" id="audio-save-as" title="Save this Bus Layout to a file.">Save As</button>
              <button class="audio-tb-btn" id="audio-load-default" title="Load the default Bus Layout.">Load Default</button>
              <button class="audio-tb-btn" id="audio-create" title="Create a new Bus Layout.">Create</button>
            </div>
            <div id="audio-buses"><div class="audio-empty">Loading audio buses&hellip;</div></div>
          </div>
        </div>
        <div class="bottom-content-tab" data-tab="scene-info">
          <div id="scene-info"></div>
        </div>
        <div class="bottom-content-tab" data-tab="script">
          <div id="script-panel">
            <div class="script-empty">Select a node with a script to view its content</div>
          </div>
        </div>
        <!-- pat-bc0tv: Shader Editor panel (Godot ShaderEditorPlugin bottom panel).
             Structure mirrors shader_editor_plugin.cpp / text_shader_editor.cpp:
             an HSplitContainer with the open-shader list (ItemList) on the left
             and the TextShaderEditor on the right — a menu bar (File / Edit /
             Search / Go To + Online Docs), a code editor with a line-number
             gutter and GDShader syntax highlighting, a warnings panel, and a
             status bar showing the error line and Line/Col. -->
        <div class="bottom-content-tab" data-tab="shader">
          <div id="shader-panel">
            <!-- files_split left: shader_list (ItemList of open shaders) -->
            <div id="shader-files">
              <div id="shader-files-list" role="listbox" aria-label="Open Shaders"></div>
            </div>
            <div id="shader-split-handle" title="Drag to resize the file list"></div>
            <!-- files_split right: TextShaderEditor -->
            <div id="shader-editor-main">
              <div id="shader-menubar">
                <div class="shader-menu" data-menu="file">
                  <button class="shader-menu-btn" type="button">File</button>
                  <div class="shader-menu-pop">
                    <div class="shader-mi" data-act="new"><span>New Shader...</span><span class="shader-mi-key">Ctrl+N</span></div>
                    <div class="shader-mi" data-act="new_include"><span>New Shader Include...</span><span class="shader-mi-key">Ctrl+Shift+N</span></div>
                    <div class="shader-mi-sep"></div>
                    <div class="shader-mi" data-act="open"><span>Load Shader File...</span><span class="shader-mi-key">Ctrl+O</span></div>
                    <div class="shader-mi" data-act="open_include"><span>Load Shader Include File...</span><span class="shader-mi-key">Ctrl+Shift+O</span></div>
                    <div class="shader-mi shader-mi-file" data-act="save"><span>Save</span><span class="shader-mi-key">Ctrl+Alt+S</span></div>
                    <div class="shader-mi shader-mi-file" data-act="save_as"><span>Save As...</span><span class="shader-mi-key"></span></div>
                    <div class="shader-mi-sep"></div>
                    <div class="shader-mi shader-mi-file" data-act="inspect"><span>Open File in Inspector</span><span class="shader-mi-key"></span></div>
                    <div class="shader-mi shader-mi-file" data-act="inspect_native"><span>Inspect Native Shader Code...</span><span class="shader-mi-key"></span></div>
                    <div class="shader-mi-sep"></div>
                    <div class="shader-mi shader-mi-file" data-act="close"><span>Close</span><span class="shader-mi-key">Ctrl+W</span></div>
                    <div class="shader-mi-sep"></div>
                    <div class="shader-mi" data-act="toggle_files"><span>Toggle Files Panel</span><span class="shader-mi-key">Ctrl+\</span></div>
                  </div>
                </div>
                <div class="shader-menu" data-menu="edit">
                  <button class="shader-menu-btn" type="button">Edit</button>
                  <div class="shader-menu-pop">
                    <div class="shader-mi" data-act="undo"><span>Undo</span><span class="shader-mi-key">Ctrl+Z</span></div>
                    <div class="shader-mi" data-act="redo"><span>Redo</span><span class="shader-mi-key">Ctrl+Shift+Z</span></div>
                    <div class="shader-mi-sep"></div>
                    <div class="shader-mi" data-act="cut"><span>Cut</span><span class="shader-mi-key">Ctrl+X</span></div>
                    <div class="shader-mi" data-act="copy"><span>Copy</span><span class="shader-mi-key">Ctrl+C</span></div>
                    <div class="shader-mi" data-act="paste"><span>Paste</span><span class="shader-mi-key">Ctrl+V</span></div>
                    <div class="shader-mi-sep"></div>
                    <div class="shader-mi" data-act="select_all"><span>Select All</span><span class="shader-mi-key">Ctrl+A</span></div>
                    <div class="shader-mi-sep"></div>
                    <div class="shader-mi" data-act="move_up"><span>Move Up</span><span class="shader-mi-key">Alt+Up</span></div>
                    <div class="shader-mi" data-act="move_down"><span>Move Down</span><span class="shader-mi-key">Alt+Down</span></div>
                    <div class="shader-mi" data-act="indent"><span>Indent</span><span class="shader-mi-key"></span></div>
                    <div class="shader-mi" data-act="unindent"><span>Unindent</span><span class="shader-mi-key">Shift+Tab</span></div>
                    <div class="shader-mi" data-act="delete_line"><span>Delete Line</span><span class="shader-mi-key">Ctrl+Shift+K</span></div>
                    <div class="shader-mi" data-act="toggle_comment"><span>Toggle Comment</span><span class="shader-mi-key">Ctrl+K</span></div>
                    <div class="shader-mi" data-act="duplicate_selection"><span>Duplicate Selection</span><span class="shader-mi-key">Ctrl+Shift+D</span></div>
                    <div class="shader-mi" data-act="duplicate_lines"><span>Duplicate Lines</span><span class="shader-mi-key">Ctrl+Alt+Down</span></div>
                    <div class="shader-mi" data-act="toggle_word_wrap"><span>Toggle Word Wrap</span><span class="shader-mi-key">Alt+Z</span></div>
                    <div class="shader-mi-sep"></div>
                    <div class="shader-mi" data-act="complete"><span>Complete Symbol</span><span class="shader-mi-key">Ctrl+Space</span></div>
                  </div>
                </div>
                <div class="shader-menu" data-menu="search">
                  <button class="shader-menu-btn" type="button">Search</button>
                  <div class="shader-menu-pop">
                    <div class="shader-mi" data-act="find"><span>Find...</span><span class="shader-mi-key">Ctrl+F</span></div>
                    <div class="shader-mi" data-act="find_next"><span>Find Next</span><span class="shader-mi-key">F3</span></div>
                    <div class="shader-mi" data-act="find_prev"><span>Find Previous</span><span class="shader-mi-key">Shift+F3</span></div>
                    <div class="shader-mi" data-act="replace"><span>Replace...</span><span class="shader-mi-key">Ctrl+R</span></div>
                  </div>
                </div>
                <div class="shader-menu" data-menu="goto">
                  <button class="shader-menu-btn" type="button">Go To</button>
                  <div class="shader-menu-pop">
                    <div class="shader-mi" data-act="goto_line"><span>Go to Line...</span><span class="shader-mi-key">Ctrl+G</span></div>
                    <div class="shader-mi-sep"></div>
                    <div class="shader-mi shader-mi-sub" data-submenu="bookmarks"><span>Bookmarks</span><span class="shader-mi-key">&#9656;</span>
                      <div class="shader-menu-pop shader-submenu-pop">
                        <div class="shader-mi" data-act="toggle_bookmark"><span>Toggle Bookmark</span><span class="shader-mi-key">Ctrl+Alt+B</span></div>
                        <div class="shader-mi" data-act="remove_bookmarks"><span>Remove All Bookmarks</span><span class="shader-mi-key"></span></div>
                        <div class="shader-mi" data-act="next_bookmark"><span>Go to Next Bookmark</span><span class="shader-mi-key">Ctrl+B</span></div>
                        <div class="shader-mi" data-act="prev_bookmark"><span>Go to Previous Bookmark</span><span class="shader-mi-key">Ctrl+Shift+B</span></div>
                      </div>
                    </div>
                  </div>
                </div>
                <span class="shader-menu-spacer"></span>
                <button id="shader-online-docs" class="shader-docs-btn" type="button" title="Open Godot online documentation.">Online Docs</button>
              </div>
              <!-- FindReplaceBar (Godot), hidden until Find/Replace is invoked -->
              <div id="shader-find-bar">
                <input type="text" id="shader-find-input" placeholder="Find" spellcheck="false" autocomplete="off">
                <button class="shader-find-btn" id="shader-find-prev" type="button" title="Find Previous">&#8593;</button>
                <button class="shader-find-btn" id="shader-find-next" type="button" title="Find Next">&#8595;</button>
                <span id="shader-find-count" class="shader-find-count"></span>
                <input type="text" id="shader-replace-input" placeholder="Replace" spellcheck="false" autocomplete="off" style="display:none">
                <button class="shader-find-btn" id="shader-replace-one" type="button" style="display:none">Replace</button>
                <button class="shader-find-btn" id="shader-replace-all" type="button" style="display:none">Replace All</button>
                <button class="shader-find-btn" id="shader-find-close" type="button" title="Close">&times;</button>
              </div>
              <div id="shader-code-area">
                <div id="shader-gutter" aria-hidden="true"></div>
                <div id="shader-code-wrap">
                  <pre id="shader-highlight" aria-hidden="true"></pre>
                  <textarea id="shader-source" spellcheck="false" autocomplete="off" autocapitalize="off" wrap="off" placeholder="// shader source"></textarea>
                </div>
                <div id="shader-empty" class="shader-empty">No shaders open.<br>Open a <b>.gdshader</b> file from the FileSystem dock, or use <b>File &#9656; New Shader...</b></div>
              </div>
              <!-- warnings_panel (RichTextLabel), hidden when the shader is clean -->
              <div id="shader-warnings"></div>
              <!-- CodeTextEditor status bar: error indicator + Line/Col -->
              <div id="shader-status">
                <span id="shader-status-err" class="shader-status-err"></span>
                <span class="shader-status-spacer"></span>
                <span id="shader-status-zoom" class="shader-status-zoom" title="Zoom">100%</span>
                <span id="shader-status-linecol" class="shader-status-linecol">Line 1, Col 1</span>
              </div>
            </div>
          </div>
        </div>
        <div class="bottom-content-tab" data-tab="animation">
          <!-- AnimationTree state-machine view: contained in the bottom panel,
               shown only when the tree button toggles it (pat-v4o3a). -->
          <div id="anim-tree-dialog">
            <div id="anim-tree-inner">
              <div id="anim-tree-header">
                <span>AnimationTree</span>
                <button id="anim-tree-close" title="Close">&times;</button>
              </div>
              <div id="anim-tree-body">
                <div class="anim-tree-node active">AnimationNodeStateMachine (root)</div>
                <div class="anim-tree-node">AnimationNodeAnimation: idle</div>
                <div class="anim-tree-node">AnimationNodeAnimation: walk</div>
                <div class="anim-tree-node">AnimationNodeAnimation: run</div>
                <div style="padding:8px 0;color:var(--text-dim);font-size:11px">Connect nodes to create animation state transitions.</div>
              </div>
            </div>
          </div>
          <div id="animation-panel">
            <div class="anim-toolbar">
              <select id="anim-select"><option value="">-- No Animation --</option></select>
              <button id="anim-new-btn" title="New Animation">+</button>
              <button id="anim-delete-btn" title="Delete Animation">&#x2715;</button>
              <span class="anim-sep"></span>
              <button id="anim-record-btn" class="anim-record" title="Toggle Recording">&#9679;</button>
              <button id="anim-play-btn" title="Play">&#9654;</button>
              <button id="anim-stop-btn" title="Stop">&#9632;</button>
              <button id="anim-curve-btn" title="Edit Curve (select a keyframe first)">&#x2312;</button>
              <button id="anim-onion-btn" class="anim-onion" title="Toggle Onion Skinning">&#x1f9c5;</button>
              <button id="anim-tree-btn" title="Open AnimationTree">&#x1f332;</button>
              <span id="anim-time-display">0.00 / 0.00</span>
            </div>
            <div class="anim-blend-toolbar" id="anim-blend-toolbar" style="display:none;padding:2px 8px;gap:6px;align-items:center;font-size:11px;border-bottom:1px solid var(--border);background:var(--panel)">
              <span style="color:var(--text-dim);white-space:nowrap">Blend:</span>
              <select id="anim-blend-select" style="max-width:120px"><option value="">-- None --</option></select>
              <input type="range" id="anim-blend-slider" min="0" max="100" value="0" style="flex:1;min-width:60px;max-width:160px" disabled>
              <span id="anim-blend-weight" style="color:var(--accent);min-width:32px">0%</span>
            </div>
            <div class="anim-timeline-area">
              <div class="anim-tracks" id="anim-tracks">
                <div class="anim-empty">No animation selected</div>
              </div>
              <div class="anim-timeline" id="anim-timeline">
                <canvas id="anim-timeline-canvas" width="600" height="120"></canvas>
                <div id="anim-playhead" class="anim-playhead"></div>
              </div>
            </div>
            <div class="anim-add-track-bar">
              <button id="anim-add-track-btn">+ Add Track</button>
            </div>
          </div>
        </div>
        <div class="bottom-content-tab" data-tab="debugger">
          <!-- Debugger (Godot ScriptEditorDebugger parity, script_editor_debugger.cpp:2062-2386):
               sub-tab strip Stack Trace / Errors / Evaluator / Profiler / Visual
               Profiler / Monitors / Video RAM / Misc / Network Profiler. The debug
               control toolbar + Thread/Stack Frames/Breakpoints live in Stack Trace. -->
          <div id="debugger-panel">
            <div id="debugger-subtabs">
              <span class="dbg-subtab active" data-dbgtab="stack-trace">Stack Trace</span>
              <span class="dbg-subtab" data-dbgtab="errors">Errors</span>
              <span class="dbg-subtab" data-dbgtab="evaluator">Evaluator</span>
              <span class="dbg-subtab" data-dbgtab="profiler">Profiler</span>
              <span class="dbg-subtab" data-dbgtab="visual-profiler">Visual Profiler</span>
              <span class="dbg-subtab" data-dbgtab="monitors">Monitors</span>
              <span class="dbg-subtab" data-dbgtab="video-ram">Video RAM</span>
              <span class="dbg-subtab" data-dbgtab="misc">Misc</span>
              <span class="dbg-subtab" data-dbgtab="network-profiler">Network Profiler</span>
              <span class="dbg-subtabs-spacer"></span>
              <button class="dbg-tool" id="debugger-expand" title="Expand Bottom Panel">&#9974;</button>
              <button class="dbg-tool" id="debugger-float" title="Make Floating">&#9707;</button>
            </div>
            <div id="debugger-subcontent">
              <div class="dbg-subpanel active" data-dbgtab="stack-trace">
                <div id="debug-headrow">
                  <div class="debug-thread-row"><span class="debug-thread-label">Thread:</span><select id="debug-thread"><option>Main</option></select></div>
                  <input type="text" id="debug-var-filter" class="debug-filter-input" placeholder="Filter Stack Variables" autocomplete="off">
                  <span id="debug-status" class="debug-status"></span>
                  <div id="debug-toolbar">
                    <button class="dbg-tool" id="debug-btn-skip-bp" title="Skip Breakpoints">&#8856;</button>
                    <button class="dbg-tool" id="debug-btn-ignore-errors" title="Ignore Error Breaks">&#9888;&#65038;</button>
                    <button class="dbg-tool" id="debug-btn-copy-error" title="Copy Error">&#10697;</button>
                    <span class="dbg-tb-sep"></span>
                    <button class="dbg-tool" id="debug-btn-step-in" title="Step Into (F11)">&#8628;</button>
                    <button class="dbg-tool" id="debug-btn-step-over" title="Step Over (F10)">&#8631;</button>
                    <button class="dbg-tool" id="debug-btn-step-out" title="Step Out (Shift+F11)">&#8624;</button>
                    <span class="dbg-tb-sep"></span>
                    <button class="dbg-tool" id="debug-btn-break" title="Break">&#9208;&#65038;</button>
                    <button class="dbg-tool" id="debug-btn-continue" title="Continue (F12)">&#9654;&#65038;</button>
                  </div>
                </div>
                <div id="debug-split">
                  <div class="debug-col">
                    <div class="debug-section-label">Stack Frames</div>
                    <div id="debug-stack-frames"><div class="insp-empty" style="padding:8px">No active debug session</div></div>
                  </div>
                  <div class="debug-col">
                    <div class="debug-section-label">Locals</div>
                    <div id="debug-variables"><div class="insp-empty" style="padding:8px">Not paused</div></div>
                    <div class="debug-section-label">Globals</div>
                    <div id="debug-globals"><div class="insp-empty" style="padding:8px">Not paused</div></div>
                  </div>
                  <div class="debug-col">
                    <div class="debug-section-label">Breakpoints</div>
                    <div id="debug-breakpoints-list"><div class="insp-empty" style="padding:8px">No breakpoints set</div></div>
                  </div>
                </div>
              </div>
              <div class="dbg-subpanel" data-dbgtab="errors">
                <div id="debug-errors-toolbar">
                  <button class="dbg-text-btn" id="debug-errors-expand">Expand All</button>
                  <button class="dbg-text-btn" id="debug-errors-collapse">Collapse All</button>
                  <button class="dbg-text-btn" id="debug-errors-clear" style="margin-left:auto">Clear</button>
                </div>
                <div id="debug-errors-list"><div class="insp-empty" style="padding:8px">No errors.</div></div>
              </div>
              <div class="dbg-subpanel" data-dbgtab="evaluator">
                <!-- pat-j906k: Evaluator REPL -->
                <div id="dbg-eval-output" style="flex:1;overflow:auto;font-family:monospace;font-size:11px;padding:4px;min-height:60px;"></div>
                <div style="display:flex;gap:4px;margin-top:4px;flex-shrink:0;">
                  <input type="text" id="dbg-eval-input" placeholder="Evaluate expression (e.g. 2 + 3)..." style="flex:1;font-family:monospace;font-size:11px;" autocomplete="off">
                  <button class="dbg-text-btn" id="dbg-eval-run">Evaluate</button>
                </div>
              </div>
              <div class="dbg-subpanel" data-dbgtab="profiler">
                <!-- Profiler (Godot EditorProfiler parity, editor_profiler.cpp:680-787):
                     toolbar [Start, Clear, Autostart, Measure:, Time:, Frame #:] then an
                     HSplit of a Name|Time|Calls function tree + the frame-time graph. -->
                <div id="profiler-panel">
                  <div id="profiler-toolbar">
                    <button class="dbg-text-btn" id="profiler-activate">Start</button>
                    <button class="dbg-text-btn" id="profiler-clear">Clear</button>
                    <label class="profiler-check"><input type="checkbox" id="profiler-autostart"> Autostart</label>
                    <span class="profiler-tb-group"><span class="profiler-tb-label">Measure:</span><select id="profiler-measure" class="profiler-select"><option>Frame Time (ms)</option><option>Average Time (ms)</option><option>Frame %</option><option>Physics Frame %</option></select></span>
                    <span class="profiler-tb-group"><span class="profiler-tb-label">Time:</span><select id="profiler-time" class="profiler-select" title="Inclusive: Includes time from other functions called by this function.&#10;Self: Only count the time spent in the function itself."><option>Inclusive</option><option>Self</option></select></span>
                    <span class="profiler-tb-spacer"></span>
                    <span class="profiler-tb-group"><span class="profiler-tb-label">Frame #:</span><input type="number" id="profiler-frame" class="profiler-frame-input" value="0" min="0"></span>
                  </div>
                  <div id="profiler-split">
                    <div id="profiler-tree">
                      <div id="profiler-tree-head"><span class="pcol pcol-name">Name</span><span class="pcol pcol-time">Time</span><span class="pcol pcol-calls">Calls</span></div>
                      <div id="profiler-func-breakdown"></div>
                    </div>
                    <div id="profiler-graph-wrap">
                      <canvas id="profiler-graph-canvas" class="monitor-graph" width="600" height="240"></canvas>
                    </div>
                  </div>
                  <span id="profiler-fps" style="display:none"></span><span id="profiler-avg" style="display:none"></span><span id="profiler-max" style="display:none"></span>
                </div>
              </div>
              <div class="dbg-subpanel" data-dbgtab="visual-profiler">
                <!-- pat-j906k: visual frame-time profiler -->
                <div style="font-size:11px;color:var(--text-dim);margin-bottom:4px;">Frame time (per captured frame)</div>
                <canvas id="dbg-visual-canvas" class="monitor-graph" width="600" height="160"></canvas>
                <div id="dbg-visual-empty" class="insp-empty" style="padding:8px">No frames captured. Start the Profiler.</div>
              </div>
              <div class="dbg-subpanel" data-dbgtab="monitors">
                <!-- Monitors (Godot EditorPerformanceProfiler parity, editor_performance_profiler.cpp:413-444):
                     HSplit of a Monitor|Value tree (categories: Time/Memory/Object/Raster/Video/
                     Physics 2D/Physics 3D/Navigation/Audio, each monitor a checkbox) + a graph that
                     plots the checked monitors. -->
                <div id="monitors-panel">
                  <div id="monitors-split">
                    <div id="monitor-tree">
                      <div id="monitor-tree-head"><span class="mcol mcol-name">Monitor</span><span class="mcol mcol-value">Value</span></div>
                      <div id="monitor-tree-body"></div>
                    </div>
                    <div id="monitor-graph-wrap">
                      <div id="monitor-graph-empty">Pick one or more items from the list to display the graph.</div>
                      <canvas id="monitor-graph-canvas" class="monitor-graph" width="600" height="400" style="display:none"></canvas>
                    </div>
                  </div>
                  <span id="monitor-fps" style="display:none"></span><span id="monitor-avg" style="display:none"></span><span id="monitor-max" style="display:none"></span>
                </div>
              </div>
              <!-- Video RAM (Godot ScriptEditorDebugger vmem, script_editor_debugger.cpp:2308-2369):
                   header "List of Video Memory Usage by Resource:" + notice + Total: field +
                   refresh/export, then a Resource Path | Type | Format | Usage table. -->
              <div class="dbg-subpanel" data-dbgtab="video-ram">
                <div id="dbg-vram-header">
                  <span class="dbg-vram-title">List of Video Memory Usage by Resource:</span>
                  <span class="dbg-vram-notice" title="Notice: This tool only reports memory allocations tracked by the engine, so total VRAM usage is inaccurate compared to the Monitors tab or external tools. Buffer Memory (e.g. GPUParticles) is not tracked; Meshes are not tracked in the Compatibility renderer.">&#9432;</span>
                  <span class="dbg-vram-spacer"></span>
                  <span class="dbg-vram-totallabel">Total:</span>
                  <input type="text" id="dbg-vram-total" class="dbg-vram-totalinput" readonly>
                  <button class="dbg-tool" id="dbg-vram-refresh" title="Refresh Video RAM">&#8635;</button>
                  <button class="dbg-tool" id="dbg-vram-export" title="Export list to a CSV file">&#128190;</button>
                </div>
                <div id="dbg-vram-table">
                  <div id="dbg-vram-head"><span class="vrcol vrcol-path">Resource Path</span><span class="vrcol vrcol-type">Type</span><span class="vrcol vrcol-format">Format</span><span class="vrcol vrcol-usage">Usage</span></div>
                  <div id="dbg-vram-list"></div>
                </div>
              </div>
              <div class="dbg-subpanel" data-dbgtab="misc">
                <!-- Misc (Godot ScriptEditorDebugger misc tab, script_editor_debugger.cpp:2383-2432):
                     a 2-column GridContainer of Clicked Control: / Clicked Control Type:
                     read-only fields and a Live Edit Root: field with Set From Tree / Clear,
                     a separator, then the Export measures as CSV button. -->
                <div id="dbg-misc">
                  <div id="dbg-misc-grid">
                    <label class="dbg-misc-label">Clicked Control:</label>
                    <input type="text" id="dbg-clicked-ctrl" class="dbg-misc-input" readonly>
                    <label class="dbg-misc-label">Clicked Control Type:</label>
                    <input type="text" id="dbg-clicked-ctrl-type" class="dbg-misc-input" readonly>
                    <label class="dbg-misc-label">Live Edit Root:</label>
                    <div class="dbg-misc-lehb">
                      <input type="text" id="dbg-live-edit-root" class="dbg-misc-input" readonly>
                      <button class="dbg-text-btn" id="dbg-le-set" title="Set the Live Edit Root from the selected scene-tree node">Set From Tree</button>
                      <button class="dbg-text-btn" id="dbg-le-clear" title="Reset the Live Edit Root to /root">Clear</button>
                    </div>
                  </div>
                  <div class="dbg-misc-sep"></div>
                  <div class="dbg-misc-buttons">
                    <button class="dbg-text-btn" id="dbg-export-csv">Export measures as CSV</button>
                  </div>
                </div>
              </div>
              <div class="dbg-subpanel" data-dbgtab="network-profiler">
                <!-- Network Profiler (Godot EditorNetworkProfiler parity,
                     editor_network_profiler.cpp:318-446): a FlowContainer toolbar
                     [Start/Stop toggle, Clear, Autostart, spacer, Down <bandwidth>
                     Up <bandwidth>] over an HSplit of the RPC counters tree
                     (Node | Incoming RPC | Outgoing RPC) and the replication tree
                     (Root | Synchronizer | Config | Count | Size). -->
                <div id="net-profiler-panel">
                  <div id="net-profiler-toolbar">
                    <button class="dbg-text-btn" id="net-activate" disabled title="Start/stop capturing network traffic">&#9654; Start</button>
                    <button class="dbg-text-btn" id="net-clear" disabled title="Clear captured RPC and replication data">&#10006; Clear</button>
                    <label class="profiler-check"><input type="checkbox" id="net-autostart"> Autostart</label>
                    <span class="net-tb-spacer"></span>
                    <span class="net-bw-group">
                      <span class="net-bw-label" id="net-down-label">Down</span>
                      <span class="net-bw-field"><span class="net-bw-val net-bw-dim" id="net-incoming">0 B/s</span><span class="net-bw-arrow">&#8595;</span></span>
                    </span>
                    <span class="net-bw-mid"></span>
                    <span class="net-bw-group">
                      <span class="net-bw-label" id="net-up-label">Up</span>
                      <span class="net-bw-field"><span class="net-bw-val net-bw-dim" id="net-outgoing">0 B/s</span><span class="net-bw-arrow">&#8593;</span></span>
                    </span>
                  </div>
                  <div id="net-profiler-split">
                    <div class="net-tree" id="net-rpc-tree">
                      <div class="net-tree-head">
                        <span class="ncol ncol-node">Node</span>
                        <span class="ncol ncol-num">Incoming RPC</span>
                        <span class="ncol ncol-num">Outgoing RPC</span>
                      </div>
                      <div class="net-tree-body" id="net-rpc-body"></div>
                    </div>
                    <div class="net-tree" id="net-repl-tree">
                      <div class="net-tree-head">
                        <span class="ncol ncol-root">Root</span>
                        <span class="ncol ncol-sync">Synchronizer</span>
                        <span class="ncol ncol-config">Config</span>
                        <span class="ncol ncol-num ncol-count">Count</span>
                        <span class="ncol ncol-num ncol-size">Size</span>
                      </div>
                      <div class="net-tree-body" id="net-repl-body"></div>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>

  <!-- Inspector / Node dock (drag the left edge to resize, Godot-style) -->
  <div id="inspector-divider" title="Drag to resize the inspector"></div>
  <div id="inspector-panel">
    <div class="right-panel-tabs">
      <button class="right-panel-tab active" data-rptab="inspector">Inspector</button>
      <button class="right-panel-tab" data-rptab="node">Node</button>
    </div>
    <div id="inspector-content" class="right-panel-content active" data-rptab="inspector">
      <div id="inspector">
        <div class="insp-empty">Select a node to inspect</div>
      </div>
    </div>
    <div id="node-dock-content" class="right-panel-content" data-rptab="node">
      <div id="node-dock">
        <div class="insp-empty">Select a node to view signals</div>
      </div>
    </div>
  </div>
</div>

<!-- Connect signal dialog -->
<div class="connect-dialog-overlay" id="connect-dialog-overlay">
  <div class="connect-dialog">
    <h3>Connect Signal</h3>
    <label>Signal</label>
    <input type="text" id="connect-signal-name" readonly>
    <label>Target Method</label>
    <input type="text" id="connect-method-name" placeholder="_on_signal_name">
    <!-- pat-jwuv1.3: advanced connect flags (Godot ConnectDialog deferred/oneshot)
         + bound extra call arguments. -->
    <label>Extra Call Arguments (comma-separated)</label>
    <input type="text" id="connect-binds" placeholder="e.g. 42, &quot;hello&quot;">
    <label style="display:flex;align-items:center;gap:6px;margin-top:6px;cursor:pointer;">
      <input type="checkbox" id="connect-deferred"> Deferred
      <span style="color:var(--text-dim);font-weight:normal">— emit at idle, not immediately</span>
    </label>
    <label style="display:flex;align-items:center;gap:6px;cursor:pointer;">
      <input type="checkbox" id="connect-oneshot"> One Shot
      <span style="color:var(--text-dim);font-weight:normal">— disconnect after first emission</span>
    </label>
    <div class="connect-dialog-buttons">
      <button id="connect-cancel">Cancel</button>
      <button id="connect-confirm" style="border-color:var(--accent);color:var(--accent)">Connect</button>
    </div>
  </div>
</div>


<!-- Command palette (pat-ugb0p) -->
<div id="command-palette">
  <div id="command-palette-inner">
    <input type="text" id="cmd-search" placeholder="Type a command..." autocomplete="off" spellcheck="false">
    <div id="cmd-results"></div>
  </div>
</div>

<!-- Settings dialog (pat-sbdts: categorized with theme, keybindings, plugins) -->
<div id="settings-dialog">
  <div id="settings-dialog-inner" style="width:620px;max-height:80vh;display:flex;flex-direction:column;">
    <!-- Header with tabs -->
    <div style="display:flex;border-bottom:1px solid var(--border);padding:0;">
      <h3 style="font-size:14px;color:var(--accent);margin:0;padding:12px 16px;flex-shrink:0;">Editor Settings</h3>
      <div id="eset-tabs" style="display:flex;align-items:stretch;gap:0;margin-left:auto;">
        <div class="eset-tab active" data-tab="general" style="padding:10px 14px;font-size:11px;cursor:pointer;border-bottom:2px solid var(--accent);color:var(--accent);">General</div>
        <div class="eset-tab" data-tab="theme" style="padding:10px 14px;font-size:11px;cursor:pointer;border-bottom:2px solid transparent;color:var(--text-dim);">Theme</div>
        <div class="eset-tab" data-tab="keybindings" style="padding:10px 14px;font-size:11px;cursor:pointer;border-bottom:2px solid transparent;color:var(--text-dim);">Keybindings</div>
        <div class="eset-tab" data-tab="plugins" style="padding:10px 14px;font-size:11px;cursor:pointer;border-bottom:2px solid transparent;color:var(--text-dim);">Plugins</div>
      </div>
    </div>
    <!-- Tab content -->
    <div style="flex:1;overflow-y:auto;padding:12px 16px;min-height:260px;">
      <!-- General tab -->
      <div class="eset-section" data-tab="general">
        <div class="settings-row"><span class="settings-label">Grid Snap</span><div class="settings-value"><input type="checkbox" id="set-grid-snap"> <label for="set-grid-snap">Enabled</label></div></div>
        <div class="settings-row"><span class="settings-label">Snap Size</span><div class="settings-value"><select id="set-snap-size"><option value="8">8</option><option value="16">16</option><option value="32">32</option><option value="64">64</option></select></div></div>
        <div class="settings-row"><span class="settings-label">Grid Visible</span><div class="settings-value"><input type="checkbox" id="set-grid-visible" checked></div></div>
        <div class="settings-row"><span class="settings-label">Rulers Visible</span><div class="settings-value"><input type="checkbox" id="set-rulers-visible" checked></div></div>
        <div class="settings-row"><span class="settings-label">Font Size</span><div class="settings-value"><select id="set-font-size"><option value="small">Small</option><option value="medium" selected>Medium</option><option value="large">Large</option></select></div></div>
        <div class="settings-row"><span class="settings-label">Physics FPS</span><div class="settings-value"><select id="set-physics-fps"><option value="30">30</option><option value="60" selected>60</option><option value="120">120</option></select></div></div>
      </div>
      <!-- Theme tab -->
      <div class="eset-section" data-tab="theme" style="display:none;">
        <div class="settings-row"><span class="settings-label">Color Theme</span><div class="settings-value"><select id="set-theme"><option value="dark" selected>Dark</option><option value="light">Light</option></select></div></div>
        <div class="settings-row"><span class="settings-label">Accent Color</span><div class="settings-value"><select id="set-accent-color"><option value="#579eff" selected>Godot Blue</option><option value="#4ec9b0">Teal</option><option value="#c586c0">Purple</option><option value="#6a9955">Green</option><option value="#cc8e5c">Burnt Sienna</option><option value="#ce9178">Coral</option></select></div></div>
        <div class="settings-row"><span class="settings-label">Icon Size</span><div class="settings-value"><select id="set-icon-size"><option value="small">Small</option><option value="medium" selected>Medium</option><option value="large">Large</option></select></div></div>
        <div style="margin-top:12px;padding:8px;border:1px solid var(--border);border-radius:4px;font-size:11px;color:var(--text-dim);">
          <strong style="color:var(--text);">Preview:</strong>
          <div id="eset-theme-preview" style="margin-top:6px;padding:6px;background:var(--bg);border-radius:3px;">
            <span style="color:var(--accent);">Accent text</span> &middot;
            <span style="color:var(--text);">Normal text</span> &middot;
            <span style="color:var(--text-dim);">Dim text</span>
          </div>
        </div>
      </div>
      <!-- Keybindings tab -->
      <div class="eset-section" data-tab="keybindings" style="display:none;">
        <div style="margin-bottom:8px;"><input type="text" id="eset-kb-search" placeholder="Search keybindings..." style="width:100%;font-size:11px;padding:4px 8px;"></div>
        <div id="eset-kb-list" style="max-height:240px;overflow-y:auto;">
          <!-- Populated by JS -->
        </div>
      </div>
      <!-- Plugins tab -->
      <div class="eset-section" data-tab="plugins" style="display:none;">
        <div id="eset-plugin-list">
          <!-- Populated by JS -->
          <div style="color:var(--text-dim);font-size:11px;padding:8px;">No plugins registered.</div>
        </div>
      </div>
    </div>
    <!-- Footer -->
    <div style="padding:10px 16px;border-top:1px solid var(--border);text-align:right;">
      <button id="settings-close">Close</button>
    </div>
  </div>
</div>

<!-- Project Settings dialog (pat-kj4 / pat-c4zlm) -->
<div id="project-settings-dialog" style="display:none;position:fixed;top:0;left:0;width:100%;height:100%;background:rgba(0,0,0,0.5);z-index:300;align-items:center;justify-content:center;">
  <div style="background:var(--panel);border:1px solid var(--border);border-radius:6px;width:620px;max-height:80vh;display:flex;flex-direction:column;box-shadow:0 8px 32px rgba(0,0,0,0.6);">
    <div style="display:flex;align-items:center;padding:12px 16px;border-bottom:1px solid var(--border);">
      <h3 style="font-size:14px;color:var(--accent);margin:0;flex:1;">Project Settings</h3>
      <input type="text" id="pset-search" placeholder="Filter settings..." style="width:160px;font-size:11px;margin-right:8px;">
      <button id="pset-close" style="background:none;border:none;color:var(--text-dim);cursor:pointer;font-size:16px;">&times;</button>
    </div>
    <div style="display:flex;flex:1;overflow:hidden;min-height:340px;">
      <!-- Category sidebar -->
      <div id="pset-categories" style="width:130px;border-right:1px solid var(--border);padding:8px 0;">
        <div class="pset-cat active" data-cat="application" style="padding:6px 12px;cursor:pointer;font-size:11px;">Application</div>
        <!-- pat-db37w: Input Map tab (Godot Project Settings > Input Map) -->
        <div class="pset-cat" data-cat="input" style="padding:6px 12px;cursor:pointer;font-size:11px;">Input Map</div>
        <div class="pset-cat" data-cat="display" style="padding:6px 12px;cursor:pointer;font-size:11px;">Display</div>
        <div class="pset-cat" data-cat="physics" style="padding:6px 12px;cursor:pointer;font-size:11px;">Physics</div>
        <div class="pset-cat" data-cat="audio" style="padding:6px 12px;cursor:pointer;font-size:11px;">Audio</div>
        <div class="pset-cat" data-cat="rendering" style="padding:6px 12px;cursor:pointer;font-size:11px;">Rendering</div>
      </div>
      <!-- Properties panel -->
      <div id="pset-props" style="flex:1;overflow-y:auto;padding:12px 16px;">
        <!-- Application -->
        <div class="pset-section" data-cat="application">
          <div class="settings-row"><span class="settings-label">Project Name</span><div class="settings-value"><input type="text" id="pset-name" data-key="project_name" style="width:100%"></div></div>
          <div class="settings-row"><span class="settings-label">Main Scene</span><div class="settings-value"><input type="text" id="pset-main-scene" data-key="main_scene" placeholder="res://..." style="width:100%"></div></div>
          <div class="settings-row"><span class="settings-label">Description</span><div class="settings-value"><input type="text" id="pset-desc" data-key="description" style="width:100%"></div></div>
          <div class="settings-row"><span class="settings-label">Icon Path</span><div class="settings-value"><input type="text" id="pset-icon" data-key="icon" placeholder="res://icon.svg" style="width:100%"></div></div>
        </div>
        <!-- pat-db37w: Input Map (live: backed by /api/input_map) -->
        <div class="pset-section" data-cat="input" style="display:none;">
          <div id="pset-input-map"></div>
          <div class="input-map-add" style="display:flex;gap:4px;margin-top:8px;">
            <input type="text" id="pset-input-new-action" placeholder="New action name..." style="flex:1;font-size:11px;">
            <button id="pset-input-add" style="font-size:11px;padding:2px 8px;">Add Action</button>
          </div>
        </div>
        <!-- Display -->
        <div class="pset-section" data-cat="display" style="display:none;">
          <div class="settings-row"><span class="settings-label">Resolution Width</span><div class="settings-value"><input type="number" id="pset-res-w" data-key="resolution_w" min="1" max="7680" style="width:80px"></div></div>
          <div class="settings-row"><span class="settings-label">Resolution Height</span><div class="settings-value"><input type="number" id="pset-res-h" data-key="resolution_h" min="1" max="4320" style="width:80px"></div></div>
          <div class="settings-row"><span class="settings-label">Stretch Mode</span><div class="settings-value"><select id="pset-stretch-mode" data-key="stretch_mode"><option value="disabled">disabled</option><option value="canvas_items">canvas_items</option><option value="viewport">viewport</option></select></div></div>
          <div class="settings-row"><span class="settings-label">Stretch Aspect</span><div class="settings-value"><select id="pset-stretch-aspect" data-key="stretch_aspect"><option value="ignore">ignore</option><option value="keep">keep</option><option value="keep_width">keep_width</option><option value="keep_height">keep_height</option><option value="expand">expand</option></select></div></div>
          <div class="settings-row"><span class="settings-label">Fullscreen</span><div class="settings-value"><input type="checkbox" id="pset-fullscreen" data-key="fullscreen"></div></div>
          <div class="settings-row"><span class="settings-label">V-Sync</span><div class="settings-value"><input type="checkbox" id="pset-vsync" data-key="vsync"></div></div>
        </div>
        <!-- Physics -->
        <div class="pset-section" data-cat="physics" style="display:none;">
          <div class="settings-row"><span class="settings-label">Physics FPS</span><div class="settings-value"><input type="number" id="pset-physics-fps" data-key="physics_fps" min="1" max="240" style="width:80px"></div></div>
          <div class="settings-row"><span class="settings-label">Default Gravity</span><div class="settings-value"><input type="number" id="pset-gravity" data-key="gravity" step="0.1" style="width:80px"></div></div>
          <div class="settings-row"><span class="settings-label">Default Linear Damp</span><div class="settings-value"><input type="number" id="pset-linear-damp" data-key="linear_damp" step="0.01" min="0" max="100" style="width:80px"></div></div>
          <div class="settings-row"><span class="settings-label">Default Angular Damp</span><div class="settings-value"><input type="number" id="pset-angular-damp" data-key="angular_damp" step="0.01" min="0" max="100" style="width:80px"></div></div>
        </div>
        <!-- Audio -->
        <div class="pset-section" data-cat="audio" style="display:none;">
          <div class="settings-row"><span class="settings-label">Default Bus Layout</span><div class="settings-value"><input type="text" id="pset-bus-layout" data-key="bus_layout" style="width:100%"></div></div>
          <div class="settings-row"><span class="settings-label">Master Volume (dB)</span><div class="settings-value"><input type="number" id="pset-master-vol" data-key="master_volume_db" step="0.1" min="-80" max="24" style="width:80px"></div></div>
          <div class="settings-row"><span class="settings-label">Enable Audio Input</span><div class="settings-value"><input type="checkbox" id="pset-audio-input" data-key="audio_input"></div></div>
        </div>
        <!-- Rendering -->
        <div class="pset-section" data-cat="rendering" style="display:none;">
          <div class="settings-row"><span class="settings-label">Renderer</span><div class="settings-value"><select id="pset-renderer" data-key="renderer"><option value="forward_plus">forward_plus</option><option value="mobile">mobile</option><option value="compatibility">compatibility</option></select></div></div>
          <div class="settings-row"><span class="settings-label">Anti-Aliasing</span><div class="settings-value"><select id="pset-anti-aliasing" data-key="anti_aliasing"><option value="disabled">disabled</option><option value="fxaa">fxaa</option><option value="msaa_2x">msaa_2x</option><option value="msaa_4x">msaa_4x</option><option value="msaa_8x">msaa_8x</option></select></div></div>
          <div class="settings-row"><span class="settings-label">Default Environment</span><div class="settings-value"><input type="text" id="pset-env-default" data-key="environment_default" placeholder="res://..." style="width:100%"></div></div>
        </div>
      </div>
    </div>
    <div style="padding:12px 16px;border-top:1px solid var(--border);display:flex;gap:6px;justify-content:flex-end;">
      <button id="pset-cancel">Cancel</button>
      <button id="pset-save" style="border-color:var(--accent);color:var(--accent);">Save</button>
    </div>
  </div>
</div>

<!-- Export dialog (pat-4cexp) -->
<div id="export-dialog" style="display:none;position:fixed;top:0;left:0;width:100%;height:100%;background:rgba(0,0,0,0.5);z-index:300;align-items:center;justify-content:center;">
  <div style="background:var(--panel);border:1px solid var(--border);border-radius:6px;width:720px;max-height:80vh;display:flex;flex-direction:column;box-shadow:0 8px 32px rgba(0,0,0,0.6);">
    <div style="display:flex;align-items:center;padding:12px 16px;border-bottom:1px solid var(--border);">
      <h3 style="font-size:14px;color:var(--accent);margin:0;flex:1;">Export</h3>
      <button id="export-close" style="background:none;border:none;color:var(--text-dim);cursor:pointer;font-size:16px;">&times;</button>
    </div>
    <div style="display:flex;flex:1;overflow:hidden;min-height:320px;">
      <!-- Preset sidebar -->
      <div style="width:200px;border-right:1px solid var(--border);display:flex;flex-direction:column;">
        <div id="export-preset-list" style="flex:1;overflow-y:auto;padding:4px 0;"></div>
        <div style="padding:8px;border-top:1px solid var(--border);display:flex;gap:4px;">
          <select id="export-add-platform" style="flex:1;font-size:10px;">
            <option value="windows">Windows</option>
            <option value="linux">Linux</option>
            <option value="macos">macOS</option>
            <option value="web">Web</option>
            <option value="android">Android</option>
            <option value="ios">iOS</option>
          </select>
          <button id="export-add-btn" style="font-size:10px;padding:2px 8px;">+</button>
        </div>
      </div>
      <!-- Preset config panel -->
      <div id="export-config" style="flex:1;overflow-y:auto;padding:12px 16px;">
        <div id="export-no-preset" style="color:var(--text-dim);font-size:11px;padding:20px;">Select or add an export preset.</div>
        <div id="export-preset-form" style="display:none;">
          <div class="settings-row"><span class="settings-label">Name</span><div class="settings-value"><input type="text" id="exp-name" style="width:100%;"></div></div>
          <div class="settings-row"><span class="settings-label">Platform</span><div class="settings-value"><span id="exp-platform" style="color:var(--text-dim);font-size:11px;"></span></div></div>
          <div class="settings-row"><span class="settings-label">Build</span><div class="settings-value"><select id="exp-build"><option value="debug">Debug</option><option value="release">Release</option></select></div></div>
          <div class="settings-row"><span class="settings-label">Export Path</span><div class="settings-value"><input type="text" id="exp-path" style="width:100%;" placeholder="export/platform/name.exe"></div></div>
          <div class="settings-row"><span class="settings-label">App Name</span><div class="settings-value"><input type="text" id="exp-app-name" style="width:100%;" placeholder="(use project name)"></div></div>
          <div class="settings-row"><span class="settings-label">Include</span><div class="settings-value"><input type="text" id="exp-include" style="width:100%;" placeholder="*"></div></div>
          <div class="settings-row"><span class="settings-label">Exclude</span><div class="settings-value"><input type="text" id="exp-exclude" style="width:100%;" placeholder=""></div></div>
          <div id="exp-custom-props" style="margin-top:8px;border-top:1px solid var(--border);padding-top:8px;">
            <div style="font-size:11px;color:var(--accent);margin-bottom:6px;">Platform Properties</div>
            <div id="exp-custom-list"></div>
          </div>
          <div style="margin-top:12px;display:flex;gap:6px;justify-content:flex-end;">
            <button id="exp-delete" style="color:#e55;border-color:#e55;">Delete Preset</button>
            <button id="exp-save" style="border-color:var(--accent);color:var(--accent);">Save Preset</button>
          </div>
        </div>
      </div>
    </div>
  </div>
</div>

<!-- Status bar -->
<div id="statusbar">
  <span>Selected: <span class="accent" id="status-selected">None</span></span>
  <span>Path: <span id="status-path">&mdash;</span></span>
  <span>Nodes: <span id="status-nodes">0</span></span>
  <span>Tool: <span id="status-tool">Select</span></span>
  <span>Snap: <span id="status-snap">Off</span></span>
  <span>Zoom: <span id="status-zoom">100%</span></span>
  <span id="status-cursor" style="display:none">Cursor: <span id="status-cursor-pos">(0, 0)</span></span>
</div>

<script>
(function() {
  'use strict';

  // State
  var selectedNodeId = null;
  var selectedNodeData = null;
  var selectedNodeIds = new Set();  // Multi-select set
  var sceneData = null;
  var expandedNodes = new Set();
  var searchFilter = '';
  var contextNodeId = null;
  var currentToolMode = 'select';
  // Which 3D spatial toolbar tool button is highlighted. Godot has both a
  // "Transform" and a "Select" tool that both map to Patina's "select" gizmo
  // mode, so the exact button is tracked separately from currentToolMode.
  var currentVpTool = 'transform';
  var collapsedSections = {};
  var lastLogCount = -1;
  var inspectorHistory = [];
  var inspectorHistoryIndex = -1;
  var inspectorHistoryNavigating = false;
  var subResourceStack = []; // breadcrumb stack for sub-resource navigation

  // Editor settings
  var editorSettings = { grid_snap_enabled: false, grid_snap_size: 8, grid_visible: true, rulers_visible: true, background_color: [0.08,0.08,0.1,1], font_size: 'medium' };

  // Box select state
  var isBoxSelecting = false;
  var boxSelectStart = null;
  var boxSelectOverlay = null;

  // Drag-drop state for tree reordering
  var treeDragNodeId = null;
  var treeDragOverRow = null;
  var treeDragZone = null;

  // Default property values for revert
  var PROPERTY_DEFAULTS = {
    position: { type: 'Vector2', value: [0, 0] },
    rotation: { type: 'Float', value: 0 },
    scale: { type: 'Vector2', value: [1, 1] },
    visible: { type: 'Bool', value: true },
    z_index: { type: 'Int', value: 0 },
    skew: { type: 'Float', value: 0 },
    show_behind_parent: { type: 'Bool', value: false },
    light_mask: { type: 'Int', value: 1 },
    collision_layer: { type: 'Int', value: 1 },
    collision_mask: { type: 'Int', value: 1 },
    collision_priority: { type: 'Float', value: 1 },
    disable_mode: { type: 'Int', value: 0 },
    input_pickable: { type: 'Bool', value: true },
    physics_material_override: { type: 'Resource', value: null },
    constant_linear_velocity: { type: 'Vector2', value: [0, 0] },
    constant_angular_velocity: { type: 'Float', value: 0 },
    modulate: { type: 'Color', value: [1, 1, 1, 1] },
    self_modulate: { type: 'Color', value: [1, 1, 1, 1] },
    offset: { type: 'Vector2', value: [0, 0] },
    flip_h: { type: 'Bool', value: false },
    flip_v: { type: 'Bool', value: false },
    zoom: { type: 'Vector2', value: [1, 1] },
    current: { type: 'Bool', value: false },
    z_as_relative: { type: 'Bool', value: true },
    y_sort_enabled: { type: 'Bool', value: false },
    top_level: { type: 'Bool', value: false },
    clip_children: { type: 'Int', value: 0 },
    visibility_layer: { type: 'Int', value: 1 },
    texture_filter: { type: 'Int', value: 0 },
    texture_repeat: { type: 'Int', value: 0 },
    use_parent_material: { type: 'Bool', value: false },
    process_mode: { type: 'Int', value: 0 },
    process_priority: { type: 'Int', value: 0 },
    process_physics_priority: { type: 'Int', value: 0 },
    process_thread_group: { type: 'Int', value: 0 },
    physics_interpolation_mode: { type: 'Int', value: 0 },
    auto_translate_mode: { type: 'Int', value: 0 },
    editor_description: { type: 'String', value: '' },
    unique_name_in_owner: { type: 'Bool', value: false }
  };

  // Class-specific default properties that should always show
  var CLASS_DEFAULT_PROPS = {
    Node2D: ['position', 'rotation', 'scale', 'skew', 'visible', 'modulate', 'self_modulate', 'show_behind_parent', 'z_index', 'light_mask', 'material'],
    Sprite2D: ['position', 'rotation', 'scale', 'visible', 'z_index', 'texture', 'offset', 'flip_h', 'flip_v', 'modulate'],
    Camera2D: ['position', 'rotation', 'scale', 'visible', 'z_index', 'zoom', 'current'],
    Control: ['position', 'rotation', 'scale', 'visible', 'anchor_left', 'anchor_top', 'anchor_right', 'anchor_bottom', 'offset_left', 'offset_top', 'offset_right', 'offset_bottom', 'size_flags_horizontal', 'size_flags_vertical'],
    Label: ['position', 'rotation', 'scale', 'visible', 'text', 'modulate'],
    Button: ['position', 'rotation', 'scale', 'visible', 'text', 'modulate']
  };

  // Unit suffixes shown after numeric editors (Godot inspector convention).
  var PROP_UNITS = {
    position: 'px', global_position: 'px', offset: 'px',
    rotation: '°', global_rotation: '°', skew: '°',
    velocity: 'px/s', linear_velocity: 'px/s', constant_linear_velocity: 'px/s',
    angular_velocity: '°/s', constant_angular_velocity: '°/s'
  };

  // Godot class hierarchy (most-derived first) — drives the EditorInspector
  // class-category bars. Each property is shown under the class that declares it.
  var CLASS_CHAIN = {
    Node: ['Node'],
    CanvasItem: ['CanvasItem', 'Node'],
    Node2D: ['Node2D', 'CanvasItem', 'Node'],
    Sprite2D: ['Sprite2D', 'Node2D', 'CanvasItem', 'Node'],
    AnimatedSprite2D: ['AnimatedSprite2D', 'Node2D', 'CanvasItem', 'Node'],
    Camera2D: ['Camera2D', 'Node2D', 'CanvasItem', 'Node'],
    CollisionShape2D: ['CollisionShape2D', 'Node2D', 'CanvasItem', 'Node'],
    Area2D: ['Area2D', 'CollisionObject2D', 'Node2D', 'CanvasItem', 'Node'],
    StaticBody2D: ['StaticBody2D', 'PhysicsBody2D', 'CollisionObject2D', 'Node2D', 'CanvasItem', 'Node'],
    RigidBody2D: ['RigidBody2D', 'PhysicsBody2D', 'CollisionObject2D', 'Node2D', 'CanvasItem', 'Node'],
    CharacterBody2D: ['CharacterBody2D', 'PhysicsBody2D', 'CollisionObject2D', 'Node2D', 'CanvasItem', 'Node'],
    Control: ['Control', 'CanvasItem', 'Node'],
    Label: ['Label', 'Control', 'CanvasItem', 'Node'],
    Button: ['Button', 'BaseButton', 'Control', 'CanvasItem', 'Node'],
    Node3D: ['Node3D', 'Node']
  };
  // Property -> { declaring class, subgroup }. '__self__' = the node's own class.
  var PROP_DECL = {
    position: { cls: 'Node2D', group: 'Transform' }, rotation: { cls: 'Node2D', group: 'Transform' },
    scale: { cls: 'Node2D', group: 'Transform' }, skew: { cls: 'Node2D', group: 'Transform' },
    visible: { cls: 'CanvasItem', group: 'Visibility' }, modulate: { cls: 'CanvasItem', group: 'Visibility' },
    self_modulate: { cls: 'CanvasItem', group: 'Visibility' }, show_behind_parent: { cls: 'CanvasItem', group: 'Visibility' },
    z_index: { cls: 'CanvasItem', group: 'Ordering' }, z_as_relative: { cls: 'CanvasItem', group: 'Ordering' },
    y_sort_enabled: { cls: 'CanvasItem', group: 'Ordering' }, light_mask: { cls: 'CanvasItem', group: 'Visibility' },
    top_level: { cls: 'CanvasItem', group: 'Visibility' }, clip_children: { cls: 'CanvasItem', group: 'Visibility' }, visibility_layer: { cls: 'CanvasItem', group: 'Visibility' },
    texture_filter: { cls: 'CanvasItem', group: 'Texture' }, texture_repeat: { cls: 'CanvasItem', group: 'Texture' },
    material: { cls: 'CanvasItem', group: 'Material' }, use_parent_material: { cls: 'CanvasItem', group: 'Material' },
    texture: { cls: '__self__', group: '' }, centered: { cls: 'Sprite2D', group: 'Offset' },
    offset: { cls: '__self__', group: 'Offset' }, flip_h: { cls: 'Sprite2D', group: 'Offset' }, flip_v: { cls: 'Sprite2D', group: 'Offset' },
    hframes: { cls: 'Sprite2D', group: 'Animation' }, vframes: { cls: 'Sprite2D', group: 'Animation' }, frame: { cls: 'Sprite2D', group: 'Animation' },
    zoom: { cls: 'Camera2D', group: '' }, current: { cls: 'Camera2D', group: '' },
    text: { cls: '__self__', group: '' },
    collision_layer: { cls: 'CollisionObject2D', group: 'Collision' }, collision_mask: { cls: 'CollisionObject2D', group: 'Collision' },
    collision_priority: { cls: 'CollisionObject2D', group: 'Collision' },
    disable_mode: { cls: 'CollisionObject2D', group: '' },
    input_pickable: { cls: 'CollisionObject2D', group: 'Input' },
    physics_material_override: { cls: 'StaticBody2D', group: '' },
    constant_linear_velocity: { cls: 'StaticBody2D', group: '' },
    constant_angular_velocity: { cls: 'StaticBody2D', group: '' },
    velocity: { cls: '__self__', group: '' }, mass: { cls: 'RigidBody2D', group: '' }, gravity_scale: { cls: 'RigidBody2D', group: '' },
    process_mode: { cls: 'Node', group: 'Process' }, process_priority: { cls: 'Node', group: 'Process' }, process_physics_priority: { cls: 'Node', group: 'Process' },
    process_thread_group: { cls: 'Node', group: 'Thread Group' },
    physics_interpolation_mode: { cls: 'Node', group: 'Physics Interpolation' },
    auto_translate_mode: { cls: 'Node', group: 'Auto Translate' },
    editor_description: { cls: 'Node', group: 'Editor Description' },
    unique_name_in_owner: { cls: 'Node', group: '' }
  };
  function getClassChain(cls) {
    if (CLASS_CHAIN[cls]) return CLASS_CHAIN[cls];
    if (/3[dD]/.test(cls || '')) return [cls, 'Node3D', 'Node'];
    if (/2[dD]/.test(cls || '')) return [cls, 'Node2D', 'CanvasItem', 'Node'];
    return [cls || 'Node', 'Node'];
  }
  function getPropDecl(name, nodeClass, chain) {
    var d = PROP_DECL[name];
    if (d) {
      var c = d.cls === '__self__' ? nodeClass : d.cls;
      if (chain.indexOf(c) >= 0) return { cls: c, group: d.group };
    }
    return { cls: nodeClass, group: '' };
  }
  function makeClassCategoryBar(cls) {
    var bar = document.createElement('div');
    bar.className = 'insp-class-category';
    bar.innerHTML = classIconHtml(cls) + '<span>' + escapeHtml(cls) + '</span>';
    return bar;
  }
  function renderClassBucket(container, nodeId, bucket) {
    var ung = bucket.groups[''] || [];
    for (var i = 0; i < ung.length; i++) container.appendChild(createPropertyRow(nodeId, ung[i]));
    for (var gi = 0; gi < bucket.order.length; gi++) {
      var g = bucket.order[gi];
      if (g === '') continue;
      var key = 'sub-' + g;
      var header = document.createElement('div');
      header.className = 'insp-subgroup';
      var arrow = document.createElement('span');
      arrow.className = 'insp-subgroup-arrow' + (collapsedSections[key] ? ' collapsed' : '');
      arrow.textContent = '▼';
      var label = document.createElement('span');
      label.textContent = g;
      header.appendChild(arrow); header.appendChild(label);
      var body = document.createElement('div');
      body.className = 'insp-subgroup-body' + (collapsedSections[key] ? ' collapsed' : '');
      var rows = bucket.groups[g];
      for (var ri = 0; ri < rows.length; ri++) {
        var rowEl = createPropertyRow(nodeId, rows[ri]);
        // Godot strips the subgroup-name prefix from labels ("Collision Layer" -> "Layer").
        var lblEl = rowEl.querySelector('.insp-label');
        if (lblEl && lblEl.textContent.indexOf(g + ' ') === 0) lblEl.textContent = lblEl.textContent.slice(g.length + 1);
        body.appendChild(rowEl);
      }
      (function(k, a, bd) { header.addEventListener('click', function() { collapsedSections[k] = !collapsedSections[k]; a.classList.toggle('collapsed'); bd.classList.toggle('collapsed'); }); })(key, arrow, body);
      container.appendChild(header); container.appendChild(body);
    }
  }

  // Property category mapping
  var PROP_CATEGORIES = {
    position: 'Transform', rotation: 'Transform', scale: 'Transform', transform: 'Transform',
    global_position: 'Transform', global_rotation: 'Transform', global_scale: 'Transform', skew: 'Transform',
    visible: 'Rendering', modulate: 'Rendering', self_modulate: 'Rendering', texture: 'Rendering',
    color: 'Rendering', z_index: 'Rendering', z_as_relative: 'Rendering', material: 'Rendering',
    light_mask: 'Rendering', offset: 'Rendering', flip_h: 'Rendering', flip_v: 'Rendering',
    velocity: 'Physics', mass: 'Physics', gravity_scale: 'Physics',
    linear_velocity: 'Physics', angular_velocity: 'Physics', friction: 'Physics', bounce: 'Physics',
    body_type: 'Physics',
    zoom: 'Camera', current: 'Camera',
    text: 'Content',
    anchor_left: 'Layout', anchor_top: 'Layout', anchor_right: 'Layout', anchor_bottom: 'Layout',
    offset_left: 'Layout', offset_top: 'Layout', offset_right: 'Layout', offset_bottom: 'Layout',
    size_flags_horizontal: 'Layout', size_flags_vertical: 'Layout'
  };

  function getPropCategory(name) {
    if (PROP_CATEGORIES[name]) return PROP_CATEGORIES[name];
    if (name === 'script' || name.startsWith('script_') || name.startsWith('metadata/')) return 'Script';
    if (name === 'groups' || name === 'signal_connections') return 'Internal';
    return 'Misc';
  }

  var CATEGORY_ORDER = ['Transform', 'Rendering', 'Camera', 'Layout', 'Content', 'Physics', 'Script', 'Misc'];

  // ---- API helpers ----
  async function api(method, path, body) {
    var opts = { method: method };
    if (body !== undefined) {
      opts.headers = { 'Content-Type': 'application/json' };
      opts.body = JSON.stringify(body);
    }
    // pat-vl1mk: bound every request with an AbortController timeout so a slow
    // or stuck endpoint can never leave a fetch in-flight forever. Unbounded
    // fetches piled up (a capture showed 138 requests all 'pending') and jammed
    // the renderer main thread within minutes.
    var ctrl = new AbortController();
    var timer = setTimeout(function() { ctrl.abort(); }, 8000);
    opts.signal = ctrl.signal;
    try {
      var resp = await fetch(path, opts);
      var text = await resp.text();
      if (!text || text === 'null') return null;
      return JSON.parse(text);
    } catch (e) {
      return null;
    } finally {
      clearTimeout(timer);
    }
  }

  // ---- Node type icons ----
  function classIconHtml(cls) {
    if (!cls) return '<span class="tree-icon" style="color:var(--icon-node)">&#9679;</span>';
    var c = cls.toLowerCase();
    if (c === 'sprite2d') return '<span class="tree-icon" style="color:var(--icon-sprite2d)">&#9724;</span>';
    if (c === 'camera2d') return '<span class="tree-icon" style="color:var(--icon-camera2d)">&#9965;</span>';
    if (c === 'node2d') return '<span class="tree-icon" style="color:var(--icon-node2d)">&#9670;</span>';
    if (c === 'node3d') return '<span class="tree-icon" style="color:var(--icon-node3d)">&#9670;</span>';
    if (c === 'label') return '<span class="tree-icon" style="color:var(--icon-label);font-weight:bold">A</span>';
    if (c === 'button') return '<span class="tree-icon" style="color:var(--icon-button)">&#9109;</span>';
    if (c === 'control') return '<span class="tree-icon" style="color:var(--icon-control)">&#9645;</span>';
    if (c.includes('2d')) return '<span class="tree-icon" style="color:var(--icon-node2d)">&#9670;</span>';
    if (c.includes('3d')) return '<span class="tree-icon" style="color:var(--icon-node3d)">&#9670;</span>';
    return '<span class="tree-icon" style="color:var(--icon-node)">&#9679;</span>';
  }

  // ---- Search filter helpers ----
  // pat-l695a: Godot's filter supports name (default), type ("type:"/"t:"), and
  // group ("group:"/"g:") prefixes, case-insensitive (scene_tree_dock.cpp
  // _filter_changed / SceneTreeEditor::_update_filter).
  function nodeMatchesFilter(node, filter) {
    if (!filter) return true;
    var f = filter.trim();
    var m = /^(type|t|group|g):\s*(.*)$/i.exec(f);
    if (m) {
      var term = m[2].toLowerCase();
      if (!term) return true;
      var kind = m[1].toLowerCase();
      if (kind === 'type' || kind === 't') {
        return (node.class || node.type || '').toLowerCase().indexOf(term) >= 0;
      }
      // group / g
      return (node.groups || []).some(function(g) { return g.toLowerCase().indexOf(term) >= 0; });
    }
    return node.name && node.name.toLowerCase().indexOf(f.toLowerCase()) >= 0;
  }

  function subtreeMatchesFilter(node, filter) {
    if (!filter) return true;
    if (nodeMatchesFilter(node, filter)) return true;
    if (node.children) {
      for (var i = 0; i < node.children.length; i++) {
        if (subtreeMatchesFilter(node.children[i], filter)) return true;
      }
    }
    return false;
  }

  // ---- Scene tree ----
  function countNodes(node) {
    if (!node || !node.children) return 1;
    return 1 + node.children.reduce(function(s, c) { return s + countNodes(c); }, 0);
  }

  function renderTree(node, depth, container) {
    if (!node) return;
    if (searchFilter && !subtreeMatchesFilter(node, searchFilter)) return;

    var div = document.createElement('div');
    div.className = 'tree-node';
    div.style.paddingLeft = (depth * 16) + 'px';

    var hasChildren = node.children && node.children.length > 0;
    var isExpanded = expandedNodes.has(node.id);
    if (searchFilter && hasChildren) isExpanded = true;

    var row = document.createElement('div');
    row.className = 'tree-row' + (selectedNodeIds.has(node.id) || node.id === selectedNodeId ? ' selected' : '');
    if (node.visible === false) row.className += ' hidden-node';
    row.setAttribute('data-node-id', node.id);

    var toggle = document.createElement('span');
    toggle.className = 'tree-toggle';
    toggle.textContent = hasChildren ? (isExpanded ? '\u25BC' : '\u25B6') : '';
    if (hasChildren) {
      toggle.addEventListener('click', (function(nid) { return function(e) {
        e.stopPropagation();
        if (expandedNodes.has(nid)) expandedNodes.delete(nid);
        else expandedNodes.add(nid);
        refreshTree();
      }; })(node.id));
    }

    var iconWrapper = document.createElement('span');
    if (node.is_instance) {
      iconWrapper.innerHTML = '<span class="tree-icon" style="color:#579eff" title="Instanced Scene">&#128279;</span>';
    } else {
      iconWrapper.innerHTML = classIconHtml(node['class']);
    }
    var icon = iconWrapper.firstChild;

    var name = document.createElement('span');
    name.className = 'tree-name';
    name.textContent = node.name;

    if (searchFilter && nodeMatchesFilter(node, searchFilter)) {
      name.style.color = 'var(--accent)';
      name.style.fontWeight = 'bold';
    }

    // pat-t0c: Tree indicator badges (script, signal, group)
    var scriptBadge = document.createElement('span');
    scriptBadge.className = 'tree-badge';
    if (node.has_script) {
      scriptBadge.textContent = '\u{1F4DC}';
      scriptBadge.title = 'Has script attached';
    }
    var signalBadge = document.createElement('span');
    signalBadge.className = 'tree-badge';
    if (node.has_signals) {
      signalBadge.innerHTML = '&#9889;&#8594;';
      signalBadge.title = 'Has connected signals';
      signalBadge.style.color = '#50c878';
      signalBadge.style.fontSize = '10px';
    }
    var groupBadge = document.createElement('span');
    groupBadge.className = 'tree-badge';
    if (node.groups && node.groups.length > 0) {
      groupBadge.textContent = '[G]';
      groupBadge.title = 'Groups: ' + node.groups.join(', ');
      groupBadge.style.color = 'var(--text-dim)';
      groupBadge.style.fontSize = '9px';
      groupBadge.style.fontWeight = 'bold';
    }

    var visBtn = document.createElement('span');
    visBtn.className = 'tree-visibility' + (node.visible === false ? ' vis-hidden' : '');
    visBtn.innerHTML = node.visible === false ? '&#9673;' : '&#9678;';
    visBtn.title = node.visible === false ? 'Show' : 'Hide';
    visBtn.addEventListener('click', (function(nid, isVis) { return function(e) {
      e.stopPropagation();
      api('POST', '/api/property/set', {
        node_id: nid, property: 'visible',
        value: { type: 'Bool', value: !isVis }
      }).then(function() { fetchScene(); });
    }; })(node.id, node.visible !== false));

    // pat-zgwgu: Unique name badge (%Name)
    var uniqueBadge = document.createElement('span');
    uniqueBadge.className = 'tree-badge unique-name';
    if (node.unique_name) {
      uniqueBadge.textContent = '%';
      uniqueBadge.title = 'Unique name: %' + node.name;
    }

    // pat-zgwgu: Lock button (prevent selection in viewport)
    var lockBtn = document.createElement('span');
    lockBtn.className = 'tree-lock' + (node.locked ? ' locked' : '');
    lockBtn.innerHTML = node.locked ? '&#128274;' : '&#128275;';
    lockBtn.title = node.locked ? 'Unlock (allow selection)' : 'Lock (prevent selection)';
    lockBtn.addEventListener('click', (function(nid, isLocked) { return function(e) {
      e.stopPropagation();
      api('POST', '/api/property/set', {
        node_id: nid, property: 'editor/locked',
        value: { type: 'Bool', value: !isLocked }
      }).then(function() { fetchScene(); });
    }; })(node.id, !!node.locked));

    row.appendChild(toggle);
    row.appendChild(icon);
    row.appendChild(name);
    row.appendChild(uniqueBadge);
    row.appendChild(scriptBadge);
    row.appendChild(signalBadge);
    row.appendChild(groupBadge);
    row.appendChild(lockBtn);
    row.appendChild(visBtn);

    row.addEventListener('click', (function(nid) { return function(e) {
      if (e.ctrlKey || e.metaKey) {
        // Toggle selection
        if (selectedNodeIds.has(nid)) { selectedNodeIds.delete(nid); } else { selectedNodeIds.add(nid); }
        selectedNodeId = selectedNodeIds.size > 0 ? Array.from(selectedNodeIds)[0] : null;
        api('POST', '/api/node/select_multi', { node_ids: Array.from(selectedNodeIds) });
        refreshTree(); fetchSelected();
      } else if (e.shiftKey && selectedNodeId) {
        // Range select among siblings
        var parent = findNodeParent(sceneData ? sceneData.nodes : null, nid);
        if (parent && parent.children) {
          var ids = parent.children.map(function(c) { return c.id; });
          var a = ids.indexOf(selectedNodeId), b = ids.indexOf(nid);
          if (a >= 0 && b >= 0) {
            var lo = Math.min(a, b), hi = Math.max(a, b);
            for (var si = lo; si <= hi; si++) selectedNodeIds.add(ids[si]);
          }
        }
        api('POST', '/api/node/select_multi', { node_ids: Array.from(selectedNodeIds) });
        refreshTree(); fetchSelected();
      } else {
        selectedNodeIds.clear(); selectedNodeIds.add(nid); selectNode(nid);
      }
    }; })(node.id));

    row.addEventListener('contextmenu', (function(nid) { return function(e) {
      e.preventDefault(); e.stopPropagation();
      selectNode(nid);
      showContextMenu(e.clientX, e.clientY, nid);
    }; })(node.id));

    // Drag-drop
    row.draggable = true;
    row.addEventListener('dragstart', (function(nid) { return function(e) {
      treeDragNodeId = nid;
      e.dataTransfer.effectAllowed = 'move';
      e.dataTransfer.setData('text/plain', String(nid));
      setTimeout(function() { row.style.opacity = '0.4'; }, 0);
    }; })(node.id));
    row.addEventListener('dragend', function() {
      row.style.opacity = '';
      clearDragIndicators();
      treeDragNodeId = null;
    });
    row.addEventListener('dragover', (function(nid) { return function(e) {
      // Accept both tree node drags and filesystem .tscn drags.
      var hasFsDrag = false;
      try { var types = e.dataTransfer.types; hasFsDrag = types && types.indexOf('text/plain') >= 0 && treeDragNodeId === null; } catch(ex) {}
      if (treeDragNodeId === null && !hasFsDrag) return;
      if (treeDragNodeId === nid) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = hasFsDrag ? 'copy' : 'move';
      var rect = row.getBoundingClientRect();
      var y = e.clientY - rect.top;
      var h = rect.height;
      clearDragIndicators();
      if (hasFsDrag) {
        // For filesystem drops, always go inside.
        row.classList.add('drag-over-inside'); treeDragZone = 'inside';
      } else if (y < h * 0.25) { row.classList.add('drag-over-above'); treeDragZone = 'above'; }
      else if (y > h * 0.75) { row.classList.add('drag-over-below'); treeDragZone = 'below'; }
      else { row.classList.add('drag-over-inside'); treeDragZone = 'inside'; }
      treeDragOverRow = row;
    }; })(node.id));
    row.addEventListener('dragleave', function() {
      row.classList.remove('drag-over-above', 'drag-over-inside', 'drag-over-below');
    });
    row.addEventListener('drop', (function(nid) { return function(e) {
      e.preventDefault();
      clearDragIndicators();
      // Check for filesystem .tscn drop.
      try {
        var rawData = e.dataTransfer.getData('text/plain');
        if (rawData) {
          var parsed = JSON.parse(rawData);
          if (parsed && parsed.type === 'tscn_instance' && parsed.path) {
            api('POST', '/api/scene/instance', { path: parsed.path.replace('res://', ''), parent_id: nid }).then(function(result) {
              if (result && result.id) { selectedNodeId = result.id; logMessage('info', 'Instanced: ' + parsed.path); }
              expandedNodes.add(nid); fetchScene(); if (selectedNodeId) fetchSelected();
            });
            treeDragNodeId = null; treeDragZone = null;
            return;
          }
        }
      } catch(ex) { /* not JSON, fall through to tree drag */ }
      if (treeDragNodeId === null || treeDragNodeId === nid) return;
      // pat-lac: Multi-node drag — reparent all selected nodes
      var dragIds = selectedNodeIds.size > 1 && selectedNodeIds.has(treeDragNodeId) ?
        Array.from(selectedNodeIds) : [treeDragNodeId];
      if (treeDragZone === 'inside') {
        (async function() {
          for (var di = 0; di < dragIds.length; di++) {
            if (dragIds[di] !== nid) {
              await api('POST', '/api/node/reparent', { node_id: dragIds[di], new_parent_id: nid });
            }
          }
          expandedNodes.add(nid); fetchScene();
        })();
      } else {
        var targetParent = findNodeParent(sceneData.nodes, nid);
        if (targetParent) {
          (async function() {
            for (var di = 0; di < dragIds.length; di++) {
              if (dragIds[di] !== targetParent.id) {
                await api('POST', '/api/node/reparent', { node_id: dragIds[di], new_parent_id: targetParent.id });
              }
            }
            fetchScene();
          })();
        }
      }
      treeDragNodeId = null; treeDragZone = null;
    }; })(node.id));

    div.appendChild(row);
    container.appendChild(div);

    if (hasChildren && isExpanded) {
      var childContainer = document.createElement('div');
      childContainer.className = 'tree-children expanded';
      for (var i = 0; i < node.children.length; i++) {
        renderTree(node.children[i], depth + 1, childContainer);
      }
      container.appendChild(childContainer);
    }
  }

  function findNodeParent(tree, targetId) {
    if (!tree || !tree.children) return null;
    for (var i = 0; i < tree.children.length; i++) {
      if (tree.children[i].id === targetId) return tree;
      var found = findNodeParent(tree.children[i], targetId);
      if (found) return found;
    }
    return null;
  }

  function clearDragIndicators() {
    var rows = document.querySelectorAll('.drag-over-above,.drag-over-inside,.drag-over-below');
    for (var i = 0; i < rows.length; i++) {
      rows[i].classList.remove('drag-over-above', 'drag-over-inside', 'drag-over-below');
    }
  }

  function refreshTree() {
    var el = document.getElementById('scene-tree');
    el.innerHTML = '';
    // Godot parity (SceneTreeDock NOTIFICATION_PROCESS): with no scene root the
    // dock shows the "Create Root Node:" panel in place of the tree.
    if (sceneIsEmpty()) {
      renderCreateRootPanel(el);
      document.getElementById('status-nodes').textContent = '0';
      return;
    }
    if (sceneData && sceneData.nodes) {
      renderTree(sceneData.nodes, 0, el);
      document.getElementById('status-nodes').textContent = countNodes(sceneData.nodes);
    }
  }

  async function fetchScene() {
    var data = await api('GET', '/api/scene');
    if (data) {
      sceneData = data;
      if (expandedNodes.size === 0 && data.nodes) expandedNodes.add(data.nodes.id);
      refreshTree();
      maybeAutoSelectView(data);
    }
  }

  // Scene > New Scene (Godot's EditorNode::new_scene): opens a fresh empty,
  // untitled scene in its own tab and focuses it. The new scene has no root
  // node yet, so the Scene dock shows the "Create Root Node:" panel until the
  // user picks a root type — matching Godot 1:1.
  async function newScene() {
    var res = await api('POST', '/api/scene/new', {});
    if (!res || res.ok !== true) {
      if (typeof logMessage === 'function') logMessage('error', 'Could not create a new scene');
      return;
    }
    // Drop selection/expansion carried over from the previous scene's nodes.
    selectedNodeId = null;
    if (typeof selectedNodeIds !== 'undefined' && selectedNodeIds.clear) selectedNodeIds.clear();
    expandedNodes.clear();
    await fetchScene();
    if (typeof fetchSceneTabs === 'function') await fetchSceneTabs();
    if (typeof logMessage === 'function') logMessage('info', 'New scene created');
  }

  // Scene > Close Scene (Godot's EditorNode::_scene_tab_closed): closes the
  // active scene tab. A modified scene prompts before its changes are discarded;
  // an unmodified (or empty) scene closes immediately. Closing the last tab
  // leaves a fresh empty scene, so there is always a scene open.
  async function closeCurrentScene() {
    var t = sceneTabs && sceneTabs[activeTabIndex];
    if (!t) return;
    // Close Scene acts on the active scene, so read its live modified state (the
    // tab's cached flag can lag edits). Godot only prompts when it's unsaved.
    var info = await api('GET', '/api/scene/info');
    var modified = info && typeof info.modified === 'boolean' ? info.modified : !!t.modified;
    closeSceneTab(t.id, t.name, modified);
  }

  // Shared close path for both the Scene menu and each tab's × button. Mirrors
  // Godot: unsaved scenes confirm before discarding; then the tab is closed and
  // the tree + tab bar refresh (closing the active tab changes the shown scene).
  function closeSceneTab(tabId, tabName, modified) {
    function doClose() {
      return api('POST', '/api/scene/tabs/close', { tab_id: tabId }).then(function() {
        if (typeof fetchSceneTabs === 'function') fetchSceneTabs();
        return fetchScene();
      });
    }
    if (modified) {
      return showConfirm(
        'Save changes to &quot;' + escapeHtml(tabName) + '&quot; before closing?<br><br>' +
          'Your changes will be lost if you don’t save them.',
        { title: 'Close Scene', okText: 'Close Without Saving', danger: true }
      ).then(function(ok) { if (ok) return doClose(); });
    }
    return doClose();
  }

  // Whether the active scene has no root node yet (a freshly created, empty
  // scene). Patina always keeps a synthetic window "root"; an empty edited
  // scene is that window root with zero children — the equivalent of Godot's
  // `get_edited_scene_root() == nullptr`.
  function sceneIsEmpty() {
    return !!(sceneData && sceneData.nodes &&
      (!sceneData.nodes.children || sceneData.nodes.children.length === 0));
  }

  // Create the scene's root node from the "Create Root Node:" panel, mirroring
  // SceneTreeDock's TOOL_CREATE_2D_SCENE / _3D_SCENE / _USER_INTERFACE. The new
  // node is added under the window root, becoming the edited scene's root.
  async function createRootNode(kind) {
    if (!sceneData || !sceneData.nodes) return;
    var rootId = sceneData.nodes.id;
    var cls, name;
    if (kind === '2d') { cls = 'Node2D'; name = 'Node2D'; }
    else if (kind === '3d') { cls = 'Node3D'; name = 'Node3D'; }
    else if (kind === 'ui') { cls = 'Control'; name = 'Control'; }
    else return;
    var res = await api('POST', '/api/node/add', { parent_id: rootId, name: name, class_name: cls });
    if (res && res.id !== undefined) {
      selectedNodeId = res.id;
      if (typeof selectedNodeIds !== 'undefined' && selectedNodeIds.clear) {
        selectedNodeIds.clear(); selectedNodeIds.add(res.id);
      }
      expandedNodes.add(rootId);
    }
    // Godot switches the main screen to fit the new root (edit_node selects the
    // Node3D vs CanvasItem editor plugin): a 3D root opens the 3D viewport, a
    // 2D/UI root the 2D canvas. Re-arm the one-shot auto-view for this refresh.
    autoViewPending = true;
    await fetchScene();
  }

  // Renders Godot's "Create Root Node:" panel into the Scene dock in place of
  // the (empty) tree. Buttons mirror SceneTreeDock's beginner_node_shortcuts:
  // 2D Scene, 3D Scene, User Interface, then Other Node and Paste From Clipboard.
  function renderCreateRootPanel(container) {
    var panel = document.createElement('div');
    panel.id = 'create-root-panel';

    var header = document.createElement('div');
    header.className = 'create-root-header';
    header.textContent = 'Create Root Node:';
    panel.appendChild(header);

    var buttons = [
      { kind: '2d', label: '2D Scene', icon: 'Node2D' },
      { kind: '3d', label: '3D Scene', icon: 'Node3D' },
      { kind: 'ui', label: 'User Interface', icon: 'Control' },
      { kind: 'other', label: 'Other Node', icon: 'Add' },
      { kind: 'paste', label: 'Paste From Clipboard', icon: 'Paste' }
    ];
    buttons.forEach(function(b) {
      var btn = document.createElement('button');
      btn.className = 'create-root-btn';
      btn.id = 'create-root-' + b.kind;
      var ic = document.createElement('span');
      ic.className = 'create-root-icon';
      if (b.icon === 'Add') ic.innerHTML = '<span class="tree-icon" style="color:var(--icon-node)">➕</span>';
      else if (b.icon === 'Paste') ic.innerHTML = '<span class="tree-icon" style="color:var(--icon-node)">📋</span>';
      else ic.innerHTML = classIconHtml(b.icon);
      var lbl = document.createElement('span');
      lbl.textContent = b.label;
      btn.appendChild(ic);
      btn.appendChild(lbl);
      btn.addEventListener('click', function() {
        if (b.kind === 'other') { openAddNodeDialog(); }
        else if (b.kind === 'paste') {
          api('POST', '/api/node/paste', { parent_id: sceneData.nodes.id }).then(function() { fetchScene(); });
        } else { createRootNode(b.kind); }
      });
      panel.appendChild(btn);
    });
    container.appendChild(panel);
  }

  // ---- Context menu ----
  function showContextMenu(x, y, nodeId) {
    contextNodeId = nodeId;
    var menu = document.getElementById('context-menu');
    menu.style.left = x + 'px';
    menu.style.top = y + 'px';
    menu.classList.add('open');
    setTimeout(function() {
      var rect = menu.getBoundingClientRect();
      if (rect.right > window.innerWidth) menu.style.left = (window.innerWidth - rect.width - 4) + 'px';
      if (rect.bottom > window.innerHeight) menu.style.top = (window.innerHeight - rect.height - 4) + 'px';
    }, 0);
  }

  function hideContextMenu() {
    document.getElementById('context-menu').classList.remove('open');
    contextNodeId = null;
  }

  function setupContextMenu() {
    document.addEventListener('click', function() { hideContextMenu(); });
    document.addEventListener('contextmenu', function(e) {
      if (!e.target.closest('.tree-row')) hideContextMenu();
    });
    var menuItems = document.querySelectorAll('.ctx-item');
    for (var i = 0; i < menuItems.length; i++) {
      menuItems[i].addEventListener('click', function(e) {
        e.stopPropagation();
        var action = this.getAttribute('data-action');
        var nid = contextNodeId || selectedNodeId;
        hideContextMenu();
        if (!nid) return;
        handleContextAction(action, nid);
      });
    }
  }

  async function handleContextAction(action, nodeId) {
    switch (action) {
      case 'rename': doRename(nodeId); break;
      case 'copy': doCopy(); break;
      case 'paste': doPaste(); break;
      case 'cut': doCut(); break;
      case 'duplicate': doDuplicate(nodeId); break;
      case 'delete': doDelete(nodeId); break;
      case 'add-child': doAddChild(nodeId); break;
      case 'move-up':
        await api('POST', '/api/node/reorder', { node_id: nodeId, direction: 'up' });
        await fetchScene(); break;
      case 'move-down':
        await api('POST', '/api/node/reorder', { node_id: nodeId, direction: 'down' });
        await fetchScene(); break;
      case 'attach-script': doAttachScript(nodeId); break;
      case 'instance-scene': doInstanceScene(nodeId); break;
    }
  }

  async function doInstanceScene(parentNodeId) {
    var path = prompt('Enter .tscn file path to instance:');
    if (!path) return;
    var result = await api('POST', '/api/scene/instance', { path: path, parent_id: parentNodeId });
    if (result && result.id) {
      logMessage('info', 'Instanced scene: ' + path);
      selectedNodeId = result.id;
    }
    await fetchScene();
    if (selectedNodeId) await fetchSelected();
  }

  async function doRename(nodeId) {
    var current = findNodeInTree(sceneData ? sceneData.nodes : null, nodeId);
    var currentName = current ? current.name : '';
    var newName = prompt('New name:', currentName);
    if (newName === null || newName === '' || newName === currentName) return;
    await api('POST', '/api/node/rename', { node_id: nodeId, new_name: newName });
    await fetchScene();
    if (selectedNodeId === nodeId) await fetchSelected();
  }

  async function doDuplicate(nodeId) {
    var result = await api('POST', '/api/node/duplicate', { node_id: nodeId });
    if (result && result.id) selectedNodeId = result.id;
    await fetchScene();
    if (selectedNodeId) await fetchSelected();
  }

  async function doDelete(nodeId) {
    var node = findNodeInTree(sceneData ? sceneData.nodes : null, nodeId);
    var nodeName = node ? node.name : '';
    var msg = nodeName ? 'Delete node &quot;' + escapeHtml(nodeName) + '&quot;?' : 'Delete selected node?';
    if (!await showConfirm(msg, { danger: true })) return;
    await api('POST', '/api/node/delete', { node_id: nodeId });
    if (selectedNodeId === nodeId) {
      selectedNodeId = null; selectedNodeData = null;
      renderInspectorEmpty();
    }
    await fetchScene();
  }

  function doAddChild(parentId) {
    openAddNodeDialog();
  }

  function findNodeInTree(node, id) {
    if (!node) return null;
    if (node.id === id) return node;
    if (node.children) {
      for (var i = 0; i < node.children.length; i++) {
        var found = findNodeInTree(node.children[i], id);
        if (found) return found;
      }
    }
    return null;
  }

  // ---- Selection ----
  async function selectNode(id) {
    selectedNodeId = id;
    if (!inspectorHistoryNavigating) {
      pushInspectorHistory(id);
    }
    inspectorHistoryNavigating = false;
    await api('POST', '/api/node/select', { node_id: id });
    refreshTree();
    await fetchSelected();
  }

  function pushInspectorHistory(id) {
    if (inspectorHistoryIndex < inspectorHistory.length - 1) {
      inspectorHistory = inspectorHistory.slice(0, inspectorHistoryIndex + 1);
    }
    inspectorHistory.push(id);
    inspectorHistoryIndex = inspectorHistory.length - 1;
    updateHistoryButtons();
  }

  function inspectorBack() {
    if (inspectorHistoryIndex > 0) {
      inspectorHistoryIndex--;
      inspectorHistoryNavigating = true;
      selectNode(inspectorHistory[inspectorHistoryIndex]);
      updateHistoryButtons();
    }
  }

  function inspectorForward() {
    if (inspectorHistoryIndex < inspectorHistory.length - 1) {
      inspectorHistoryIndex++;
      inspectorHistoryNavigating = true;
      selectNode(inspectorHistory[inspectorHistoryIndex]);
      updateHistoryButtons();
    }
  }

  function updateHistoryButtons() {
    var backBtn = document.getElementById('inspector-history-back');
    var fwdBtn = document.getElementById('inspector-history-forward');
    if (backBtn) backBtn.disabled = inspectorHistoryIndex <= 0;
    if (fwdBtn) fwdBtn.disabled = inspectorHistoryIndex >= inspectorHistory.length - 1;
  }

  async function fetchSelected() {
    if (selectedNodeIds.size > 1) { renderInspectorMulti(selectedNodeIds.size); return; }
    if (selectedNodeId === null) {
      renderInspectorEmpty();
      renderNodeDockEmpty();
      clearScript();
      if (typeof updateMiscButtons === 'function') updateMiscButtons();
      return;
    }
    var data = await api('GET', '/api/selected');
    if (data) {
      selectedNodeData = data;
      renderInspector(data);
      document.getElementById('status-selected').textContent = data.name || 'None';
      document.getElementById('status-path').textContent = data.path || '\u2014';
      // Misc tab's "Set From Tree" tracks the current selection (Godot _update_buttons_state).
      if (typeof updateMiscButtons === 'function') updateMiscButtons();
      // Refresh node dock if visible.
      if (currentRightTab === 'node') fetchNodeDock();
      // Check for script property and load it.
      // Prefer _script_path (real file path) over script (may be ExtResource ref).
      var scriptPath = null;
      if (data.properties) {
        for (var pi = 0; pi < data.properties.length; pi++) {
          if (data.properties[pi].name === '_script_path') {
            var sp = data.properties[pi].value;
            if (sp && sp.value && typeof sp.value === 'string') { scriptPath = sp.value; break; }
          }
        }
        if (!scriptPath) {
          for (var pi = 0; pi < data.properties.length; pi++) {
            if (data.properties[pi].name === 'script') {
              var sv = data.properties[pi].value;
              if (sv && sv.value && typeof sv.value === 'string') {
                // Ignore ExtResource references like 'ExtResource("1_player")'
                if (sv.value.indexOf('ExtResource') === -1) scriptPath = sv.value;
              }
              break;
            }
          }
        }
      }
      if (scriptPath) {
        fetchScript(scriptPath);
      } else {
        clearScript();
      }
    } else {
      renderInspectorEmpty();
      renderNodeDockEmpty();
      clearScript();
    }
  }

  // ---- Inspector ----
  // pat-mn3: Multi-object inspector showing shared properties
  function renderInspectorMulti(count) {
    var el = document.getElementById('inspector');
    el.innerHTML = '<div class="insp-empty">' + count + ' nodes selected — loading shared properties...</div>';
    document.getElementById('status-selected').textContent = count + ' nodes';
    document.getElementById('status-path').textContent = '\u2014';
    // Fetch shared properties
    api('POST', '/api/node/shared_properties', { node_ids: Array.from(selectedNodeIds) }).then(function(data) {
      if (!data || !data.properties) { el.innerHTML = '<div class="insp-empty">' + count + ' nodes selected (no shared properties)</div>'; return; }
      el.innerHTML = '<div style="padding:6px 8px;font-size:11px;color:var(--text-dim)">' + count + ' nodes — shared properties:</div>';
      if (data.properties.length === 0) { el.innerHTML += '<div class="insp-empty">No common properties</div>'; return; }
      for (var i = 0; i < data.properties.length; i++) {
        var prop = data.properties[i];
        var row = document.createElement('div');
        row.className = 'insp-row';
        row.innerHTML = '<div class="insp-label">' + escapeHtml(prop.name) + '</div><div class="insp-value"><span class="insp-readonly">' + escapeHtml(prop.type) + '</span></div>';
        el.appendChild(row);
      }
    });
  }

  function renderInspectorEmpty() {
    document.getElementById('inspector').innerHTML = '<div class="insp-empty">Select a node to inspect</div>';
    document.getElementById('status-selected').textContent = 'None';
    document.getElementById('status-path').textContent = '\u2014';
  }

  function escapeHtml(s) {
    var d = document.createElement('div');
    d.textContent = s;
    return d.innerHTML;
  }

  function renderInspector(data) {
    var el = document.getElementById('inspector');
    el.innerHTML = '';

    // Inspector action toolbar (Godot-style icon row): resource actions on the
    // left, history navigation on the right.
    var toolbar = document.createElement('div');
    toolbar.className = 'insp-toolbar';
    toolbar.innerHTML =
      '<button class="insp-tool-btn" id="insp-new-res" title="New Resource">&#10010;</button>' +
      '<button class="insp-tool-btn" id="insp-load-res" title="Load Resource">&#128193;</button>' +
      '<button class="insp-tool-btn" id="insp-save-res" title="Save Resource">&#128190;</button>' +
      '<button class="insp-tool-btn" id="insp-tool-menu" title="More options">&#8942;</button>' +
      '<span class="insp-toolbar-spacer"></span>' +
      '<button class="insp-tool-btn" id="inspector-history-back" title="Back (Alt+Left)" disabled>&larr;</button>' +
      '<button class="insp-tool-btn" id="inspector-history-forward" title="Forward (Alt+Right)" disabled>&rarr;</button>' +
      '<button class="insp-tool-btn" id="insp-history-btn" title="Object History">&#8634;</button>' +
      '<span id="resource-info" class="resource-info" style="display:none"><span class="resource-type"></span><span class="resource-path"></span></span>';
    el.appendChild(toolbar);
    (function() {
      function setAllSubgroups(collapse) {
        el.querySelectorAll('.insp-subgroup-arrow').forEach(function(a) { a.classList.toggle('collapsed', collapse); });
        el.querySelectorAll('.insp-subgroup-body').forEach(function(b) { b.classList.toggle('collapsed', collapse); });
      }
      var tm = document.getElementById('insp-tool-menu');
      if (tm) tm.addEventListener('click', function(e) { e.stopPropagation(); openSimpleMenu(tm, [['Expand All', function() { setAllSubgroups(false); }], ['Collapse All', function() { setAllSubgroups(true); }]]); });
      var nr = document.getElementById('insp-new-res'); if (nr) nr.addEventListener('click', function() { logMessage('info', 'New Resource'); });
      var lr = document.getElementById('insp-load-res'); if (lr) lr.addEventListener('click', function() { logMessage('info', 'Load Resource'); });
      var sr = document.getElementById('insp-save-res'); if (sr) sr.addEventListener('click', function() { logMessage('info', 'Save Resource'); });
      var hb = document.getElementById('insp-history-btn'); if (hb) hb.addEventListener('click', function() { logMessage('info', 'Object History'); });
    })();

    // Sub-resource breadcrumb (only if navigating sub-resources)
    if (subResourceStack.length > 0) {
      var breadcrumb = document.createElement('div');
      breadcrumb.className = 'insp-breadcrumb';
      breadcrumb.id = 'insp-breadcrumb';
      // Root item
      var rootCrumb = document.createElement('button');
      rootCrumb.className = 'insp-breadcrumb-item';
      rootCrumb.textContent = data['class'] || 'Node';
      rootCrumb.title = 'Back to node root';
      rootCrumb.addEventListener('click', function() {
        subResourceStack = [];
        fetchSelected();
      });
      breadcrumb.appendChild(rootCrumb);
      for (var bi = 0; bi < subResourceStack.length; bi++) {
        var sep = document.createElement('span');
        sep.className = 'insp-breadcrumb-sep';
        sep.textContent = '\u203A';
        breadcrumb.appendChild(sep);
        var crumb = document.createElement('button');
        crumb.className = 'insp-breadcrumb-item';
        if (bi === subResourceStack.length - 1) crumb.classList.add('active');
        crumb.textContent = subResourceStack[bi].label || ('SubResource #' + subResourceStack[bi].id);
        (function(idx) {
          crumb.addEventListener('click', function() {
            if (idx < subResourceStack.length - 1) {
              subResourceStack = subResourceStack.slice(0, idx + 1);
              fetchSelected();
            }
          });
        })(bi);
        breadcrumb.appendChild(crumb);
      }
      el.appendChild(breadcrumb);
    }

    // pat-mn3: Property search/filter
    var propFilterInput = document.createElement('input');
    propFilterInput.type = 'text';
    propFilterInput.id = 'insp-filter-input';
    propFilterInput.placeholder = 'Filter properties...';
    propFilterInput.style.cssText = 'flex:1;min-width:0;padding:3px 6px;font-size:11px;background:var(--bg);color:var(--text);border:1px solid var(--border);border-radius:2px;';
    propFilterInput.addEventListener('input', function() {
      var filter = propFilterInput.value.toLowerCase();
      var rows = el.querySelectorAll('.insp-row');
      for (var ri = 0; ri < rows.length; ri++) {
        var lbl = rows[ri].querySelector('.insp-label');
        if (lbl) {
          rows[ri].style.display = (!filter || lbl.textContent.toLowerCase().indexOf(filter) >= 0) ? '' : 'none';
        }
      }
    });
    var filterRow = document.createElement('div');
    filterRow.className = 'insp-filter-row';
    filterRow.appendChild(propFilterInput);
    var filterTools = document.createElement('button');
    filterTools.className = 'insp-tool-btn'; filterTools.id = 'insp-filter-tools';
    filterTools.innerHTML = '&#9881;'; filterTools.title = 'Property Tools';
    filterTools.addEventListener('click', function(e) {
      e.stopPropagation();
      openSimpleMenu(filterTools, [
        ['Expand All', function() { el.querySelectorAll('.insp-subgroup-arrow,.insp-subgroup-body').forEach(function(x) { x.classList.remove('collapsed'); }); }],
        ['Collapse All', function() { el.querySelectorAll('.insp-subgroup-arrow,.insp-subgroup-body').forEach(function(x) { x.classList.add('collapsed'); }); }]
      ]);
    });
    filterRow.appendChild(filterTools);
    el.appendChild(filterRow);

    // Wire up history buttons
    var hBack = document.getElementById('inspector-history-back');
    var hFwd = document.getElementById('inspector-history-forward');
    if (hBack) hBack.addEventListener('click', inspectorBack);
    if (hFwd) hFwd.addEventListener('click', inspectorForward);
    updateHistoryButtons();

    // Godot object-selector header: [class icon] NodeName [chevron].
    // Double-click the name to rename (preserves the rename action).
    var objSel = document.createElement('div');
    objSel.className = 'insp-object-selector';
    objSel.innerHTML = classIconHtml(data['class']);
    var objName = document.createElement('span');
    objName.className = 'insp-object-name';
    objName.textContent = data.name || '';
    objName.title = (data['class'] || 'Node') + ' — double-click to rename';
    objName.addEventListener('dblclick', function() {
      var inp = document.createElement('input');
      inp.type = 'text'; inp.value = data.name || '';
      inp.style.cssText = 'flex:1;font-size:12px;background:var(--bg);color:var(--text);border:1px solid var(--accent);border-radius:2px;padding:1px 4px;';
      var restore = function() { if (inp.parentNode === objSel) objSel.replaceChild(objName, inp); };
      inp.addEventListener('keydown', function(e) {
        if (e.key === 'Enter') {
          if (inp.value && inp.value !== data.name) {
            api('POST', '/api/node/rename', { node_id: data.id, new_name: inp.value }).then(function() { fetchScene(); fetchSelected(); });
          } else { restore(); }
        } else if (e.key === 'Escape') { restore(); }
      });
      inp.addEventListener('blur', restore);
      objSel.replaceChild(inp, objName);
      inp.focus(); inp.select();
    });
    objSel.appendChild(objName);
    var objChevron = document.createElement('span');
    objChevron.className = 'insp-object-chevron';
    objChevron.textContent = '▼';
    objSel.appendChild(objChevron);
    // pat-5b63c: Open Documentation button (Godot InspectorDock open_docs_button,
    // HelpSearch icon) — the one permanent inspector widget that was missing.
    // Sits at the right of the object-selector row and opens the class reference
    // for this object (OBJECT_REQUEST_HELP).
    var openDocs = document.createElement('button');
    openDocs.className = 'insp-tool-btn insp-open-docs';
    openDocs.id = 'insp-open-docs';
    openDocs.title = 'Open documentation for this object.';
    openDocs.innerHTML = '&#128270;';
    openDocs.addEventListener('click', function(ev) {
      ev.stopPropagation();
      var cls = data['class'] || 'Node';
      logMessage('info', 'Open Documentation: ' + cls);
      if (typeof toggleHelpDialog === 'function') toggleHelpDialog();
    });
    objSel.appendChild(openDocs);
    el.appendChild(objSel);

    // Update resource info in toolbar with structured type + path
    var resourceInfo = document.getElementById('resource-info');
    if (resourceInfo) {
      var typeSpan = resourceInfo.querySelector('.resource-type');
      var pathSpan = resourceInfo.querySelector('.resource-path');
      if (typeSpan) typeSpan.textContent = data['class'] || 'Node';
      if (pathSpan) pathSpan.textContent = data.path ? (' \u2014 ' + data.path) : '';
    }

    // Build property map
    var propMap = {};
    if (data.properties) {
      for (var i = 0; i < data.properties.length; i++) {
        propMap[data.properties[i].name] = data.properties[i];
      }
    }

    // Add class-specific defaults
    var cls = data['class'] || '';
    var defaults = (CLASS_DEFAULT_PROPS[cls] || []).slice();
    if (!CLASS_DEFAULT_PROPS[cls] && (cls.indexOf('2D') >= 0 || cls.indexOf('2d') >= 0)) {
      defaults = (CLASS_DEFAULT_PROPS['Node2D'] || []).slice();
    }
    // CollisionObject2D / StaticBody2D add their own properties (matches Godot).
    var chainForDefaults = getClassChain(cls);
    if (chainForDefaults.indexOf('CollisionObject2D') >= 0) {
      ['collision_layer', 'collision_mask', 'collision_priority', 'disable_mode', 'input_pickable'].forEach(function(p) { if (defaults.indexOf(p) < 0) defaults.push(p); });
    }
    if (chainForDefaults.indexOf('StaticBody2D') >= 0) {
      ['physics_material_override', 'constant_linear_velocity', 'constant_angular_velocity'].forEach(function(p) { if (defaults.indexOf(p) < 0) defaults.push(p); });
    }
    // CanvasItem-derived nodes: Texture / Material / extra Visibility + Ordering.
    if (chainForDefaults.indexOf('CanvasItem') >= 0) {
      ['z_as_relative', 'y_sort_enabled', 'top_level', 'clip_children', 'visibility_layer', 'texture_filter', 'texture_repeat', 'use_parent_material'].forEach(function(p) { if (defaults.indexOf(p) < 0) defaults.push(p); });
    }
    // Every node inherits Node: Process / Thread Group / Physics Interpolation / etc.
    ['process_mode', 'process_priority', 'process_physics_priority', 'process_thread_group', 'physics_interpolation_mode', 'auto_translate_mode', 'editor_description', 'unique_name_in_owner'].forEach(function(p) { if (defaults.indexOf(p) < 0) defaults.push(p); });
    for (var di = 0; di < defaults.length; di++) {
      var dpName = defaults[di];
      if (!propMap[dpName]) {
        var def = PROPERTY_DEFAULTS[dpName];
        if (def) {
          propMap[dpName] = { name: dpName, type: def.type, value: { type: def.type, value: def.value } };
        } else {
          propMap[dpName] = { name: dpName, type: 'String', value: { type: 'String', value: '' } };
        }
      }
    }

    // Group properties by declaring class + subgroup (Godot EditorInspector).
    var nodeClass = data['class'] || 'Node';
    var chain = getClassChain(nodeClass);
    var classBuckets = {};
    var propNames = Object.keys(propMap);
    for (var pi = 0; pi < propNames.length; pi++) {
      var prop = propMap[propNames[pi]];
      if (!prop || prop.type === 'Nil') continue;
      if (getPropCategory(prop.name) === 'Internal') continue;
      if (prop.name === 'name' || prop.name === '_script_path') continue;
      var decl = getPropDecl(prop.name, nodeClass, chain);
      var b = classBuckets[decl.cls] || (classBuckets[decl.cls] = { order: [], groups: {} });
      if (!b.groups[decl.group]) { b.groups[decl.group] = []; b.order.push(decl.group); }
      b.groups[decl.group].push(prop);
    }
    // Render class-category bars in hierarchy order (most-derived first).
    for (var ci = 0; ci < chain.length; ci++) {
      var bucket = classBuckets[chain[ci]];
      if (!bucket) continue;
      el.appendChild(makeClassCategoryBar(chain[ci]));
      renderClassBucket(el, data.id, bucket);
    }
    // Properties whose declaring class falls outside the chain → trailing bars.
    var extraClasses = Object.keys(classBuckets).filter(function(c) { return chain.indexOf(c) < 0; });
    for (var xi = 0; xi < extraClasses.length; xi++) {
      el.appendChild(makeClassCategoryBar(extraClasses[xi]));
      renderClassBucket(el, data.id, classBuckets[extraClasses[xi]]);
    }

    // Script picker + Add Metadata (Godot shows these at the bottom of the inspector).
    var sp = propMap['script'] || propMap['_script_path'];
    var scriptVal = (sp && sp.value) ? sp.value : { type: 'Resource', value: null };
    var scriptRow = document.createElement('div');
    scriptRow.className = 'insp-row';
    scriptRow.innerHTML = '<span class="insp-fav-btn"></span><div class="insp-label" title="script">Script</div>';
    var scriptValDiv = document.createElement('div'); scriptValDiv.className = 'insp-value';
    scriptValDiv.appendChild(makeResourcePicker(data.id, { name: 'script', value: scriptVal }, 'Script'));
    scriptRow.appendChild(scriptValDiv);
    el.appendChild(scriptRow);
    var metaWrap = document.createElement('div');
    metaWrap.style.cssText = 'padding:8px 0 4px;';
    var addMetaBtn = document.createElement('button');
    addMetaBtn.textContent = '+ Add Metadata';
    addMetaBtn.style.cssText = 'width:100%;font-size:11px;padding:5px;background:var(--panel);color:var(--text);border:1px solid var(--border);border-radius:3px;cursor:pointer;';
    addMetaBtn.addEventListener('click', function() { logMessage('info', 'Add Metadata (not yet implemented)'); });
    metaWrap.appendChild(addMetaBtn);
    el.appendChild(metaWrap);

    // pat-48k: @export variable display from script
    // If the node has a script, parse @export annotations and show them
    var scriptPath = null;
    if (data.properties) {
      for (var sp = 0; sp < data.properties.length; sp++) {
        if (data.properties[sp].name === '_script_path') {
          var spv = data.properties[sp].value;
          if (spv && spv.value && typeof spv.value === 'string') { scriptPath = spv.value; break; }
        }
      }
    }
    if (scriptPath) {
      api('GET', '/api/script?path=' + encodeURIComponent(scriptPath)).then(function(scriptData) {
        if (!scriptData || !scriptData.content) return;
        var exportVars = parseExportVars(scriptData.content);
        if (exportVars.length === 0) return;

        // pat-pcnuj: Group exports by @export_group / @export_subgroup
        var exportGroups = {};
        for (var ei = 0; ei < exportVars.length; ei++) {
          var ev = exportVars[ei];
          var gkey = ev.group || '(ungrouped)';
          if (!exportGroups[gkey]) exportGroups[gkey] = {};
          var sgkey = ev.subgroup || '';
          if (!exportGroups[gkey][sgkey]) exportGroups[gkey][sgkey] = [];
          exportGroups[gkey][sgkey].push(ev);
        }

        var groupKeys = Object.keys(exportGroups);
        for (var gi = 0; gi < groupKeys.length; gi++) {
          var gname = groupKeys[gi];
          var sectionTitle = gname === '(ungrouped)' ? 'Exports (@export)' : 'Exports: ' + gname;
          var expSection = createSection(sectionTitle, 'cat-exports-' + gi);
          var expBody = expSection.querySelector('.insp-section-body');
          expSection.setAttribute('data-export-group', gname);

          var subgroups = exportGroups[gname];
          var sgKeys = Object.keys(subgroups);
          for (var sgi = 0; sgi < sgKeys.length; sgi++) {
            // pat-pcnuj: Render subgroup header if non-empty
            if (sgKeys[sgi]) {
              var sgHeader = document.createElement('div');
              sgHeader.className = 'insp-subgroup-header';
              sgHeader.textContent = sgKeys[sgi];
              sgHeader.setAttribute('data-export-subgroup', sgKeys[sgi]);
              expBody.appendChild(sgHeader);
            }
            var sgVars = subgroups[sgKeys[sgi]];
            for (var svi = 0; svi < sgVars.length; svi++) {
              var sv = sgVars[svi];
              var svProp = propMap[sv.name] || { name: sv.name, type: sv.type || 'String', value: { type: sv.type || 'String', value: sv.default || '' } };
              expBody.appendChild(createPropertyRow(data.id, svProp));
            }
          }
          el.appendChild(expSection);
        }
      });
    }

    // pat-pcnuj: Favorite properties section (shown at top if any)
    var favProps = Object.keys(favoriteProperties).filter(function(fp) { return propMap[fp]; });
    if (favProps.length > 0) {
      var favSection = createSection('\u2605 Favorites', 'cat-favorites');
      var favBody = favSection.querySelector('.insp-section-body');
      for (var fi = 0; fi < favProps.length; fi++) {
        favBody.appendChild(createPropertyRow(data.id, propMap[favProps[fi]]));
      }
      // Insert favorites at the top (after node info)
      var firstSection = el.querySelector('.insp-section:nth-child(3)');
      if (firstSection) { el.insertBefore(favSection, firstSection); }
      else { el.appendChild(favSection); }
    }
  }

  // pat-48k + pat-pcnuj: Parse @export annotations from GDScript source
  // Supports @export, @export_group, @export_subgroup
  function parseExportVars(source) {
    var result = [];
    var currentGroup = '';
    var currentSubgroup = '';
    var lines = source.split('\n');
    for (var i = 0; i < lines.length; i++) {
      var line = lines[i].trim();
      // pat-pcnuj: @export_group("Name") and @export_subgroup("Name")
      var gm = line.match(/^@export_group\s*\(\s*"([^"]*)"\s*\)/);
      if (gm) { currentGroup = gm[1]; currentSubgroup = ''; continue; }
      var sgm = line.match(/^@export_subgroup\s*\(\s*"([^"]*)"\s*\)/);
      if (sgm) { currentSubgroup = sgm[1]; continue; }
      var m = line.match(/^@export\s+var\s+(\w+)\s*(?::\s*(\w+))?\s*(?:=\s*(.+))?$/);
      if (m) {
        result.push({
          name: m[1], type: m[2] || 'Variant', default: m[3] ? m[3].trim() : '',
          group: currentGroup, subgroup: currentSubgroup
        });
      }
    }
    return result;
  }

  // pat-pcnuj: Favorite properties (persisted in localStorage)
  var favoriteProperties = {};
  try {
    var favJson = localStorage.getItem('patina-fav-props');
    if (favJson) favoriteProperties = JSON.parse(favJson);
  } catch(e) {}

  function toggleFavoriteProperty(propName) {
    if (favoriteProperties[propName]) { delete favoriteProperties[propName]; }
    else { favoriteProperties[propName] = true; }
    try { localStorage.setItem('patina-fav-props', JSON.stringify(favoriteProperties)); } catch(e) {}
  }

  function isFavoriteProperty(propName) {
    return !!favoriteProperties[propName];
  }

  function createSection(title, key) {
    var section = document.createElement('div');
    section.className = 'insp-section';

    var header = document.createElement('div');
    header.className = 'insp-section-header';

    var toggleIcon = document.createElement('span');
    toggleIcon.className = 'insp-section-toggle' + (collapsedSections[key] ? ' collapsed' : '');
    toggleIcon.textContent = '\u25BC';

    var titleSpan = document.createElement('span');
    titleSpan.textContent = title;

    header.appendChild(toggleIcon);
    header.appendChild(titleSpan);

    var body = document.createElement('div');
    body.className = 'insp-section-body' + (collapsedSections[key] ? ' collapsed' : '');

    header.addEventListener('click', function() {
      collapsedSections[key] = !collapsedSections[key];
      toggleIcon.classList.toggle('collapsed');
      body.classList.toggle('collapsed');
    });

    section.appendChild(header);
    section.appendChild(body);
    return section;
  }

  function isNonDefault(propName, prop) {
    var def = PROPERTY_DEFAULTS[propName];
    if (!def) return false;
    var v = prop.value && prop.value.value;
    if (v === undefined || v === null) return false;
    if (def.type === 'Vector2' && Array.isArray(v) && Array.isArray(def.value)) {
      return v[0] !== def.value[0] || v[1] !== def.value[1];
    }
    if (def.type === 'Color' && Array.isArray(v) && Array.isArray(def.value)) {
      for (var i = 0; i < 4; i++) {
        if (Math.abs((v[i]||0) - (def.value[i]||0)) > 0.001) return true;
      }
      return false;
    }
    return v !== def.value;
  }

  // Godot humanizes property names: snake_case -> Title Case ("z_index" -> "Z Index").
  // A few labels Godot shortens beyond simple humanization.
  var LABEL_OVERRIDES = { process_thread_group: 'Group', editor_description: 'Description' };
  function humanizeProp(name) {
    if (!name) return '';
    return name.split('_').map(function(w) { return w ? w.charAt(0).toUpperCase() + w.slice(1) : ''; }).join(' ');
  }
  // Properties shown with a Godot resource picker, mapped to their resource type.
  var RESOURCE_PROPS = {
    texture: 'Texture2D', normal_map: 'Texture2D', texture_normal: 'Texture2D', texture_pressed: 'Texture2D',
    material: 'Material', physics_material_override: 'PhysicsMaterial2D', mesh: 'Mesh', shape: 'Shape2D',
    sprite_frames: 'SpriteFrames', theme: 'Theme', tile_set: 'TileSet'
  };
  function makeResourcePicker(nodeId, prop, resType) {
    var wrap = document.createElement('div');
    wrap.className = 'res-picker';
    var cur = prop.value && prop.value.value;
    var isEmpty = cur == null || cur === '' || cur === '(empty)' || cur === 'Nil';
    var main = document.createElement('div');
    main.className = 'res-picker-main';
    var lbl = document.createElement('span');
    lbl.className = 'res-picker-label' + (isEmpty ? '' : ' has-res');
    lbl.textContent = isEmpty ? '<empty>' : String(cur);
    var chev = document.createElement('span');
    chev.className = 'res-picker-chevron'; chev.textContent = '▼';
    main.appendChild(lbl); main.appendChild(chev);
    var load = document.createElement('span');
    load.className = 'res-picker-load'; load.textContent = '📁'; load.title = 'Quick Load ' + resType;
    function openMenu(anchor) {
      closeResMenu();
      var menu = document.createElement('div');
      menu.className = 'res-picker-menu'; menu.id = 'res-picker-menu';
      var items = [['New ' + resType, 'new'], ['Load...', 'load'], ['Clear', 'clear']];
      items.forEach(function(it) {
        if (it[1] === 'clear' && isEmpty) return;
        var d = document.createElement('div');
        d.textContent = it[0];
        d.addEventListener('click', function(e) {
          e.stopPropagation();
          if (it[1] === 'clear') { setProperty(nodeId, prop.name, { type: 'Resource', value: null }); }
          else { logMessage('info', it[0] + ' — ' + humanizeProp(prop.name) + ' (' + resType + ')'); }
          closeResMenu();
        });
        menu.appendChild(d);
      });
      document.body.appendChild(menu);
      var r = anchor.getBoundingClientRect();
      menu.style.left = r.left + 'px';
      menu.style.top = (r.bottom + 2) + 'px';
      setTimeout(function() { document.addEventListener('click', closeResMenu, { once: true }); }, 0);
    }
    main.addEventListener('click', function(e) { e.stopPropagation(); openMenu(main); });
    load.addEventListener('click', function(e) { e.stopPropagation(); openMenu(load); });
    wrap.appendChild(main); wrap.appendChild(load);
    return wrap;
  }
  function closeResMenu() {
    var m = document.getElementById('res-picker-menu');
    if (m && m.parentNode) m.parentNode.removeChild(m);
  }
  // Small popup menu anchored under an element (reused by toolbar/tools icons).
  function openSimpleMenu(anchor, items) {
    closeResMenu();
    var menu = document.createElement('div');
    menu.className = 'res-picker-menu'; menu.id = 'res-picker-menu';
    items.forEach(function(it) {
      var d = document.createElement('div'); d.textContent = it[0];
      d.addEventListener('click', function(e) { e.stopPropagation(); it[1](); closeResMenu(); });
      menu.appendChild(d);
    });
    document.body.appendChild(menu);
    var r = anchor.getBoundingClientRect();
    menu.style.left = Math.max(4, Math.min(r.left, window.innerWidth - 184)) + 'px';
    menu.style.top = (r.bottom + 2) + 'px';
    setTimeout(function() { document.addEventListener('click', closeResMenu, { once: true }); }, 0);
  }
  // Enum dropdown options for known Int enum properties.
  var ENUM_OPTIONS = {
    disable_mode: [['0', 'Remove'], ['1', 'Make Static'], ['2', 'Keep Active']],
    process_mode: [['0', 'Inherit'], ['1', 'Pausable'], ['2', 'When Paused'], ['3', 'Always'], ['4', 'Disabled']],
    blend_mode: [['0', 'Mix'], ['1', 'Add'], ['2', 'Subtract'], ['3', 'Multiply'], ['4', 'Premultiplied Alpha']],
    texture_filter: [['0', 'Inherit'], ['1', 'Nearest'], ['2', 'Linear'], ['3', 'Nearest Mipmap'], ['4', 'Linear Mipmap']],
    texture_repeat: [['0', 'Inherit'], ['1', 'Disabled'], ['2', 'Enabled'], ['3', 'Mirror']],
    physics_interpolation_mode: [['0', 'Inherit'], ['1', 'On'], ['2', 'Off']],
    auto_translate_mode: [['0', 'Inherit'], ['1', 'Always'], ['2', 'Disabled']],
    process_thread_group: [['0', 'Inherit'], ['1', 'Main Thread'], ['2', 'Sub Thread']],
    clip_children: [['0', 'Disabled'], ['1', 'Clip Only'], ['2', 'Clip and Draw']]
  };
  // Godot-style bit-flag grid (collision Layer/Mask): 20 numbered toggle cells.
  function makeBitFlagGrid(nodeId, prop) {
    // Godot layout: grouped 4x2 blocks (1-4/5-8, 9-12/…), an expand chevron for
    // layers 25-32, and a vertical ⋮ options menu listing every layer.
    var wrap = document.createElement('div');
    wrap.className = 'bitflag-wrap';
    var value = parseInt(prop.value && prop.value.value) || 0;
    var expanded = false;
    var gridCol = document.createElement('div');
    gridCol.className = 'bitflag-gridcol';
    var grid = document.createElement('div');
    grid.className = 'bitflag-grid';
    function commit() { setProperty(nodeId, prop.name, { type: 'Int', value: value }); }
    function build() {
      grid.innerHTML = '';
      var nBlocks = expanded ? 4 : 3;
      for (var b = 0; b < nBlocks; b++) {
        var block = document.createElement('div');
        block.className = 'bitflag-block';
        for (var i = 0; i < 8; i++) {
          var layer = b * 8 + i + 1;
          var bit = layer - 1;
          var cell = document.createElement('div');
          cell.className = 'bitflag-cell' + ((value & (1 << bit)) ? ' on' : '');
          cell.textContent = layer;
          cell.title = 'Layer ' + layer;
          (function(bb, c) { c.addEventListener('click', function() { value ^= (1 << bb); c.classList.toggle('on'); commit(); }); })(bit, cell);
          block.appendChild(cell);
        }
        grid.appendChild(block);
      }
    }
    build();
    var chevron = document.createElement('span');
    chevron.className = 'bitflag-expand'; chevron.textContent = '⌄';
    chevron.title = 'Show layers 25–32';
    chevron.addEventListener('click', function(e) { e.stopPropagation(); expanded = !expanded; chevron.textContent = expanded ? '⌃' : '⌄'; build(); });
    gridCol.appendChild(grid);
    gridCol.appendChild(chevron);
    var menuBtn = document.createElement('button');
    menuBtn.className = 'bitflag-menu'; menuBtn.innerHTML = '⋮';
    menuBtn.title = 'Layer options';
    menuBtn.addEventListener('click', function(e) {
      e.stopPropagation();
      var items = [];
      for (var L = 1; L <= 32; L++) {
        (function(layer) {
          var bit = layer - 1;
          items.push([(value & (1 << bit) ? '✓ ' : '  ') + 'Layer ' + layer, function() { value ^= (1 << bit); build(); commit(); }]);
        })(L);
      }
      items.push(['Edit Layer Names…', function() { logMessage('info', 'Edit Layer Names → Project Settings'); }]);
      openSimpleMenu(menuBtn, items);
    });
    wrap.appendChild(gridCol);
    wrap.appendChild(menuBtn);
    return wrap;
  }

  function createPropertyRow(nodeId, prop) {
    var row = document.createElement('div');
    row.className = 'insp-row';

    // pat-pcnuj: Favorite star button
    var favBtn = document.createElement('span');
    favBtn.className = 'insp-fav-btn' + (isFavoriteProperty(prop.name) ? ' active' : '');
    favBtn.textContent = '\u2605';
    favBtn.title = 'Toggle favorite';
    favBtn.addEventListener('click', function() {
      toggleFavoriteProperty(prop.name);
      favBtn.classList.toggle('active');
    });
    row.appendChild(favBtn);

    var label = document.createElement('div');
    label.className = 'insp-label';
    label.textContent = LABEL_OVERRIDES[prop.name] || humanizeProp(prop.name);
    label.title = prop.name;
    row.appendChild(label);

    var val = document.createElement('div');
    val.className = 'insp-value';

    var type = prop.type;
    var v = prop.value && prop.value.value;

    if (RESOURCE_PROPS[prop.name]) {
      val.appendChild(makeResourcePicker(nodeId, prop, RESOURCE_PROPS[prop.name]));
    } else if (type === 'String') {
      if (prop.name === 'editor_description') {
        var ta = document.createElement('textarea');
        ta.value = v != null ? String(v) : '';
        ta.rows = 2;
        ta.style.cssText = 'flex:1;min-width:0;resize:vertical;font:inherit;font-size:11px;background:var(--bg);color:var(--text);border:1px solid var(--border);border-radius:2px;padding:3px 6px;';
        ta.addEventListener('change', function() { setProperty(nodeId, prop.name, { type: 'String', value: ta.value }); });
        val.appendChild(ta);
      } else {
        var input = document.createElement('input');
        input.type = 'text';
        input.value = v != null ? String(v) : '';
        input.addEventListener('change', function() {
          setProperty(nodeId, prop.name, { type: 'String', value: input.value });
        });
        val.appendChild(input);
      }
    } else if (type === 'Int') {
      if (prop.name === 'collision_layer' || prop.name === 'collision_mask') {
        row.classList.add('insp-row-flags');
        val.appendChild(makeBitFlagGrid(nodeId, prop));
      } else if (ENUM_OPTIONS[prop.name]) {
        var esel = document.createElement('select');
        ENUM_OPTIONS[prop.name].forEach(function(o) {
          var opt = document.createElement('option');
          opt.value = o[0]; opt.textContent = o[1];
          if (String(v) === o[0]) opt.selected = true;
          esel.appendChild(opt);
        });
        esel.addEventListener('change', function() {
          setProperty(nodeId, prop.name, { type: 'Int', value: parseInt(esel.value) || 0 });
        });
        val.appendChild(esel);
      } else if (prop.name === 'body_type') {
        var sel = document.createElement('select');
        var opts = [['0','Static'],['1','Kinematic'],['2','Rigid'],['3','Character']];
        for (var oi = 0; oi < opts.length; oi++) {
          var opt = document.createElement('option');
          opt.value = opts[oi][0]; opt.textContent = opts[oi][1];
          if (String(v) === opts[oi][0]) opt.selected = true;
          sel.appendChild(opt);
        }
        sel.addEventListener('change', function() {
          setProperty(nodeId, prop.name, { type: 'Int', value: parseInt(sel.value) || 0 });
        });
        val.appendChild(sel);
      } else if (prop.name === 'size_flags_horizontal' || prop.name === 'size_flags_vertical') {
        var sel = document.createElement('select');
        var flagOpts = [['0','Fill'],['1','Expand'],['2','Shrink Center'],['3','Shrink End']];
        for (var fi = 0; fi < flagOpts.length; fi++) {
          var opt = document.createElement('option');
          opt.value = flagOpts[fi][0]; opt.textContent = flagOpts[fi][1];
          if (String(v) === flagOpts[fi][0]) opt.selected = true;
          sel.appendChild(opt);
        }
        sel.addEventListener('change', function() {
          setProperty(nodeId, prop.name, { type: 'Int', value: parseInt(sel.value) || 0 });
        });
        val.appendChild(sel);
      } else {
        var input = document.createElement('input');
        input.type = 'number'; input.step = '1';
        input.value = v != null ? v : 0;
        input.addEventListener('change', function() {
          setProperty(nodeId, prop.name, { type: 'Int', value: parseInt(input.value) || 0 });
        });
        val.appendChild(input);
      }
    } else if (type === 'Float') {
      var input = document.createElement('input');
      input.type = 'number'; input.step = '0.1';
      input.value = v != null ? v : 0;
      input.addEventListener('change', function() {
        setProperty(nodeId, prop.name, { type: 'Float', value: parseFloat(input.value) || 0 });
      });
      val.appendChild(input);
      if (PROP_UNITS[prop.name]) {
        var fu = document.createElement('span'); fu.className = 'insp-unit'; fu.textContent = PROP_UNITS[prop.name]; val.appendChild(fu);
      }
    } else if (type === 'Bool') {
      var boolDiv = document.createElement('div');
      boolDiv.className = 'bool-editor';
      var cb = document.createElement('input');
      cb.type = 'checkbox'; cb.checked = !!v;
      cb.id = 'cb-' + nodeId + '-' + prop.name;
      var lbl = document.createElement('label');
      lbl.setAttribute('for', cb.id);
      lbl.textContent = v ? 'On' : 'Off';
      cb.addEventListener('change', function() {
        lbl.textContent = cb.checked ? 'On' : 'Off';
        setProperty(nodeId, prop.name, { type: 'Bool', value: cb.checked });
      });
      boolDiv.appendChild(cb); boolDiv.appendChild(lbl);
      val.appendChild(boolDiv);
    } else if (type === 'Vector2') {
      var arr = Array.isArray(v) ? v.slice() : [0, 0];
      var unit = PROP_UNITS[prop.name] || '';
      var vec = document.createElement('div');
      vec.className = 'vec2-editor';
      var linkable = (prop.name === 'scale');
      var linked = linkable; // Godot links scale axes by default
      function makeVec2Field(axis, idx) {
        var field = document.createElement('div');
        field.className = 'vec2-field';
        var lbl = document.createElement('span');
        lbl.className = 'vec2-label ' + axis + '-label';
        lbl.textContent = axis.toUpperCase();
        var inp = document.createElement('input');
        inp.type = 'number'; inp.step = '0.1'; inp.className = 'vec2-input';
        inp.value = arr[idx] != null ? arr[idx] : 0;
        field.appendChild(lbl); field.appendChild(inp);
        if (unit) { var u = document.createElement('span'); u.className = 'insp-unit'; u.textContent = unit; field.appendChild(u); }
        return { field: field, input: inp };
      }
      var xf = makeVec2Field('x', 0);
      var yf = makeVec2Field('y', 1);
      function sendVec2() {
        arr = [parseFloat(xf.input.value)||0, parseFloat(yf.input.value)||0];
        setProperty(nodeId, prop.name, { type: 'Vector2', value: arr });
      }
      // Proportional link: edits to one axis scale the other by the same ratio.
      function onAxis(srcInp, dstInp, srcIdx, dstIdx) {
        var nv = parseFloat(srcInp.value) || 0;
        if (linked) {
          var old = arr[srcIdx];
          if (old && old !== 0) { dstInp.value = +((parseFloat(dstInp.value)||0) * (nv / old)).toFixed(4); }
          else { dstInp.value = nv; }
        }
        sendVec2();
      }
      xf.input.addEventListener('change', function() { onAxis(xf.input, yf.input, 0, 1); });
      yf.input.addEventListener('change', function() { onAxis(yf.input, xf.input, 1, 0); });
      vec.appendChild(xf.field); vec.appendChild(yf.field);
      if (linkable) {
        var link = document.createElement('span');
        link.className = 'vec2-link' + (linked ? ' active' : '');
        link.textContent = '🔗';
        link.title = 'Keep ' + prop.name + ' axes proportional';
        link.addEventListener('click', function() { linked = !linked; link.classList.toggle('active'); });
        vec.appendChild(link);
      }
      val.appendChild(vec);
    } else if (type === 'Vector3') {
      var arr = Array.isArray(v) ? v : [0, 0, 0];
      ['x','y','z'].forEach(function(axis, i) {
        var al = document.createElement('span');
        al.className = 'vec-label'; al.textContent = axis;
        var ai = document.createElement('input');
        ai.type = 'number'; ai.step = '0.1'; ai.className = 'vec-input';
        ai.value = arr[i] != null ? arr[i] : 0;
        ai.addEventListener('change', function() {
          var vals = [];
          val.querySelectorAll('.vec-input').forEach(function(inp) { vals.push(parseFloat(inp.value)||0); });
          setProperty(nodeId, prop.name, { type: 'Vector3', value: vals });
        });
        val.appendChild(al); val.appendChild(ai);
      });
    } else if (type === 'Color') {
      var colorArr = Array.isArray(v) && v.length >= 3 ? v : [1, 1, 1, 1];
      var colorDiv = document.createElement('div');
      colorDiv.className = 'color-editor';
      var swatch = document.createElement('div');
      swatch.className = 'color-swatch';
      function updateSwatch() {
        swatch.style.background = 'rgba(' +
          Math.round(colorArr[0]*255) + ',' + Math.round(colorArr[1]*255) + ',' +
          Math.round(colorArr[2]*255) + ',' + (colorArr[3] != null ? colorArr[3] : 1) + ')';
      }
      updateSwatch();
      var picker = document.createElement('input');
      picker.type = 'color'; picker.style.display = 'none';
      var rr = Math.round((colorArr[0]||0)*255), gg = Math.round((colorArr[1]||0)*255), bb = Math.round((colorArr[2]||0)*255);
      picker.value = '#' + [rr,gg,bb].map(function(c){return c.toString(16).padStart(2,'0');}).join('');
      swatch.addEventListener('click', function() { picker.click(); });
      picker.addEventListener('change', function() {
        var hex = picker.value;
        colorArr[0] = parseInt(hex.slice(1,3),16)/255;
        colorArr[1] = parseInt(hex.slice(3,5),16)/255;
        colorArr[2] = parseInt(hex.slice(5,7),16)/255;
        updateSwatch(); hexInput.value = hex;
        setProperty(nodeId, prop.name, { type: 'Color', value: colorArr.slice() });
      });
      var hexInput = document.createElement('input');
      hexInput.type = 'text'; hexInput.className = 'color-hex';
      hexInput.value = picker.value;
      hexInput.addEventListener('change', function() {
        var hex = hexInput.value;
        if (hex.match(/^#[0-9a-fA-F]{6}$/)) {
          colorArr[0] = parseInt(hex.slice(1,3),16)/255;
          colorArr[1] = parseInt(hex.slice(3,5),16)/255;
          colorArr[2] = parseInt(hex.slice(5,7),16)/255;
          updateSwatch(); picker.value = hex;
          setProperty(nodeId, prop.name, { type: 'Color', value: colorArr.slice() });
        }
      });
      var alphaGroup = document.createElement('div');
      alphaGroup.className = 'color-slider-group';
      var alphaLabel = document.createElement('span');
      alphaLabel.className = 'color-slider-label'; alphaLabel.textContent = 'A';
      var alphaInput = document.createElement('input');
      alphaInput.type = 'number'; alphaInput.min = '0'; alphaInput.max = '1'; alphaInput.step = '0.05';
      alphaInput.value = colorArr[3] != null ? colorArr[3].toFixed(2) : '1.00';
      alphaInput.style.width = '50px';
      alphaInput.addEventListener('change', function() {
        colorArr[3] = parseFloat(alphaInput.value) || 1;
        updateSwatch();
        setProperty(nodeId, prop.name, { type: 'Color', value: colorArr.slice() });
      });
      alphaGroup.appendChild(alphaLabel); alphaGroup.appendChild(alphaInput);
      colorDiv.appendChild(swatch); colorDiv.appendChild(picker);
      colorDiv.appendChild(hexInput); colorDiv.appendChild(alphaGroup);
      val.appendChild(colorDiv);
    } else if (type === 'NodePath' || prop.name === 'texture' || prop.name === 'script' || prop.name.indexOf('path') >= 0) {
      var npDiv = document.createElement('div');
      npDiv.className = 'nodepath-editor';
      var npInput = document.createElement('input');
      npInput.type = 'text'; npInput.className = 'nodepath-input';
      npInput.value = v != null ? String(v) : '';
      npInput.addEventListener('change', function() {
        setProperty(nodeId, prop.name, { type: type || 'String', value: npInput.value });
      });
      // Accept asset drops onto this property field
      npDiv.addEventListener('dragover', function(de) {
        de.preventDefault();
        de.dataTransfer.dropEffect = 'copy';
        npDiv.classList.add('insp-drop-active');
      });
      npDiv.addEventListener('dragleave', function() {
        npDiv.classList.remove('insp-drop-active');
      });
      npDiv.addEventListener('drop', function(de) {
        de.preventDefault(); de.stopPropagation();
        npDiv.classList.remove('insp-drop-active');
        var raw = de.dataTransfer.getData('text/plain');
        if (!raw) return;
        var dragData;
        try { dragData = JSON.parse(raw); } catch(ex) { return; }
        if (!dragData || !dragData.path) return;
        var resPath = dragData.path;
        npInput.value = resPath;
        setProperty(nodeId, prop.name, { type: type || 'String', value: resPath });
        logMessage('info', 'Set ' + prop.name + ' = ' + resPath);
      });
      var npBtn = document.createElement('button');
      npBtn.className = 'nodepath-pick'; npBtn.textContent = '...';
      npBtn.title = 'Pick node (not yet implemented)';
      npDiv.appendChild(npInput); npDiv.appendChild(npBtn);
      val.appendChild(npDiv);
    } else if (type === 'Array') {
      var arrDiv = document.createElement('div');
      arrDiv.className = 'array-editor';
      var items = Array.isArray(v) ? v : [];
      var header = document.createElement('div');
      header.className = 'array-header';
      header.textContent = 'Array (' + items.length + ' elements)';
      var addBtn = document.createElement('button');
      addBtn.className = 'array-add'; addBtn.textContent = '+'; addBtn.title = 'Add element';
      addBtn.addEventListener('click', function() {
        items.push({type:'String',value:''});
        setProperty(nodeId, prop.name, {type:'Array',value:items});
        setTimeout(fetchSelected, 100);
      });
      header.appendChild(addBtn);
      arrDiv.appendChild(header);
      for (var ai = 0; ai < items.length; ai++) {
        (function(idx) {
          var itemRow = document.createElement('div');
          itemRow.className = 'array-item';
          var itemLabel = document.createElement('span');
          itemLabel.textContent = '[' + idx + '] ';
          var itemVal = document.createElement('span');
          itemVal.textContent = items[idx] && items[idx].value != null ? String(items[idx].value) : '';
          var removeBtn = document.createElement('button');
          removeBtn.className = 'array-remove'; removeBtn.textContent = 'x'; removeBtn.title = 'Remove';
          removeBtn.addEventListener('click', function() {
            items.splice(idx, 1);
            setProperty(nodeId, prop.name, {type:'Array',value:items});
            setTimeout(fetchSelected, 100);
          });
          itemRow.appendChild(itemLabel); itemRow.appendChild(itemVal); itemRow.appendChild(removeBtn);
          arrDiv.appendChild(itemRow);
        })(ai);
      }
      val.appendChild(arrDiv);
    } else if (type === 'Dictionary') {
      var dictDiv = document.createElement('div');
      dictDiv.className = 'dict-editor';
      var entries = (v && typeof v === 'object') ? v : {};
      var keys = Object.keys(entries);
      var dHeader = document.createElement('div');
      dHeader.className = 'dict-header';
      dHeader.textContent = 'Dictionary (' + keys.length + ' keys)';
      var dAddBtn = document.createElement('button');
      dAddBtn.className = 'dict-add'; dAddBtn.textContent = '+'; dAddBtn.title = 'Add key';
      dAddBtn.addEventListener('click', function() {
        var newKey = prompt('Key name:');
        if (newKey && !entries[newKey]) {
          entries[newKey] = {type:'String',value:''};
          setProperty(nodeId, prop.name, {type:'Dictionary',value:entries});
          setTimeout(fetchSelected, 100);
        }
      });
      dHeader.appendChild(dAddBtn);
      dictDiv.appendChild(dHeader);
      for (var di = 0; di < keys.length; di++) {
        (function(key) {
          var kvRow = document.createElement('div');
          kvRow.className = 'dict-item';
          var kSpan = document.createElement('span');
          kSpan.className = 'dict-key'; kSpan.textContent = key + ': ';
          var vSpan = document.createElement('span');
          vSpan.textContent = entries[key] && entries[key].value != null ? String(entries[key].value) : '';
          var dRemoveBtn = document.createElement('button');
          dRemoveBtn.className = 'dict-remove'; dRemoveBtn.textContent = 'x'; dRemoveBtn.title = 'Remove key';
          dRemoveBtn.addEventListener('click', function() {
            delete entries[key];
            setProperty(nodeId, prop.name, {type:'Dictionary',value:entries});
            setTimeout(fetchSelected, 100);
          });
          kvRow.appendChild(kSpan); kvRow.appendChild(vSpan); kvRow.appendChild(dRemoveBtn);
          dictDiv.appendChild(kvRow);
        })(keys[di]);
      }
      val.appendChild(dictDiv);
    } else if (type === 'Resource') {
      var resDiv = document.createElement('div');
      resDiv.className = 'resource-editor';
      resDiv.textContent = v != null ? String(v) : '(empty)';
      val.appendChild(resDiv);
      // pat-pcnuj: Sub-resource inline edit / navigate button
      if (v && typeof v === 'string' && v.indexOf('SubResource') >= 0) {
        var editBtn = document.createElement('button');
        editBtn.className = 'insp-sub-resource-btn';
        editBtn.textContent = '\u270E Edit';
        editBtn.title = 'Edit sub-resource inline';
        editBtn.setAttribute('data-sub-resource', v);
        (function(subResRef, propName) {
          editBtn.addEventListener('click', function() {
            // Extract sub-resource ID from e.g. "SubResource(\"1\")"
            var match = subResRef.match(/SubResource\(\s*"?(\d+)"?\s*\)/);
            var subId = match ? match[1] : subResRef;
            subResourceStack.push({ id: subId, label: propName + ' (' + subId + ')' });
            logMessage('info', 'Navigating to sub-resource: ' + propName + ' #' + subId);
            fetchSelected();
          });
        })(v, prop.name);
        val.appendChild(editBtn);
      }
    } else if (type === 'StringName') {
      var snInput = document.createElement('input');
      snInput.type = 'text';
      snInput.value = v != null ? String(v) : '';
      snInput.addEventListener('change', function() {
        setProperty(nodeId, prop.name, { type: 'StringName', value: snInput.value });
      });
      val.appendChild(snInput);
    } else if (type === 'Rect2') {
      var rect = Array.isArray(v) ? v : [0, 0, 0, 0];
      var rectLabels = ['X','Y','W','H'];
      var rectDiv = document.createElement('div');
      rectDiv.className = 'rect2-editor';
      for (var ri = 0; ri < 4; ri++) {
        var rl = document.createElement('span'); rl.textContent = rectLabels[ri] + ':';
        var rinp = document.createElement('input');
        rinp.type = 'number'; rinp.step = '0.1'; rinp.className = 'rect2-input';
        rinp.value = rect[ri] != null ? rect[ri] : 0;
        rectDiv.appendChild(rl); rectDiv.appendChild(rinp);
      }
      val.appendChild(rectDiv);
    } else {
      var span = document.createElement('span');
      span.className = 'insp-readonly';
      span.textContent = type + ': ' + JSON.stringify(v);
      val.appendChild(span);
    }

    row.appendChild(val);

    // Revert button for non-default values
    if (isNonDefault(prop.name, prop)) {
      var revertBtn = document.createElement('button');
      revertBtn.className = 'insp-revert';
      revertBtn.innerHTML = '&#8634;';
      revertBtn.title = 'Reset to default';
      revertBtn.addEventListener('click', (function(pname) { return function() {
        var def = PROPERTY_DEFAULTS[pname];
        if (def) {
          setProperty(nodeId, pname, { type: def.type, value: def.value });
          setTimeout(fetchSelected, 100);
        }
      }; })(prop.name));
      row.appendChild(revertBtn);
    }

    return row;
  }

  async function setProperty(nodeId, property, value) {
    await api('POST', '/api/property/set', { node_id: nodeId, property: property, value: value });
  }

  // ---- Viewport ----
  var viewportImg = null;
  var isDragging = false;
  var dragStartX = 0;
  var dragStartY = 0;
  var DRAG_THRESHOLD = 3;
  var viewportZoom = 1.0;
  var viewportPanX = 0;
  var viewportPanY = 0;
  var isPanning = false;
  var panStartX = 0;
  var panStartY = 0;
  var panStartPanX = 0;
  var panStartPanY = 0;

  function viewportCoords(e) {
    var rect = viewportImg.getBoundingClientRect();
    var scaleX = viewportImg.naturalWidth / rect.width;
    var scaleY = viewportImg.naturalHeight / rect.height;
    return { x: Math.round((e.clientX - rect.left) * scaleX), y: Math.round((e.clientY - rect.top) * scaleY) };
  }

  // ---- 3D spatial-editor navigation (pat-3dnav) ----
  // In 3D mode the viewport is a perspective render from a server-side editor
  // camera. Mouse drag orbits (or pans with Shift) that camera and the wheel
  // zooms it, instead of running the 2D box-select / node-drag logic — which
  // would wrongly act on 2D nodes and draw 2D selection boxes over the 3D view.
  var nav3D = { active: false, kind: 'idle', mode: 'orbit', lastX: 0, lastY: 0, startX: 0, startY: 0, moved: false, add: false };
  // Latest 3D camera state (yaw/pitch/distance/focus + view-basis bx/by/bz) used
  // to draw the navigation gizmo. Refreshed on entering 3D and on every camera
  // move so the gizmo tracks the view smoothly.
  var nav3DState = null;
  // Apply a fresh 3D camera state. The cheap part (storing the state + scheduling
  // the throttled PNG refresh) runs every call, but the EXPENSIVE part — redrawing
  // the overlay canvas and rebuilding the view-menu / spatial-toolbar DOM — is
  // coalesced to one animation frame. A fast spin yields ~60 state updates/second;
  // doing the canvas+DOM work synchronously on each one thrashed the main thread
  // (it could wedge the page). rAF collapses a burst of updates into a single
  // repaint, keeping spinning smooth.
  var _vp3dVisualScheduled = false;
  function apply3DState(s) {
    if (!(s && s.bx)) return;
    nav3DState = s;
    schedule3DViewportRefresh();
    if (_vp3dVisualScheduled) return;
    _vp3dVisualScheduled = true;
    requestAnimationFrame(function() {
      _vp3dVisualScheduled = false;
      renderViewportOverlay(); updateViewMenu(); updateVp3dToolbar();
    });
  }
  // Snappy camera feedback: after a camera move, the server's idle render loop
  // repaints the 3D frame on its next tick (~50ms) once the camera changes the
  // render signature. Fetch the PNG just after so the viewport tracks the camera
  // at interactive rate instead of waiting up to the 200ms background poll. Rapid
  // drag moves coalesce into one pending refresh, and refreshViewport()'s
  // single-in-flight guard keeps fetches from stacking.
  var _vp3dRefreshTimer = null;
  function schedule3DViewportRefresh() {
    if (_vp3dRefreshTimer) return;
    // Refresh on the next animation frame — this paces the viewport fetch to the
    // display's refresh (≈60 fps) so a fast camera move renders as smoothly as the
    // monitor allows, without over-fetching. The server now produces BMP frames
    // fast enough (~60+ fps while orbiting) to feed this. refreshViewport()'s
    // single-in-flight guard still prevents stacking if a frame is slow.
    _vp3dRefreshTimer = requestAnimationFrame(function() { _vp3dRefreshTimer = null; refreshViewport(); });
  }
  // 3D view menu (Godot's "Perspective" dropdown): projection toggle + presets.
  function setupViewMenu() {
    var btn = document.getElementById('vp3d-view-btn');
    var dd = document.getElementById('vp3d-view-dropdown');
    if (!btn || !dd) return;
    btn.addEventListener('click', function(e) {
      e.stopPropagation();
      dd.style.display = dd.style.display === 'none' ? 'block' : 'none';
    });
    document.addEventListener('click', function() { dd.style.display = 'none'; });
    dd.addEventListener('click', function(e) {
      var mi = e.target.closest ? e.target.closest('.vp3d-mi') : null;
      if (!mi) return;
      e.stopPropagation();
      dd.style.display = 'none';
      if (mi.hasAttribute('data-view')) {
        api('POST', '/api/viewport/3d/view_preset', { preset: mi.getAttribute('data-view') }).then(apply3DState);
      } else if (mi.hasAttribute('data-proj')) {
        var wantOrtho = mi.getAttribute('data-proj') === 'true';
        var isOrtho = !!(nav3DState && nav3DState.ortho);
        if (wantOrtho !== isOrtho) api('POST', '/api/viewport/3d/projection', {}).then(apply3DState);
      }
    });
  }
  function updateViewMenu() {
    var menu = document.getElementById('vp3d-view-menu');
    var btn = document.getElementById('vp3d-view-btn');
    if (!menu || !btn) return;
    menu.style.display = (editorMode === '3d') ? 'block' : 'none';
    var ortho = !!(nav3DState && nav3DState.ortho);
    btn.textContent = ortho ? 'Orthogonal' : 'Perspective';
    var dd = document.getElementById('vp3d-view-dropdown');
    if (dd) {
      var items = dd.querySelectorAll('.vp3d-mi[data-proj]');
      for (var i = 0; i < items.length; i++) {
        var on = (items[i].getAttribute('data-proj') === 'true') === ortho;
        var chk = items[i].querySelector('.vp3d-check');
        if (chk) chk.innerHTML = on ? '&#10003;' : '';
      }
    }
  }
  function refresh3DState() {
    if (editorMode !== '3d') return;
    api('GET', '/api/viewport/3d/state').then(apply3DState);
  }
  // Push the active transform tool (Q/W/E/S) to the server's 3D gizmo mode so the
  // gizmo renders and axis-dragging works. No-op outside 3D mode.
  function push3DGizmoMode() {
    if (editorMode !== '3d') return;
    api('POST', '/api/viewport/3d/gizmo_mode', { mode: currentToolMode });
  }
  // Godot's spatial-editor mouse scheme:
  //   Middle / Right drag = orbit (Shift = pan); Wheel = zoom;
  //   Left click = raycast-select (Shift/Ctrl adds); Left drag on empty space =
  //   rubber-band box-select; Left drag on a gizmo axis (transform tool active)
  //   = move/rotate/scale.
  // While the camera is moving, the server renders the 3D frame at reduced
  // resolution for speed and the browser upscales it. Switch the <img> to smooth
  // (bilinear) scaling during that motion so the low-res frame looks smooth rather
  // than blocky, then back to crisp 'pixelated' when the camera stops (the frame
  // is re-rendered full-resolution then).
  function vp3dSetSmooth(on) {
    if (viewportImg) viewportImg.style.imageRendering = on ? 'auto' : 'pixelated';
  }
  function begin3DCamera(e) {            // middle button: orbit / pan
    nav3D.active = true; nav3D.kind = 'camera';
    nav3D.mode = e.shiftKey ? 'pan' : 'orbit';
    nav3D.lastX = e.clientX; nav3D.lastY = e.clientY; nav3D.moved = false;
    nav3D.gizmo = false; nav3D.gizmoPending = false;
    vp3dSetSmooth(true);
  }
  function begin3DFly(e) {               // right button: Godot free-look (+ WASD/QE)
    nav3D.active = true; nav3D.kind = 'fly';
    nav3D.lastX = e.clientX; nav3D.lastY = e.clientY; nav3D.moved = false;
    nav3D.gizmo = false; nav3D.gizmoPending = false;
    vp3dSetSmooth(true);
    api('POST', '/api/viewport/3d/fly/begin', {}).then(apply3DState);
    document.addEventListener('keydown', fly3DKeyDown);
  }
  // While free-looking, WASD flies the camera and Q/E drop/raise it (Godot).
  // Godot's freelook modifiers: holding Shift moves ×3 faster, Alt ×0.333 slower.
  function fly3DKeyDown(e) {
    if (nav3D.kind !== 'fly') return;
    var m = { w: [0, 0, 1], s: [0, 0, -1], a: [-1, 0, 0], d: [1, 0, 0], q: [0, -1, 0], e: [0, 1, 0] }[(e.key || '').toLowerCase()];
    if (!m) return;
    e.preventDefault();
    var speed = e.shiftKey ? 3.0 : (e.altKey ? 0.333333 : 1.0);
    api('POST', '/api/viewport/3d/fly/move', { x: m[0], y: m[1], z: m[2], speed: speed }).then(apply3DState);
  }
  function begin3DLeft(e) {              // left button: select / box-select / gizmo
    if (vp3dRuler) { beginRuler(e); return; }   // ruler mode measures instead
    // Emulate-3-button (Godot's trackpad / no-middle-mouse mode): Alt+left-drag
    // orbits and Alt+Shift+left-drag pans, so the camera is navigable on a laptop
    // trackpad or a mouse without a usable middle button. Routed through the same
    // 'camera' path as the middle-mouse drag.
    if (e.altKey) {
      nav3D.active = true; nav3D.kind = 'camera';
      nav3D.mode = e.shiftKey ? 'pan' : 'orbit';
      nav3D.lastX = e.clientX; nav3D.lastY = e.clientY; nav3D.moved = false;
      nav3D.gizmo = false; nav3D.gizmoPending = false;
      vp3dSetSmooth(true);
      return;
    }
    if (vp3dListSelect) {                        // list-select: a click opens a node picker
      nav3D.active = true; nav3D.kind = 'listselect';
      nav3D.startX = e.clientX; nav3D.startY = e.clientY; nav3D.moved = false;
      nav3D.lastX = e.clientX; nav3D.lastY = e.clientY;
      return;
    }
    nav3D.active = true; nav3D.kind = 'pending-left';
    nav3D.lastX = e.clientX; nav3D.lastY = e.clientY;
    nav3D.startX = e.clientX; nav3D.startY = e.clientY; nav3D.moved = false;
    nav3D.gizmo = false; nav3D.gizmoPending = false;
    nav3D.add = !!(e.shiftKey || e.ctrlKey || e.metaKey);
    // With a node selected and a transform tool active, a left press may grab a
    // gizmo axis to move/rotate/scale instead of starting a box-select. Ask the
    // server (async); drag moves are swallowed until it answers.
    if (selectedNodeId && currentToolMode !== 'select' && viewportImg) {
      nav3D.gizmoPending = true;
      var body = viewportCoords(e);
      body.snap = !!(e.ctrlKey || e.metaKey);   // Ctrl/Cmd snaps, like Godot
      api('POST', '/api/viewport/3d/gizmo/begin', body).then(function(r) {
        nav3D.gizmoPending = false;
        nav3D.gizmo = !!(r && r.dragging);
        if (nav3D.gizmo) nav3D.kind = 'gizmo';
      });
    }
  }
  // Coalesced camera-nav requests. A fast drag fires a mouse-move ~60×/s; sending
  // one orbit/pan/look POST per move floods the browser's connection pool, which
  // starves the parallel viewport-PNG fetch and makes it fail intermittently —
  // surfacing the "Loading viewport..." placeholder mid-spin. Instead we keep at
  // most ONE nav request in flight and accumulate deltas while it is pending, then
  // flush the merged delta when it returns. The camera moves by the same total
  // amount (deltas are additive) but with a fraction of the requests, so spinning
  // stays smooth and the PNG fetch never gets crowded out.
  var _navAccum = { dx: 0, dy: 0, ep: null };
  var _navInFlight = false;
  function sendNavDelta(ep, dx, dy) {
    _navAccum.dx += dx; _navAccum.dy += dy; _navAccum.ep = ep;
    if (!_navInFlight) flushNavDelta();
  }
  function flushNavDelta() {
    if (!_navAccum.ep || (_navAccum.dx === 0 && _navAccum.dy === 0)) { _navInFlight = false; return; }
    var ep = _navAccum.ep, dx = _navAccum.dx, dy = _navAccum.dy;
    _navAccum.dx = 0; _navAccum.dy = 0;
    _navInFlight = true;
    api('POST', ep, { dx: dx, dy: dy }).then(
      function(s) { apply3DState(s); flushNavDelta(); },   // send any delta that piled up
      function() { _navInFlight = false; }                 // on failure, let the next move retry
    );
  }

  function update3DNav(e) {
    if (!nav3D.active) return;
    var dx = e.clientX - nav3D.lastX, dy = e.clientY - nav3D.lastY;
    if (dx !== 0 || dy !== 0) nav3D.moved = true;
    nav3D.lastX = e.clientX; nav3D.lastY = e.clientY;
    if (nav3D.kind === 'camera') {              // orbit / pan
      if (dx === 0 && dy === 0) return;
      var ep = nav3D.mode === 'pan' ? '/api/viewport/3d/pan' : '/api/viewport/3d/orbit';
      sendNavDelta(ep, dx, dy);
      return;
    }
    if (nav3D.kind === 'fly') {                  // free-look: turn in place
      if (dx === 0 && dy === 0) return;
      sendNavDelta('/api/viewport/3d/fly/look', dx, dy);
      return;
    }
    if (nav3D.gizmoPending) return;             // awaiting gizmo hit-test result
    if (nav3D.kind === 'gizmo' || nav3D.gizmo) {
      api('POST', '/api/viewport/3d/gizmo/update', viewportCoords(e));
      return;
    }
    // Left drag on empty space → rubber-band box select.
    if (nav3D.kind === 'pending-left' && nav3D.moved) { nav3D.kind = 'boxselect'; show3DBox(); }
    if (nav3D.kind === 'boxselect') update3DBox(e);
  }
  function end3DNav(e) {
    if (!nav3D.active) return;
    var kind = nav3D.kind, wasGizmo = nav3D.gizmo || nav3D.gizmoPending,
        moved = nav3D.moved, add = nav3D.add;
    nav3D.active = false; nav3D.kind = 'idle'; nav3D.gizmo = false; nav3D.gizmoPending = false;
    vp3dSetSmooth(false);   // camera stopped → back to crisp full-res display
    if (kind === 'ruler') { endRuler(e); return; }
    if (kind === 'listselect') { showListSelectPopup(e); return; }
    if (wasGizmo) {
      api('POST', '/api/viewport/3d/gizmo/end', {}).then(function() {
        fetchScene(); if (selectedNodeId) fetchSelected();
      });
      return;
    }
    if (kind === 'fly') {
      document.removeEventListener('keydown', fly3DKeyDown);
      api('POST', '/api/viewport/3d/fly/end', {}).then(apply3DState);
      return;
    }
    if (kind === 'boxselect') { hide3DBox(); do3DBoxSelect(e, add); return; }
    // A left click without dragging selects the Node3D under the cursor (raycast).
    // Shift/Ctrl/Cmd-click adds (toggles) it to the selection (multi-select).
    if (kind === 'pending-left' && !moved && e && viewportImg) {
      var c = viewportCoords(e); c.add = add;
      api('POST', '/api/viewport/3d/select', c).then(function(result) {
        if (result && Array.isArray(result.selected_nodes)) {
          selectedNodeIds = new Set(result.selected_nodes);
        }
        selectedNodeId = (result && result.selected != null) ? result.selected : null;
        refreshTree(); fetchSelected();
      });
    }
  }
  // Rubber-band overlay reuses the #box-select-overlay element, positioned over
  // the viewport image (same convention as the 2D box-select).
  function show3DBox() {
    var el = document.getElementById('box-select-overlay');
    if (el) { el.style.display = 'block'; el.style.width = '0px'; el.style.height = '0px'; }
  }
  function hide3DBox() {
    var el = document.getElementById('box-select-overlay');
    if (el) el.style.display = 'none';
  }
  function update3DBox(e) {
    var el = document.getElementById('box-select-overlay');
    if (!el || !viewportImg) return;
    var r = viewportImg.getBoundingClientRect();
    el.style.left = (Math.min(nav3D.startX, e.clientX) - r.left) + 'px';
    el.style.top = (Math.min(nav3D.startY, e.clientY) - r.top) + 'px';
    el.style.width = Math.abs(e.clientX - nav3D.startX) + 'px';
    el.style.height = Math.abs(e.clientY - nav3D.startY) + 'px';
  }
  function do3DBoxSelect(e, add) {
    if (!viewportImg) return;
    var rect = viewportImg.getBoundingClientRect();
    var sx = viewportImg.naturalWidth / rect.width || 1;
    var sy = viewportImg.naturalHeight / rect.height || 1;
    var body = {
      x1: (nav3D.startX - rect.left) * sx, y1: (nav3D.startY - rect.top) * sy,
      x2: (e.clientX - rect.left) * sx,   y2: (e.clientY - rect.top) * sy,
      add: add
    };
    api('POST', '/api/viewport/3d/box_select', body).then(function(result) {
      if (result && Array.isArray(result.selected_nodes)) {
        selectedNodeIds = new Set(result.selected_nodes);
      }
      selectedNodeId = (result && result.selected != null) ? result.selected : null;
      refreshTree(); fetchSelected();
    });
  }
  function zoom3D(deltaY) {
    api('POST', '/api/viewport/3d/zoom', { delta: deltaY > 0 ? -1.0 : 1.0 }).then(apply3DState);
  }

  // ---- 3D navigation gizmo (top-right axis ball, like Godot) ----
  var NAV_AXES_3D = [
    { key: 'bx', col: '#df5a52', lbl: 'X' },
    { key: 'by', col: '#8ec44a', lbl: 'Y' },
    { key: 'bz', col: '#5a8be0', lbl: 'Z' }
  ];
  function draw3DNavGizmo(ctx, cw) {
    if (!nav3DState || !nav3DState.bx) return;
    var R = 26, cx = cw - R - 18, cy = R + 16;
    // Background disc.
    ctx.beginPath(); ctx.arc(cx, cy, R + 7, 0, Math.PI * 2);
    ctx.fillStyle = 'rgba(18,20,26,0.5)'; ctx.fill();
    // One endpoint per ±axis, painter-sorted so axes toward the camera draw last.
    var pts = [];
    NAV_AXES_3D.forEach(function(a) {
      var d = nav3DState[a.key];
      pts.push({ sx: cx + d[0] * R, sy: cy - d[1] * R, z: d[2], col: a.col, lbl: a.lbl, pos: true });
      pts.push({ sx: cx - d[0] * R, sy: cy + d[1] * R, z: -d[2], col: a.col, lbl: '', pos: false });
    });
    pts.sort(function(p, q) { return q.z - p.z; }); // larger z = farther → first
    ctx.lineCap = 'round';
    pts.forEach(function(p) {
      if (p.pos) {
        ctx.strokeStyle = p.col; ctx.lineWidth = 2;
        ctx.beginPath(); ctx.moveTo(cx, cy); ctx.lineTo(p.sx, p.sy); ctx.stroke();
        ctx.beginPath(); ctx.arc(p.sx, p.sy, 7, 0, Math.PI * 2); ctx.fillStyle = p.col; ctx.fill();
        ctx.fillStyle = '#fff'; ctx.font = 'bold 9px sans-serif';
        ctx.textAlign = 'center'; ctx.textBaseline = 'middle';
        ctx.fillText(p.lbl, p.sx, p.sy + 0.5);
      } else {
        ctx.beginPath(); ctx.arc(p.sx, p.sy, 6, 0, Math.PI * 2);
        ctx.fillStyle = 'rgba(18,20,26,0.65)'; ctx.fill();
        ctx.strokeStyle = p.col; ctx.lineWidth = 1.5; ctx.stroke();
      }
    });
  }

  function setupViewport() {
    var container = document.getElementById('viewport-container');
    viewportImg = document.createElement('img');
    viewportImg.id = 'viewport-img';
    viewportImg.style.display = 'none';
    viewportImg.draggable = false;
    // In 3D mode, suppress the native context menu so right-drag can orbit.
    viewportImg.addEventListener('contextmenu', function(e) { if (editorMode === '3d') e.preventDefault(); });
    viewportImg.addEventListener('mousedown', function(e) {
      e.preventDefault();
      if (editorMode === '3d') {
        // Middle = orbit/pan; Right = Godot free-look; Left = select/box/gizmo.
        if (e.button === 1) begin3DCamera(e);
        else if (e.button === 2) begin3DFly(e);
        else begin3DLeft(e);
        return;
      }
      var c = viewportCoords(e);
      dragStartX = e.clientX; dragStartY = e.clientY;
      isDragging = false;
      // If holding Alt or in select mode clicking empty space, start box select
      api('POST', '/api/viewport/drag_start', c).then(function(result) {
        if (result && !result.dragging && currentToolMode === 'select') {
          isBoxSelecting = true;
          boxSelectStart = { x: e.clientX, y: e.clientY };
          if (!boxSelectOverlay) {
            boxSelectOverlay = document.createElement('div');
            boxSelectOverlay.style.cssText = 'position:fixed;border:1px dashed rgba(100,150,255,0.8);background:rgba(100,150,255,0.15);pointer-events:none;z-index:50;display:none;';
            document.body.appendChild(boxSelectOverlay);
          }
          boxSelectOverlay.style.display = 'block';
          var bsoOverlay = document.getElementById('box-select-overlay');
          if (bsoOverlay) { bsoOverlay.style.display = 'block'; bsoOverlay.style.left = e.clientX - viewportImg.getBoundingClientRect().left + 'px'; bsoOverlay.style.top = e.clientY - viewportImg.getBoundingClientRect().top + 'px'; bsoOverlay.style.width = '0px'; bsoOverlay.style.height = '0px'; }
        }
      });
    });
    document.addEventListener('mousemove', function(e) {
      if (editorMode === '3d') { update3DNav(e); return; }
      if (isBoxSelecting && boxSelectStart) {
        var x = Math.min(boxSelectStart.x, e.clientX);
        var y = Math.min(boxSelectStart.y, e.clientY);
        var w = Math.abs(e.clientX - boxSelectStart.x);
        var h = Math.abs(e.clientY - boxSelectStart.y);
        boxSelectOverlay.style.left = x + 'px'; boxSelectOverlay.style.top = y + 'px';
        boxSelectOverlay.style.width = w + 'px'; boxSelectOverlay.style.height = h + 'px';
        var bsoOverlay = document.getElementById('box-select-overlay');
        if (bsoOverlay) { var r = viewportImg.getBoundingClientRect(); var left = Math.min(boxSelectStart.x, e.clientX) - r.left; var top = Math.min(boxSelectStart.y, e.clientY) - r.top; bsoOverlay.style.left = left + 'px'; bsoOverlay.style.top = top + 'px'; bsoOverlay.style.width = Math.abs(e.clientX - boxSelectStart.x) + 'px'; bsoOverlay.style.height = Math.abs(e.clientY - boxSelectStart.y) + 'px'; }
        return;
      }
      if (dragStartX === 0 && dragStartY === 0) return;
      if (!viewportImg) return;
      var dx = e.clientX - dragStartX;
      var dy = e.clientY - dragStartY;
      if (!isDragging && (Math.abs(dx) > DRAG_THRESHOLD || Math.abs(dy) > DRAG_THRESHOLD)) isDragging = true;
      if (isDragging) api('POST', '/api/viewport/drag', viewportCoords(e));
    });
    document.addEventListener('mouseup', function(e) {
      if (editorMode === '3d') { end3DNav(e); return; }
      if (isBoxSelecting && boxSelectStart) {
        isBoxSelecting = false;
        if (boxSelectOverlay) boxSelectOverlay.style.display = 'none';
        var bsoOverlay = document.getElementById('box-select-overlay');
        if (bsoOverlay) bsoOverlay.style.display = 'none';
        var rect = viewportImg.getBoundingClientRect();
        var scaleX = viewportImg.naturalWidth / rect.width;
        var scaleY = viewportImg.naturalHeight / rect.height;
        var x1 = (Math.min(boxSelectStart.x, e.clientX) - rect.left) * scaleX;
        var y1 = (Math.min(boxSelectStart.y, e.clientY) - rect.top) * scaleY;
        var x2 = (Math.max(boxSelectStart.x, e.clientX) - rect.left) * scaleX;
        var y2 = (Math.max(boxSelectStart.y, e.clientY) - rect.top) * scaleY;
        api('POST', '/api/viewport/box_select', { x1: x1, y1: y1, x2: x2, y2: y2, add: e.shiftKey }).then(function(result) {
          if (result && result.selected_nodes) {
            selectedNodeIds = new Set(result.selected_nodes);
            selectedNodeId = result.selected_nodes.length > 0 ? result.selected_nodes[0] : null;
          }
          refreshTree(); fetchSelected();
        });
        boxSelectStart = null; dragStartX = 0; dragStartY = 0;
        return;
      }
      if (dragStartX === 0 && dragStartY === 0) return;
      if (!viewportImg) return;
      var c = viewportCoords(e);
      if (isDragging) {
        api('POST', '/api/viewport/drag_end', c).then(function() {
          fetchScene(); if (selectedNodeId) fetchSelected();
        });
      } else {
        api('POST', '/api/viewport/click', c).then(function(result) {
          if (result && result.selected) { selectedNodeId = result.selected; selectedNodeIds.clear(); selectedNodeIds.add(result.selected); }
          else { selectedNodeId = null; selectedNodeIds.clear(); }
          refreshTree(); fetchSelected(); fetchScene();
        });
      }
      isDragging = false; dragStartX = 0; dragStartY = 0;
    });
    container.appendChild(viewportImg);

    // Asset drag-drop onto viewport: instance scenes, create Sprite2D from textures
    container.addEventListener('dragover', function(e) {
      e.preventDefault();
      e.dataTransfer.dropEffect = 'copy';
      container.classList.add('viewport-drop-active');
    });
    container.addEventListener('dragleave', function(e) {
      container.classList.remove('viewport-drop-active');
    });
    container.addEventListener('drop', function(e) {
      e.preventDefault();
      container.classList.remove('viewport-drop-active');
      var raw = e.dataTransfer.getData('text/plain');
      if (!raw) return;
      var dragData;
      try { dragData = JSON.parse(raw); } catch(ex) { return; }
      if (!dragData || !dragData.type || !dragData.path) return;
      // Compute world position from drop pixel coords
      var rect = viewportImg.getBoundingClientRect();
      var scaleX = viewportImg.naturalWidth / rect.width;
      var scaleY = viewportImg.naturalHeight / rect.height;
      var pixelX = Math.round((e.clientX - rect.left) * scaleX);
      var pixelY = Math.round((e.clientY - rect.top) * scaleY);
      var parentId = selectedNodeId || (sceneData && sceneData.nodes ? sceneData.nodes.id : null);
      if (!parentId) { logMessage('warn', 'No parent node for drop'); return; }

      if (dragData.type === 'tscn_instance') {
        // Instance scene at drop position
        api('POST', '/api/viewport/drop', {
          asset_type: 'scene', path: dragData.path.replace('res://', ''),
          parent_id: parentId, pixel_x: pixelX, pixel_y: pixelY
        }).then(function(result) {
          if (result && result.id) { selectedNodeId = result.id; logMessage('info', 'Instanced: ' + (dragData.name || dragData.path)); }
          fetchScene(); if (selectedNodeId) fetchSelected();
        });
      } else if (dragData.type === 'texture') {
        // Create Sprite2D with texture at drop position
        api('POST', '/api/viewport/drop', {
          asset_type: 'texture', path: dragData.path.replace('res://', ''),
          parent_id: parentId, pixel_x: pixelX, pixel_y: pixelY
        }).then(function(result) {
          if (result && result.id) { selectedNodeId = result.id; logMessage('info', 'Created Sprite2D: ' + (dragData.name || dragData.path)); }
          fetchScene(); if (selectedNodeId) fetchSelected();
        });
      } else if (dragData.type === 'audio') {
        // Create AudioStreamPlayer at drop position
        api('POST', '/api/viewport/drop', {
          asset_type: 'audio', path: dragData.path.replace('res://', ''),
          parent_id: parentId, pixel_x: pixelX, pixel_y: pixelY
        }).then(function(result) {
          if (result && result.id) { selectedNodeId = result.id; logMessage('info', 'Created AudioStreamPlayer: ' + (dragData.name || dragData.path)); }
          fetchScene(); if (selectedNodeId) fetchSelected();
        });
      } else {
        logMessage('info', 'Unsupported drop type for viewport: ' + dragData.type);
      }
    });

    // Zoom with mouse wheel
    container.addEventListener('wheel', function(e) {
      e.preventDefault();
      if (editorMode === '3d') {
        // Godot: the wheel scales free-look flight speed while the right mouse
        // button is held (freelook active), instead of dollying the camera.
        if (nav3D.kind === 'fly') {
          var f = e.deltaY > 0 ? (1 / 1.08) : 1.08;   // ZOOM_FREELOOK_MULTIPLIER
          api('POST', '/api/viewport/3d/fly/speed', { factor: f }).then(function(s) {
            apply3DState(s);
            if (s && typeof s.fly_speed === 'number') logMessage('info', 'Freelook speed: ' + s.fly_speed.toFixed(2) + ' m/s');
          });
          return;
        }
        zoom3D(e.deltaY); return;
      }
      var delta = e.deltaY > 0 ? -0.1 : 0.1;
      viewportZoom = Math.max(0.1, Math.min(16.0, viewportZoom + delta * viewportZoom));
      api('POST', '/api/viewport/zoom', { zoom: viewportZoom });
      updateZoomIndicator();
      renderViewportOverlay();
    }, { passive: false });

    // Pan with middle-mouse or Shift+left drag
    container.addEventListener('mousedown', function(e) {
      if (editorMode === '3d') return; // 3D nav is handled on viewportImg (begin3DCamera/begin3DLeft)
      if (e.button === 1 || (e.button === 0 && e.shiftKey)) {
        e.preventDefault();
        isPanning = true;
        panStartX = e.clientX; panStartY = e.clientY;
        panStartPanX = viewportPanX; panStartPanY = viewportPanY;
      }
    });
    document.addEventListener('mousemove', function(e) {
      if (!isPanning) return;
      viewportPanX = panStartPanX + (e.clientX - panStartX);
      viewportPanY = panStartPanY + (e.clientY - panStartY);
      api('POST', '/api/viewport/pan', { x: viewportPanX, y: viewportPanY });
      renderViewportOverlay();
    });
    document.addEventListener('mouseup', function(e) {
      if (isPanning) { isPanning = false; }
    });

    // Zoom indicator
    var zoomIndicator = document.createElement('div');
    zoomIndicator.id = 'zoom-indicator';
    zoomIndicator.style.cssText = 'position:absolute;bottom:8px;right:8px;background:var(--panel);border:1px solid var(--border);padding:2px 8px;font-size:11px;color:var(--text-dim);border-radius:3px;display:flex;gap:4px;align-items:center;z-index:10;';
    zoomIndicator.innerHTML = '<button id="zoom-out" style="background:none;border:none;color:var(--text);cursor:pointer;padding:0 2px;font-size:13px">-</button><span id="zoom-label">100%</span><button id="zoom-in" style="background:none;border:none;color:var(--text);cursor:pointer;padding:0 2px;font-size:13px">+</button><button id="zoom-reset" style="background:none;border:none;color:var(--text-dim);cursor:pointer;padding:0 4px;font-size:10px">Reset</button>';
    container.style.position = 'relative';
    container.appendChild(zoomIndicator);

    document.getElementById('zoom-in').addEventListener('click', function() {
      viewportZoom = Math.min(16.0, viewportZoom * 1.25);
      api('POST', '/api/viewport/zoom', { zoom: viewportZoom });
      updateZoomIndicator();
      renderViewportOverlay();
    });
    document.getElementById('zoom-out').addEventListener('click', function() {
      viewportZoom = Math.max(0.1, viewportZoom / 1.25);
      api('POST', '/api/viewport/zoom', { zoom: viewportZoom });
      updateZoomIndicator();
      renderViewportOverlay();
    });
    document.getElementById('zoom-reset').addEventListener('click', function() {
      viewportZoom = 1.0; viewportPanX = 0; viewportPanY = 0;
      api('POST', '/api/viewport/zoom', { zoom: 1.0 });
      api('POST', '/api/viewport/pan', { x: 0, y: 0 });
      updateZoomIndicator();
      renderViewportOverlay();
    });

    // Fetch initial zoom/pan
    api('GET', '/api/viewport/zoom_pan').then(function(data) {
      if (data) {
        viewportZoom = data.zoom || 1.0;
        viewportPanX = data.pan_x || 0;
        viewportPanY = data.pan_y || 0;
        updateZoomIndicator();
      }
    });

    // Cursor position in status bar
    var cursorEl = document.getElementById('status-cursor');
    var cursorPosEl = document.getElementById('status-cursor-pos');
    container.addEventListener('mousemove', function(e) {
      if (!viewportImg || viewportImg.style.display === 'none') return;
      var rect = viewportImg.getBoundingClientRect();
      if (e.clientX < rect.left || e.clientX > rect.right || e.clientY < rect.top || e.clientY > rect.bottom) {
        cursorEl.style.display = 'none'; return;
      }
      var scaleX = viewportImg.naturalWidth / rect.width;
      var scaleY = viewportImg.naturalHeight / rect.height;
      var px = Math.round((e.clientX - rect.left) * scaleX);
      var py = Math.round((e.clientY - rect.top) * scaleY);
      // Convert pixel coords to world coords using zoom/pan
      var wx = Math.round((px - viewportPanX) / viewportZoom);
      var wy = Math.round((py - viewportPanY) / viewportZoom);
      cursorPosEl.textContent = '(' + wx + ', ' + wy + ')';
      cursorEl.style.display = '';
    });
    container.addEventListener('mouseleave', function() {
      cursorEl.style.display = 'none';
    });
  }

  function updateZoomIndicator() {
    var label = document.getElementById('zoom-label');
    if (label) label.textContent = Math.round(viewportZoom * 100) + '%';
    var szl = document.getElementById('status-zoom');
    if (szl) szl.textContent = Math.round(viewportZoom * 100) + '%';
  }

  var _viewportLoading = false;
  var _viewportEverLoaded = false;
  function refreshViewport() {
    if (!viewportImg) return;
    // pat-vl1mk: single-in-flight. Skip this frame if the previous PNG is still
    // loading so 5Hz viewport polls can't stack up decode work on a slow link.
    if (_viewportLoading) return;
    _viewportLoading = true;
    var img = new Image();
    img.onload = function() {
      _viewportLoading = false;
      _viewportEverLoaded = true;
      viewportImg.src = img.src;
      viewportImg.style.display = 'block';
      var ph = document.getElementById('viewport-placeholder');
      if (ph) ph.style.display = 'none';
      // pat-xwdlg: feed the same frame to the Game view's screen so the running
      // game renders live there too (the runtime renders the game to this PNG).
      var gimg = document.getElementById('game-screen-img');
      if (gimg) gimg.src = img.src;
    };
    img.onerror = function() {
      _viewportLoading = false;
      // Only show the "Loading viewport..." placeholder before the FIRST frame.
      // After a frame has been shown, a transient PNG fetch hiccup (e.g. a busy
      // connection pool mid-spin) must NOT blank the viewport back to "Loading" —
      // keep the last good frame so spinning the camera stays flicker-free.
      if (!_viewportEverLoaded) {
        viewportImg.style.display = 'none';
        var ph = document.getElementById('viewport-placeholder');
        if (ph) ph.style.display = 'block';
      }
    };
    // Stream the frame as BMP (/api/viewport), not PNG: BMP is ~7× cheaper for the
    // server to encode (a memcpy vs a deflate) — the single biggest lever for the
    // interactive framerate — and the browser decodes it with no inflate. It is
    // larger on the wire but that is free over localhost. The server keeps the PNG
    // cache fresh for full-res/idle frames, so nothing else that wants PNG breaks.
    img.src = '/api/viewport?t=' + Date.now();
  }

  // ---- Viewport auto-resize (3D-viewport fill) ----
  // The server renders the editor viewport to a PNG at whatever size it's told;
  // if never told, it falls back to a fixed 640x480 that letterboxes into a
  // centered square. Report the panel's real pixel size so the render fills it
  // edge-to-edge like Godot, and re-report whenever the panel resizes (dock
  // drag, window resize, bottom-panel split, entering 2D/3D).
  var _lastViewportSize = '';
  var _viewportResizeTimer = null;
  function reportViewportSize() {
    var c = document.getElementById('viewport-container');
    if (!c) return;
    var w = Math.round(c.clientWidth);
    var h = Math.round(c.clientHeight);
    if (w < 8 || h < 8) return;             // panel hidden / not laid out yet
    var key = w + 'x' + h;
    if (key === _lastViewportSize) return;  // unchanged — skip the round-trip
    _lastViewportSize = key;
    api('POST', '/api/viewport/resize', { w: w, h: h }).then(function() { refreshViewport(); });
  }
  function scheduleViewportResize() {
    if (_viewportResizeTimer) clearTimeout(_viewportResizeTimer);
    _viewportResizeTimer = setTimeout(reportViewportSize, 120);
  }
  function setupViewportResize() {
    var c = document.getElementById('viewport-container');
    if (c && typeof ResizeObserver !== 'undefined') {
      new ResizeObserver(scheduleViewportResize).observe(c);
    }
    window.addEventListener('resize', scheduleViewportResize);
    reportViewportSize();
  }

  // ---- Tool mode ----
  function setupToolMode() {
    var toolBtns = document.querySelectorAll('.tool-btn');
    for (var i = 0; i < toolBtns.length; i++) {
      toolBtns[i].addEventListener('click', function() {
        setToolMode(this.getAttribute('data-tool'));
      });
    }
  }

  // Map a Patina gizmo mode to the spatial toolbar's representative tool button.
  var VP3D_MODE_TO_TOOL = { select: 'transform', move: 'move', rotate: 'rotate', scale: 'scale' };
  function syncVp3dToolActive() {
    var vts = document.querySelectorAll('.vp3d-tool');
    for (var i = 0; i < vts.length; i++) {
      vts[i].classList.toggle('active', vts[i].getAttribute('data-vptool') === currentVpTool);
    }
  }

  function setToolMode(mode) {
    currentToolMode = mode;
    var toolEl = document.getElementById('status-tool');
    if (toolEl) toolEl.textContent = mode.charAt(0).toUpperCase() + mode.slice(1);
    var btns = document.querySelectorAll('.tool-btn');
    for (var i = 0; i < btns.length; i++) {
      btns[i].classList.toggle('active', btns[i].getAttribute('data-tool') === mode);
    }
    currentVpTool = VP3D_MODE_TO_TOOL[mode] || 'transform';
    syncVp3dToolActive();
    push3DGizmoMode();
  }

  // Spatial toolbar tool buttons: Transform and Select both map to the Patina
  // "select" gizmo mode; mark the exact clicked button active afterwards.
  function setVp3dTool(vptool) {
    var modeMap = { transform: 'select', move: 'move', rotate: 'rotate', scale: 'scale', select: 'select' };
    setToolMode(modeMap[vptool] || 'select');
    currentVpTool = vptool;
    syncVp3dToolActive();
  }

  // ---- 3D spatial editor toolbar (Godot Node3DEditor parity) ----
  var vp3dRuler = false;        // ruler mode: LMB-drag measures a world distance
  var vp3dListSelect = false;   // list-select mode (highlight; select on click)
  var vp3dRulerStart = null;    // {vx,vy} viewport pixel where a ruler drag began

  function setupVp3dToolbar() {
    var tb = document.getElementById('vp3d-toolbar');
    if (!tb) return;
    var tools = tb.querySelectorAll('.vp3d-tool');
    for (var i = 0; i < tools.length; i++) {
      tools[i].addEventListener('click', function() { setVp3dTool(this.getAttribute('data-vptool')); });
    }
    // Boolean view/preview toggles (sun / environment / local_space / snap).
    var flags = tb.querySelectorAll('.vp3d-tb-btn[data-flag]');
    for (var f = 0; f < flags.length; f++) {
      flags[f].addEventListener('click', function() {
        api('POST', '/api/viewport/3d/view_flag', { flag: this.getAttribute('data-flag') }).then(apply3DState);
      });
    }
    vpBind('vp3d-list-select', function() { vp3dListSelect = !vp3dListSelect; vp3dRuler = false; vp3dRulerStart = null; hideRulerLabel(); refreshVp3dModeButtons(); });
    vpBind('vp3d-ruler', function() { vp3dRuler = !vp3dRuler; vp3dListSelect = false; vp3dRulerStart = null; hideRulerLabel(); refreshVp3dModeButtons(); });
    vpBind('vp3d-lock', function() { api('POST', '/api/viewport/3d/lock', { locked: true }).then(afterVp3dEdit); });
    vpBind('vp3d-unlock', function() { api('POST', '/api/viewport/3d/lock', { locked: false }).then(afterVp3dEdit); });
    vpBind('vp3d-group', function() { api('POST', '/api/viewport/3d/group', { grouped: true }).then(afterVp3dEdit); });
    vpBind('vp3d-ungroup', function() { api('POST', '/api/viewport/3d/group', { grouped: false }).then(afterVp3dEdit); });
    vpBind('vp3d-sun-environ-settings', function() { openSunEnvironPopup(this); });
    setupVp3dMenu('vp3d-transform-menu-btn', 'vp3d-transform-dropdown');
    setupVp3dMenu('vp3d-viewmenu-btn', 'vp3d-viewmenu-dropdown');
    bindVp3dMenuActions('vp3d-transform-dropdown');
    bindVp3dMenuActions('vp3d-viewmenu-dropdown');
  }
  function vpBind(id, fn) { var el = document.getElementById(id); if (el) el.addEventListener('click', fn); }
  function afterVp3dEdit() { refreshTree(); fetchSelected(); refreshViewport(); }
  function refreshVp3dModeButtons() {
    var ls = document.getElementById('vp3d-list-select'); if (ls) ls.classList.toggle('active', vp3dListSelect);
    var ru = document.getElementById('vp3d-ruler'); if (ru) ru.classList.toggle('active', vp3dRuler);
  }

  // Show/hide + sync the spatial toolbar from the current mode and 3D state.
  function updateVp3dToolbar() {
    var tb = document.getElementById('vp3d-toolbar');
    if (!tb) return;
    tb.style.display = (editorMode === '3d') ? 'flex' : 'none';
    var s = nav3DState || {};
    setVp3dActive('vp3d-sun', s.sun !== false);
    setVp3dActive('vp3d-environ', s.environ !== false);
    setVp3dActive('vp3d-local-space', !!s.local);
    setVp3dActive('vp3d-snap', !!s.snap);
    setVp3dCheck('view-origin', s.origin !== false);
    setVp3dCheck('view-grid', s.grid !== false);
    syncVp3dToolActive();
    refreshVp3dModeButtons();
  }
  function setVp3dActive(id, on) { var el = document.getElementById(id); if (el) el.classList.toggle('active', !!on); }
  function setVp3dCheck(act, on) {
    var c = document.querySelector('.vp3d-mi[data-act="' + act + '"] .vp3d-check');
    if (c) c.innerHTML = on ? '&#10003;' : '';
  }

  function setupVp3dMenu(btnId, ddId) {
    var btn = document.getElementById(btnId), dd = document.getElementById(ddId);
    if (!btn || !dd) return;
    btn.addEventListener('click', function(e) {
      e.stopPropagation();
      var isOpen = dd.style.display === 'block';
      closeAllVp3dDropdowns();
      if (!isOpen) { dd.style.display = 'block'; btn.classList.add('open'); }
    });
  }
  function closeAllVp3dDropdowns() {
    var all = document.querySelectorAll('.vp3d-tb-dropdown');
    for (var i = 0; i < all.length; i++) all[i].style.display = 'none';
    var mb = document.querySelectorAll('.vp3d-tb-menubtn.open');
    for (var j = 0; j < mb.length; j++) mb[j].classList.remove('open');
  }
  document.addEventListener('click', closeAllVp3dDropdowns);

  function bindVp3dMenuActions(ddId) {
    var dd = document.getElementById(ddId);
    if (!dd) return;
    dd.addEventListener('click', function(e) {
      var mi = e.target.closest ? e.target.closest('.vp3d-mi') : null;
      if (!mi || mi.classList.contains('vp3d-disabled')) return;
      e.stopPropagation();
      closeAllVp3dDropdowns();
      handleVp3dMenuAction(mi.getAttribute('data-act'));
    });
  }
  function handleVp3dMenuAction(act) {
    if (act === 'snap-floor') { api('POST', '/api/viewport/3d/snap_to_floor', {}).then(afterVp3dEdit); }
    else if (act === 'configure-snap') { openConfigureSnapPopup(); }
    else if (act === 'transform-dialog') { openTransformDialog(); }
    else if (act === 'view-origin') { api('POST', '/api/viewport/3d/view_flag', { flag: 'origin' }).then(apply3DState); }
    else if (act === 'view-grid') { api('POST', '/api/viewport/3d/view_flag', { flag: 'grid' }).then(apply3DState); }
    else if (act === 'view-settings') { openViewSettingsPopup(); }
    // 'view-1' is a no-op: Patina is single-viewport (already checked).
  }

  // ---- Toolbar popups (Configure Snap / Transform Dialog / Settings) ----
  var _vp3dPopup = null;
  function closeVp3dPopup() { if (_vp3dPopup) { _vp3dPopup.remove(); _vp3dPopup = null; } }
  function openVp3dPopup(title, bodyHtml, onApply, anchor) {
    closeVp3dPopup();
    var pop = document.createElement('div');
    pop.className = 'vp3d-popup';
    pop.innerHTML = '<h4>' + title + '</h4>' + bodyHtml +
      '<div class="vp3d-popup-actions"><button data-act="cancel">Cancel</button><button class="primary" data-act="ok">OK</button></div>';
    document.body.appendChild(pop);
    var a = anchor || document.getElementById('vp3d-toolbar');
    var r = a ? a.getBoundingClientRect() : { left: 80, bottom: 80 };
    pop.style.left = Math.max(8, Math.min(r.left, window.innerWidth - pop.offsetWidth - 12)) + 'px';
    pop.style.top = (r.bottom + 4) + 'px';
    pop.addEventListener('click', function(e) { e.stopPropagation(); });
    pop.querySelector('[data-act="cancel"]').addEventListener('click', closeVp3dPopup);
    pop.querySelector('[data-act="ok"]').addEventListener('click', function() { if (onApply) onApply(pop); closeVp3dPopup(); });
    setTimeout(function() { document.addEventListener('click', closeVp3dPopup, { once: true }); }, 0);
    return pop;
  }
  function openConfigureSnapPopup() {
    var s = nav3DState || {};
    var t = (s.snap_t != null ? s.snap_t : 1), r = (s.snap_r != null ? s.snap_r : 15), sc = (s.snap_s != null ? s.snap_s : 0.1);
    openVp3dPopup('Configure Snap',
      '<label>Translate <input type="number" id="vp3d-cs-t" step="0.1" value="' + t + '"></label>' +
      '<label>Rotate (deg) <input type="number" id="vp3d-cs-r" step="1" value="' + r + '"></label>' +
      '<label>Scale <input type="number" id="vp3d-cs-s" step="0.01" value="' + sc + '"></label>',
      function() {
        api('POST', '/api/viewport/3d/snap_config', {
          translate: parseFloat(document.getElementById('vp3d-cs-t').value) || 0,
          rotate: parseFloat(document.getElementById('vp3d-cs-r').value) || 0,
          scale: parseFloat(document.getElementById('vp3d-cs-s').value) || 0
        }).then(apply3DState);
      });
  }
  function openTransformDialog() {
    openVp3dPopup('Transform Dialog (translate selection)',
      '<label>Translate X <input type="number" id="vp3d-td-x" step="0.1" value="0"></label>' +
      '<label>Translate Y <input type="number" id="vp3d-td-y" step="0.1" value="0"></label>' +
      '<label>Translate Z <input type="number" id="vp3d-td-z" step="0.1" value="0"></label>',
      function() {
        api('POST', '/api/viewport/3d/translate_selection', {
          dx: parseFloat(document.getElementById('vp3d-td-x').value) || 0,
          dy: parseFloat(document.getElementById('vp3d-td-y').value) || 0,
          dz: parseFloat(document.getElementById('vp3d-td-z').value) || 0
        }).then(afterVp3dEdit);
      });
  }
  function openViewSettingsPopup() {
    var s = nav3DState || {};
    var dist = (s.distance != null) ? Number(s.distance).toFixed(2) : '?';
    openVp3dPopup('View Settings',
      '<label>Projection <span>' + (s.ortho ? 'Orthogonal' : 'Perspective') + '</span></label>' +
      '<label>Camera distance <span>' + dist + '</span></label>' +
      '<div style="margin-top:6px"><button id="vp3d-reset-cam">Reset Camera</button></div>', null);
    var rb = document.getElementById('vp3d-reset-cam');
    if (rb) rb.addEventListener('click', function() { api('POST', '/api/viewport/3d/reset', {}).then(function(st) { apply3DState(st); closeVp3dPopup(); }); });
  }
  function openSunEnvironPopup(anchor) {
    openVp3dPopup('Sun and Environment',
      '<label>Sun energy <input type="number" id="vp3d-se-e" step="0.1" min="0" value="1"></label>' +
      '<label>Sun dir X <input type="number" id="vp3d-se-x" step="0.1" value="-0.5"></label>' +
      '<label>Sun dir Y <input type="number" id="vp3d-se-y" step="0.1" value="-0.71"></label>' +
      '<label>Sun dir Z <input type="number" id="vp3d-se-z" step="0.1" value="-0.5"></label>',
      function() {
        api('POST', '/api/viewport/3d/sun_config', {
          energy: parseFloat(document.getElementById('vp3d-se-e').value) || 0,
          dx: parseFloat(document.getElementById('vp3d-se-x').value),
          dy: parseFloat(document.getElementById('vp3d-se-y').value),
          dz: parseFloat(document.getElementById('vp3d-se-z').value)
        }).then(apply3DState);
      }, anchor);
  }

  // ---- Ruler tool: LMB-drag measures a world-space distance on the ground ----
  function beginRuler(e) {
    nav3D.active = true; nav3D.kind = 'ruler';
    nav3D.lastX = e.clientX; nav3D.lastY = e.clientY;
    var c = viewportCoords(e);
    vp3dRulerStart = { vx: c.x, vy: c.y };
  }
  function endRuler(e) {
    if (!vp3dRulerStart) return;
    var c = viewportCoords(e), a = vp3dRulerStart;
    vp3dRulerStart = null;
    Promise.all([
      api('POST', '/api/viewport/3d/ground_point', { x: a.vx, y: a.vy }),
      api('POST', '/api/viewport/3d/ground_point', { x: c.x, y: c.y })
    ]).then(function(res) {
      var p1 = res[0], p2 = res[1];
      if (p1 && p1.hit && p2 && p2.hit) {
        var dx = p2.x - p1.x, dy = p2.y - p1.y, dz = p2.z - p1.z;
        showRulerLabel(e.clientX, e.clientY, Math.sqrt(dx * dx + dy * dy + dz * dz).toFixed(3) + ' m');
      } else {
        showRulerLabel(e.clientX, e.clientY, '(off ground)');
      }
    });
  }
  function showRulerLabel(x, y, text) {
    var el = document.getElementById('vp3d-ruler-label');
    if (!el) {
      el = document.createElement('div'); el.id = 'vp3d-ruler-label';
      el.style.cssText = 'position:fixed;z-index:300;background:rgba(20,22,28,0.92);color:#fff;border:1px solid #5b8def;border-radius:3px;padding:2px 8px;font-size:12px;pointer-events:none;';
      document.body.appendChild(el);
    }
    el.textContent = text;
    el.style.left = (x + 12) + 'px'; el.style.top = (y + 8) + 'px'; el.style.display = 'block';
  }
  function hideRulerLabel() { var el = document.getElementById('vp3d-ruler-label'); if (el) el.style.display = 'none'; }

  // List-select (Godot "Show list of selectable nodes at position clicked"):
  // present the scene's nodes as a popup at the cursor so the user can pick one
  // to select — useful when nodes overlap in the viewport.
  function showListSelectPopup(e) {
    var rows = document.querySelectorAll('[data-node-id]');
    var seen = {}, items = [];
    for (var i = 0; i < rows.length; i++) {
      var id = rows[i].getAttribute('data-node-id');
      if (id == null || seen[id]) continue;
      seen[id] = 1;
      var nm = (rows[i].textContent || ('Node ' + id)).replace(/\s+/g, ' ').trim();
      items.push({ id: id, name: nm || ('Node ' + id) });
    }
    closeVp3dPopup();
    var pop = document.createElement('div');
    pop.className = 'vp3d-popup';
    pop.style.maxHeight = '280px'; pop.style.overflowY = 'auto'; pop.style.minWidth = '160px';
    var html = '<h4>Select node</h4>';
    if (!items.length) html += '<div style="color:var(--text-dim)">(no nodes)</div>';
    for (var k = 0; k < items.length; k++) html += '<div class="vp3d-mi" data-pick="' + items[k].id + '">' + items[k].name + '</div>';
    pop.innerHTML = html;
    document.body.appendChild(pop);
    pop.style.left = Math.max(8, Math.min(e.clientX, window.innerWidth - pop.offsetWidth - 12)) + 'px';
    pop.style.top = (e.clientY + 6) + 'px';
    pop.addEventListener('click', function(ev) {
      ev.stopPropagation();
      var mi = ev.target.closest ? ev.target.closest('.vp3d-mi') : null;
      if (!mi) return;
      api('POST', '/api/node/select', { node_id: parseInt(mi.getAttribute('data-pick')) }).then(function() { refreshTree(); fetchSelected(); });
      closeVp3dPopup();
    });
    _vp3dPopup = pop;
    setTimeout(function() { document.addEventListener('click', closeVp3dPopup, { once: true }); }, 0);
  }

  // ---- Shader Editor (pat-bc0tv) — Godot ShaderEditorPlugin parity ----
  // Multi-shader model: shader_tabs (TabContainer) + shader_list (ItemList).
  var shaderTabs = [];          // {path, source, saved, cursorPos, bookmarks:[], valid, errors:[], warnings:[]}
  var activeShaderIdx = -1;
  var shaderValidateTimer = null;
  var shaderWordWrap = false;

  function shActiveTab() { return activeShaderIdx >= 0 ? shaderTabs[activeShaderIdx] : null; }

  // GDShader token tables (mirror ShaderHighlightKind in shader_editor.rs).
  var SH_KW = {'shader_type':1,'render_mode':1,'uniform':1,'varying':1,'const':1,'struct':1,'void':1,'if':1,'else':1,'for':1,'while':1,'do':1,'switch':1,'case':1,'default':1,'break':1,'continue':1,'return':1,'discard':1,'instance':1,'global':1,'group_uniforms':1};
  var SH_QUAL = {'in':1,'out':1,'inout':1,'flat':1,'smooth':1,'lowp':1,'mediump':1,'highp':1};
  var SH_TYPE = {'bool':1,'int':1,'uint':1,'float':1,'vec2':1,'vec3':1,'vec4':1,'ivec2':1,'ivec3':1,'ivec4':1,'uvec2':1,'uvec3':1,'uvec4':1,'bvec2':1,'bvec3':1,'bvec4':1,'mat2':1,'mat3':1,'mat4':1,'sampler2D':1,'isampler2D':1,'usampler2D':1,'sampler2DArray':1,'sampler3D':1,'samplerCube':1,'samplerCubeArray':1,'samplerExternalOES':1};
  var SH_BOOL = {'true':1,'false':1};
  var SH_BUILTIN = {};
  ['texture','textureLod','textureProj','textureSize','texelFetch','mix','clamp','step','smoothstep','fract','floor','ceil','round','abs','sign','mod','min','max','pow','exp','exp2','log','log2','sqrt','inversesqrt','sin','cos','tan','asin','acos','atan','dot','cross','length','distance','normalize','reflect','refract','transpose','inverse','determinant','radians','degrees','dFdx','dFdy','fwidth',
   'COLOR','VERTEX','UV','UV2','NORMAL','TANGENT','BINORMAL','FRAGCOORD','TIME','PI','TAU','E','ALBEDO','ALPHA','METALLIC','ROUGHNESS','SPECULAR','EMISSION','NORMAL_MAP','AO','RIM','TEXTURE','NORMAL_TEXTURE','TEXTURE_PIXEL_SIZE','SCREEN_UV','SCREEN_TEXTURE','DEPTH_TEXTURE','MODEL_MATRIX','VIEW_MATRIX','PROJECTION_MATRIX','INV_VIEW_MATRIX','POINT_COORD','POINT_SIZE','LIGHT','LIGHT_COLOR','ATTENUATION','DIFFUSE_LIGHT','SPECULAR_LIGHT'
  ].forEach(function(k){ SH_BUILTIN[k]=1; });

  // Tokenize + colorize GDShader source into HTML spans.
  function shHighlight(src) {
    var re = /(\/\/[^\n]*|\/\*[\s\S]*?\*\/)|([A-Za-z_][A-Za-z0-9_]*)|(\.\d[\d.]*|\d[\d.]*(?:[eE][+-]?\d+)?[uUfF]?)|(\s+)|([^\sA-Za-z0-9_])/g;
    var out = '', m, last = 0;
    while ((m = re.exec(src)) !== null) {
      if (m.index > last) out += escapeHtml(src.slice(last, m.index));
      last = re.lastIndex;
      if (m[1] !== undefined) { out += '<span class="sh-comment">' + escapeHtml(m[1]) + '</span>'; }
      else if (m[2] !== undefined) {
        var w = m[2], cls = null;
        if (SH_KW[w]) cls = 'sh-keyword'; else if (SH_QUAL[w]) cls = 'sh-qualifier';
        else if (SH_TYPE[w]) cls = 'sh-type'; else if (SH_BOOL[w]) cls = 'sh-bool';
        else if (SH_BUILTIN[w]) cls = 'sh-builtin';
        out += cls ? '<span class="' + cls + '">' + escapeHtml(w) + '</span>' : escapeHtml(w);
      }
      else if (m[3] !== undefined) { out += '<span class="sh-number">' + escapeHtml(m[3]) + '</span>'; }
      else if (m[4] !== undefined) { out += escapeHtml(m[4]); }
      else { out += '<span class="sh-op">' + escapeHtml(m[5]) + '</span>'; }
    }
    if (last < src.length) out += escapeHtml(src.slice(last));
    if (src.length === 0 || src.charAt(src.length - 1) === '\n') out += ' ';
    return out;
  }

  function shLineCount(src) { return src.length ? src.split('\n').length : 1; }

  function renderShaderGutter() {
    var g = document.getElementById('shader-gutter');
    var tab = shActiveTab();
    if (!g) return;
    if (!tab) { g.innerHTML = ''; return; }
    var n = shLineCount(tab.source), errLine = (tab.errors && tab.errors[0]) ? tab.errors[0].line : 0;
    var marks = {}; (tab.bookmarks || []).forEach(function(l){ marks[l] = 1; });
    var html = '';
    for (var i = 1; i <= n; i++) {
      var c = 'sg-ln'; if (marks[i]) c += ' bookmarked'; if (i === errLine) c += ' errline';
      html += '<span class="' + c + '">' + i + '</span>' + (i < n ? '\n' : '');
    }
    g.innerHTML = html;
  }

  function renderShaderHighlight() {
    var pre = document.getElementById('shader-highlight');
    var tab = shActiveTab();
    if (pre) pre.innerHTML = tab ? shHighlight(tab.source) : '';
  }

  function syncShaderScroll() {
    var ta = document.getElementById('shader-source');
    var pre = document.getElementById('shader-highlight');
    var g = document.getElementById('shader-gutter');
    if (!ta) return;
    if (pre) { pre.scrollTop = ta.scrollTop; pre.scrollLeft = ta.scrollLeft; }
    if (g) g.scrollTop = ta.scrollTop;
  }

  function updateShaderStatus() {
    var ta = document.getElementById('shader-source');
    var lc = document.getElementById('shader-status-linecol');
    var errEl = document.getElementById('shader-status-err');
    var tab = shActiveTab();
    if (ta && lc) {
      var upto = ta.value.slice(0, ta.selectionStart);
      var line = upto.split('\n').length;
      var col = ta.selectionStart - (upto.lastIndexOf('\n') + 1) + 1;
      lc.textContent = 'Line ' + line + ', Col ' + col;
    }
    if (errEl) {
      if (!tab) { errEl.textContent = ''; errEl.className = 'shader-status-err'; }
      else if (tab.errors && tab.errors.length) {
        var e = tab.errors[0];
        errEl.textContent = 'Error: ' + e.message + (e.line ? ' (line ' + e.line + ')' : '');
        errEl.className = 'shader-status-err';
      } else {
        var wc = (tab.warnings || []).length;
        errEl.textContent = wc ? (wc + ' warning' + (wc > 1 ? 's' : '')) : '✓ Compiled';
        errEl.className = 'shader-status-err ok';
      }
    }
  }

  function renderShaderWarnings() {
    var w = document.getElementById('shader-warnings');
    var tab = shActiveTab();
    if (!w) return;
    var rows = [];
    if (tab) {
      (tab.errors || []).forEach(function(e){ rows.push({cls:'error', line:e.line, msg:e.message}); });
      (tab.warnings || []).forEach(function(x){ rows.push({cls:'warn', line:x.line, msg:x.message}); });
    }
    if (!rows.length) { w.classList.remove('open'); w.innerHTML = ''; return; }
    var html = '';
    rows.forEach(function(r){
      var label = r.line ? ('Line ' + r.line + ':') : '';
      html += '<div class="shader-warn-row ' + (r.cls === 'error' ? 'error' : '') + '">' +
              (label ? '<span class="sw-line" data-line="' + r.line + '">' + label + '</span>' : '') +
              '<span class="sw-msg">' + escapeHtml(r.msg) + '</span></div>';
    });
    w.innerHTML = html;
    w.classList.add('open');
    Array.prototype.forEach.call(w.querySelectorAll('.sw-line'), function(el){
      el.addEventListener('click', function(){ shaderGotoLine(parseInt(el.getAttribute('data-line'), 10)); });
    });
  }

  function renderShaderFilesList() {
    var list = document.getElementById('shader-files-list');
    if (!list) return;
    if (!shaderTabs.length) { list.innerHTML = '<div class="shader-files-empty">No open shaders</div>'; return; }
    var html = '';
    shaderTabs.forEach(function(t, i){
      var name = t.path.split('/').pop() + (t.saved === t.source ? '' : '(*)');
      var err = (t.valid === false) ? '<span class="sfi-err" title="Shader has errors">&#9888;</span>' : '';
      html += '<div class="shader-file-item' + (i === activeShaderIdx ? ' active' : '') + '" data-idx="' + i + '" title="' + escapeAttr(t.path) + '">' +
              '<span class="sfi-icon">&#9673;</span>' +
              '<span class="sfi-name">' + escapeHtml(name) + '</span>' + err +
              '<span class="sfi-close" data-close="' + i + '" title="Close">&times;</span></div>';
    });
    list.innerHTML = html;
    Array.prototype.forEach.call(list.querySelectorAll('.shader-file-item'), function(el){
      el.addEventListener('click', function(ev){
        if (ev.target && ev.target.getAttribute('data-close') !== null) { closeShaderTab(parseInt(ev.target.getAttribute('data-close'), 10)); return; }
        switchShaderTab(parseInt(el.getAttribute('data-idx'), 10));
      });
      // Middle-click closes (Godot _shader_list_clicked MIDDLE).
      el.addEventListener('auxclick', function(ev){ if (ev.button === 1) { ev.preventDefault(); closeShaderTab(parseInt(el.getAttribute('data-idx'), 10)); } });
    });
  }

  function renderShaderEditor() {
    var ta = document.getElementById('shader-source');
    var area = document.getElementById('shader-code-area');
    var tab = shActiveTab();
    if (area) area.classList.toggle('no-shader', !tab);
    if (ta) {
      if (tab) { if (ta.value !== tab.source) ta.value = tab.source; ta.disabled = false; }
      else { ta.value = ''; ta.disabled = true; }
    }
    // File-specific menu items are disabled with no shader (Godot _set_file_specific_items_disabled).
    ['save','save_as','inspect','inspect_native','close'].forEach(function(a){
      var el = document.querySelector('.shader-mi[data-act="' + a + '"]');
      if (el) el.classList.toggle('disabled', !tab);
    });
    renderShaderHighlight();
    renderShaderGutter();
    renderShaderWarnings();
    updateShaderStatus();
    renderShaderFilesList();
  }

  function openShaderInEditor(path, source) {
    var existing = -1;
    shaderTabs.forEach(function(t, i){ if (t.path === path) existing = i; });
    if (existing >= 0) { switchShaderTab(existing); return existing; }
    shaderTabs.push({ path: path, source: source || '', saved: source || '', bookmarks: [], valid: null, errors: [], warnings: [] });
    activeShaderIdx = shaderTabs.length - 1;
    renderShaderEditor();
    validateActiveShader(true);
    return activeShaderIdx;
  }

  function switchShaderTab(idx) {
    if (idx < 0 || idx >= shaderTabs.length) return;
    activeShaderIdx = idx;
    renderShaderEditor();
    var ta = document.getElementById('shader-source'); if (ta) ta.focus();
  }

  async function closeShaderTab(idx) {
    if (idx < 0 || idx >= shaderTabs.length) return;
    var t = shaderTabs[idx];
    if (t && t.saved !== t.source) {
      var ok = await showConfirm('Close <b>' + escapeHtml(t.path.split('/').pop()) + '</b> without saving?', { title: 'Unsaved Shader', okText: 'Close', danger: true });
      if (!ok) return;
    }
    shaderTabs.splice(idx, 1);
    if (shaderTabs.length === 0) activeShaderIdx = -1;
    else if (activeShaderIdx >= shaderTabs.length) activeShaderIdx = shaderTabs.length - 1;
    else if (activeShaderIdx > idx) activeShaderIdx -= 1;
    renderShaderEditor();
  }

  async function loadShaderFile(path) {
    var data = await api('GET', '/api/shader_source?path=' + encodeURIComponent(path));
    if (!data) return;
    openShaderInEditor(data.path, data.source || '');
    // Auto-open the Shader Editor panel (honors the Pin lock, Godot parity).
    if (window.bottomPanelOpen) { window.bottomPanelOpen('shader', true); }
    else { var tab = document.querySelector('.bottom-tab[data-tab="shader"]'); if (tab) tab.click(); }
    var ta = document.getElementById('shader-source'); if (ta) setTimeout(function(){ ta.focus(); }, 0);
  }

  function validateActiveShader(immediate) {
    if (shaderValidateTimer) { clearTimeout(shaderValidateTimer); shaderValidateTimer = null; }
    var run = function() {
      var tab = shActiveTab();
      if (!tab) return;
      var src = tab.source;
      api('POST', '/api/shader_validate', { source: src }).then(function(r){
        if (!r) return;
        // Ignore stale responses if the user switched tabs.
        var cur = shActiveTab();
        if (!cur || cur.source !== src) { if (cur) { /* revalidate current */ } return; }
        cur.valid = !!r.ok; cur.errors = r.errors || []; cur.warnings = r.warnings || [];
        renderShaderGutter(); renderShaderWarnings(); updateShaderStatus(); renderShaderFilesList();
      });
    };
    if (immediate) run(); else shaderValidateTimer = setTimeout(run, 400);
  }

  function saveActiveShader() {
    var tab = shActiveTab();
    if (!tab) return;
    api('POST', '/api/shader_source', { path: tab.path, source: tab.source }).then(function(r){
      if (r && r.ok) { tab.saved = tab.source; showFsToast('Saved ' + tab.path.split('/').pop()); renderShaderFilesList(); }
    });
  }

  // ---- textarea editing primitives (Godot Edit menu) ----
  function shTA() { return document.getElementById('shader-source'); }
  function shApplyEdit(newValue, selStart, selEnd) {
    var ta = shTA(), tab = shActiveTab();
    if (!ta || !tab) return;
    ta.value = newValue; tab.source = newValue;
    if (selStart !== undefined) ta.setSelectionRange(selStart, selEnd === undefined ? selStart : selEnd);
    renderShaderHighlight(); renderShaderGutter(); updateShaderStatus(); renderShaderFilesList();
    validateActiveShader();
  }
  function shLineBounds(v, pos) {
    var start = v.lastIndexOf('\n', pos - 1) + 1;
    var end = v.indexOf('\n', pos); if (end < 0) end = v.length;
    return [start, end];
  }
  function shSelectedLineRange(ta) {
    var v = ta.value;
    var s = v.lastIndexOf('\n', ta.selectionStart - 1) + 1;
    var e = v.indexOf('\n', ta.selectionEnd); if (e < 0) e = v.length;
    return [s, e];
  }
  function shIndent(unindent) {
    var ta = shTA(); if (!ta) return;
    var v = ta.value, rng = shSelectedLineRange(ta), block = v.slice(rng[0], rng[1]);
    var lines = block.split('\n');
    lines = lines.map(function(ln){
      if (unindent) return ln.replace(/^(\t| {1,4})/, '');
      return '\t' + ln;
    });
    var nb = lines.join('\n');
    shApplyEdit(v.slice(0, rng[0]) + nb + v.slice(rng[1]), rng[0], rng[0] + nb.length);
  }
  function shToggleComment() {
    var ta = shTA(); if (!ta) return;
    var v = ta.value, rng = shSelectedLineRange(ta), block = v.slice(rng[0], rng[1]);
    var lines = block.split('\n');
    var allComment = lines.every(function(ln){ return ln.trim() === '' || /^\s*\/\//.test(ln); });
    lines = lines.map(function(ln){
      if (ln.trim() === '') return ln;
      if (allComment) return ln.replace(/^(\s*)\/\/ ?/, '$1');
      return ln.replace(/^(\s*)/, '$1// ');
    });
    var nb = lines.join('\n');
    shApplyEdit(v.slice(0, rng[0]) + nb + v.slice(rng[1]), rng[0], rng[0] + nb.length);
  }
  function shMoveLines(dir) {
    var ta = shTA(); if (!ta) return;
    var v = ta.value, rng = shSelectedLineRange(ta);
    if (dir < 0) {
      if (rng[0] === 0) return;
      var prevStart = v.lastIndexOf('\n', rng[0] - 2) + 1;
      var prevLine = v.slice(prevStart, rng[0] - 1);
      var block = v.slice(rng[0], rng[1]);
      var nv = v.slice(0, prevStart) + block + '\n' + prevLine + v.slice(rng[1]);
      shApplyEdit(nv, prevStart, prevStart + block.length);
    } else {
      if (rng[1] >= v.length) return;
      var nextEnd = v.indexOf('\n', rng[1] + 1); if (nextEnd < 0) nextEnd = v.length;
      var nextLine = v.slice(rng[1] + 1, nextEnd);
      var block2 = v.slice(rng[0], rng[1]);
      var nv2 = v.slice(0, rng[0]) + nextLine + '\n' + block2 + v.slice(nextEnd);
      var ns = rng[0] + nextLine.length + 1;
      shApplyEdit(nv2, ns, ns + block2.length);
    }
  }
  function shDeleteLine() {
    var ta = shTA(); if (!ta) return;
    var v = ta.value, rng = shSelectedLineRange(ta);
    var end = rng[1] < v.length ? rng[1] + 1 : rng[1];
    var start = rng[0]; if (end === rng[1] && start > 0) start -= 1;
    shApplyEdit(v.slice(0, start) + v.slice(end), start);
  }
  function shDuplicate(wholeLines) {
    var ta = shTA(); if (!ta) return;
    var v = ta.value;
    if (!wholeLines && ta.selectionStart !== ta.selectionEnd) {
      var sel = v.slice(ta.selectionStart, ta.selectionEnd);
      var at = ta.selectionEnd;
      shApplyEdit(v.slice(0, at) + sel + v.slice(at), at, at + sel.length);
    } else {
      var rng = shSelectedLineRange(ta), block = v.slice(rng[0], rng[1]);
      shApplyEdit(v.slice(0, rng[1]) + '\n' + block + v.slice(rng[1]), rng[1] + 1, rng[1] + 1 + block.length);
    }
  }
  function shaderGotoLine(line) {
    var ta = shTA(); if (!ta || !line) return;
    var lines = ta.value.split('\n');
    var pos = 0; for (var i = 0; i < line - 1 && i < lines.length; i++) pos += lines[i].length + 1;
    ta.focus(); ta.setSelectionRange(pos, pos);
    // Center the line vertically.
    var lh = 18; ta.scrollTop = Math.max(0, (line - 1) * lh - ta.clientHeight / 2);
    syncShaderScroll(); updateShaderStatus();
  }
  function shToggleBookmark() {
    var ta = shTA(), tab = shActiveTab(); if (!ta || !tab) return;
    var line = ta.value.slice(0, ta.selectionStart).split('\n').length;
    var i = tab.bookmarks.indexOf(line);
    if (i >= 0) tab.bookmarks.splice(i, 1); else tab.bookmarks.push(line);
    tab.bookmarks.sort(function(a, b){ return a - b; });
    renderShaderGutter();
  }
  function shGotoBookmark(dir) {
    var ta = shTA(), tab = shActiveTab(); if (!ta || !tab || !tab.bookmarks.length) return;
    var line = ta.value.slice(0, ta.selectionStart).split('\n').length, target = null;
    if (dir > 0) { for (var i = 0; i < tab.bookmarks.length; i++) if (tab.bookmarks[i] > line) { target = tab.bookmarks[i]; break; } if (target === null) target = tab.bookmarks[0]; }
    else { for (var j = tab.bookmarks.length - 1; j >= 0; j--) if (tab.bookmarks[j] < line) { target = tab.bookmarks[j]; break; } if (target === null) target = tab.bookmarks[tab.bookmarks.length - 1]; }
    shaderGotoLine(target);
  }

  // ---- Find / Replace bar ----
  function shOpenFind(withReplace) {
    var bar = document.getElementById('shader-find-bar'); if (!bar) return;
    bar.classList.add('open');
    ['shader-replace-input','shader-replace-one','shader-replace-all'].forEach(function(id){
      var el = document.getElementById(id); if (el) el.style.display = withReplace ? '' : 'none';
    });
    var inp = document.getElementById('shader-find-input');
    var ta = shTA();
    if (inp) { if (ta && ta.selectionStart !== ta.selectionEnd) inp.value = ta.value.slice(ta.selectionStart, ta.selectionEnd); inp.focus(); inp.select(); }
  }
  function shCloseFind() { var bar = document.getElementById('shader-find-bar'); if (bar) bar.classList.remove('open'); var ta = shTA(); if (ta) ta.focus(); }
  function shFind(dir) {
    var ta = shTA(), inp = document.getElementById('shader-find-input'); if (!ta || !inp) return;
    var term = inp.value; if (!term) return;
    var v = ta.value, from;
    if (dir >= 0) { from = ta.selectionEnd; var idx = v.indexOf(term, from); if (idx < 0) idx = v.indexOf(term, 0); if (idx >= 0) { ta.focus(); ta.setSelectionRange(idx, idx + term.length); shaderScrollToSel(); } }
    else { from = ta.selectionStart - 1; var idx2 = v.lastIndexOf(term, from < 0 ? v.length : from); if (idx2 < 0) idx2 = v.lastIndexOf(term); if (idx2 >= 0) { ta.focus(); ta.setSelectionRange(idx2, idx2 + term.length); shaderScrollToSel(); } }
    var cnt = document.getElementById('shader-find-count');
    if (cnt) { var n = v.split(term).length - 1; cnt.textContent = n ? (n + ' found') : 'No match'; }
  }
  function shReplaceOne() {
    var ta = shTA(), inp = document.getElementById('shader-find-input'), rep = document.getElementById('shader-replace-input'); if (!ta || !inp) return;
    if (ta.selectionStart !== ta.selectionEnd && ta.value.slice(ta.selectionStart, ta.selectionEnd) === inp.value) {
      var at = ta.selectionStart;
      shApplyEdit(ta.value.slice(0, at) + rep.value + ta.value.slice(ta.selectionEnd), at, at + rep.value.length);
    }
    shFind(1);
  }
  function shReplaceAll() {
    var ta = shTA(), inp = document.getElementById('shader-find-input'), rep = document.getElementById('shader-replace-input'); if (!ta || !inp || !inp.value) return;
    shApplyEdit(ta.value.split(inp.value).join(rep.value), 0);
  }
  function shaderScrollToSel() {
    var ta = shTA(); if (!ta) return;
    var line = ta.value.slice(0, ta.selectionStart).split('\n').length, lh = 18;
    if ((line - 1) * lh < ta.scrollTop || (line - 1) * lh > ta.scrollTop + ta.clientHeight - lh) ta.scrollTop = Math.max(0, (line - 1) * lh - ta.clientHeight / 2);
    syncShaderScroll();
  }

  // Inline prompt dialog (Godot uses inline popups like GotoLinePopup /
  // ShaderCreateDialog, never a blocking native prompt). Returns the entered
  // string, or null if cancelled.
  function shaderPrompt(title, defaultVal, okText) {
    return new Promise(function(resolve){
      var ov = document.createElement('div');
      ov.className = 'shader-prompt-overlay';
      var box = document.createElement('div'); box.className = 'shader-prompt-box';
      var t = document.createElement('div'); t.className = 'shader-prompt-title'; t.textContent = title;
      var inp = document.createElement('input'); inp.type = 'text'; inp.className = 'shader-prompt-input'; inp.spellcheck = false; inp.value = defaultVal || '';
      var btns = document.createElement('div'); btns.className = 'shader-prompt-btns';
      var cancel = document.createElement('button'); cancel.className = 'shader-prompt-cancel'; cancel.textContent = 'Cancel';
      var ok = document.createElement('button'); ok.className = 'shader-prompt-ok'; ok.textContent = okText || 'OK';
      btns.appendChild(cancel); btns.appendChild(ok);
      box.appendChild(t); box.appendChild(inp); box.appendChild(btns); ov.appendChild(box);
      document.body.appendChild(ov);
      inp.focus(); inp.select();
      function done(v){ if (ov.parentNode) ov.parentNode.removeChild(ov); resolve(v); }
      ok.addEventListener('click', function(){ done(inp.value); });
      cancel.addEventListener('click', function(){ done(null); });
      ov.addEventListener('click', function(e){ if (e.target === ov) done(null); });
      inp.addEventListener('keydown', function(e){
        if (e.key === 'Enter') { e.preventDefault(); done(inp.value); }
        else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); done(null); }
      });
    });
  }

  // ---- Menu dispatch (Godot _menu_option / _menu_item_pressed) ----
  function shaderMenuAction(act) {
    var tab = shActiveTab(), ta = shTA();
    switch (act) {
      case 'new': {
        shaderPrompt('New Shader — res:// path', 'res://fixtures/shaders/new_shader.gdshader', 'Create').then(function(name){
          if (!name) return;
          var tmpl = 'shader_type canvas_item;\n\nvoid fragment() {\n\tCOLOR = vec4(1.0);\n}\n';
          api('POST', '/api/shader_source', { path: name, source: tmpl }).then(function(r){ if (r && r.ok) { openShaderInEditor(name, tmpl); showFsToast('Created ' + name.split('/').pop()); if (typeof fetchFileSystem === "function") fetchFileSystem(); } });
        });
      } break;
      case 'new_include': {
        shaderPrompt('New Shader Include — res:// path', 'res://fixtures/shaders/new_include.gdshaderinc', 'Create').then(function(iname){
          if (!iname) return;
          api('POST', '/api/shader_source', { path: iname, source: '// Shader include\n' }).then(function(r){ if (r && r.ok) { openShaderInEditor(iname, '// Shader include\n'); if (typeof fetchFileSystem === "function") fetchFileSystem(); } });
        });
      } break;
      case 'open': case 'open_include': {
        shaderPrompt('Load Shader File — res:// path', 'res://fixtures/shaders/', 'Open').then(function(p){ if (p) loadShaderFile(p); });
      } break;
      case 'save': saveActiveShader(); break;
      case 'save_as': {
        if (!tab) return;
        shaderPrompt('Save Shader As — res:// path', tab.path, 'Save').then(function(np){
          if (!np) return;
          api('POST', '/api/shader_source', { path: np, source: tab.source }).then(function(r){ if (r && r.ok) { openShaderInEditor(np, tab.source); showFsToast('Saved ' + np.split('/').pop()); if (typeof fetchFileSystem === "function") fetchFileSystem(); } });
        });
      } break;
      case 'inspect': if (tab) { showFsToast('Inspecting ' + tab.path.split('/').pop()); } break;
      case 'inspect_native': showFsToast('Native shader code inspection is not available in Patina yet.'); break;
      case 'close': if (activeShaderIdx >= 0) closeShaderTab(activeShaderIdx); break;
      case 'toggle_files': { var f = document.getElementById('shader-files'); if (f) f.classList.toggle('hidden'); } break;
      case 'undo': if (ta) { ta.focus(); document.execCommand && document.execCommand('undo'); shSyncFromTextarea(); } break;
      case 'redo': if (ta) { ta.focus(); document.execCommand && document.execCommand('redo'); shSyncFromTextarea(); } break;
      case 'cut': if (ta) { ta.focus(); document.execCommand && document.execCommand('cut'); shSyncFromTextarea(); } break;
      case 'copy': if (ta) { ta.focus(); document.execCommand && document.execCommand('copy'); } break;
      case 'paste': if (ta && navigator.clipboard) { navigator.clipboard.readText().then(function(txt){ var s = ta.selectionStart, e = ta.selectionEnd; shApplyEdit(ta.value.slice(0, s) + txt + ta.value.slice(e), s + txt.length); }); } break;
      case 'select_all': if (ta) { ta.focus(); ta.select(); } break;
      case 'move_up': shMoveLines(-1); break;
      case 'move_down': shMoveLines(1); break;
      case 'indent': shIndent(false); break;
      case 'unindent': shIndent(true); break;
      case 'delete_line': shDeleteLine(); break;
      case 'toggle_comment': shToggleComment(); break;
      case 'duplicate_selection': shDuplicate(false); break;
      case 'duplicate_lines': shDuplicate(true); break;
      case 'toggle_word_wrap': shToggleWordWrap(); break;
      case 'complete': showFsToast('Autocomplete: type shader keywords (highlighting is live).'); break;
      case 'find': shOpenFind(false); break;
      case 'find_next': shFind(1); break;
      case 'find_prev': shFind(-1); break;
      case 'replace': shOpenFind(true); break;
      case 'goto_line': shaderPrompt('Go to Line', '', 'Go').then(function(l){ if (l) shaderGotoLine(parseInt(l, 10)); }); break;
      case 'toggle_bookmark': shToggleBookmark(); break;
      case 'remove_bookmarks': if (tab) { tab.bookmarks = []; renderShaderGutter(); } break;
      case 'next_bookmark': shGotoBookmark(1); break;
      case 'prev_bookmark': shGotoBookmark(-1); break;
    }
  }
  function shToggleWordWrap() {
    shaderWordWrap = !shaderWordWrap;
    var ta = shTA(), pre = document.getElementById('shader-highlight');
    if (ta) { ta.setAttribute('wrap', shaderWordWrap ? 'soft' : 'off'); ta.style.whiteSpace = shaderWordWrap ? 'pre-wrap' : 'pre'; }
    if (pre) pre.style.whiteSpace = shaderWordWrap ? 'pre-wrap' : 'pre';
  }
  function shSyncFromTextarea() {
    var ta = shTA(), tab = shActiveTab();
    if (ta && tab) { tab.source = ta.value; renderShaderHighlight(); renderShaderGutter(); updateShaderStatus(); renderShaderFilesList(); validateActiveShader(); }
  }

  function setupShaderEditor() {
    var ta = document.getElementById('shader-source');
    if (ta) {
      ta.addEventListener('input', shSyncFromTextarea);
      ta.addEventListener('scroll', syncShaderScroll);
      ta.addEventListener('keyup', updateShaderStatus);
      ta.addEventListener('click', updateShaderStatus);
      ta.addEventListener('keydown', function(e){
        // Tab inserts a tab (indent selection when spanning lines).
        if (e.key === 'Tab') { e.preventDefault(); if (e.shiftKey) shIndent(true); else if (ta.selectionStart !== ta.selectionEnd) shIndent(false); else { var s = ta.selectionStart; shApplyEdit(ta.value.slice(0, s) + '\t' + ta.value.slice(ta.selectionEnd), s + 1); } return; }
        var mod = e.ctrlKey || e.metaKey;
        if (mod && !e.shiftKey && !e.altKey && (e.key === 's' || e.key === 'S')) { e.preventDefault(); saveActiveShader(); }
        else if (mod && (e.key === 'f' || e.key === 'F')) { e.preventDefault(); shOpenFind(false); }
        else if (mod && (e.key === 'r' || e.key === 'R')) { e.preventDefault(); shOpenFind(true); }
        else if (mod && (e.key === 'g' || e.key === 'G')) { e.preventDefault(); shaderMenuAction('goto_line'); }
        else if (mod && (e.key === 'k' || e.key === 'K') && !e.shiftKey) { e.preventDefault(); shToggleComment(); }
        else if (mod && e.shiftKey && (e.key === 'k' || e.key === 'K')) { e.preventDefault(); shDeleteLine(); }
        else if (e.altKey && e.key === 'ArrowUp') { e.preventDefault(); shMoveLines(-1); }
        else if (e.altKey && e.key === 'ArrowDown') { e.preventDefault(); shMoveLines(1); }
      });
    }
    // Menu bar: click to toggle dropdowns; hover-switch when one is open.
    var menus = document.querySelectorAll('#shader-menubar .shader-menu');
    Array.prototype.forEach.call(menus, function(menu){
      var btn = menu.querySelector('.shader-menu-btn');
      if (btn) btn.addEventListener('click', function(ev){
        ev.stopPropagation();
        var wasOpen = menu.classList.contains('open');
        Array.prototype.forEach.call(menus, function(m){ m.classList.remove('open'); });
        if (!wasOpen) menu.classList.add('open');
      });
      menu.addEventListener('mouseenter', function(){ if (document.querySelector('#shader-menubar .shader-menu.open') && !menu.classList.contains('open')) { Array.prototype.forEach.call(menus, function(m){ m.classList.remove('open'); }); menu.classList.add('open'); } });
    });
    // Menu item clicks.
    Array.prototype.forEach.call(document.querySelectorAll('#shader-menubar .shader-mi'), function(mi){
      mi.addEventListener('click', function(ev){
        ev.stopPropagation();
        if (mi.classList.contains('disabled') || mi.getAttribute('data-submenu')) return;
        var act = mi.getAttribute('data-act');
        Array.prototype.forEach.call(menus, function(m){ m.classList.remove('open'); });
        if (act) shaderMenuAction(act);
      });
    });
    document.addEventListener('click', function(){ Array.prototype.forEach.call(menus, function(m){ m.classList.remove('open'); }); });
    // Online Docs button (Godot site_search → Godot online documentation).
    var docs = document.getElementById('shader-online-docs');
    if (docs) docs.addEventListener('click', function(){ window.open('https://docs.godotengine.org/en/stable/tutorials/shaders/shader_reference/index.html', '_blank'); });
    // Find bar wiring.
    var fi = document.getElementById('shader-find-input');
    if (fi) { fi.addEventListener('input', function(){ shFind(0); }); fi.addEventListener('keydown', function(e){ if (e.key === 'Enter') { e.preventDefault(); shFind(e.shiftKey ? -1 : 1); } else if (e.key === 'Escape') shCloseFind(); }); }
    var bind = function(id, fn){ var el = document.getElementById(id); if (el) el.addEventListener('click', fn); };
    bind('shader-find-next', function(){ shFind(1); });
    bind('shader-find-prev', function(){ shFind(-1); });
    bind('shader-find-close', shCloseFind);
    bind('shader-replace-one', shReplaceOne);
    bind('shader-replace-all', shReplaceAll);
    // Split handle drag.
    var handle = document.getElementById('shader-split-handle'), files = document.getElementById('shader-files');
    if (handle && files) {
      var dragging = false;
      handle.addEventListener('mousedown', function(e){ dragging = true; e.preventDefault(); });
      document.addEventListener('mousemove', function(e){ if (!dragging) return; var rect = files.parentElement.getBoundingClientRect(); var w = Math.max(90, Math.min(400, e.clientX - rect.left)); files.style.width = w + 'px'; });
      document.addEventListener('mouseup', function(){ dragging = false; });
    }
    // Scripting hook: lets external callers open a shader (as the FileSystem
    // dock does) and drive menu actions — used by the editor's e2e tests.
    window.PatinaShaderEditor = {
      load: loadShaderFile,
      menuAction: shaderMenuAction,
      activeTab: function(){ return shActiveTab(); },
      tabCount: function(){ return shaderTabs.length; }
    };
    renderShaderEditor();
  }

  // ---- Bottom panel ----
  function setupBottomPanel() {
    var panel = document.getElementById('bottom-panel');
    var tabs = document.querySelectorAll('.bottom-tab');
    var contents = document.querySelectorAll('.bottom-content-tab');
    var resizeHandle = document.getElementById('bottom-resize-handle');
    var pinBtn = document.getElementById('bottom-pin');
    var expandBtn = document.getElementById('bottom-expand');

    function lastTab() {
      var a = document.querySelector('.bottom-content-tab.active');
      return a ? a.getAttribute('data-tab') : 'output';
    }
    // Open a panel above the tab strip (Godot: make_item_visible shows the dock).
    function openTab(tabName) {
      for (var j = 0; j < tabs.length; j++) tabs[j].classList.toggle('active', tabs[j].getAttribute('data-tab') === tabName);
      for (var j = 0; j < contents.length; j++) contents[j].classList.toggle('active', contents[j].getAttribute('data-tab') === tabName);
      panel.classList.remove('collapsed');
    }
    // Collapse to just the tab strip and deselect every tab (Godot
    // set_deselect_enabled(true): hiding the pressed button leaves none toggled).
    function collapse() {
      for (var j = 0; j < tabs.length; j++) tabs[j].classList.remove('active');
      panel.classList.remove('expanded');
      panel.classList.add('collapsed');
      if (expandBtn) expandBtn.classList.remove('active');
    }

    // Programmatic open that honors the Pin lock (Godot make_item_visible early
    // out when lock_panel_switching && pin visible). Used by auto-open triggers.
    window.bottomPanelOpen = function(tabName, isAuto) {
      if (isAuto && window.bottomPanelLocked) return;
      openTab(tabName);
    };

    for (var i = 0; i < tabs.length; i++) {
      tabs[i].addEventListener('click', function() {
        var tabName = this.getAttribute('data-tab');
        // Godot: clicking the already-open tab toggles the panel shut.
        if (this.classList.contains('active') && !panel.classList.contains('collapsed')) {
          collapse();
        } else {
          openTab(tabName);
        }
      });
    }

    // Pin: lock panel switching (blocks auto-open triggers, not manual clicks).
    if (pinBtn) pinBtn.addEventListener('click', function() {
      window.bottomPanelLocked = !window.bottomPanelLocked;
      pinBtn.classList.toggle('active', !!window.bottomPanelLocked);
    });

    // Expand: grow the bottom panel to a large height (Godot expand_button,
    // Shift+F12). Opens the last panel first if collapsed.
    if (expandBtn) expandBtn.addEventListener('click', function() {
      if (panel.classList.contains('collapsed')) openTab(lastTab());
      expandBtn.classList.toggle('active', panel.classList.toggle('expanded'));
    });

    // Resize: drag the top edge; dragging up grows the panel upward.
    var isResizing = false;
    var startY = 0;
    var startH = 0;
    resizeHandle.addEventListener('mousedown', function(e) {
      if (panel.classList.contains('collapsed')) { openTab(lastTab()); }
      isResizing = true; startY = e.clientY; startH = panel.offsetHeight; e.preventDefault();
    });
    document.addEventListener('mousemove', function(e) {
      if (!isResizing) return;
      var maxH = Math.max(120, window.innerHeight - 140);
      var newH = Math.max(60, Math.min(maxH, startH + (startY - e.clientY)));
      panel.style.height = newH + 'px';
      panel.classList.remove('expanded');
      panel.classList.remove('collapsed');
    });
    document.addEventListener('mouseup', function() { isResizing = false; });
  }

  // ---- Audio bus layout editor (bottom Audio tab, Godot EditorAudioBuses parity) ----
  var audioEffectTypes = [];
  var audioVolumeTimer = null;

  function escapeAttr(s) {
    return String(s == null ? '' : s)
      .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;').replace(/'/g, '&#39;');
  }

  function setupAudioPanel() {
    var busesEl = document.getElementById('audio-buses');
    if (!busesEl) return;
    var addBus = document.getElementById('audio-add-bus');
    var loadBtn = document.getElementById('audio-load');
    var saveAs = document.getElementById('audio-save-as');
    var loadDefault = document.getElementById('audio-load-default');
    var createBtn = document.getElementById('audio-create');

    if (addBus) addBus.addEventListener('click', function() { audioPost('/api/audio/bus/add', {}); });
    if (loadDefault) loadDefault.addEventListener('click', function() { audioPost('/api/audio/layout/load_default', {}); });
    if (createBtn) createBtn.addEventListener('click', function() { audioPost('/api/audio/layout/load_default', {}); });
    if (loadBtn) loadBtn.addEventListener('click', function() { logMessage('editor', 'Audio: Load Bus Layout'); });
    if (saveAs) saveAs.addEventListener('click', function() { logMessage('editor', 'Audio: Save Bus Layout As'); });

    // Bus strips are rendered dynamically; use event delegation.
    busesEl.addEventListener('click', onAudioBusClick);
    busesEl.addEventListener('input', onAudioBusInput);
    busesEl.addEventListener('change', onAudioBusChange);

    refreshAudioBuses();
  }

  async function refreshAudioBuses() {
    var data = await api('GET', '/api/audio/buses');
    if (data) renderAudioBuses(data);
  }

  async function audioPost(path, body) {
    var data = await api('POST', path, body);
    if (data) renderAudioBuses(data);
  }

  function audioMeterPct(db, muted) {
    if (muted) return 0;
    return Math.max(0, Math.min(100, (db + 80) / 86 * 100));
  }

  function renderAudioBuses(data) {
    if (data.effect_types) audioEffectTypes = data.effect_types;
    var pathEl = document.getElementById('audio-layout-path');
    if (pathEl && data.layout_path) pathEl.textContent = String(data.layout_path).replace(/^res:\/\//, '');
    var busesEl = document.getElementById('audio-buses');
    if (!busesEl) return;
    var buses = data.buses || [];
    var busNames = buses.map(function(b) { return b.name; });
    if (!buses.length) { busesEl.innerHTML = '<div class="audio-empty">No audio buses</div>'; return; }
    busesEl.innerHTML = buses.map(function(bus) { return renderAudioBusStrip(bus, busNames); }).join('');
  }

  function renderAudioBusStrip(bus, busNames) {
    var idx = bus.index;
    var nameField = bus.is_master
      ? '<div class="audio-bus-name">' + escapeHtml(bus.name) + '</div>'
      : '<input class="audio-bus-name" data-act="rename" value="' + escapeAttr(bus.name) + '" title="Rename bus">';

    var menuBtn = bus.is_master ? '' :
      '<button class="abus-toggle abus-menu" data-act="bus-remove" title="Delete Bus">&#8942;</button>';
    var toggles =
      '<div class="audio-bus-toggles">' +
      '<button class="abus-toggle solo' + (bus.soloed ? ' active' : '') + '" data-act="solo" title="Solo">S</button>' +
      '<button class="abus-toggle mute' + (bus.muted ? ' active' : '') + '" data-act="mute" title="Mute">M</button>' +
      '<button class="abus-toggle bypass' + (bus.bypassed ? ' active' : '') + '" data-act="bypass" title="Bypass">B</button>' +
      menuBtn +
      '</div>';

    var fill = audioMeterPct(bus.volume_db, bus.muted);
    // The fader sits beside a dB scale (like Godot) and a VU meter.
    var faderMeter =
      '<div class="audio-bus-meter-row">' +
      '<div class="abus-db-scale"><span>+6</span><span>0</span><span>-6</span><span>-12</span><span>-24</span><span>-72</span></div>' +
      '<input type="range" class="abus-fader" data-act="volume" min="-80" max="6" step="0.1" value="' + bus.volume_db + '" title="Volume (dB)">' +
      '<div class="abus-meter"><div class="abus-meter-fill" style="height:' + fill + '%"></div></div>' +
      '</div>';

    var dbReadout = '<div class="abus-db">' + Number(bus.volume_db).toFixed(1) + ' dB</div>';

    var effects = '<div class="abus-effects">';
    (bus.effects || []).forEach(function(fx, fi) {
      effects +=
        '<div class="abus-effect' + (fx.enabled ? '' : ' disabled') + '">' +
        '<input type="checkbox" data-act="effect-toggle" data-fx="' + fi + '"' + (fx.enabled ? ' checked' : '') + ' title="Enable/disable effect">' +
        '<span class="abus-effect-name">' + escapeHtml(fx.name) + '</span>' +
        '<button class="abus-effect-del" data-act="effect-del" data-fx="' + fi + '" title="Delete Effect">&times;</button>' +
        '</div>';
    });
    effects += '</div>';

    var addEffect = '<select class="abus-add-effect" data-act="effect-add" title="Add an audio effect to this bus."><option value="">Add Effect</option>';
    audioEffectTypes.forEach(function(t) {
      addEffect += '<option value="' + escapeAttr(t) + '">' + escapeHtml(t) + '</option>';
    });
    addEffect += '</select>';

    // Master routes to the system speakers (fixed, like Godot); other buses pick
    // an output bus from a dropdown.
    var output;
    if (bus.is_master) {
      output = '<select class="abus-output" disabled title="Master outputs to the system speakers."><option>Speakers</option></select>';
    } else {
      output = '<select class="abus-output" data-act="output" title="Bus output routing.">';
      busNames.forEach(function(n, ni) {
        if (ni === idx) return; // a bus cannot route to itself
        output += '<option value="' + escapeAttr(n) + '"' + (n === bus.output ? ' selected' : '') + '>' + escapeHtml(n) + '</option>';
      });
      output += '</select>';
    }

    return '<div class="audio-bus' + (bus.is_master ? ' master' : '') + '" data-index="' + idx + '">' +
      nameField + toggles + faderMeter + dbReadout + effects + addEffect + output + '</div>';
  }

  function audioBusIndex(el) {
    var strip = el.closest ? el.closest('.audio-bus') : null;
    return strip ? parseInt(strip.getAttribute('data-index'), 10) : -1;
  }

  function onAudioBusClick(e) {
    var t = e.target.closest ? e.target.closest('[data-act]') : null;
    if (!t) return;
    var act = t.getAttribute('data-act');
    var idx = audioBusIndex(t);
    if (idx < 0) return;
    if (act === 'solo') audioPost('/api/audio/bus/solo', { index: idx });
    else if (act === 'mute') audioPost('/api/audio/bus/mute', { index: idx });
    else if (act === 'bypass') audioPost('/api/audio/bus/bypass', { index: idx });
    else if (act === 'bus-remove') audioPost('/api/audio/bus/remove', { index: idx });
    else if (act === 'effect-del') audioPost('/api/audio/effect/remove', { index: idx, effect_index: parseInt(t.getAttribute('data-fx'), 10) });
  }

  function onAudioBusInput(e) {
    var t = e.target;
    if (!t.getAttribute || t.getAttribute('data-act') !== 'volume') return;
    var idx = audioBusIndex(t);
    if (idx < 0) return;
    var db = parseFloat(t.value);
    // Live-update the readout/meter locally without re-rendering, so dragging
    // the fader stays smooth; the value is debounced to the server.
    var strip = t.closest('.audio-bus');
    if (strip) {
      var readout = strip.querySelector('.abus-db');
      if (readout) readout.textContent = db.toFixed(1) + ' dB';
      var fill = strip.querySelector('.abus-meter-fill');
      if (fill) fill.style.height = audioMeterPct(db, false) + '%';
    }
    if (audioVolumeTimer) clearTimeout(audioVolumeTimer);
    audioVolumeTimer = setTimeout(function() {
      api('POST', '/api/audio/bus/volume', { index: idx, volume_db: db });
    }, 60);
  }

  function onAudioBusChange(e) {
    var t = e.target;
    var act = t.getAttribute ? t.getAttribute('data-act') : null;
    if (!act) return;
    var idx = audioBusIndex(t);
    if (idx < 0) return;
    if (act === 'effect-add') {
      if (t.value) audioPost('/api/audio/effect/add', { index: idx, name: t.value });
    } else if (act === 'effect-toggle') {
      audioPost('/api/audio/effect/toggle', { index: idx, effect_index: parseInt(t.getAttribute('data-fx'), 10) });
    } else if (act === 'output') {
      audioPost('/api/audio/bus/output', { index: idx, target: t.value });
    } else if (act === 'rename') {
      audioPost('/api/audio/bus/rename', { index: idx, name: t.value });
    }
  }

  // ---- Output panel (Godot EditorLog parity) ----
  var outputFilters = { std: true, error: true, warning: true, editor: true };
  var outputSearch = '';
  var clearedLogCount = 0; // logs before this server index are "cleared" from view

  function logCat(level) {
    if (level === 'error') return 'error';
    if (level === 'warn' || level === 'warning') return 'warning';
    if (level === 'editor') return 'editor';
    return 'std';
  }
  function _setText(id, v) { var e = document.getElementById(id); if (e) e.textContent = v; }
  function updateOutputCounts() {
    var logEl = document.getElementById('output-log');
    if (!logEl) return;
    var c = { std: 0, error: 0, warning: 0, editor: 0 };
    var entries = logEl.querySelectorAll('.log-entry');
    for (var i = 0; i < entries.length; i++) { var k = entries[i].getAttribute('data-cat') || 'std'; if (c[k] !== undefined) c[k]++; }
    _setText('output-count-std', c.std); _setText('output-count-error', c.error);
    _setText('output-count-warning', c.warning); _setText('output-count-editor', c.editor);
  }
  function applyOutputFilters() {
    var logEl = document.getElementById('output-log');
    if (!logEl) return;
    var entries = logEl.querySelectorAll('.log-entry');
    for (var i = 0; i < entries.length; i++) {
      var e = entries[i];
      var cat = e.getAttribute('data-cat') || 'std';
      var visible = outputFilters[cat] !== false;
      if (visible && outputSearch) { visible = (e.textContent || '').toLowerCase().indexOf(outputSearch) >= 0; }
      e.classList.toggle('log-hidden', !visible);
    }
  }
  function setupOutputPanel() {
    var clear = document.getElementById('output-clear');
    if (clear) clear.addEventListener('click', function() {
      clearedLogCount = lastLogCount;
      var logEl = document.getElementById('output-log'); if (logEl) logEl.innerHTML = '';
      updateOutputCounts();
    });
    var copy = document.getElementById('output-copy');
    if (copy) copy.addEventListener('click', function() {
      var logEl = document.getElementById('output-log'); if (!logEl) return;
      var text = '';
      logEl.querySelectorAll('.log-entry:not(.log-hidden) .log-msg').forEach(function(m) { text += m.textContent + '\n'; });
      if (navigator.clipboard) navigator.clipboard.writeText(text);
    });
    var collapse = document.getElementById('output-collapse');
    if (collapse) collapse.addEventListener('click', function() {
      collapseLogs = !collapseLogs;
      collapse.classList.toggle('active', collapseLogs);
      forceLogRerender = true; fetchLogs(); // pat-d6uah: re-render merged/unmerged
    });
    var showSearch = document.getElementById('output-show-search');
    if (showSearch) showSearch.addEventListener('click', function() {
      var on = showSearch.classList.toggle('active');
      var box = document.getElementById('output-search'); if (box) { box.style.display = on ? '' : 'none'; if (on) box.focus(); }
    });
    var search = document.getElementById('output-search');
    if (search) search.addEventListener('input', function() { outputSearch = this.value.toLowerCase(); applyOutputFilters(); });
    [['std', 'output-filter-std'], ['error', 'output-filter-error'], ['warning', 'output-filter-warning'], ['editor', 'output-filter-editor']].forEach(function(pair) {
      var btn = document.getElementById(pair[1]);
      if (btn) btn.addEventListener('click', function() { outputFilters[pair[0]] = !outputFilters[pair[0]]; btn.classList.toggle('active', outputFilters[pair[0]]); applyOutputFilters(); });
    });
  }

  // pat-d6uah: collapse-duplicates state + a force flag so toggling collapse
  // re-renders even when the log length hasn't changed.
  var collapseLogs = false;
  var forceLogRerender = false;
  async function fetchLogs() {
    var data = await api('GET', '/api/logs');
    if (!data || !Array.isArray(data)) return;
    if (data.length === lastLogCount && !forceLogRerender) return;
    forceLogRerender = false;
    lastLogCount = data.length;
    if (clearedLogCount > data.length) clearedLogCount = 0; // server log reset
    var logEl = document.getElementById('output-log');
    logEl.innerHTML = '';
    var slice = data.slice(clearedLogCount); // oldest -> newest
    // pat-d6uah: count raw messages per category (the type-filter counts reflect
    // real categories, not the merged view).
    var counts = { std: 0, error: 0, warning: 0, editor: 0 };
    slice.forEach(function(e) { var k = e.category || logCat(e.level); if (counts[k] !== undefined) counts[k]++; });
    _setText('output-count-std', counts.std); _setText('output-count-error', counts.error);
    _setText('output-count-warning', counts.warning); _setText('output-count-editor', counts.editor);
    // Build rows, merging consecutive duplicates when Collapse is on
    // (Godot EditorLog _add_log_line collapse / shows occurrence count).
    var rows = [];
    slice.forEach(function(e) {
      var last = rows[rows.length - 1];
      if (collapseLogs && last && last.e.message === e.message && last.e.level === e.level) {
        last.count++;
      } else {
        rows.push({ e: e, count: 1 });
      }
    });
    // Render newest-first.
    for (var i = rows.length - 1; i >= 0; i--) {
      var entry = rows[i].e, count = rows[i].count;
      var cat = entry.category || logCat(entry.level);
      var div = document.createElement('div');
      div.className = 'log-entry' + (cat === 'warning' ? ' log-warn' : '') + (cat === 'error' ? ' log-error' : '');
      div.setAttribute('data-cat', cat);
      var time = new Date(entry.timestamp);
      var timeStr = time.toLocaleTimeString();
      var badge = count > 1 ? '<span class="log-count" title="' + count + ' occurrences">' + count + '</span>' : '';
      div.innerHTML = '<span class="log-time">[' + escapeHtml(timeStr) + ']</span><span class="log-msg">' + escapeHtml(entry.message) + '</span>' + badge;
      logEl.appendChild(div);
    }
    applyOutputFilters();
  }

  async function fetchSceneInfo() {
    var data = await api('GET', '/api/scene/info');
    if (!data) return;
    var el = document.getElementById('scene-info');
    var html = '<div class="scene-info-row"><span class="scene-info-label">Total nodes:</span><span>' + (data.node_count || 0) + '</span></div>';
    if (data.scene_file) {
      html += '<div class="scene-info-row"><span class="scene-info-label">Scene file:</span><span>' + escapeHtml(data.scene_file) + '</span></div>';
    }
    html += '<div class="scene-info-row"><span class="scene-info-label">Modified:</span><span>' + (data.modified ? 'Yes' : 'No') + '</span></div>';
    if (data.type_breakdown) {
      html += '<div class="scene-info-row"><span class="scene-info-label">Node types:</span></div>';
      var types = Object.keys(data.type_breakdown).sort();
      for (var i = 0; i < types.length; i++) {
        html += '<div class="scene-info-row" style="padding-left:16px"><span class="scene-info-label">' +
          escapeHtml(types[i]) + ':</span><span>' + data.type_breakdown[types[i]] + '</span></div>';
      }
    }
    el.innerHTML = html;
    updateSceneFileIndicator(data.scene_file, data.modified);
  }

  function updateSceneFileIndicator(file, modified) {
    var el = document.getElementById('scene-file-indicator');
    if (file) {
      var name = file.split('/').pop().split('\\').pop();
      el.innerHTML = (modified ? '<span class="modified">*</span>' : '') + escapeHtml(name);
    } else {
      el.innerHTML = modified ? '<span class="modified">* unsaved</span>' : '';
    }
    // Update scene tab
    var tab = document.getElementById('scene-tab-current');
    if (tab) {
      var tabName = file ? file.split('/').pop().split('\\').pop() : 'Untitled';
      tab.innerHTML = escapeHtml(tabName) + (modified ? '<span class="modified-indicator"> *</span>' : '');
    }
  }

  // ---- Add Node Dialog (1:1 port of Godot's CreateDialog, create_dialog.cpp) ----
  // The server (/api/node/create_dialog) mirrors _update_search: it returns the
  // hierarchy tree of subsequence matches plus their ancestors, and the
  // best-scoring candidate to auto-select. Favorites and Recent persist
  // per-project like Godot's favorites.Node / create_recent.Node files.
  var addNodeSelectedType = null;
  var favoriteNodeTypes = [];
  var recentNodeTypes = [];
  var addNodeTreeData = null;    // {class_name, matched, instantiable, description, inheritance_chain, children}
  var addNodeBestMatch = null;
  var addNodeInfo = {};          // class_name -> tree node
  var addNodeFetchSeq = 0;
  var addNodeDragFav = null;

  function openAddNodeDialog() {
    addNodeSelectedType = null;
    var input = document.getElementById('add-node-search');
    input.value = '';
    updateAddNodeSearchClear();
    document.getElementById('add-node-dialog').classList.add('open');
    setTimeout(function() { input.focus(); }, 50);
    fetchAddNodeDialog('');
  }

  function closeAddNodeDialog() {
    document.getElementById('add-node-dialog').classList.remove('open');
    addNodeSelectedType = null;
  }

  function updateAddNodeSearchClear() {
    var input = document.getElementById('add-node-search');
    document.getElementById('add-node-search-wrap').classList.toggle('has-text', input.value.length > 0);
  }

  // Godot boosts search scores for types matching the active editor context
  // (preferred_search_result_type set by the scene tree dock).
  function addNodePreferredType() {
    return (typeof editorMode !== 'undefined' && editorMode === '3d') ? 'Node3D' : 'Node2D';
  }

  async function fetchAddNodeDialog(search) {
    var seq = ++addNodeFetchSeq;
    var resp = await api('POST', '/api/node/create_dialog', {
      search: search, base_class: 'Node', preferred: addNodePreferredType()
    });
    if (seq !== addNodeFetchSeq || !resp) return; // stale or failed response
    addNodeTreeData = resp.tree || null;
    addNodeBestMatch = resp.best_match || null;
    favoriteNodeTypes = resp.favorites || [];
    recentNodeTypes = (resp.recent || []).map(function(r) { return r.class_name; });
    renderAddNodeSidebar();
    renderAddNodeTree(search);
    // Selection mirrors _update_search: empty search selects the base type,
    // otherwise the best-scoring match; nothing matching disables the dialog.
    if (!search) selectAddNodeType('Node', false);
    else if (addNodeBestMatch) selectAddNodeType(addNodeBestMatch, true);
    else setAddNodeNoResults(search);
  }

  // Godot collapse rule (_configure_search_option_item): searching expands
  // everything; otherwise only the root and abstract first-level types stay
  // expanded.
  function addNodeDefaultCollapsed(node, parentName, searching) {
    if (searching) return false;
    if (node.class_name === 'Node') return false;
    return parentName !== 'Node' || node.instantiable;
  }

  function renderAddNodeTree(search) {
    var list = document.getElementById('add-node-list');
    list.innerHTML = '';
    addNodeInfo = {};
    if (addNodeTreeData) appendAddNodeRow(list, addNodeTreeData, null, 0, !!search);
  }

  function appendAddNodeRow(container, node, parentName, depth, searching) {
    addNodeInfo[node.class_name] = node;
    var row = document.createElement('div');
    row.className = 'add-node-row' + (node.instantiable ? '' : ' disabled-class');
    row.dataset.className = node.class_name;
    row.style.paddingLeft = (6 + depth * 16) + 'px';

    var twist = document.createElement('span');
    twist.className = 'add-node-twist';
    var hasKids = node.children && node.children.length > 0;
    twist.textContent = hasKids ? '\u25BC' : '';
    row.appendChild(twist);

    var icon = document.createElement('span');
    icon.className = 'node-type-icon';
    icon.innerHTML = classIconHtml(node.class_name).replace('tree-icon', 'node-type-icon');
    row.appendChild(icon);

    var nameSpan = document.createElement('span');
    nameSpan.className = 'add-node-name';
    nameSpan.textContent = node.class_name;
    row.appendChild(nameSpan);

    row.addEventListener('click', function() { selectAddNodeType(node.class_name, false); });
    row.addEventListener('dblclick', function() {
      selectAddNodeType(node.class_name, false);
      createSelectedNode();
    });
    container.appendChild(row);

    if (hasKids) {
      var kids = document.createElement('div');
      kids.className = 'add-node-children';
      for (var i = 0; i < node.children.length; i++) {
        appendAddNodeRow(kids, node.children[i], node.class_name, depth + 1, searching);
      }
      container.appendChild(kids);
      var collapsed = addNodeDefaultCollapsed(node, parentName, searching);
      if (collapsed) { kids.style.display = 'none'; twist.textContent = '\u25B6'; }
      twist.addEventListener('click', function(e) {
        e.stopPropagation();
        toggleAddNodeRowCollapse(row, kids, twist);
      });
    }
  }

  function toggleAddNodeRowCollapse(row, kids, twist) {
    var hidden = kids.style.display === 'none';
    kids.style.display = hidden ? '' : 'none';
    twist.textContent = hidden ? '\u25BC' : '\u25B6';
  }

  function addNodeRowEl(name) {
    var rows = document.querySelectorAll('#add-node-list .add-node-row');
    for (var i = 0; i < rows.length; i++) {
      if (rows[i].dataset.className === name) return rows[i];
    }
    return null;
  }

  function selectAddNodeType(name, scroll) {
    var info = addNodeInfo[name];
    if (!info) return;
    addNodeSelectedType = name;
    document.querySelectorAll('#add-node-list .add-node-row').forEach(function(el) {
      el.classList.toggle('selected', el.dataset.className === name);
    });
    var row = addNodeRowEl(name);
    if (row && scroll) row.scrollIntoView({ block: 'nearest' });
    renderAddNodeDescription(info);
    var favBtn = document.getElementById('add-node-fav-btn');
    favBtn.disabled = false;
    favBtn.classList.toggle('pressed', favoriteNodeTypes.indexOf(name) >= 0);
    var createBtn = document.getElementById('add-node-create');
    createBtn.disabled = !info.instantiable;
    createBtn.title = info.instantiable ? '' : "The selected class can't be instantiated.";
  }

  // Description panel mirrors Godot's EditorHelpBit: "Class X < Parent < ... "
  // inheritance chain, then the class's brief description.
  function renderAddNodeDescription(info) {
    var d = document.getElementById('add-node-description');
    var chainHtml = 'Class <b>' + escapeHtml(info.class_name) + '</b>';
    var chain = info.inheritance_chain || [];
    for (var i = 1; i < chain.length; i++) chainHtml += ' &lt; ' + escapeHtml(chain[i]);
    d.innerHTML = '<div class="desc-class-chain">' + chainHtml + '</div><div>' +
      escapeHtml(info.description || 'No description available.') + '</div>';
  }

  function setAddNodeNoResults(search) {
    addNodeSelectedType = null;
    document.getElementById('add-node-description').textContent = 'No results for "' + search + '".';
    var favBtn = document.getElementById('add-node-fav-btn');
    favBtn.disabled = true;
    favBtn.classList.remove('pressed');
    document.getElementById('add-node-create').disabled = true;
  }

  // Arrow keys in the search box drive the Matches tree, like Godot's
  // FilterLineEdit forwarding navigation events to the Tree control.
  function addNodeVisibleRows() {
    var out = [];
    var rows = document.querySelectorAll('#add-node-list .add-node-row');
    for (var i = 0; i < rows.length; i++) {
      if (rows[i].offsetParent !== null) out.push(rows[i]);
    }
    return out;
  }

  function moveAddNodeSelection(delta) {
    var rows = addNodeVisibleRows();
    if (!rows.length) return;
    var idx = -1;
    for (var i = 0; i < rows.length; i++) {
      if (rows[i].dataset.className === addNodeSelectedType) { idx = i; break; }
    }
    var next = Math.max(0, Math.min(rows.length - 1, idx < 0 ? 0 : idx + delta));
    selectAddNodeType(rows[next].dataset.className, true);
  }

  // ---- Favorites / Recent side panes ----
  function renderAddNodeSidebar() {
    var favBox = document.getElementById('add-node-favorites');
    favBox.innerHTML = '';
    favoriteNodeTypes.forEach(function(name) {
      favBox.appendChild(makeAddNodeSideItem(name, true));
    });
    var recBox = document.getElementById('add-node-recent');
    recBox.innerHTML = '';
    recentNodeTypes.forEach(function(name) {
      recBox.appendChild(makeAddNodeSideItem(name, false));
    });
  }

  function makeAddNodeSideItem(name, isFavorite) {
    var item = document.createElement('div');
    item.className = 'add-node-side-item';
    item.dataset.className = name;
    var icon = document.createElement('span');
    icon.className = 'node-type-icon';
    icon.innerHTML = classIconHtml(name).replace('tree-icon', 'node-type-icon');
    var nameSpan = document.createElement('span');
    nameSpan.textContent = name;
    item.appendChild(icon);
    item.appendChild(nameSpan);
    // Clicking a favorite/recent sets the search text to the type \u2014 the exact
    // match then scores 1.0 and is auto-selected (Godot _favorite_selected /
    // _history_selected). Double-click confirms.
    item.addEventListener('click', function() { pickAddNodeSideItem(name); });
    item.addEventListener('dblclick', function() {
      pickAddNodeSideItem(name).then(function() { createSelectedNode(); });
    });
    if (isFavorite) {
      item.draggable = true;
      item.addEventListener('dragstart', function(e) {
        addNodeDragFav = name;
        e.dataTransfer.effectAllowed = 'move';
      });
      item.addEventListener('dragover', function(e) {
        e.preventDefault();
        item.classList.add('drag-over');
      });
      item.addEventListener('dragleave', function() { item.classList.remove('drag-over'); });
      item.addEventListener('drop', function(e) {
        e.preventDefault();
        item.classList.remove('drag-over');
        dropAddNodeFavorite(name);
      });
    }
    return item;
  }

  function pickAddNodeSideItem(name) {
    var input = document.getElementById('add-node-search');
    input.value = name;
    updateAddNodeSearchClear();
    return fetchAddNodeDialog(name);
  }

  async function dropAddNodeFavorite(targetName) {
    if (!addNodeDragFav || addNodeDragFav === targetName) return;
    var from = favoriteNodeTypes.indexOf(addNodeDragFav);
    var to = favoriteNodeTypes.indexOf(targetName);
    if (from < 0 || to < 0) return;
    favoriteNodeTypes.splice(from, 1);
    favoriteNodeTypes.splice(to > from ? to - 1 : to, 0, addNodeDragFav);
    addNodeDragFav = null;
    renderAddNodeSidebar();
    await api('POST', '/api/node/create_dialog/favorites', { favorites: favoriteNodeTypes });
  }

  async function toggleAddNodeFavorite() {
    if (!addNodeSelectedType) return;
    var resp = await api('POST', '/api/node/create_dialog/toggle_favorite', { class_name: addNodeSelectedType });
    if (!resp) return;
    favoriteNodeTypes = resp.favorites || [];
    renderAddNodeSidebar();
    document.getElementById('add-node-fav-btn').classList.toggle('pressed', resp.is_favorite);
  }

  // Godot creates the node named after its class and de-duplicates against
  // siblings (Sprite2D, Sprite2D2, ...) \u2014 no name prompt.
  function uniqueChildName(parentId, className) {
    var parent = findNodeInTree(sceneData ? sceneData.nodes : null, parentId);
    var siblings = {};
    if (parent && parent.children) {
      for (var i = 0; i < parent.children.length; i++) siblings[parent.children[i].name] = true;
    }
    if (!siblings[className]) return className;
    var n = 2;
    while (siblings[className + n]) n++;
    return className + n;
  }

  async function createSelectedNode() {
    if (!addNodeSelectedType) return;
    var info = addNodeInfo[addNodeSelectedType];
    if (info && !info.instantiable) return;
    var parentId = selectedNodeId || (sceneData && sceneData.nodes ? sceneData.nodes.id : null);
    if (parentId === null) return;
    var name = uniqueChildName(parentId, addNodeSelectedType);
    await api('POST', '/api/node/add', { parent_id: parentId, name: name, class_name: addNodeSelectedType });
    if (selectedNodeId) expandedNodes.add(selectedNodeId);
    closeAddNodeDialog();
    await fetchScene();
  }

  function setupAddNodeDialog() {
    var input = document.getElementById('add-node-search');
    input.addEventListener('input', function() {
      updateAddNodeSearchClear();
      fetchAddNodeDialog(this.value.trim());
    });
    input.addEventListener('keydown', function(e) {
      if (e.key === 'Enter') { e.preventDefault(); createSelectedNode(); }
      else if (e.key === 'Escape') { e.preventDefault(); closeAddNodeDialog(); }
      else if (e.key === 'ArrowDown') { e.preventDefault(); moveAddNodeSelection(1); }
      else if (e.key === 'ArrowUp') { e.preventDefault(); moveAddNodeSelection(-1); }
      else if (e.key === ' ' && addNodeSelectedType) {
        // Godot's ui_select in the search box toggles the selected item's
        // collapse without inserting a space.
        e.preventDefault();
        var row = addNodeRowEl(addNodeSelectedType);
        if (row && row.nextElementSibling && row.nextElementSibling.className === 'add-node-children') {
          toggleAddNodeRowCollapse(row, row.nextElementSibling, row.querySelector('.add-node-twist'));
        }
      }
    });
    document.getElementById('add-node-search-clear').addEventListener('click', function() {
      input.value = '';
      updateAddNodeSearchClear();
      input.focus();
      fetchAddNodeDialog('');
    });
    document.getElementById('add-node-fav-btn').addEventListener('click', toggleAddNodeFavorite);
    document.getElementById('add-node-close').addEventListener('click', closeAddNodeDialog);
    document.getElementById('add-node-cancel').addEventListener('click', closeAddNodeDialog);
    document.getElementById('add-node-create').addEventListener('click', createSelectedNode);
    document.getElementById('add-node-dialog').addEventListener('click', function(e) {
      if (e.target === this) closeAddNodeDialog();
    });
  }

  // ---- FileSystem dock ----
  var fsData = null;
  var fsExpandedDirs = new Set();

  function fsIcon(entry) {
    if (entry.is_dir) return '\uD83D\uDCC1';
    var ext = entry.name.split('.').pop().toLowerCase();
    if (ext === 'tscn') return '\uD83D\uDCC4';
    if (ext === 'gd') return '\uD83D\uDCDC';
    if (ext === 'tres') return '\uD83D\uDCE6';
    if (['png','jpg','jpeg','webp','svg'].indexOf(ext) >= 0) return '\uD83D\uDDBC';
    if (['gdshader','glsl'].indexOf(ext) >= 0) return '\uD83C\uDFA8';
    if (ext === 'cfg') return '\u2699';
    return '\uD83D\uDCC4';
  }

  function renderFsTree(entries, depth, container) {
    if (!entries) return;
    for (var i = 0; i < entries.length; i++) {
      var entry = entries[i];
      if (!fsSubtreeMatches(entry)) continue;   // honor the Filter Files box
      var node = document.createElement('div');
      node.className = 'fs-node';
      node.setAttribute('data-fs-path', entry.path);
      node.setAttribute('data-fs-dir', entry.is_dir ? '1' : '0');
      node.style.paddingLeft = (depth * 16) + 'px';

      var row = document.createElement('div');
      row.className = 'fs-row';

      var toggle = document.createElement('span');
      toggle.className = 'fs-toggle';
      if (entry.is_dir) {
        var isExpanded = fsFilter ? true : fsExpandedDirs.has(entry.path);
        toggle.textContent = isExpanded ? '\u25BC' : '\u25B6';
        (function(e, t) {
          t.addEventListener('click', function(ev) {
            ev.stopPropagation();
            if (fsExpandedDirs.has(e.path)) fsExpandedDirs.delete(e.path);
            else fsExpandedDirs.add(e.path);
            refreshFsTree();
          });
        })(entry, toggle);
      }

      var icon = document.createElement('span');
      icon.className = 'fs-icon';
      icon.textContent = fsIcon(entry);

      var name = document.createElement('span');
      name.className = 'fs-name';
      name.textContent = entry.name;

      row.appendChild(toggle);
      row.appendChild(icon);
      row.appendChild(name);

      if (!entry.is_dir) {
        (function(e) {
          // Single click: show preview + select; load scene on tscn click.
          row.addEventListener('click', function() {
            // Highlight selected file
            var prev = document.querySelector('.fs-row.selected');
            if (prev) prev.classList.remove('selected');
            row.classList.add('selected');
            // Show preview (single-click selects only — Godot behavior).
            showFilePreview(e);
          });
          // Double click a .tscn: open it as the current scene (Godot behavior).
          // Drag a .tscn into the scene tree to instance it instead.
          row.addEventListener('dblclick', function(ev) {
            ev.stopPropagation();
            var ext = e.name.split('.').pop();
            if (ext === 'tscn') { doOpenScene(e.path); }
          });
          // Drag support for asset files (scenes, textures, scripts, resources).
          var dragExt = e.name.split('.').pop().toLowerCase();
          var draggableExts = ['tscn', 'tres', 'gd', 'png', 'jpg', 'jpeg', 'webp', 'svg', 'gdshader', 'glsl', 'wav', 'ogg', 'mp3'];
          if (draggableExts.indexOf(dragExt) >= 0) {
            row.setAttribute('draggable', 'true');
            row.addEventListener('dragstart', function(ev) {
              var dragType = 'resource';
              if (dragExt === 'tscn') dragType = 'tscn_instance';
              else if (['png','jpg','jpeg','webp','svg'].indexOf(dragExt) >= 0) dragType = 'texture';
              else if (dragExt === 'gd') dragType = 'script';
              else if (['gdshader','glsl'].indexOf(dragExt) >= 0) dragType = 'shader';
              else if (['wav','ogg','mp3'].indexOf(dragExt) >= 0) dragType = 'audio';
              ev.dataTransfer.setData('text/plain', JSON.stringify({ type: dragType, path: e.path, name: e.name }));
              ev.dataTransfer.effectAllowed = 'copyMove';
              row.classList.add('fs-dragging');
            });
            row.addEventListener('dragend', function() {
              row.classList.remove('fs-dragging');
            });
          }
        })(entry);
      }

      // pat-flr: Show file size/type for files
      if (!entry.is_dir && entry.size !== undefined) {
        var meta = document.createElement('span');
        meta.style.cssText = 'margin-left:auto;font-size:10px;color:var(--text-dim);';
        var sizeStr = entry.size < 1024 ? entry.size + ' B' : (entry.size / 1024).toFixed(1) + ' KB';
        meta.textContent = (entry.file_type || '') + ' ' + sizeStr;
        row.appendChild(meta);
      }

      // pat-flr: Context menu for rename/delete/mkdir
      (function(e, r) {
        r.addEventListener('contextmenu', function(ev) {
          ev.preventDefault();
          ev.stopPropagation();
          var oldMenu = document.getElementById('fs-context-menu');
          if (oldMenu) oldMenu.remove();
          var menu = document.createElement('div');
          menu.id = 'fs-context-menu';
          menu.style.cssText = 'position:fixed;top:' + ev.clientY + 'px;left:' + ev.clientX + 'px;background:var(--panel);border:1px solid var(--border);border-radius:3px;padding:4px 0;z-index:500;box-shadow:0 4px 12px rgba(0,0,0,0.5);min-width:140px;';
          // pat-ypgqq: full Godot file/folder context menu
          // (filesystem_dock.cpp _file_and_folders_fill_popup). Files expose the
          // resource-only items (Edit Dependencies, View Owners, Copy UID);
          // folders omit them.
          var actions = [];
          actions.push({ label: e.is_dir ? 'Open' : 'Open', action: 'open' });
          if (!e.is_dir) {
            actions.push({ label: 'Edit Dependencies...', action: 'deps' });
            actions.push({ label: 'View Owners...', action: 'owners' });
          }
          actions.push({ sep: true });
          actions.push({ label: 'Copy Path', action: 'copy_path' });
          actions.push({ label: 'Copy Absolute Path', action: 'copy_abs' });
          if (!e.is_dir) actions.push({ label: 'Copy UID', action: 'copy_uid' });
          actions.push({ label: 'Rename...', action: 'rename' });
          actions.push({ label: 'Duplicate...', action: 'duplicate' });
          actions.push({ label: 'Delete', action: 'delete' });
          actions.push({ sep: true });
          actions.push({ label: e.is_dir ? 'Open in File Manager' : 'Show in File Manager', action: 'show_in_fm' });
          // Create New targets the folder itself, or a file's parent folder
          // (Godot shows it for any single path-dependent selection).
          actions.push({ label: 'New...', action: 'mkdir' });
          // pat-k6600: directories can be pinned to the Favorites header.
          if (e.is_dir) {
            var alreadyFav = fsIsFavorite(e.path);
            actions.push({ sep: true });
            actions.push({ label: alreadyFav ? 'Remove Favorite' : 'Add to Favorites', action: alreadyFav ? 'unfavorite' : 'favorite' });
          }
          for (var ai = 0; ai < actions.length; ai++) {
            (function(act) {
              if (act.sep) {
                var div = document.createElement('div');
                div.style.cssText = 'height:1px;background:var(--border);margin:4px 0;';
                menu.appendChild(div);
                return;
              }
              var item = document.createElement('div');
              item.textContent = act.label;
              item.style.cssText = 'padding:4px 12px;cursor:pointer;font-size:12px;white-space:nowrap;';
              item.addEventListener('mouseenter', function() { item.style.background = 'var(--hover)'; });
              item.addEventListener('mouseleave', function() { item.style.background = ''; });
              item.addEventListener('click', function() {
                menu.remove();
                fsContextAction(act.action, e);
              });
              menu.appendChild(item);
            })(actions[ai]);
          }
          document.body.appendChild(menu);
          var dismiss = function() { menu.remove(); document.removeEventListener('click', dismiss); };
          setTimeout(function() { document.addEventListener('click', dismiss); }, 0);
        });
      })(entry, row);

      node.appendChild(row);
      container.appendChild(node);

      if (entry.is_dir && entry.children && (fsFilter ? true : fsExpandedDirs.has(entry.path))) {
        var childContainer = document.createElement('div');
        renderFsTree(entry.children, depth + 1, childContainer);
        container.appendChild(childContainer);
      }
    }
  }

  // pat-ypgqq: copy text to the clipboard with a graceful fallback + a toast.
  function fsCopyToClipboard(text, label) {
    var done = function() { showFsToast((label || 'Copied') + ': ' + text); };
    try {
      if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(text).then(done, done);
      } else {
        var ta = document.createElement('textarea');
        ta.value = text; document.body.appendChild(ta); ta.select();
        try { document.execCommand('copy'); } catch (e) {}
        document.body.removeChild(ta); done();
      }
    } catch (e) { done(); }
  }

  // A transient bottom toast (used for copy/duplicate/show-in-fm feedback).
  function showFsToast(msg) {
    var t = document.createElement('div');
    t.className = 'fs-toast';
    t.textContent = msg;
    t.style.cssText = 'position:fixed;bottom:16px;left:50%;transform:translateX(-50%);background:var(--panel);border:1px solid var(--border);border-radius:4px;padding:8px 14px;font-size:12px;color:var(--text);z-index:1000;box-shadow:0 4px 12px rgba(0,0,0,0.5);max-width:60%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;';
    document.body.appendChild(t);
    setTimeout(function() { if (t.parentNode) t.remove(); }, 2500);
  }

  // A dismissible list popup for Edit Dependencies / View Owners.
  function showFsInfoPopup(title, items) {
    var old = document.getElementById('fs-info-popup'); if (old) old.remove();
    var overlay = document.createElement('div');
    overlay.id = 'fs-info-popup';
    overlay.style.cssText = 'position:fixed;inset:0;background:rgba(0,0,0,0.4);z-index:900;display:flex;align-items:center;justify-content:center;';
    var box = document.createElement('div');
    box.style.cssText = 'background:var(--panel);border:1px solid var(--border);border-radius:6px;min-width:280px;max-width:70%;max-height:70%;overflow:auto;box-shadow:0 8px 24px rgba(0,0,0,0.6);';
    var head = document.createElement('div');
    head.textContent = title;
    head.style.cssText = 'padding:8px 12px;font-weight:600;border-bottom:1px solid var(--border);';
    box.appendChild(head);
    var body = document.createElement('div');
    body.style.cssText = 'padding:8px 12px;font-size:12px;';
    if (!items || !items.length) {
      body.textContent = '(none)';
    } else {
      for (var i = 0; i < items.length; i++) {
        var li = document.createElement('div'); li.textContent = items[i]; li.style.padding = '2px 0'; body.appendChild(li);
      }
    }
    box.appendChild(body);
    overlay.appendChild(box);
    overlay.addEventListener('click', function() { overlay.remove(); });
    document.body.appendChild(overlay);
  }

  // pat-ypgqq: dispatch a context-menu action against its target entry.
  function fsContextAction(action, e) {
    var infoUrl = '/api/filesystem/file_info?path=' + encodeURIComponent(e.path);
    if (action === 'open') {
      if (e.is_dir) {
        if (fsExpandedDirs.has(e.path)) fsExpandedDirs.delete(e.path); else fsExpandedDirs.add(e.path);
        refreshFsTree();
      } else if (e.name.split('.').pop() === 'tscn') { doOpenScene(e.path); }
      else { showFilePreview(e); }
    } else if (action === 'deps') {
      api('GET', infoUrl).then(function(info) { showFsInfoPopup('Dependencies of ' + e.name, info ? info.dependencies : []); });
    } else if (action === 'owners') {
      api('GET', infoUrl).then(function(info) { showFsInfoPopup('Owners of ' + e.name, info ? info.owners : []); });
    } else if (action === 'copy_path') {
      fsCopyToClipboard(e.path, 'Copied Path');
    } else if (action === 'copy_abs') {
      api('GET', infoUrl).then(function(info) { if (info) fsCopyToClipboard(info.abs_path, 'Copied Absolute Path'); });
    } else if (action === 'copy_uid') {
      api('GET', infoUrl).then(function(info) { if (info) fsCopyToClipboard(info.uid, 'Copied UID'); });
    } else if (action === 'rename') {
      var newName = prompt('Rename to:', e.name);
      if (newName && newName !== e.name) api('POST', '/api/filesystem/rename', { old_path: e.path, new_name: newName }).then(function() { fetchFileSystem(); });
    } else if (action === 'duplicate') {
      api('POST', '/api/filesystem/duplicate', { path: e.path }).then(function(resp) { fetchFileSystem(); if (resp && resp.new_path) showFsToast('Duplicated to ' + resp.new_path); });
    } else if (action === 'delete') {
      showConfirm('Delete &quot;' + escapeHtml(e.name) + '&quot;?', { danger: true }).then(function(ok) { if (ok) api('POST', '/api/filesystem/delete', { path: e.path }).then(function() { fetchFileSystem(); }); });
    } else if (action === 'show_in_fm') {
      api('GET', infoUrl).then(function(info) { if (info) showFsToast('File location: ' + info.abs_path); });
    } else if (action === 'mkdir') {
      var folderName = prompt('New folder name:');
      if (folderName) {
        // Files create the folder in their parent dir; folders create inside.
        var baseDir = e.is_dir ? e.path : e.path.substring(0, e.path.lastIndexOf('/'));
        api('POST', '/api/filesystem/mkdir', { path: baseDir + '/' + folderName }).then(function() { fetchFileSystem(); });
      }
    } else if (action === 'favorite') {
      api('POST', '/api/filesystem/favorite', { path: e.path, action: 'add' }).then(function() { fetchFileSystem(); });
    } else if (action === 'unfavorite') {
      api('POST', '/api/filesystem/favorite', { path: e.path, action: 'remove' }).then(function() { fetchFileSystem(); });
    }
  }

  // pat-k6600: is this res:// path currently pinned to Favorites?
  function fsIsFavorite(path) {
    var favs = (fsData && fsData.favorites) || [];
    for (var i = 0; i < favs.length; i++) {
      if (favs[i].path === path) return true;
    }
    return false;
  }

  // pat-k6600: render the favorited directories pinned under the always-present
  // "★ Favorites:" header. Each row navigates on click; right-click removes the
  // favorite. Mirrors Godot's FileSystemDock favorites_item children.
  function renderFsFavorites() {
    var list = document.getElementById('fs-fav-list');
    if (!list) return;
    list.innerHTML = '';
    var favs = (fsData && fsData.favorites) || [];
    for (var i = 0; i < favs.length; i++) {
      (function(fav) {
        var node = document.createElement('div');
        node.className = 'fs-node fs-fav-item';
        node.setAttribute('data-fs-fav-path', fav.path);
        var row = document.createElement('div');
        row.className = 'fs-row';
        var icon = document.createElement('span');
        icon.className = 'fs-icon';
        icon.textContent = fsIcon({ is_dir: fav.is_dir, name: fav.name });
        var name = document.createElement('span');
        name.className = 'fs-name';
        name.textContent = fav.name;
        row.appendChild(icon);
        row.appendChild(name);
        row.addEventListener('click', function() { fsSetCurrentPath(fav.path); });
        row.addEventListener('contextmenu', function(ev) {
          ev.preventDefault();
          ev.stopPropagation();
          var oldMenu = document.getElementById('fs-context-menu');
          if (oldMenu) oldMenu.remove();
          var menu = document.createElement('div');
          menu.id = 'fs-context-menu';
          menu.style.cssText = 'position:fixed;top:' + ev.clientY + 'px;left:' + ev.clientX + 'px;background:var(--panel);border:1px solid var(--border);border-radius:3px;padding:4px 0;z-index:500;box-shadow:0 4px 12px rgba(0,0,0,0.5);min-width:140px;';
          var item = document.createElement('div');
          item.textContent = 'Remove Favorite';
          item.style.cssText = 'padding:4px 12px;cursor:pointer;font-size:12px;white-space:nowrap;';
          item.addEventListener('mouseenter', function() { item.style.background = 'var(--hover)'; });
          item.addEventListener('mouseleave', function() { item.style.background = ''; });
          item.addEventListener('click', function() {
            menu.remove();
            api('POST', '/api/filesystem/favorite', { path: fav.path, action: 'remove' }).then(function() { fetchFileSystem(); });
          });
          menu.appendChild(item);
          document.body.appendChild(menu);
          var dismiss = function() { menu.remove(); document.removeEventListener('click', dismiss); };
          setTimeout(function() { document.addEventListener('click', dismiss); }, 0);
        });
        node.appendChild(row);
        list.appendChild(node);
      })(favs[i]);
    }
  }

  // pat-96yv4: split display mode. Find a node by its res:// path anywhere in
  // the tree, so the file-list panel can show a chosen directory's files.
  function fsFindNodeByPath(entries, path) {
    if (!entries) return null;
    for (var i = 0; i < entries.length; i++) {
      if (entries[i].path === path) return entries[i];
      if (entries[i].is_dir && entries[i].children) {
        var found = fsFindNodeByPath(entries[i].children, path);
        if (found) return found;
      }
    }
    return null;
  }

  // The directory whose files the file-list shows: the current path if it's a
  // directory, else its parent; defaults to res:// (the project root).
  function fsCurrentDirPath() {
    var input = document.getElementById('fs-path');
    var p = (input && input.value) || 'res://';
    var node = fsFindNodeByPath(fsData && fsData.files, p);
    if (node && !node.is_dir) {
      var idx = p.lastIndexOf('/');
      return idx > 5 ? p.substring(0, idx) : 'res://';
    }
    return p || 'res://';
  }

  function fsFilesInDir(path) {
    var entries;
    if (!path || path === 'res://') {
      entries = (fsData && fsData.files) || [];
    } else {
      var node = fsFindNodeByPath(fsData && fsData.files, path);
      entries = (node && node.children) || [];
    }
    return entries.filter(function(e) { return !e.is_dir; });
  }

  // Apply the split mode class to the dock chrome and reflect it on the toggle.
  function applyFsDisplayMode() {
    var chrome = document.getElementById('fs-chrome');
    if (!chrome) return;
    chrome.classList.remove('fs-mode-tree_only', 'fs-mode-vsplit', 'fs-mode-hsplit');
    chrome.classList.add('fs-mode-' + (fsDisplayMode || 'tree_only'));
    var btn = document.getElementById('fs-split-toggle');
    if (btn) btn.classList.toggle('active', fsDisplayMode !== 'tree_only');
  }

  // Render the separate file list shown in split modes (the files of the
  // current directory). Mirrors Godot's file_list_vb in DISPLAY_MODE_*SPLIT.
  function renderFsFileList() {
    var list = document.getElementById('fs-file-list');
    if (!list) return;
    list.innerHTML = '';
    if (fsDisplayMode === 'tree_only') return;
    var files = fsFilesInDir(fsCurrentDirPath());
    if (!files.length) {
      var empty = document.createElement('div');
      empty.className = 'fs-file-empty';
      empty.textContent = '(no files)';
      list.appendChild(empty);
      return;
    }
    for (var i = 0; i < files.length; i++) {
      (function(e) {
        var row = document.createElement('div');
        row.className = 'fs-file-row';
        row.setAttribute('data-fs-path', e.path);
        var icon = document.createElement('span');
        icon.className = 'fs-icon';
        icon.textContent = fsIcon(e);
        var name = document.createElement('span');
        name.className = 'fs-name';
        name.textContent = e.name;
        row.appendChild(icon);
        row.appendChild(name);
        row.addEventListener('click', function() {
          var prev = list.querySelector('.fs-file-row.selected');
          if (prev) prev.classList.remove('selected');
          row.classList.add('selected');
          showFilePreview(e);
        });
        row.addEventListener('dblclick', function() {
          if (e.name.split('.').pop() === 'tscn') doOpenScene(e.path);
        });
        list.appendChild(row);
      })(files[i]);
    }
  }

  function refreshFsTree() {
    var el = document.getElementById('fs-tree');
    el.innerHTML = '';
    if (fsData && fsData.files) {
      fsSortEntries(fsData.files);
      renderFsTree(fsData.files, 0, el);
    }
    renderFsFavorites();
    applyFsDisplayMode();
    renderFsFileList();
  }

  async function fetchFileSystem() {
    var data = await api('GET', '/api/filesystem');
    if (data) {
      fsData = data;
      if (data.display_mode) fsDisplayMode = data.display_mode;
      refreshFsTree();
    }
  }

  // ---- FileSystem dock chrome (Godot parity) ----
  // The toolbars (nav/path/split, filter/sort) and the always-present Favorites
  // section. Godot's FileSystemDock builds all of these unconditionally in its
  // constructor (upstream/godot/editor/docks/filesystem_dock.cpp), so they are
  // permanent chrome — visible before you ever favorite or filter anything.
  var fsFilter = '';
  var fsSortDesc = false;
  var fsNavHistory = [];
  var fsNavIdx = -1;
  var fsDisplayMode = 'tree_only'; // pat-96yv4: tree_only | vsplit | hsplit

  // A node is shown when it (or any descendant) matches the Filter Files box, so
  // a match deep in the tree keeps its parent folders visible.
  function fsEntryMatches(e) { return e.name.toLowerCase().indexOf(fsFilter) >= 0; }
  function fsSubtreeMatches(e) {
    if (!fsFilter) return true;
    if (fsEntryMatches(e)) return true;
    if (e.is_dir && e.children) {
      for (var i = 0; i < e.children.length; i++) {
        if (fsSubtreeMatches(e.children[i])) return true;
      }
    }
    return false;
  }

  // Sort a tree of entries in place: directories first (Godot default), then by
  // name in the current ascending/descending order.
  function fsSortEntries(entries) {
    if (!entries) return;
    entries.sort(function(a, b) {
      if (a.is_dir !== b.is_dir) return a.is_dir ? -1 : 1;
      var an = a.name.toLowerCase(), bn = b.name.toLowerCase();
      var c = an < bn ? -1 : (an > bn ? 1 : 0);
      return fsSortDesc ? -c : c;
    });
    for (var i = 0; i < entries.length; i++) {
      if (entries[i].is_dir && entries[i].children) fsSortEntries(entries[i].children);
    }
  }

  function fsUpdateNavButtons() {
    var prev = document.getElementById('fs-nav-prev');
    var next = document.getElementById('fs-nav-next');
    if (prev) prev.disabled = fsNavIdx <= 0;
    if (next) next.disabled = fsNavIdx >= fsNavHistory.length - 1;
  }

  function fsSetCurrentPath(path, fromNav) {
    var input = document.getElementById('fs-path');
    if (input) input.value = path || 'res://';
    if (!fromNav && path) {
      // New selection truncates any forward history, then is pushed on top.
      if (fsNavIdx < fsNavHistory.length - 1) fsNavHistory = fsNavHistory.slice(0, fsNavIdx + 1);
      if (fsNavHistory[fsNavHistory.length - 1] !== path) {
        fsNavHistory.push(path);
        fsNavIdx = fsNavHistory.length - 1;
      }
    }
    fsUpdateNavButtons();
  }

  function fsNav(delta) {
    var target = fsNavIdx + delta;
    if (target < 0 || target >= fsNavHistory.length) return;
    fsNavIdx = target;
    fsSetCurrentPath(fsNavHistory[fsNavIdx], true);
  }

  function setupFileSystemChrome() {
    var filter = document.getElementById('fs-filter');
    if (filter) filter.addEventListener('input', function() {
      fsFilter = this.value.trim().toLowerCase();
      refreshFsTree();
    });
    var sort = document.getElementById('fs-sort');
    if (sort) sort.addEventListener('click', function() {
      fsSortDesc = !fsSortDesc;
      sort.classList.toggle('active', fsSortDesc);
      refreshFsTree();
    });
    var prev = document.getElementById('fs-nav-prev');
    var next = document.getElementById('fs-nav-next');
    if (prev) prev.addEventListener('click', function() { fsNav(-1); });
    if (next) next.addEventListener('click', function() { fsNav(1); });
    // pat-96yv4: cycle the dock split mode server-side (persists), then re-render.
    var split = document.getElementById('fs-split-toggle');
    if (split) split.addEventListener('click', function() {
      api('POST', '/api/filesystem/display_mode', { action: 'cycle' }).then(function(resp) {
        if (resp && resp.display_mode) fsDisplayMode = resp.display_mode;
        applyFsDisplayMode();
        renderFsFileList();
      });
    });
    var favHeader = document.getElementById('fs-fav-header');
    if (favHeader) favHeader.addEventListener('click', function() {
      var fav = document.getElementById('fs-favorites');
      var toggle = document.getElementById('fs-fav-toggle');
      var collapsed = fav.classList.toggle('collapsed');
      if (toggle) toggle.textContent = collapsed ? '▶' : '▼';
    });
    // Track the selected path for the path bar + nav history. Delegated so every
    // row participates without editing the per-row handlers.
    var tree = document.getElementById('fs-tree');
    if (tree) tree.addEventListener('click', function(ev) {
      var node = ev.target.closest ? ev.target.closest('.fs-node') : null;
      if (node && node.getAttribute('data-fs-path')) {
        fsSetCurrentPath(node.getAttribute('data-fs-path'));
        // pat-96yv4: in split modes, the file list follows the selected dir.
        if (fsDisplayMode !== 'tree_only') renderFsFileList();
        // pat-h5s93: selecting a directory (not a resource) clears the Import
        // dock back to its empty-state.
        if (node.getAttribute('data-fs-dir') === '1') {
          currentImportResource = null;
          if (importDockActive) renderImportDock();
        }
      }
    });
    fsUpdateNavButtons();
  }

  // ---- History dock (tabbed with FileSystem, bottom-left) ----
  // Mirrors Godot's History dock: a newest-first list of undo/redo actions with
  // "The Beginning" anchored at the bottom, the current version highlighted, and
  // future (redoable) actions dimmed. Scene/Global checkboxes filter by scope.
  // Clicking any row jumps the edit state to that version.
  var historyData = null;
  var historyDockActive = false;

  // ---- Script editor toolbar (Godot menu_hb parity) ----
  // File/Search/Debug menu items mirror script_editor_plugin.cpp:4328-4385.
  var SCRIPT_FILE_MENU = [
    {label:'New Script...', sc:'Ctrl+N', act:'new'},
    {label:'New Text File...', sc:'Ctrl+Shift+N', act:'new_text'},
    {label:'Open...', act:'open'},
    {label:'Open Recent', act:'open_recent'},
    {sep:true},
    {label:'Save', sc:'Ctrl+Alt+S', act:'save'},
    {label:'Save As...', act:'save_as'},
    {label:'Save All', sc:'Shift+Alt+S', act:'save_all'},
    {sep:true},
    {label:'Copy Script Path', act:'copy_path'},
    {label:'Show in FileSystem', act:'show_in_fs'},
    {sep:true},
    {label:'History Previous', sc:'Alt+Left', act:'hist_prev'},
    {label:'History Next', sc:'Alt+Right', act:'hist_next'},
    {sep:true},
    {label:'Close', sc:'Ctrl+W', act:'close'},
    {label:'Close All', act:'close_all'},
    {sep:true},
    {label:'Run', sc:'Ctrl+Shift+X', act:'run'},
    {label:'Toggle Files Panel', sc:'Ctrl+\\', act:'toggle_files'}
  ];
  var SCRIPT_SEARCH_MENU = [
    {label:'Find...', sc:'Ctrl+F', act:'find'},
    {label:'Find Next', sc:'F3', act:'find_next'},
    {label:'Find Previous', sc:'Shift+F3', act:'find_prev'},
    {label:'Replace...', sc:'Ctrl+R', act:'replace'},
    {sep:true},
    {label:'Find in Files...', sc:'Ctrl+Shift+F', act:'find_in_files'},
    {label:'Replace in Files...', sc:'Ctrl+Shift+R', act:'replace_in_files'},
    {sep:true},
    {label:'Contextual Help', sc:'Alt+F1', act:'contextual_help'}
  ];
  var SCRIPT_DEBUG_MENU = [
    {label:'Step Into', sc:'F11', act:'step_into'},
    {label:'Step Over', sc:'F10', act:'step_over'},
    {label:'Break', act:'break'},
    {label:'Continue', sc:'F12', act:'continue'},
    {sep:true},
    {label:'Keep Debugger Open', act:'keep_open'},
    {label:'Debug with External Editor', act:'external'}
  ];

  function setupScriptToolbar() {
    bindScriptMenu('script-file-menu', SCRIPT_FILE_MENU);
    bindScriptMenu('script-search-menu', SCRIPT_SEARCH_MENU);
    bindScriptMenu('script-debug-menu', SCRIPT_DEBUG_MENU);
    var docs = document.getElementById('script-online-docs');
    if (docs) docs.addEventListener('click', function() { window.open('https://docs.godotengine.org/en/stable/', '_blank'); });
    // pat-nrwy2: Search Help opens an in-editor reference search dialog (Godot
    // EditorHelpSearch), not the web docs.
    var help = document.getElementById('script-search-help');
    if (help) help.addEventListener('click', openScriptHelpSearch);
    // pat-nrwy2: history arrows navigate the edited-document stack.
    var back = document.getElementById('script-back');
    if (back) back.addEventListener('click', function() { scriptHistoryGo(-1); });
    var fwd = document.getElementById('script-forward');
    if (fwd) fwd.addEventListener('click', function() { scriptHistoryGo(1); });
    var filter = document.getElementById('script-filter');
    if (filter) filter.addEventListener('input', renderScriptList);
    // pat-f64ui: members overview filter + alpha-sort toggle.
    var mfilter = document.getElementById('script-method-filter');
    if (mfilter) mfilter.addEventListener('input', renderScriptMembers);
    var msort = document.getElementById('script-members-sort');
    if (msort) msort.addEventListener('click', function() {
      scriptMembersAlphaSort = !scriptMembersAlphaSort;
      msort.classList.toggle('active', scriptMembersAlphaSort);
      renderScriptMembers();
    });
  }

  // pat-nrwy2: in-editor reference (Help) search dialog. Mirrors Godot's
  // EditorHelpSearch: a modal with a filter box over a list of reference topics
  // (classes); selecting one shows its summary. Not a web redirect.
  var HELP_TOPICS = [
    { name: 'Node', desc: 'Base class for all scene objects.' },
    { name: 'Node2D', desc: '2D game object, inherits transform from CanvasItem.' },
    { name: 'Node3D', desc: 'Most basic 3D game object with a 3D transform.' },
    { name: 'Sprite2D', desc: 'General-purpose Sprite node for 2D textures.' },
    { name: 'CharacterBody2D', desc: '2D physics body for characters moved by script.' },
    { name: 'RigidBody2D', desc: '2D physics body moved by the physics simulation.' },
    { name: 'Area2D', desc: '2D region that detects overlap and influences physics.' },
    { name: 'Control', desc: 'Base class for all GUI/UI nodes.' },
    { name: 'Label', desc: 'Displays plain text.' },
    { name: 'Button', desc: 'Standard clickable button.' },
    { name: 'Camera2D', desc: 'Camera that scrolls the 2D scene it is in.' },
    { name: 'AnimationPlayer', desc: 'Plays back animation resources.' },
    { name: 'Timer', desc: 'Counts down and emits a signal on timeout.' },
    { name: 'CollisionShape2D', desc: 'Provides a collision shape to a physics body.' },
    { name: 'TileMap', desc: 'Node for 2D tile-based maps.' },
    { name: 'CanvasItem', desc: 'Base class for everything drawn in 2D.' },
    { name: 'Resource', desc: 'Base class for serializable data containers.' },
    { name: 'SceneTree', desc: 'Manages the active scene tree and the game loop.' }
  ];
  function openScriptHelpSearch() {
    var old = document.getElementById('script-help-dialog'); if (old) old.remove();
    var overlay = document.createElement('div');
    overlay.id = 'script-help-dialog';
    overlay.className = 'help-search-overlay';
    var box = document.createElement('div'); box.className = 'help-search-box';
    var head = document.createElement('div'); head.className = 'help-search-head'; head.textContent = 'Search Help';
    var input = document.createElement('input'); input.type = 'text'; input.className = 'help-search-input'; input.placeholder = 'Search the reference documentation...';
    var results = document.createElement('div'); results.className = 'help-search-results';
    var detail = document.createElement('div'); detail.className = 'help-search-detail';
    function render() {
      var q = input.value.toLowerCase();
      results.innerHTML = '';
      HELP_TOPICS.filter(function(t) { return !q || t.name.toLowerCase().indexOf(q) >= 0 || t.desc.toLowerCase().indexOf(q) >= 0; })
        .forEach(function(t) {
          var item = document.createElement('div'); item.className = 'help-search-item'; item.textContent = t.name;
          item.addEventListener('click', function() {
            results.querySelectorAll('.help-search-item.active').forEach(function(e) { e.classList.remove('active'); });
            item.classList.add('active');
            detail.innerHTML = '<b>' + t.name + '</b><br>' + t.desc;
          });
          results.appendChild(item);
        });
    }
    input.addEventListener('input', render);
    box.appendChild(head); box.appendChild(input); box.appendChild(results); box.appendChild(detail);
    overlay.appendChild(box);
    overlay.addEventListener('click', function(e) { if (e.target === overlay) overlay.remove(); });
    document.body.appendChild(overlay);
    render(); input.focus();
  }

  function bindScriptMenu(btnId, items) {
    var btn = document.getElementById(btnId);
    if (!btn) return;
    btn.addEventListener('click', function(ev) {
      ev.stopPropagation();
      var existing = document.getElementById('script-active-dropdown');
      if (existing) { existing.remove(); btn.classList.remove('open'); return; }
      var menu = document.createElement('div');
      menu.className = 'script-dropdown';
      menu.id = 'script-active-dropdown';
      var r = btn.getBoundingClientRect();
      menu.style.left = r.left + 'px';
      menu.style.top = r.bottom + 'px';
      items.forEach(function(it) {
        if (it.sep) { var s = document.createElement('div'); s.className = 'script-dropdown-sep'; menu.appendChild(s); return; }
        var item = document.createElement('div');
        item.className = 'script-dropdown-item';
        item.innerHTML = '<span>' + it.label + '</span>' + (it.sc ? '<span class="sd-shortcut">' + it.sc + '</span>' : '');
        (function(act) { item.addEventListener('click', function() { menu.remove(); btn.classList.remove('open'); scriptMenuAction(act); }); })(it.act);
        menu.appendChild(item);
      });
      document.body.appendChild(menu);
      btn.classList.add('open');
      var dismiss = function() { var m = document.getElementById('script-active-dropdown'); if (m) m.remove(); btn.classList.remove('open'); document.removeEventListener('click', dismiss); };
      setTimeout(function() { document.addEventListener('click', dismiss); }, 0);
    });
  }

  // pat-kgfgf: the File/Search/Debug menu actions perform real work.
  var scriptLastFindTerm = '';
  function clickDbg(id) { var b = document.getElementById(id); if (b) b.click(); }
  function scriptFindInEditor(term, fromStart) {
    var ta = document.getElementById('script-textarea');
    if (!ta || !term) return false;
    var hay = ta.value.toLowerCase(), needle = term.toLowerCase();
    var start = fromStart ? 0 : (ta.selectionEnd || 0);
    var idx = hay.indexOf(needle, start);
    if (idx < 0) idx = hay.indexOf(needle, 0); // wrap
    if (idx < 0) { showFsToast('Not found: ' + term); return false; }
    ta.focus(); ta.setSelectionRange(idx, idx + term.length);
    var line = ta.value.slice(0, idx).split('\n').length;
    ta.scrollTop = Math.max(0, (line - 3) * 16);
    return true;
  }
  function scriptReplaceInEditor(term, repl) {
    var ta = document.getElementById('script-textarea');
    if (!ta || !term) return;
    var sel = ta.value.substring(ta.selectionStart, ta.selectionEnd);
    if (sel.toLowerCase() === term.toLowerCase() && typeof ta.setRangeText === 'function') {
      ta.setRangeText(repl, ta.selectionStart, ta.selectionEnd, 'end');
      ta.dispatchEvent(new Event('input'));
    }
    scriptFindInEditor(term, false);
  }
  async function scriptFindInFiles(term) {
    if (!term) return;
    var data = await api('GET', '/api/search?q=' + encodeURIComponent(term));
    var results = (data && (data.results || data.matches || data.hits)) || [];
    var lines = results.map(function(r) {
      if (typeof r === 'string') return r;
      return (r.path || r.file || '') + (r.line ? (':' + r.line) : '') + (r.text ? ('  ' + String(r.text).trim()) : '');
    });
    showFsInfoPopup('Find in Files: ' + term, lines);
  }
  function scriptCloseActive() {
    var idx = -1;
    for (var i = 0; i < scriptListData.length; i++) { if (scriptListData[i].active) { idx = i; break; } }
    if (idx < 0) return;
    scriptListData.splice(idx, 1);
    if (scriptListData.length) { fetchScript(scriptListData[Math.max(0, idx - 1)].path); }
    else { scriptCloseAll(); }
  }
  function scriptCloseAll() {
    scriptListData = []; currentScriptPath = null; activeScriptMethods = [];
    var el = document.getElementById('script-panel');
    if (el) el.innerHTML = '<div class="script-empty">Select a node with a script to view its content</div>';
    setScriptName(''); renderScriptMembers(); renderScriptList();
  }
  function scriptMenuAction(act) {
    switch (act) {
      case 'new': case 'new_text': if (typeof createNewScript === 'function') createNewScript(); return;
      case 'open': { var p = prompt('Open script (res:// path):'); if (p && p.trim()) fetchScript(p.trim()); return; }
      case 'save': saveScript(); return;
      case 'save_all': saveScript(); logMessage('info', 'Saved all open scripts'); return;
      case 'save_as': {
        var np = prompt('Save script as (res:// path):', currentScriptPath || '');
        if (np && np.trim()) {
          var ta = document.getElementById('script-textarea');
          api('POST', '/api/script/save', { path: np.trim(), content: ta ? ta.value : '' }).then(function(r) {
            if (r && r.ok) { showFsToast('Saved ' + np.trim()); fetchScript(np.trim()); }
          });
        }
        return;
      }
      case 'close': scriptCloseActive(); return;
      case 'close_all': scriptCloseAll(); return;
      case 'copy_path': if (currentScriptPath) fsCopyToClipboard(currentScriptPath, 'Copied Script Path'); return;
      case 'show_in_fs': showFsToast(currentScriptPath ? ('Show in FileSystem: ' + currentScriptPath) : 'No script open'); return;
      case 'hist_prev': scriptHistoryGo(-1); return;
      case 'hist_next': scriptHistoryGo(1); return;
      case 'find': { var t = prompt('Find:', scriptLastFindTerm); if (t) { scriptLastFindTerm = t; scriptFindInEditor(t, true); } return; }
      case 'find_next': if (scriptLastFindTerm) scriptFindInEditor(scriptLastFindTerm, false); else scriptMenuAction('find'); return;
      case 'find_prev': if (scriptLastFindTerm) scriptFindInEditor(scriptLastFindTerm, false); else scriptMenuAction('find'); return;
      case 'replace': {
        var ft = prompt('Find:', scriptLastFindTerm); if (!ft) return;
        var rt = prompt('Replace with:'); if (rt === null) return;
        scriptLastFindTerm = ft; scriptReplaceInEditor(ft, rt); return;
      }
      case 'find_in_files': { var q = prompt('Find in Files:'); if (q && q.trim()) scriptFindInFiles(q.trim()); return; }
      case 'step_into': clickDbg('debug-btn-step-in'); return;
      case 'step_over': clickDbg('debug-btn-step-over'); return;
      case 'break': clickDbg('debug-btn-break'); return;
      case 'continue': clickDbg('debug-btn-continue'); return;
      default: if (typeof logMessage === 'function') logMessage('info', 'Script menu: ' + act); return;
    }
  }

  // The open-scripts list (left column). Mirrors Godot's script_list ItemList.
  var scriptListData = [];
  function setScriptName(name) {
    var lbl = document.getElementById('script-name-label');
    if (lbl) lbl.textContent = name || '';
  }
  function renderScriptList() {
    var list = document.getElementById('script-list');
    if (!list) return;
    var q = ((document.getElementById('script-filter') || {}).value || '').toLowerCase();
    list.innerHTML = '';
    scriptListData.forEach(function(s) {
      if (q && s.name.toLowerCase().indexOf(q) < 0) return;
      var item = document.createElement('div');
      item.className = 'script-list-item' + (s.active ? ' active' : '');
      item.textContent = s.name;
      // pat-f64ui: clicking an open script switches to it.
      item.addEventListener('click', function() { if (!s.active) fetchScript(s.path); });
      list.appendChild(item);
    });
  }

  // ---- Game view toolbar (Godot game_view_plugin.cpp parity) ----
  // pat-j9yqh: the game-view toolbar drives the running game over HTTP.
  var gameTimeScales = [1.0, 2.0, 4.0, 0.5, 0.25];
  var gameSpeedIdx = 0;
  function setupGameToolbar() {
    // Input / 2D / 3D switch the interaction mode (Godot node_type_button).
    var modeMap = { 'game-input': 'input', 'game-node-2d': '2d', 'game-node-3d': '3d' };
    Object.keys(modeMap).forEach(function(id) {
      var b = document.getElementById(id);
      if (b) b.addEventListener('click', function() {
        Object.keys(modeMap).forEach(function(o) { var e = document.getElementById(o); if (e) e.classList.toggle('active', o === id); });
        api('POST', '/api/runtime/input_mode', { mode: modeMap[id] });
      });
    });
    // Select / List-select are a mutually-exclusive group.
    var selModes = ['game-select-single', 'game-select-list'];
    selModes.forEach(function(id) {
      var b = document.getElementById(id);
      if (b) b.addEventListener('click', function() {
        selModes.forEach(function(o) { var e = document.getElementById(o); if (e) e.classList.toggle('active', o === id); });
      });
    });
    // Suspend pauses the SceneTree.
    var suspend = document.getElementById('game-suspend');
    if (suspend) suspend.addEventListener('click', function() {
      var on = suspend.classList.toggle('active');
      api('POST', '/api/runtime/suspend', { suspended: on });
    });
    // Next Frame advances one frame (while suspended).
    var nextFrame = document.getElementById('game-next-frame');
    if (nextFrame) nextFrame.addEventListener('click', function() { api('POST', '/api/runtime/step'); });
    // Speed control cycles the time scale; reset returns to 1.0.
    var speed = document.getElementById('game-speed');
    if (speed) speed.addEventListener('click', function() {
      gameSpeedIdx = (gameSpeedIdx + 1) % gameTimeScales.length;
      var sc = gameTimeScales[gameSpeedIdx];
      speed.innerHTML = sc.toFixed(sc < 1 ? 2 : 1) + '&times;';
      api('POST', '/api/runtime/time_scale', { scale: sc });
    });
    var resetSpeed = document.getElementById('game-reset-speed');
    if (resetSpeed) resetSpeed.addEventListener('click', function() {
      gameSpeedIdx = 0;
      if (speed) speed.innerHTML = '1.0&times;';
      api('POST', '/api/runtime/time_scale', { scale: 1.0 });
    });
    // Hide Selection toggles selection visibility in the running game.
    var hideSel = document.getElementById('game-hide-selection');
    if (hideSel) hideSel.addEventListener('click', function() {
      var on = hideSel.classList.toggle('active');
      api('POST', '/api/runtime/hide_selection', { hidden: on });
    });
    // Camera Override drives the in-game camera.
    var camOverride = document.getElementById('game-camera-override');
    if (camOverride) camOverride.addEventListener('click', function() {
      var on = camOverride.classList.toggle('active');
      api('POST', '/api/runtime/camera_override', { enabled: on });
    });
    // Mute silences the game audio.
    var mute = document.getElementById('game-mute');
    if (mute) mute.addEventListener('click', function() {
      var on = mute.classList.toggle('active');
      mute.innerHTML = on ? '&#128263;' : '&#128266;'; // 🔇 / 🔊
      api('POST', '/api/runtime/mute', { muted: on });
    });
  }

  // ---- Debugger sub-tabs (Godot ScriptEditorDebugger parity) ----
  function setupDebuggerSubtabs() {
    var subtabs = document.querySelectorAll('#debugger-subtabs .dbg-subtab');
    for (var i = 0; i < subtabs.length; i++) {
      (function(tab) {
        tab.addEventListener('click', function() {
          var name = tab.getAttribute('data-dbgtab');
          var all = document.querySelectorAll('#debugger-subtabs .dbg-subtab');
          for (var j = 0; j < all.length; j++) all[j].classList.toggle('active', all[j].getAttribute('data-dbgtab') === name);
          var panels = document.querySelectorAll('#debugger-subcontent .dbg-subpanel');
          for (var k = 0; k < panels.length; k++) panels[k].classList.toggle('active', panels[k].getAttribute('data-dbgtab') === name);
          loadDebuggerSubtab(name); // pat-j906k: populate the activated sub-tab
        });
      })(subtabs[i]);
    }
    // New debug-toolbar controls (continue/step-in/over/out keep their existing handlers).
    var brk = document.getElementById('debug-btn-break');
    if (brk) brk.addEventListener('click', function() { api('POST', '/api/debug/break'); });
    var skip = document.getElementById('debug-btn-skip-bp');
    if (skip) skip.addEventListener('click', function() {
      var on = skip.classList.toggle('active');
      api('POST', '/api/debug/skip_breakpoints', { enabled: on }); // pat-j906k
    });
    var ignoreErr = document.getElementById('debug-btn-ignore-errors');
    if (ignoreErr) ignoreErr.addEventListener('click', function() { ignoreErr.classList.toggle('active'); });
    var dbgExpand = document.getElementById('debugger-expand');
    if (dbgExpand) dbgExpand.addEventListener('click', function() {
      var bp = document.getElementById('bottom-panel');
      if (bp) dbgExpand.classList.toggle('active', bp.classList.toggle('expanded'));
    });
    var dbgFloat = document.getElementById('debugger-float');
    if (dbgFloat) dbgFloat.addEventListener('click', function() { dbgFloat.classList.toggle('active'); });
    var copyErr = document.getElementById('debug-btn-copy-error');
    if (copyErr) copyErr.addEventListener('click', function() {
      // pat-j906k: copy the Errors list (fallback to the stack frames).
      var le = document.getElementById('debug-errors-list');
      var text = (le && le.querySelector('.dbg-error-row')) ? le.textContent : (document.getElementById('debug-stack-frames') || {}).textContent || '';
      if (navigator.clipboard) navigator.clipboard.writeText(text || '');
      showFsToast('Copied error');
    });
    var clr = document.getElementById('debug-errors-clear');
    if (clr) clr.addEventListener('click', function() {
      api('POST', '/api/debug/clear_errors').then(function() { renderDebugErrors(); }); // pat-j906k
    });
    // pat-j906k: Evaluator REPL.
    var evalRun = document.getElementById('dbg-eval-run');
    var evalInput = document.getElementById('dbg-eval-input');
    function doEval() {
      var expr = (evalInput.value || '').trim();
      if (!expr) return;
      api('POST', '/api/debug/evaluate', { expression: expr }).then(function(r) {
        var out = document.getElementById('dbg-eval-output');
        if (out && r) {
          var line = document.createElement('div');
          line.innerHTML = '<span style="color:var(--text-dim)">&gt; ' + escapeHtml(r.expression) + '</span> = <span style="color:var(--accent)">' + escapeHtml(r.result) + '</span>';
          out.appendChild(line); out.scrollTop = out.scrollHeight;
        }
        evalInput.value = '';
      });
    }
    if (evalRun) evalRun.addEventListener('click', doEval);
    if (evalInput) evalInput.addEventListener('keydown', function(e) { if (e.key === 'Enter') doEval(); });
  }

  // pat-j906k: populate a debugger sub-tab with live data on activation.
  function loadDebuggerSubtab(name) {
    if (name === 'errors') renderDebugErrors();
    else if (name === 'visual-profiler') renderVisualProfiler();
    else if (name === 'video-ram' || name === 'misc' || name === 'network-profiler') {
      renderDebugInfo();
      if (name === 'network-profiler') refreshNetActivateEnabled();
    }
  }
  function renderDebugErrors() {
    var list = document.getElementById('debug-errors-list');
    if (!list) return;
    api('GET', '/api/debug/errors').then(function(data) {
      var errs = (data && data.errors) || [];
      if (!errs.length) { list.innerHTML = '<div class="insp-empty" style="padding:8px">No errors.</div>'; return; }
      list.innerHTML = '';
      errs.forEach(function(e) {
        var row = document.createElement('div');
        row.className = 'dbg-error-row';
        row.style.cssText = 'padding:2px 6px;font-size:11px;display:flex;gap:6px;align-items:baseline;';
        var icon = e.level === 'warning' ? '⚠️' : '⛔';
        row.innerHTML = '<span>' + icon + '</span><span style="color:' + (e.level === 'warning' ? '#e0c050' : 'var(--error)') + '">' + escapeHtml(e.message) + '</span>';
        list.appendChild(row);
      });
    });
  }
  function renderVisualProfiler() {
    var canvas = document.getElementById('dbg-visual-canvas');
    var empty = document.getElementById('dbg-visual-empty');
    if (!canvas) return;
    api('GET', '/api/profiler').then(function(data) {
      var frames = (data && data.frames) || [];
      if (!frames.length) { if (empty) empty.style.display = ''; return; }
      if (empty) empty.style.display = 'none';
      var ctx = canvas.getContext('2d'); var w = canvas.width, h = canvas.height;
      ctx.clearRect(0, 0, w, h);
      var maxT = Math.max(16.6, data.max_ms || 16.6);
      var step = w / frames.length;
      frames.forEach(function(f, i) {
        var bh = (f.total_ms / maxT) * h;
        ctx.fillStyle = f.total_ms > 33.3 ? '#e05050' : (f.total_ms > 16.6 ? '#ffcc66' : '#8eef97');
        ctx.fillRect(i * step, h - bh, Math.max(2, step - 1), bh);
      });
    });
  }
  function renderDebugInfo() {
    api('GET', '/api/debug/info').then(function(d) {
      if (!d) return;
      // Video RAM.
      var vlist = document.getElementById('dbg-vram-list');
      var vtotal = document.getElementById('dbg-vram-total');
      if (vlist) {
        vlist.innerHTML = '';
        (d.video_ram || []).forEach(function(r) {
          var row = document.createElement('div');
          row.className = 'vram-row';
          var path = r.path || r.resource || '';
          row.innerHTML = '<span class="vrcol vrcol-path" title="' + escapeHtml(path) + '">' + escapeHtml(path) + '</span>' +
            '<span class="vrcol vrcol-type">' + escapeHtml(r.type || '') + '</span>' +
            '<span class="vrcol vrcol-format">' + escapeHtml(r.format || '—') + '</span>' +
            '<span class="vrcol vrcol-usage">' + fmtMonitor('bytes', r.usage_bytes || 0) + '</span>';
          vlist.appendChild(row);
        });
      }
      if (vtotal) vtotal.value = fmtMonitor('bytes', d.total_vram_bytes || 0);
      // Misc (Godot: clicked-control read-outs + Live Edit Root field).
      if (d.misc) {
        var cc = document.getElementById('dbg-clicked-ctrl');
        var cct = document.getElementById('dbg-clicked-ctrl-type');
        var ler = document.getElementById('dbg-live-edit-root');
        if (cc) cc.value = d.misc.clicked_control || '';
        if (cct) cct.value = d.misc.clicked_control_type || '';
        if (ler) ler.value = d.misc.live_edit_root || '/root';
        updateMiscButtons();
      }
      // Network Profiler (Godot EditorNetworkProfiler).
      if (d.network) renderNetworkProfiler(d.network);
    });
  }

  // ---- Network Profiler sub-tab (Godot EditorNetworkProfiler,
  //      editor_network_profiler.cpp) ----
  // Godot humanizes byte counts with 1024-based binary units (String::humanize_size,
  // ustring.cpp:4236): "0 B", "1.5 KiB", … We mirror that exactly so the bandwidth
  // readouts and per-RPC sizes read identically to the C++ editor.
  function humanizeSize(bytes) {
    bytes = bytes || 0;
    var div = 1, magnitude = 0;
    while (bytes > div * 1024 && magnitude < 6) { div *= 1024; magnitude++; }
    if (magnitude === 0) return Math.round(bytes) + ' B';
    // Godot's _humanize_digits (ustring.cpp:4226): 2 decimals < 100, 1 < 1024, else 0.
    var value = bytes / div;
    var digits = value < 100 ? 2 : (value < 1024 ? 1 : 0);
    var suffix = ['B', 'KiB', 'MiB', 'GiB', 'TiB', 'PiB', 'EiB'][magnitude];
    return value.toFixed(digits) + ' ' + suffix;
  }

  function renderNetworkProfiler(net) {
    // Bandwidth read-outs: "<humanized>/s", dimmed (50% alpha) when zero, like
    // Godot's set_bandwidth() font_uneditable_color override.
    var inc = net.incoming_bandwidth || 0, out = net.outgoing_bandwidth || 0;
    var incEl = document.getElementById('net-incoming');
    var outEl = document.getElementById('net-outgoing');
    if (incEl) { incEl.textContent = humanizeSize(inc) + '/s'; incEl.classList.toggle('net-bw-dim', inc <= 0); }
    if (outEl) { outEl.textContent = humanizeSize(out) + '/s'; outEl.classList.toggle('net-bw-dim', out <= 0); }

    // RPC counters tree: Node | Incoming RPC | Outgoing RPC. A count of 0 shows
    // "-", otherwise "<count> (<humanized avg size>)" (refresh_rpc_data()).
    var rpcs = net.rpcs || [];
    var rpcBody = document.getElementById('net-rpc-body');
    if (rpcBody) {
      if (!rpcs.length) {
        rpcBody.innerHTML = '<div class="net-empty">No RPCs recorded.</div>';
      } else {
        rpcBody.innerHTML = rpcs.map(function(r) {
          var incTxt = (r.incoming_rpc | 0) === 0 ? '-' : (r.incoming_rpc + ' (' + humanizeSize(r.incoming_size) + ')');
          var outTxt = (r.outgoing_rpc | 0) === 0 ? '-' : (r.outgoing_rpc + ' (' + humanizeSize(r.outgoing_size) + ')');
          return '<div class="net-row">' +
            '<span class="ncol ncol-node" title="' + escapeHtml(r.node_path || '') + '">' + escapeHtml(r.node_path || '') + '</span>' +
            '<span class="ncol ncol-num">' + escapeHtml(incTxt) + '</span>' +
            '<span class="ncol ncol-num">' + escapeHtml(outTxt) + '</span>' +
          '</div>';
        }).join('');
      }
    }

    // Replication tree: Root | Synchronizer | Config | Count | Size. Count and
    // Size are "<incoming> - <outgoing>" pairs (refresh_replication_data()).
    var syncs = net.syncs || [];
    var replBody = document.getElementById('net-repl-body');
    if (replBody) {
      if (!syncs.length) {
        replBody.innerHTML = '<div class="net-empty">No replication traffic.</div>';
      } else {
        replBody.innerHTML = syncs.map(function(sy) {
          // Godot's replication tree prints Count and Size as raw "%d - %d"
          // incoming-outgoing pairs (refresh_replication_data(), set_text 3 & 4) —
          // unlike the RPC tree, the Size column is NOT humanized.
          var count = (sy.incoming_syncs | 0) + ' - ' + (sy.outgoing_syncs | 0);
          var size = (sy.incoming_size | 0) + ' - ' + (sy.outgoing_size | 0);
          return '<div class="net-row">' +
            '<span class="ncol ncol-root" title="' + escapeHtml(sy.root_path || sy.root || '') + '"><span class="net-cell-icon"><span class="net-ic">&#9673;</span><span class="net-cell-text">' + escapeHtml(sy.root || '') + '</span></span></span>' +
            '<span class="ncol ncol-sync" title="' + escapeHtml(sy.synchronizer_path || sy.synchronizer || '') + '"><span class="net-cell-icon"><span class="net-ic">&#8644;</span><span class="net-cell-text">' + escapeHtml(sy.synchronizer || '') + '</span></span></span>' +
            '<span class="ncol ncol-config" title="' + escapeHtml(sy.config_path || sy.config || '') + '">' + escapeHtml(sy.config || '') + '</span>' +
            '<span class="ncol ncol-num ncol-count">' + escapeHtml(count) + '</span>' +
            '<span class="ncol ncol-num ncol-size">' + escapeHtml(size) + '</span>' +
          '</div>';
        }).join('');
      }
    }

    // Toolbar state: the Autostart checkbox reflects persisted state; Clear is
    // enabled once any data has been captured (Godot add_*_frame_data()).
    var auto = document.getElementById('net-autostart');
    if (auto && document.activeElement !== auto) auto.checked = !!net.autostart;
    var clr = document.getElementById('net-clear');
    if (clr) clr.disabled = !(rpcs.length || syncs.length);
    // Reflect the profiling (Start/Stop) state from the server.
    setNetActivateState(!!net.profiling);
  }

  // The Start button is enabled only while a game is running (Godot
  // EditorNetworkProfiler::started()/stopped() toggle activate->set_disabled()).
  function refreshNetActivateEnabled() {
    var act = document.getElementById('net-activate');
    if (!act) return;
    api('GET', '/api/runtime/status').then(function(s) {
      act.disabled = !(s && s.running);
    });
  }

  function setNetActivateState(on) {
    var act = document.getElementById('net-activate');
    if (!act) return;
    act.classList.toggle('active', on);
    act.innerHTML = on ? '&#9209; Stop' : '&#9654; Start';
  }

  // Mirror Godot EditorNetworkProfiler::started()/stopped()
  // (editor_network_profiler.cpp:211-225): the Start toggle is enabled only
  // while a game runs. Starting a run enables it and — when Autostart is
  // checked — begins profiling automatically; stopping disables it and resets
  // profiling back off. Driven off the live `runtimeRunning` flag (kept current
  // by play/stop/poll) so the button tracks the session without polling.
  var _netRunSynced = false;
  function syncNetProfilerRunState() {
    var act = document.getElementById('net-activate');
    if (!act) return;
    if (runtimeRunning === _netRunSynced) { act.disabled = !runtimeRunning; return; }
    _netRunSynced = runtimeRunning;
    if (runtimeRunning) {
      act.disabled = false; // started()
      var auto = document.getElementById('net-autostart');
      if (auto && auto.checked && !act.classList.contains('active')) {
        setNetActivateState(true);
        api('POST', '/api/debug/network_profiler', { enable: true });
      }
    } else {
      act.disabled = true; // stopped() -> set_profiling(false)
      if (act.classList.contains('active')) {
        setNetActivateState(false);
        api('POST', '/api/debug/network_profiler', { enable: false });
      }
    }
  }

  function setupNetworkProfilerTab() {
    var act = document.getElementById('net-activate');
    if (act) act.addEventListener('click', function() {
      var on = !act.classList.contains('active');
      setNetActivateState(on);
      api('POST', '/api/debug/network_profiler', { enable: on });
    });
    var clr = document.getElementById('net-clear');
    if (clr) clr.addEventListener('click', function() {
      api('POST', '/api/debug/network_clear', {}).then(function() {
        var rb = document.getElementById('net-rpc-body');
        if (rb) rb.innerHTML = '<div class="net-empty">No RPCs recorded.</div>';
        var pb = document.getElementById('net-repl-body');
        if (pb) pb.innerHTML = '<div class="net-empty">No replication traffic.</div>';
        var incEl = document.getElementById('net-incoming');
        var outEl = document.getElementById('net-outgoing');
        if (incEl) { incEl.textContent = '0 B/s'; incEl.classList.add('net-bw-dim'); }
        if (outEl) { outEl.textContent = '0 B/s'; outEl.classList.add('net-bw-dim'); }
        clr.disabled = true;
      });
    });
    var auto = document.getElementById('net-autostart');
    if (auto) auto.addEventListener('change', function() {
      api('POST', '/api/debug/network_autostart', { enabled: auto.checked });
    });
  }

  // ---- Video RAM tab toolbar (Godot vmem: Refresh + Export CSV) ----
  function setupVramTab() {
    var rf = document.getElementById('dbg-vram-refresh');
    if (rf) rf.addEventListener('click', function() { renderDebugInfo(); });
    var ex = document.getElementById('dbg-vram-export');
    if (ex) ex.addEventListener('click', function() {
      var rows = document.querySelectorAll('#dbg-vram-list .vram-row');
      var csv = 'Resource Path,Type,Format,Usage\n';
      rows.forEach(function(r) {
        var c = r.querySelectorAll('.vrcol');
        csv += [0, 1, 2, 3].map(function(i) { return '"' + (c[i] ? c[i].textContent.replace(/"/g, '""') : '') + '"'; }).join(',') + '\n';
      });
      if (navigator.clipboard) navigator.clipboard.writeText(csv);
      if (typeof logMessage === 'function') logMessage('info', 'Video RAM list copied as CSV (' + rows.length + ' rows)');
    });
  }

  // ---- Misc tab (Godot ScriptEditorDebugger misc: Clicked Control read-outs,
  //      Live Edit Root + Set From Tree / Clear, Export measures as CSV) ----
  function updateMiscButtons() {
    var setBtn = document.getElementById('dbg-le-set');
    var clrBtn = document.getElementById('dbg-le-clear');
    var ler = document.getElementById('dbg-live-edit-root');
    // le_set is enabled when a scene-tree node is selected (Godot uses the
    // remote tree selection); le_clear when the root differs from "/root".
    if (setBtn) setBtn.disabled = (typeof selectedNodeId === 'undefined' || selectedNodeId === null);
    if (clrBtn) clrBtn.disabled = !ler || (ler.value || '/root') === '/root';
  }
  function csvCell(v) {
    var s = String(v);
    return /[",\n]/.test(s) ? '"' + s.replace(/"/g, '""') + '"' : s;
  }
  function downloadCsv(filename, csv) {
    try {
      var blob = new Blob([csv], { type: 'text/csv' });
      var url = URL.createObjectURL(blob);
      var a = document.createElement('a');
      a.href = url; a.download = filename;
      document.body.appendChild(a); a.click(); document.body.removeChild(a);
      setTimeout(function() { URL.revokeObjectURL(url); }, 0);
    } catch (e) {
      if (navigator.clipboard) navigator.clipboard.writeText(csv);
    }
  }
  // Godot _export_csv: one column per performance monitor, a header row of
  // monitor names, then one row per captured sample (most recent first).
  function exportMeasuresCsv() {
    var cols = [];
    MONITOR_SPEC.forEach(function(g) { g.items.forEach(function(m) { cols.push(m.key); }); });
    var maxLen = 0;
    cols.forEach(function(k) { var h = monitorHistory[k] || []; if (h.length > maxLen) maxLen = h.length; });
    var lines = [cols.map(csvCell).join(',')];
    for (var r = 0; r < maxLen; r++) {
      var row = cols.map(function(k) {
        var h = monitorHistory[k] || [];
        var idx = h.length - 1 - r;
        return idx >= 0 ? csvCell(h[idx]) : '';
      });
      lines.push(row.join(','));
    }
    downloadCsv('profile.csv', lines.join('\n') + '\n');
    if (typeof logMessage === 'function') logMessage('info', 'Exported ' + maxLen + ' monitor measure(s) as CSV');
  }
  function setupMiscTab() {
    var setBtn = document.getElementById('dbg-le-set');
    var clrBtn = document.getElementById('dbg-le-clear');
    var exp = document.getElementById('dbg-export-csv');
    if (setBtn) setBtn.addEventListener('click', function() {
      var path = (selectedNodeData && selectedNodeData.path) || null;
      if (!path) { if (typeof logMessage === 'function') logMessage('warning', 'Select a scene-tree node to set the Live Edit Root'); return; }
      api('POST', '/api/debug/live_edit_root', { path: path }).then(function(rsp) {
        var ler = document.getElementById('dbg-live-edit-root');
        if (ler) ler.value = (rsp && rsp.live_edit_root) || path;
        updateMiscButtons();
      });
    });
    if (clrBtn) clrBtn.addEventListener('click', function() {
      api('POST', '/api/debug/live_edit_root', { path: '' }).then(function(rsp) {
        var ler = document.getElementById('dbg-live-edit-root');
        if (ler) ler.value = (rsp && rsp.live_edit_root) || '/root';
        updateMiscButtons();
      });
    });
    if (exp) exp.addEventListener('click', exportMeasuresCsv);
    updateMiscButtons();
  }

  // ---- Scene / Import dock tabs (Godot parity: left dock is Scene | Import) ----
  function setupSceneDockTabs() {
    var sceneTab = document.getElementById('scene-dock-tab');
    var importTab = document.getElementById('import-dock-tab');
    if (sceneTab) sceneTab.addEventListener('click', function() { showSceneDockTab('scene'); });
    if (importTab) importTab.addEventListener('click', function() { showSceneDockTab('import'); });
  }

  // pat-h5s93: the resource the Import dock is editing + whether it is shown.
  var currentImportResource = null;
  var importDockActive = false;

  // Populate the Import dock from the selected resource's importer, presets, and
  // options (Godot ImportDock _update_options). No/unimportable resource shows
  // the empty-state.
  async function renderImportDock() {
    var empty = document.getElementById('import-empty');
    var content = document.getElementById('import-content');
    if (!empty || !content) return;
    if (!currentImportResource) {
      empty.textContent = 'Select a resource file in the FileSystem dock to import it.';
      empty.style.display = ''; content.style.display = 'none';
      return;
    }
    var info = await api('GET', '/api/import_settings?path=' + encodeURIComponent(currentImportResource));
    if (!info || !info.importable) {
      empty.textContent = currentImportResource.split('/').pop() + ' is not an imported resource.';
      empty.style.display = ''; content.style.display = 'none';
      return;
    }
    empty.style.display = 'none'; content.style.display = '';
    // Import As: dropdown of compatible importers.
    var importAs = document.getElementById('import-as');
    importAs.innerHTML = '';
    (info.importers && info.importers.length ? info.importers : [info.importer]).forEach(function(name) {
      var o = document.createElement('option'); o.value = name; o.textContent = name; importAs.appendChild(o);
    });
    importAs.value = info.importer; importAs.disabled = false;
    // Preset menu.
    var presetBtn = document.getElementById('import-preset');
    presetBtn.textContent = 'Preset'; presetBtn.disabled = false;
    presetBtn.onclick = function(ev) {
      ev.stopPropagation();
      var old = document.getElementById('simple-context-menu'); if (old) old.remove();
      var r = presetBtn.getBoundingClientRect();
      showSimpleContextMenu(Math.round(r.left), Math.round(r.bottom + 2), (info.presets || []).map(function(p) {
        return { label: p, act: function() { presetBtn.textContent = p; showFsToast('Preset: ' + p); } };
      }));
    };
    // Editable import options.
    var optHost = document.getElementById('import-options');
    optHost.innerHTML = '';
    (info.options || []).forEach(function(op) {
      var row = document.createElement('div'); row.className = 'import-row import-opt-row';
      var label = document.createElement('span'); label.className = 'import-label'; label.textContent = op.label || op.name;
      var ctrl;
      if (op.type === 'bool') {
        ctrl = document.createElement('input'); ctrl.type = 'checkbox'; ctrl.checked = !!op.value;
      } else if (op.type === 'enum') {
        ctrl = document.createElement('select'); ctrl.className = 'import-select';
        (op.choices || []).forEach(function(c) { var o = document.createElement('option'); o.value = c; o.textContent = c; ctrl.appendChild(o); });
        ctrl.value = op.value;
      } else if (op.type === 'int' || op.type === 'float') {
        ctrl = document.createElement('input'); ctrl.type = 'number'; ctrl.value = op.value; ctrl.className = 'import-input';
        if (op.type === 'float') ctrl.step = '0.05';
      } else {
        ctrl = document.createElement('input'); ctrl.type = 'text'; ctrl.value = op.value; ctrl.className = 'import-input';
      }
      ctrl.setAttribute('data-opt', op.name);
      ctrl.addEventListener('change', function() {
        var val = (op.type === 'bool') ? ctrl.checked : ctrl.value;
        api('POST', '/api/import_settings', { path: currentImportResource, option: op.name, value: val });
      });
      row.appendChild(label); row.appendChild(ctrl); optHost.appendChild(row);
    });
    // Reimport button.
    var reBtn = document.getElementById('import-reimport');
    reBtn.disabled = false;
    reBtn.onclick = function() {
      api('POST', '/api/reimport', { path: currentImportResource }).then(function(resp) {
        if (resp && resp.reimported) showFsToast('Reimported ' + resp.reimported.split('/').pop());
      });
    };
  }

  function showSceneDockTab(which) {
    var importActive = (which === 'import');
    importDockActive = importActive;
    var sceneTab = document.getElementById('scene-dock-tab');
    var importTab = document.getElementById('import-dock-tab');
    var search = document.getElementById('scene-search');
    var toolbar = document.getElementById('scene-tree-toolbar');
    var tree = document.getElementById('scene-tree');
    var dock = document.getElementById('import-dock');
    if (sceneTab) sceneTab.classList.toggle('active', !importActive);
    if (importTab) importTab.classList.toggle('active', importActive);
    if (search) search.style.display = importActive ? 'none' : '';
    if (toolbar) toolbar.style.display = importActive ? 'none' : '';
    if (tree) tree.style.display = importActive ? 'none' : '';
    if (dock) dock.style.display = importActive ? 'flex' : 'none';
    if (importActive) renderImportDock();
  }

  function setupHistoryDock() {
    var fsTab = document.getElementById('fs-dock-tab');
    var histTab = document.getElementById('history-dock-tab');
    if (fsTab) fsTab.addEventListener('click', function() { showFsDockTab('filesystem'); });
    if (histTab) histTab.addEventListener('click', function() { showFsDockTab('history'); });
    var sc = document.getElementById('history-scene');
    var gl = document.getElementById('history-global');
    if (sc) sc.addEventListener('change', renderHistory);
    if (gl) gl.addEventListener('change', renderHistory);
  }

  function showFsDockTab(which) {
    historyDockActive = (which === 'history');
    var fsTab = document.getElementById('fs-dock-tab');
    var histTab = document.getElementById('history-dock-tab');
    var fsChrome = document.getElementById('fs-chrome');
    var histDock = document.getElementById('history-dock');
    if (fsTab) fsTab.classList.toggle('active', !historyDockActive);
    if (histTab) histTab.classList.toggle('active', historyDockActive);
    // The whole FileSystem chrome (toolbars + favorites + tree + preview) shows
    // on the FileSystem tab; the history dock shows on the History tab.
    if (fsChrome) fsChrome.style.display = historyDockActive ? 'none' : 'flex';
    if (histDock) histDock.style.display = historyDockActive ? 'flex' : 'none';
    if (historyDockActive) refreshHistoryDock();
  }

  async function refreshHistoryDock() {
    if (!historyDockActive) return;
    var data = await api('GET', '/api/history');
    if (data) {
      historyData = data;
      renderHistory();
    }
  }

  function renderHistory() {
    var list = document.getElementById('history-list');
    if (!list || !historyData) return;
    var sc = document.getElementById('history-scene');
    var gl = document.getElementById('history-global');
    var showScene = sc ? sc.checked : true;
    var showGlobal = gl ? gl.checked : true;
    list.innerHTML = '';
    var entries = historyData.entries || [];
    for (var i = 0; i < entries.length; i++) {
      var e = entries[i];
      // "The Beginning" (scope 'all') is always shown as the timeline anchor;
      // action rows are filtered by their scope via the checkboxes.
      if (e.scope === 'scene' && !showScene) continue;
      if (e.scope === 'global' && !showGlobal) continue;
      var row = document.createElement('div');
      row.className = 'history-entry' + (e.current ? ' current' : '') +
        (e.future ? ' future' : '') + (e.version === 0 ? ' beginning' : '');
      var icon = document.createElement('span');
      icon.className = 'history-icon';
      icon.textContent = e.current ? '▶' : '';
      var label = document.createElement('span');
      label.className = 'history-label';
      label.textContent = e.label;
      row.appendChild(icon);
      row.appendChild(label);
      (function(version) {
        row.addEventListener('click', function() { gotoHistory(version); });
      })(e.version);
      list.appendChild(row);
    }
  }

  async function gotoHistory(version) {
    if (!historyData || version === historyData.current) return;
    await api('POST', '/api/history/goto', { version: version });
    await refreshHistoryDock();
    // Reflect the jumped-to state everywhere the edit affects.
    if (typeof fetchScene === 'function') fetchScene();
    if (typeof fetchSelected === 'function') fetchSelected();
    if (typeof refreshViewport === 'function') refreshViewport();
  }

  // ---- File preview ----
  var selectedFsPath = null;

  async function showFilePreview(entry) {
    // pat-h5s93: track the selected resource for the Import dock and refresh it
    // live when that tab is showing.
    currentImportResource = (entry && !entry.is_dir) ? entry.path : null;
    if (importDockActive) renderImportDock();
    // pat-bc0tv: selecting a shader opens it in the Shader Editor bottom panel.
    if (entry && !entry.is_dir && /\.(gdshader|shader|glsl)$/i.test(entry.name)) {
      loadShaderFile(entry.path);
    }
    var panel = document.getElementById('fs-preview');
    if (!entry || entry.is_dir) {
      panel.className = '';
      panel.innerHTML = '';
      selectedFsPath = null;
      return;
    }
    selectedFsPath = entry.path;
    var ext = entry.name.split('.').pop().toLowerCase();
    var isImage = ['png', 'jpg', 'jpeg', 'webp', 'svg'].indexOf(ext) >= 0;

    if (isImage) {
      // Show image preview directly via the preview endpoint
      var url = '/api/preview/file?path=' + encodeURIComponent(entry.path);
      panel.innerHTML = '<div class="preview-header">' + entry.name + '</div>' +
        '<img class="preview-img" src="' + url + '" alt="' + entry.name + '">';
      panel.className = 'active';
    } else {
      // Fetch structured preview data
      var data = await api('GET', '/api/preview/file?path=' + encodeURIComponent(entry.path));
      if (!data || data.error) {
        panel.className = '';
        panel.innerHTML = '';
        return;
      }

      var html = '<div class="preview-header">' + entry.name + '</div>';

      if (data.type === 'script') {
        html += '<div class="preview-code">' + escapeHtml(data.preview || '') + '</div>';
        html += '<div class="preview-info"><span>Lines:</span> ' + (data.lines || 0) + '</div>';
      } else if (data.type === 'scene') {
        html += '<div class="preview-info">';
        html += '<span>Root:</span> ' + (data.root_type || 'Node') + '<br>';
        html += '<span>Nodes:</span> ' + (data.node_count || 0) + '<br>';
        if (data.ext_resources) html += '<span>Ext resources:</span> ' + data.ext_resources + '<br>';
        if (data.sub_resources) html += '<span>Sub resources:</span> ' + data.sub_resources;
        html += '</div>';
      } else if (data.type === 'resource') {
        html += '<div class="preview-info">';
        html += '<span>Type:</span> ' + (data.resource_type || 'Resource') + '<br>';
        if (data.sub_resources) html += '<span>Sub resources:</span> ' + data.sub_resources + '<br>';
        var sz = data.size || 0;
        html += '<span>Size:</span> ' + (sz < 1024 ? sz + ' B' : (sz / 1024).toFixed(1) + ' KB');
        html += '</div>';
      } else if (data.type === 'config') {
        html += '<div class="preview-code">' + escapeHtml(data.preview || '') + '</div>';
      }

      panel.innerHTML = html;
      panel.className = 'active';
    }
  }

  function escapeHtml(s) {
    return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  }

  // ---- Left panel divider resize ----
  function setupLeftDivider() {
    var divider = document.getElementById('left-divider');
    var scenePanel = document.getElementById('scene-panel');
    var fsPanel = document.getElementById('filesystem-panel');
    var isResizing = false;
    var startY = 0;
    var startSceneH = 0;

    divider.addEventListener('mousedown', function(e) {
      isResizing = true;
      startY = e.clientY;
      startSceneH = scenePanel.offsetHeight;
      e.preventDefault();
    });
    document.addEventListener('mousemove', function(e) {
      if (!isResizing) return;
      var delta = e.clientY - startY;
      var newH = Math.max(80, startSceneH + delta);
      scenePanel.style.flex = 'none';
      scenePanel.style.height = newH + 'px';
      fsPanel.style.flex = '1';
    });
    document.addEventListener('mouseup', function() { isResizing = false; });
  }

  // ---- Inspector panel resize (drag the left edge, Godot-style) ----
  function setupInspectorDivider() {
    var divider = document.getElementById('inspector-divider');
    var panel = document.getElementById('inspector-panel');
    if (!divider || !panel) return;
    try {
      var saved = parseInt(localStorage.getItem('patina-inspector-width'));
      if (saved && saved >= 220) panel.style.width = saved + 'px';
    } catch (e) {}
    var resizing = false;
    divider.addEventListener('mousedown', function(e) {
      resizing = true; e.preventDefault();
      document.body.style.cursor = 'ew-resize';
      document.body.style.userSelect = 'none';
    });
    document.addEventListener('mousemove', function(e) {
      if (!resizing) return;
      var newW = window.innerWidth - e.clientX;
      newW = Math.max(220, Math.min(newW, window.innerWidth - 360));
      panel.style.width = newW + 'px';
    });
    document.addEventListener('mouseup', function() {
      if (!resizing) return;
      resizing = false;
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
      try { localStorage.setItem('patina-inspector-width', parseInt(panel.style.width)); } catch (e) {}
    });
  }

  // ---- Scene tabs (multi-tab) ----
  var sceneTabs = [];
  var activeTabIndex = 0;
  var currentScenePath = '';

  async function fetchSceneTabs() {
    var data = await api('GET', '/api/scene/tabs');
    if (data && data.tabs) {
      sceneTabs = data.tabs;
      activeTabIndex = data.active_tab_index || 0;
      var active = sceneTabs[activeTabIndex];
      if (active && active.path) currentScenePath = active.path;
      renderSceneTabs();
    }
  }

  function renderSceneTabs() {
    var container = document.getElementById('scene-tabs');
    if (!container) return;
    container.innerHTML = '';
    for (var i = 0; i < sceneTabs.length; i++) {
      var t = sceneTabs[i];
      var tab = document.createElement('div');
      tab.className = 'scene-tab' + (i === activeTabIndex ? ' active' : '');
      tab.setAttribute('data-tab-id', t.id);
      tab.setAttribute('data-tab-index', i);
      var nameSpan = '<span class="scene-tab-name">' + escapeHtml(t.name) + (t.modified ? ' *' : '') + '</span>';
      var closeSpan = '<span class="scene-tab-close" title="Close tab">&times;</span>';
      tab.innerHTML = nameSpan + closeSpan;
      tab.addEventListener('click', (function(tabId) {
        return function(e) {
          if (e.target.classList.contains('scene-tab-close')) return;
          api('POST', '/api/scene/tabs/switch', { tab_id: tabId }).then(fetchSceneTabs);
        };
      })(t.id));
      var closeBtn = tab.querySelector('.scene-tab-close');
      if (closeBtn) {
        closeBtn.addEventListener('click', (function(tabId, tabName, modified) {
          return function(e) {
            e.stopPropagation();
            // Godot lets you close the last tab too (it opens a fresh empty
            // scene). Route through the shared close path so the unsaved-change
            // prompt and scene refresh match the Scene > Close Scene menu.
            closeSceneTab(tabId, tabName, modified);
          };
        })(t.id, t.name, !!t.modified));
      }
      container.appendChild(tab);
    }
  }

  function updateSceneTab() {
    fetchSceneTabs();
  }

  // ---- Runtime state ----
  var runtimeRunning = false, runtimePaused = false, runtimeFrameCount = 0, runtimeFps = 0, runtimeStatusInterval = null;
  // Godot game_view_plugin.cpp `screen_index_before_start`: the main-screen mode
  // active before Play, so Stop can return to it. null when not launched from a
  // non-game view.
  var prePlayMode = null;

  function updatePlayButtonStates() {
    var bp = document.getElementById('btn-play'), bpa = document.getElementById('btn-pause'), bs = document.getElementById('btn-stop');
    if (runtimeRunning) {
      bp.style.borderColor = 'var(--accent)'; bp.style.background = 'rgba(80,200,120,0.15)';
      bs.style.borderColor = '#e05050';
      bpa.style.borderColor = runtimePaused ? 'var(--accent)' : ''; bpa.style.background = runtimePaused ? 'rgba(224,192,80,0.15)' : '';
    } else { bp.style.borderColor = ''; bp.style.background = ''; bpa.style.borderColor = ''; bpa.style.background = ''; bs.style.borderColor = ''; }
    syncNetProfilerRunState(); // Godot started()/stopped(): Start toggle follows the run state.
  }
  function showPlayingOverlay(show) {
    var o = document.getElementById('runtime-overlay');
    if (show) {
      if (!o) { o = document.createElement('div'); o.id = 'runtime-overlay'; o.style.cssText = 'position:absolute;top:8px;left:50%;transform:translateX(-50%);background:rgba(80,200,120,0.85);color:#000;padding:3px 14px;border-radius:3px;font-size:12px;font-weight:bold;z-index:20;letter-spacing:1px;'; var c = document.getElementById('viewport-container'); if (c) { c.style.position = 'relative'; c.appendChild(o); } }
      o.textContent = runtimePaused ? 'PAUSED' : 'PLAYING'; o.style.background = runtimePaused ? 'rgba(224,192,80,0.85)' : 'rgba(80,200,120,0.85)';
    } else { if (o) o.remove(); }
  }
  function updateRuntimeStatusBar() {
    var sb = document.getElementById('statusbar'), rs = document.getElementById('status-runtime');
    if (runtimeRunning) {
      if (!rs) { rs = document.createElement('span'); rs.id = 'status-runtime'; sb.appendChild(rs); }
      rs.innerHTML = 'Frame: <span class="accent">' + runtimeFrameCount + '</span> | FPS: <span class="accent">' + runtimeFps.toFixed(0) + '</span>';
    } else { if (rs) rs.remove(); }
  }
  function setRuntimeEditingDisabled(d) {
    var sp = document.getElementById('scene-panel'), ip = document.getElementById('inspector-panel');
    if (d) { [sp, ip].forEach(function(p) { if (!p) return; if (!p.querySelector('.runtime-edit-msg')) { var m = document.createElement('div'); m.className = 'runtime-edit-msg'; m.style.cssText = 'padding:12px;color:var(--text-dim);font-size:12px;text-align:center;background:rgba(0,0,0,0.3);'; m.textContent = 'Stop the scene to edit'; p.appendChild(m); } }); }
    else { document.querySelectorAll('.runtime-edit-msg').forEach(function(e) { e.remove(); }); }
  }
  var _runtimeStatusInflight = false;
  async function pollRuntimeStatus() {
    if (!runtimeRunning) return;
    // pat-vl1mk: single-in-flight — at 200ms this is the fastest poll, so never
    // let a second status request start while the previous is outstanding.
    if (_runtimeStatusInflight) return;
    _runtimeStatusInflight = true;
    try {
      var r = await api('GET', '/api/runtime/status');
      if (r) { runtimeRunning = r.running; runtimePaused = r.paused; runtimeFrameCount = r.frame_count; runtimeFps = r.fps; updatePlayButtonStates(); showPlayingOverlay(runtimeRunning); updateRuntimeStatusBar(); if (!runtimeRunning) { restoreViewAfterStop(); setRuntimeEditingDisabled(false); stopRuntimePolling(); stopGameInput(); } }
    } finally { _runtimeStatusInflight = false; }
  }
  function startRuntimePolling() { if (runtimeStatusInterval) return; runtimeStatusInterval = setInterval(pollRuntimeStatus, 200); }
  function stopRuntimePolling() { if (runtimeStatusInterval) { clearInterval(runtimeStatusInterval); runtimeStatusInterval = null; } }

  // ---- Play buttons ----
  // Godot game_view_plugin.cpp:459-491 (GameView::_play_pressed): launching the
  // scene switches the editor main screen to the Game view so the running scene
  // renders where the editor viewport was, remembering the screen we came from
  // (screen_index_before_start) to restore on stop. The server already set
  // editor_mode="game" (editor_server.rs api_runtime_play); this mirrors that
  // switch in the browser UI so the game is actually shown — without it the
  // editor stays on the 2D/3D viewport and the run is invisible.
  function enterGameViewOnPlay() {
    if (editorMode !== 'game') prePlayMode = editorMode;
    applyMode('game');
  }
  // Godot game_view_plugin.cpp:493-515 (GameView::_stop_pressed): stop returns to
  // the screen that was active before play.
  function restoreViewAfterStop() {
    if (prePlayMode !== null) { applyMode(prePlayMode); prePlayMode = null; }
  }
  function setupPlayButtons() {
    // pat-j8sfr: renderer selector dropdown. Godot requires an editor restart to
    // change the renderer, so selecting one logs + notes that, then reverts the
    // visible selection to the active renderer (Forward+).
    var rendererSel = document.getElementById('renderer-select');
    if (rendererSel) rendererSel.addEventListener('change', function() {
      var label = this.options[this.selectedIndex].textContent;
      logMessage('info', 'Rendering method: ' + label);
      if (typeof showFsToast === 'function') showFsToast('Changing the renderer requires restarting the editor.');
      this.value = 'forward_plus';
    });
    document.getElementById('btn-play').addEventListener('click', async function() {
      if (runtimeRunning) return;
      var r = await api('POST', '/api/runtime/play');
      if (r && r.ok) { runtimeRunning = true; runtimePaused = false; runtimeFrameCount = 0; logMessage('info', 'Play started (F5)'); enterGameViewOnPlay(); updatePlayButtonStates(); showPlayingOverlay(true); updateRuntimeStatusBar(); setRuntimeEditingDisabled(true); startRuntimePolling(); startGameInput(); }
    });
    document.getElementById('btn-pause').addEventListener('click', async function() {
      if (!runtimeRunning) return;
      var r = await api('POST', '/api/runtime/pause');
      if (r && r.ok) { runtimePaused = r.paused; logMessage('info', runtimePaused ? 'Paused (F7)' : 'Resumed (F7)'); updatePlayButtonStates(); showPlayingOverlay(true); }
    });
    document.getElementById('btn-stop').addEventListener('click', async function() {
      var r = await api('POST', '/api/runtime/stop');
      if (r && r.ok) { runtimeRunning = false; runtimePaused = false; runtimeFrameCount = 0; logMessage('info', 'Stopped (F8)'); restoreViewAfterStop(); updatePlayButtonStates(); showPlayingOverlay(false); updateRuntimeStatusBar(); setRuntimeEditingDisabled(false); stopRuntimePolling(); stopGameInput(); }
    });
    document.getElementById('btn-play-current').addEventListener('click', async function() {
      if (runtimeRunning) return;
      var r = await api('POST', '/api/runtime/play');
      if (r && r.ok) { runtimeRunning = true; runtimePaused = false; runtimeFrameCount = 0; logMessage('info', 'Play Current Scene (F6)'); enterGameViewOnPlay(); updatePlayButtonStates(); showPlayingOverlay(true); updateRuntimeStatusBar(); setRuntimeEditingDisabled(true); startRuntimePolling(); startGameInput(); }
    });
    // pat-12qzs: Play Custom Scene — pick a scene file and run it (Godot's
    // play_custom_scene_button). Defaults to the selected resource if it's a
    // .tscn; opens it, then plays.
    var playCustom = document.getElementById('btn-play-custom');
    if (playCustom) playCustom.addEventListener('click', async function() {
      if (runtimeRunning) return;
      var def = (currentImportResource && /\.tscn$/i.test(currentImportResource)) ? currentImportResource : '';
      var scene = prompt('Play a custom scene (res:// path):', def);
      if (!scene || !scene.trim()) return;
      if (typeof doOpenScene === 'function') { await doOpenScene(scene.trim()); }
      var r = await api('POST', '/api/runtime/play');
      if (r && r.ok) { runtimeRunning = true; runtimePaused = false; runtimeFrameCount = 0; logMessage('info', 'Play Custom Scene: ' + scene.trim()); enterGameViewOnPlay(); updatePlayButtonStates(); showPlayingOverlay(true); updateRuntimeStatusBar(); setRuntimeEditingDisabled(true); startRuntimePolling(); startGameInput(); }
    });
    // pat-uqj6w: Movie Maker mode toggle. When enabled, a play records the
    // run to a video file at stable FPS (Godot write_movie_button). Here it
    // toggles the mode flag + button state.
    var movieBtn = document.getElementById('btn-movie-maker');
    if (movieBtn) movieBtn.addEventListener('click', function() {
      movieMakerEnabled = !movieMakerEnabled;
      movieBtn.classList.toggle('active', movieMakerEnabled);
      logMessage('info', 'Movie Maker mode ' + (movieMakerEnabled ? 'enabled' : 'disabled'));
    });
  }
  var movieMakerEnabled = false;

  // ---- Game Input Capture ----
  var gameInputActive = false;
  var gameInputKeydownHandler = null;
  var gameInputKeyupHandler = null;
  var gameInputMousemoveHandler = null;
  var gameInputMousedownHandler = null;
  var gameInputMouseupHandler = null;
  var GAME_KEYS = new Set(['ArrowLeft','ArrowRight','ArrowUp','ArrowDown',' ','Tab']);

  function startGameInput() {
    if (gameInputActive) return;
    gameInputActive = true;
    gameInputKeydownHandler = function(e) {
      if (!runtimeRunning) return;
      if (e.key === 'F5' || e.key === 'F6' || e.key === 'F7' || e.key === 'F8') return;
      var tag = document.activeElement ? document.activeElement.tagName : '';
      if ((tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') && !GAME_KEYS.has(e.key)) return;
      if (GAME_KEYS.has(e.key)) e.preventDefault();
      api('POST', '/api/runtime/input/key_down', { key: e.key });
    };
    gameInputKeyupHandler = function(e) {
      if (!runtimeRunning) return;
      if (e.key === 'F5' || e.key === 'F6' || e.key === 'F7' || e.key === 'F8') return;
      api('POST', '/api/runtime/input/key_up', { key: e.key });
    };
    document.addEventListener('keydown', gameInputKeydownHandler, true);
    document.addEventListener('keyup', gameInputKeyupHandler, true);
    var vp = document.getElementById('viewport-container');
    if (vp) {
      gameInputMousemoveHandler = function(e) {
        if (!runtimeRunning) return;
        var rect = vp.getBoundingClientRect();
        api('POST', '/api/runtime/input/mouse_move', { x: e.clientX - rect.left, y: e.clientY - rect.top });
      };
      gameInputMousedownHandler = function(e) {
        if (!runtimeRunning) return;
        api('POST', '/api/runtime/input/mouse_down', { button: e.button });
      };
      gameInputMouseupHandler = function(e) {
        if (!runtimeRunning) return;
        api('POST', '/api/runtime/input/mouse_up', { button: e.button });
      };
      vp.addEventListener('mousemove', gameInputMousemoveHandler);
      vp.addEventListener('mousedown', gameInputMousedownHandler);
      vp.addEventListener('mouseup', gameInputMouseupHandler);
    }
    showGameInputIndicator(true);
  }

  function stopGameInput() {
    if (!gameInputActive) return;
    gameInputActive = false;
    if (gameInputKeydownHandler) { document.removeEventListener('keydown', gameInputKeydownHandler, true); gameInputKeydownHandler = null; }
    if (gameInputKeyupHandler) { document.removeEventListener('keyup', gameInputKeyupHandler, true); gameInputKeyupHandler = null; }
    var vp = document.getElementById('viewport-container');
    if (vp) {
      if (gameInputMousemoveHandler) { vp.removeEventListener('mousemove', gameInputMousemoveHandler); gameInputMousemoveHandler = null; }
      if (gameInputMousedownHandler) { vp.removeEventListener('mousedown', gameInputMousedownHandler); gameInputMousedownHandler = null; }
      if (gameInputMouseupHandler) { vp.removeEventListener('mouseup', gameInputMouseupHandler); gameInputMouseupHandler = null; }
    }
    showGameInputIndicator(false);
  }

  function showGameInputIndicator(show) {
    var existing = document.getElementById('game-input-indicator');
    if (show) {
      if (!existing) {
        var ind = document.createElement('div');
        ind.id = 'game-input-indicator';
        ind.style.cssText = 'position:absolute;bottom:8px;left:50%;transform:translateX(-50%);background:rgba(80,160,255,0.85);color:#fff;padding:2px 10px;border-radius:3px;font-size:11px;font-weight:bold;z-index:20;letter-spacing:0.5px;pointer-events:none;';
        ind.textContent = 'GAME INPUT ACTIVE';
        var c = document.getElementById('viewport-container');
        if (c) { c.style.position = 'relative'; c.appendChild(ind); }
      }
      var vc = document.getElementById('viewport-container');
      if (vc) vc.style.boxShadow = 'inset 0 0 0 2px rgba(80,160,255,0.6)';
    } else {
      if (existing) existing.remove();
      var vc = document.getElementById('viewport-container');
      if (vc) vc.style.boxShadow = '';
    }
  }

  function logMessage(level, message) {
    // Add a log entry to the output panel directly
    var logEl = document.getElementById('output-log');
    var div = document.createElement('div');
    div.className = 'log-entry' + (level === 'warn' ? ' log-warn' : '') + (level === 'error' ? ' log-error' : '');
    div.setAttribute('data-cat', logCat(level));
    var time = new Date();
    var timeStr = time.toLocaleTimeString();
    div.innerHTML = '<span class="log-time">[' + escapeHtml(timeStr) + ']</span><span class="log-msg">' + escapeHtml(message) + '</span>';
    logEl.insertBefore(div, logEl.firstChild);
    // pat-rjd: Cap log entries to prevent unbounded DOM growth
    while (logEl.children.length > 200) { logEl.removeChild(logEl.lastChild); }
    if (typeof updateOutputCounts === 'function') { updateOutputCounts(); applyOutputFilters(); }
  }

  // ---- Toolbar actions ----
  function setupToolbar() {
    var btnAdd = document.getElementById('btn-add');
    btnAdd.addEventListener('click', function(e) { e.stopPropagation(); openAddNodeDialog(); });

    // pat-l695a: scene-tree dock permanent toolbar buttons.
    var stAdd = document.getElementById('st-add-child');
    if (stAdd) stAdd.addEventListener('click', function(e) { e.stopPropagation(); openAddNodeDialog(); });
    var stInst = document.getElementById('st-instantiate');
    if (stInst) stInst.addEventListener('click', function(e) { e.stopPropagation(); doInstanceScene(selectedNodeId); });
    var stAttach = document.getElementById('st-attach-script');
    if (stAttach) stAttach.addEventListener('click', function(e) {
      e.stopPropagation();
      if (selectedNodeId !== null) doAttachScript(selectedNodeId); else showFsToast('Select a node to attach a script');
    });
    var stDetach = document.getElementById('st-detach-script');
    if (stDetach) stDetach.addEventListener('click', function(e) {
      e.stopPropagation();
      showFsToast(selectedNodeId !== null ? 'Detach Script from selected node' : 'Select a node first');
    });
    var stExtend = document.getElementById('st-extend-script');
    if (stExtend) stExtend.addEventListener('click', function(e) {
      e.stopPropagation();
      showFsToast(selectedNodeId !== null ? 'Extend Script of selected node' : 'Select a node first');
    });
    var stMenu = document.getElementById('st-tree-menu');
    if (stMenu) stMenu.addEventListener('click', function(ev) {
      ev.stopPropagation();
      var old = document.getElementById('st-tree-menu-popup');
      if (old) { old.remove(); return; }
      var menu = document.createElement('div');
      menu.id = 'st-tree-menu-popup';
      var r = stMenu.getBoundingClientRect();
      menu.style.cssText = 'position:fixed;top:' + (r.bottom + 2) + 'px;left:' + Math.max(4, r.right - 180) + 'px;background:var(--panel);border:1px solid var(--border);border-radius:3px;padding:4px 0;z-index:600;box-shadow:0 4px 12px rgba(0,0,0,0.5);min-width:180px;';
      // Godot's "Extra scene options" tree menu (subset).
      var opts = [
        { label: 'Expand/Collapse Branch', act: function() { showFsToast('Expand/Collapse Branch'); } },
        { label: 'Show in FileSystem', act: function() { showFsToast('Show in FileSystem'); } },
        { label: 'Copy Node Path', act: function() {
            if (selectedNodeId !== null) { fsCopyToClipboard(String(selectedNodeId), 'Copied Node Path'); } else { showFsToast('Select a node first'); }
          } }
      ];
      for (var oi = 0; oi < opts.length; oi++) {
        (function(opt) {
          var item = document.createElement('div');
          item.textContent = opt.label;
          item.style.cssText = 'padding:4px 12px;cursor:pointer;font-size:12px;white-space:nowrap;';
          item.addEventListener('mouseenter', function() { item.style.background = 'var(--hover)'; });
          item.addEventListener('mouseleave', function() { item.style.background = ''; });
          item.addEventListener('click', function() { menu.remove(); opt.act(); });
          menu.appendChild(item);
        })(opts[oi]);
      }
      document.body.appendChild(menu);
      var dismiss = function() { menu.remove(); document.removeEventListener('click', dismiss); };
      setTimeout(function() { document.addEventListener('click', dismiss); }, 0);
    });

    document.getElementById('btn-delete').addEventListener('click', async function() {
      if (selectedNodeId === null) return;
      await doDelete(selectedNodeId);
    });

    document.getElementById('btn-undo').addEventListener('click', async function() {
      await api('POST', '/api/undo'); await fetchScene();
      if (selectedNodeId) await fetchSelected();
    });
    document.getElementById('btn-redo').addEventListener('click', async function() {
      await api('POST', '/api/redo'); await fetchScene();
      if (selectedNodeId) await fetchSelected();
    });

    document.getElementById('btn-save').addEventListener('click', async function() {
      // Godot-style (SCENE_SAVE_SCENE): Save writes to the current scene path
      // with no prompt. An empty scene (no root node) can't be saved, and a
      // scene with no path yet falls through to Save As.
      if (sceneIsEmpty()) { saveSceneAs(); return; }
      var path = currentScenePath;
      if (!path) {
        var tabs = await api('GET', '/api/scene/tabs');
        var at = tabs && tabs.tabs && tabs.tabs[(tabs.active_tab_index || 0)];
        if (at && at.path) path = at.path;
      }
      if (!path) { saveSceneAs(); return; }
      var result = await api('POST', '/api/scene/save', { path: path });
      if (result && result.ok) {
        currentScenePath = path;
        var btn = document.getElementById('btn-save');
        btn.style.borderColor = 'var(--accent)';
        setTimeout(function(){ btn.style.borderColor = ''; }, 500);
        fetchSceneInfo(); fetchSceneTabs();
        logMessage('info', 'Saved: ' + path);
      } else if (result && result.error) {
        logMessage('error', result.error.message || 'Save failed');
      }
    });

    document.getElementById('btn-load').addEventListener('click', function() { openSceneDialog(); });

    // Open Scene picker dialog wiring.
    var osDlg = document.getElementById('open-scene-dialog');
    if (osDlg) {
      document.getElementById('open-scene-close').addEventListener('click', function() { osDlg.classList.remove('open'); });
      document.getElementById('open-scene-cancel').addEventListener('click', function() { osDlg.classList.remove('open'); });
      osDlg.addEventListener('click', function(e) { if (e.target === osDlg) osDlg.classList.remove('open'); });
      document.getElementById('open-scene-filter').addEventListener('input', function(e) { renderOpenSceneList(e.target.value); });
    }

    // Open Project dialog wiring (pat-v89tw).
    var opDlg = document.getElementById('open-project-dialog');
    if (opDlg) {
      document.getElementById('open-project-close').addEventListener('click', function() { opDlg.classList.remove('open'); });
      document.getElementById('open-project-cancel').addEventListener('click', function() { opDlg.classList.remove('open'); });
      document.getElementById('open-project-confirm').addEventListener('click', function() { doOpenProject(); });
      opDlg.addEventListener('click', function(e) { if (e.target === opDlg) opDlg.classList.remove('open'); });
      document.getElementById('open-project-path').addEventListener('keydown', function(e) { if (e.key === 'Enter') doOpenProject(); });
    }
  }

  // ---- Open Scene picker (lists .tscn files from the project) ----
  function openSceneDialog() {
    var dlg = document.getElementById('open-scene-dialog');
    var list = document.getElementById('open-scene-list');
    var filterInput = document.getElementById('open-scene-filter');
    filterInput.value = '';
    list.innerHTML = '<div style="padding:14px;color:var(--text-dim);font-size:12px">Loading scenes…</div>';
    dlg.classList.add('open');
    setTimeout(function() { filterInput.focus(); }, 30);
    api('GET', '/api/filesystem').then(function(data) {
      var scenes = [];
      (function walk(nodes) {
        (nodes || []).forEach(function(n) {
          if (n.is_dir) walk(n.children);
          else if (/\.tscn$/i.test(n.name || '')) scenes.push({ name: n.name, path: n.path });
        });
      })(data && data.files);
      scenes.sort(function(a, b) { return a.name.localeCompare(b.name); });
      dlg._scenes = scenes;
      renderOpenSceneList('');
    });
  }
  function renderOpenSceneList(filter) {
    var dlg = document.getElementById('open-scene-dialog');
    var list = document.getElementById('open-scene-list');
    var scenes = dlg._scenes || [];
    var f = (filter || '').toLowerCase();
    list.innerHTML = '';
    var shown = scenes.filter(function(s) { return !f || s.name.toLowerCase().indexOf(f) >= 0 || (s.path || '').toLowerCase().indexOf(f) >= 0; });
    if (!shown.length) { list.innerHTML = '<div style="padding:14px;color:var(--text-dim);font-size:12px">No scenes found</div>'; return; }
    shown.forEach(function(s) {
      var item = document.createElement('div');
      item.className = 'open-scene-item';
      item.innerHTML = '<span class="osi-icon">&#9638;</span><span class="osi-name"></span><span class="osi-path"></span>';
      item.querySelector('.osi-name').textContent = s.name;
      item.querySelector('.osi-path').textContent = s.path;
      item.addEventListener('click', function() { doOpenScene(s.path); });
      list.appendChild(item);
    });
  }
  function doOpenScene(path) {
    var rel = path.replace('res://', '');
    api('POST', '/api/scene/load', { path: rel }).then(function() {
      // Re-arm the scene-open auto-select so the newly opened scene picks its
      // matching 2D/3D editor (Godot reselects the main screen on every open).
      autoViewPending = true;
      currentScenePath = rel;
      selectedNodeId = null; selectedNodeData = null;
      if (typeof selectedNodeIds !== 'undefined' && selectedNodeIds.clear) selectedNodeIds.clear();
      expandedNodes.clear(); renderInspectorEmpty();
      fetchScene(); fetchSceneInfo(); fetchSceneTabs();
      document.getElementById('open-scene-dialog').classList.remove('open');
      logMessage('info', 'Opened scene: ' + path);
    });
  }

  // ---- Open Project (roots the editor at a chosen game project dir) ----
  // pat-v89tw: the FileSystem dock otherwise lists the example process cwd. This
  // flow POSTs the chosen directory to /api/project/open, which validates a
  // project.godot manifest and re-roots the dock + texture cache at the project,
  // so the dock then shows that project's res:// .tscn/.gd tree.
  function openProjectDialog() {
    var dlg = document.getElementById('open-project-dialog');
    if (!dlg) return;
    var input = document.getElementById('open-project-path');
    var err = document.getElementById('open-project-error');
    if (err) err.textContent = '';
    dlg.classList.add('open');
    setTimeout(function() { if (input) input.focus(); }, 30);
  }
  function doOpenProject() {
    var input = document.getElementById('open-project-path');
    var err = document.getElementById('open-project-error');
    var path = (input && input.value || '').trim();
    if (!path) { if (err) err.textContent = 'Enter a project directory path.'; return; }
    if (err) err.textContent = 'Opening…';
    api('POST', '/api/project/open', { path: path }).then(function(data) {
      if (data && data.ok) {
        document.getElementById('open-project-dialog').classList.remove('open');
        // Re-root the FileSystem dock at the freshly-opened project.
        fetchFileSystem();
        logMessage('info', 'Opened project: ' + (data.project_root || path) + ' (' + (data.file_count || 0) + ' files)');
      } else {
        var msg = (data && data.error && data.error.message) || 'Failed to open project (is there a project.godot in that directory?)';
        if (err) err.textContent = msg;
      }
    });
  }
  // Godot uses snake_case scene file names by default (adjust_scene_name_casing
  // with the default scene_name_casing). Convert the root node's name and append
  // the .tscn extension for the suggested filename.
  function toSceneFileName(rootName) {
    var base = (rootName || 'scene')
      .replace(/([a-z0-9])([A-Z])/g, '$1_$2')
      .replace(/([A-Z]+)([A-Z][a-z])/g, '$1_$2')
      .toLowerCase()
      .replace(/[^a-z0-9_]+/g, '_')
      .replace(/^_+|_+$/g, '') || 'scene';
    return base + '.tscn';
  }

  // The scene's root node (first child of the synthetic window root), or null for
  // an empty scene. Its name seeds the Save As filename, matching Godot.
  function sceneRootNode() {
    return sceneData && sceneData.nodes && sceneData.nodes.children &&
      sceneData.nodes.children.length ? sceneData.nodes.children[0] : null;
  }

  var saveSceneDir = '';   // current folder in the Save As dialog (rel to res://)

  // Scene > Save Scene As... — opens the file-save dialog (Godot's
  // EditorFileDialog FILE_MODE_SAVE_FILE). Refuses an empty scene, mirroring
  // "A root node is required to save the scene."
  function saveSceneAs() {
    var root = sceneRootNode();
    if (!root) {
      showConfirm(
        'A root node is required to save the scene.<br><br>You can add a root node using the Scene tree dock.',
        { title: 'Save Scene As...', okText: 'OK' }
      );
      return;
    }
    // Default to the current scene's folder, else the project root.
    var path = currentScenePath || '';
    saveSceneDir = path.indexOf('/') >= 0 ? path.slice(0, path.lastIndexOf('/')) : '';
    document.getElementById('save-scene-name').value = toSceneFileName(root.name);
    document.getElementById('save-scene-error').textContent = '';
    document.getElementById('save-scene-dialog').classList.add('open');
    renderSaveSceneList();
    setTimeout(function() {
      var inp = document.getElementById('save-scene-name');
      inp.focus();
      // Select just the base name (before .tscn) for quick renaming.
      var dot = inp.value.lastIndexOf('.');
      inp.setSelectionRange(0, dot > 0 ? dot : inp.value.length);
    }, 40);
  }

  function closeSaveSceneDialog() {
    document.getElementById('save-scene-dialog').classList.remove('open');
  }

  // Finds the directory node at `dir` (a res://-relative path) within the
  // filesystem tree, returning its child entries (or the top-level files).
  function fsChildrenAt(files, dir) {
    if (!dir) return files || [];
    var parts = dir.split('/');
    var nodes = files || [];
    for (var i = 0; i < parts.length; i++) {
      var found = null;
      for (var j = 0; j < nodes.length; j++) {
        if (nodes[j].is_dir && nodes[j].name === parts[i]) { found = nodes[j]; break; }
      }
      if (!found) return [];
      nodes = found.children || [];
    }
    return nodes;
  }

  async function renderSaveSceneList() {
    var list = document.getElementById('save-scene-list');
    var pathEl = document.getElementById('save-scene-path');
    pathEl.textContent = 'res://' + (saveSceneDir ? saveSceneDir + '/' : '');
    list.innerHTML = '';
    var data = await api('GET', '/api/filesystem');
    var entries = fsChildrenAt(data && data.files, saveSceneDir);
    // "Up a level" entry, except at the project root.
    if (saveSceneDir) {
      var up = document.createElement('div');
      up.className = 'save-scene-item ssi-dir';
      up.innerHTML = '<span class="ssi-icon">📁</span><span class="ssi-name">..</span>';
      up.addEventListener('click', function() {
        saveSceneDir = saveSceneDir.indexOf('/') >= 0 ? saveSceneDir.slice(0, saveSceneDir.lastIndexOf('/')) : '';
        renderSaveSceneList();
      });
      list.appendChild(up);
    }
    // Directories first, then .tscn files.
    var dirs = entries.filter(function(e) { return e.is_dir; });
    var scenes = entries.filter(function(e) { return !e.is_dir && /\.tscn$/i.test(e.name); });
    dirs.sort(function(a, b){ return a.name.localeCompare(b.name); });
    scenes.sort(function(a, b){ return a.name.localeCompare(b.name); });
    dirs.forEach(function(d) {
      var el = document.createElement('div');
      el.className = 'save-scene-item ssi-dir';
      el.innerHTML = '<span class="ssi-icon">📁</span><span class="ssi-name">' + escapeHtml(d.name) + '</span>';
      el.addEventListener('click', function() {
        saveSceneDir = saveSceneDir ? saveSceneDir + '/' + d.name : d.name;
        renderSaveSceneList();
      });
      list.appendChild(el);
    });
    scenes.forEach(function(f) {
      var el = document.createElement('div');
      el.className = 'save-scene-item ssi-file';
      el.innerHTML = '<span class="ssi-icon">📄</span><span class="ssi-name">' + escapeHtml(f.name) + '</span>';
      // Clicking an existing scene targets it (overwrite), like a save dialog.
      el.addEventListener('click', function() {
        document.getElementById('save-scene-name').value = f.name;
      });
      list.appendChild(el);
    });
  }

  function doSaveSceneConfirm() {
    var errEl = document.getElementById('save-scene-error');
    var name = document.getElementById('save-scene-name').value.trim();
    if (!name) { errEl.textContent = 'Enter a file name.'; return; }
    if (!/\.tscn$/i.test(name)) name += '.tscn';
    if (/[\\/]/.test(name)) { errEl.textContent = 'File name can’t contain slashes.'; return; }
    var rel = saveSceneDir ? saveSceneDir + '/' + name : name;
    api('POST', '/api/scene/save', { path: rel }).then(function(result) {
      if (result && result.ok) {
        currentScenePath = rel;
        closeSaveSceneDialog();
        fetchScene(); fetchSceneInfo(); fetchSceneTabs(); fetchFileSystem();
        logMessage('info', 'Saved: ' + rel);
      } else {
        errEl.textContent = (result && result.error && result.error.message) || 'Save failed.';
      }
    });
  }

  function setupSaveSceneDialog() {
    var dlg = document.getElementById('save-scene-dialog');
    if (!dlg) return;
    document.getElementById('save-scene-close').addEventListener('click', closeSaveSceneDialog);
    document.getElementById('save-scene-cancel').addEventListener('click', closeSaveSceneDialog);
    document.getElementById('save-scene-confirm').addEventListener('click', doSaveSceneConfirm);
    dlg.addEventListener('click', function(e) { if (e.target === dlg) closeSaveSceneDialog(); });
    document.getElementById('save-scene-name').addEventListener('keydown', function(e) {
      if (e.key === 'Enter') { e.preventDefault(); doSaveSceneConfirm(); }
      if (e.key === 'Escape') { e.preventDefault(); closeSaveSceneDialog(); }
    });
  }

  // ---- Search ----
  function setupSearch() {
    var searchInput = document.getElementById('scene-search');
    searchInput.addEventListener('input', function() { searchFilter = searchInput.value.trim(); refreshTree(); });
    document.addEventListener('keydown', function(e) {
      if (e.ctrlKey && e.key === 'f' && !e.shiftKey) {
        if (document.activeElement && document.activeElement.tagName === 'INPUT') return;
        e.preventDefault(); searchInput.focus(); searchInput.select();
      }
    });
  }

  // ---- Keyboard shortcuts ----
  function setupKeyboardShortcuts() {
    document.addEventListener('keydown', function(e) {
      if (document.activeElement && (document.activeElement.tagName === 'INPUT' || document.activeElement.tagName === 'TEXTAREA' || document.activeElement.tagName === 'SELECT')) return;

      if (e.key === 'Delete') { e.preventDefault(); if (selectedNodeIds.size > 1) { doDeleteMulti(); } else if (selectedNodeId !== null) { doDelete(selectedNodeId); } return; }
      if (e.key === 'F2' && selectedNodeId !== null) { e.preventDefault(); doRename(selectedNodeId); return; }
      if (e.ctrlKey && e.key === 'd' && selectedNodeId !== null) { e.preventDefault(); doDuplicate(selectedNodeId); return; }
      if (e.ctrlKey && e.key === 'c') { e.preventDefault(); doCopy(); return; }
      if (e.ctrlKey && e.key === 'v') { e.preventDefault(); doPaste(); return; }
      if (e.ctrlKey && e.key === 'x') { e.preventDefault(); doCut(); return; }
      if (e.ctrlKey && e.key === 'z' && !e.shiftKey) {
        e.preventDefault(); api('POST', '/api/undo').then(function() { fetchScene(); if (selectedNodeId) fetchSelected(); }); return;
      }
      if ((e.ctrlKey && e.key === 'y') || (e.ctrlKey && e.shiftKey && e.key === 'z')) {
        e.preventDefault(); api('POST', '/api/redo').then(function() { fetchScene(); if (selectedNodeId) fetchSelected(); }); return;
      }
      if (e.ctrlKey && e.key === 's') { e.preventDefault(); document.getElementById('btn-save').click(); return; }

      // Inspector history navigation (Alt+Left/Right)
      if (e.altKey && e.key === 'ArrowLeft') { e.preventDefault(); inspectorBack(); return; }
      if (e.altKey && e.key === 'ArrowRight') { e.preventDefault(); inspectorForward(); return; }

      // Zoom shortcuts
      if (e.ctrlKey && (e.key === '=' || e.key === '+')) {
        e.preventDefault(); viewportZoom = Math.min(16.0, viewportZoom * 1.25);
        api('POST', '/api/viewport/zoom', { zoom: viewportZoom }); updateZoomIndicator(); return;
      }
      if (e.ctrlKey && e.key === '-') {
        e.preventDefault(); viewportZoom = Math.max(0.1, viewportZoom / 1.25);
        api('POST', '/api/viewport/zoom', { zoom: viewportZoom }); updateZoomIndicator(); return;
      }
      if (e.ctrlKey && e.key === '0') {
        e.preventDefault(); viewportZoom = 1.0; viewportPanX = 0; viewportPanY = 0;
        api('POST', '/api/viewport/zoom', { zoom: 1.0 }); api('POST', '/api/viewport/pan', { x: 0, y: 0 }); updateZoomIndicator(); return;
      }

      // 3D viewport navigation shortcuts (Godot-style): F frames the selection;
      // 1/3/7 snap to front/right/top views.
      if (editorMode === '3d') {
        if (e.key === 'f' || e.key === 'F') {
          e.preventDefault();
          api('POST', '/api/viewport/3d/frame', {}).then(apply3DState);
          return;
        }
        if (e.key === '1') { e.preventDefault(); api('POST', '/api/viewport/3d/view_preset', { preset: 'front' }).then(apply3DState); return; }
        if (e.key === '3') { e.preventDefault(); api('POST', '/api/viewport/3d/view_preset', { preset: 'right' }).then(apply3DState); return; }
        if (e.key === '7') { e.preventDefault(); api('POST', '/api/viewport/3d/view_preset', { preset: 'top' }).then(apply3DState); return; }
      }

      // Tool mode shortcuts. While 3D free-look is active (right mouse held), Godot
      // captures W/A/S/D/Q/E for camera flight, so don't let them double as the
      // transform-tool shortcuts — fly3DKeyDown handles the movement.
      if (editorMode === '3d' && nav3D.kind === 'fly' &&
          ['w','a','s','d','q','e'].indexOf((e.key || '').toLowerCase()) >= 0) { return; }
      if (e.key === 'q' || e.key === 'Q') { setToolMode('select'); return; }
      if (e.key === 'w' || e.key === 'W') { setToolMode('move'); return; }
      if (e.key === 'e' || e.key === 'E') { setToolMode('rotate'); return; }
      // pat-kwznj: Godot binds Scale Mode to R (canvas_item_editor_plugin.cpp
      // ED_SHORTCUT("canvas_item_editor/scale_mode", ..., Key::R)), not S.
      if (e.key === 'r' || e.key === 'R') { setToolMode('scale'); return; }

      // Play shortcuts
      if (e.key === 'F5') { e.preventDefault(); document.getElementById('btn-play').click(); return; }
      if (e.key === 'F6') { e.preventDefault(); document.getElementById('btn-play-current').click(); return; }
      if (e.key === 'F7') { e.preventDefault(); document.getElementById('btn-pause').click(); return; }
      if (e.key === 'F8') { e.preventDefault(); document.getElementById('btn-stop').click(); return; }

      // Help shortcut
      if (e.key === 'F1' || e.key === '?') { e.preventDefault(); toggleHelpDialog(); return; }
    });
  }

  // ---- Confirmation dialog (Godot-style, replaces native confirm()) ----
  // Returns a Promise<boolean>: true = confirmed, false = cancelled.
  var confirmResolve = null;
  function showConfirm(messageHtml, opts) {
    opts = opts || {};
    return new Promise(function(resolve) {
      // If a prior confirm is somehow still pending, treat it as cancelled.
      if (confirmResolve) { var prev = confirmResolve; confirmResolve = null; prev(false); }
      var dlg = document.getElementById('confirm-dialog');
      document.getElementById('confirm-dialog-title').textContent = opts.title || 'Please Confirm...';
      document.getElementById('confirm-dialog-body').innerHTML = messageHtml;
      var okBtn = document.getElementById('confirm-dialog-ok');
      var cancelBtn = document.getElementById('confirm-dialog-cancel');
      okBtn.textContent = opts.okText || 'OK';
      cancelBtn.textContent = opts.cancelText || 'Cancel';
      okBtn.classList.toggle('confirm-danger', !!opts.danger);
      confirmResolve = resolve;
      dlg.classList.add('open');
      okBtn.focus();
    });
  }
  function closeConfirm(result) {
    document.getElementById('confirm-dialog').classList.remove('open');
    if (confirmResolve) { var r = confirmResolve; confirmResolve = null; r(result); }
  }
  function setupConfirmDialog() {
    document.getElementById('confirm-dialog-ok').addEventListener('click', function() { closeConfirm(true); });
    document.getElementById('confirm-dialog-cancel').addEventListener('click', function() { closeConfirm(false); });
    document.getElementById('confirm-dialog-x').addEventListener('click', function() { closeConfirm(false); });
    document.getElementById('confirm-dialog').addEventListener('click', function(e) { if (e.target === this) closeConfirm(false); });
    // Capture-phase so Enter/Escape resolve the dialog before other shortcut handlers run.
    document.addEventListener('keydown', function(e) {
      if (!document.getElementById('confirm-dialog').classList.contains('open')) return;
      if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); closeConfirm(false); }
      else if (e.key === 'Enter') { e.preventDefault(); e.stopPropagation(); closeConfirm(true); }
    }, true);
  }

  // ---- Help dialog ----
  function toggleHelpDialog() {
    var dlg = document.getElementById('help-dialog');
    dlg.classList.toggle('open');
  }
  function setupHelpDialog() {
    document.getElementById('help-close').addEventListener('click', function() {
      document.getElementById('help-dialog').classList.remove('open');
    });
    document.getElementById('help-dialog').addEventListener('click', function(e) {
      if (e.target === this) this.classList.remove('open');
    });
  }

  // ---- Multi-delete ----
  async function doDeleteMulti() {
    if (selectedNodeIds.size === 0) return;
    var n = selectedNodeIds.size;
    if (!await showConfirm('Delete ' + n + ' selected node' + (n === 1 ? '' : 's') + '?', { danger: true })) return;
    for (var id of selectedNodeIds) { await api('POST', '/api/node/delete', { node_id: id }); }
    selectedNodeId = null; selectedNodeData = null; selectedNodeIds.clear();
    renderInspectorEmpty(); await fetchScene();
  }

  // ---- Copy/Paste ----
  async function doCopy() {
    var ids = selectedNodeIds.size > 0 ? Array.from(selectedNodeIds) : (selectedNodeId ? [selectedNodeId] : []);
    if (ids.length === 0) return;
    await api('POST', '/api/node/copy', { node_ids: ids });
    logMessage('info', 'Copied ' + ids.length + ' node(s)');
  }
  async function doPaste() {
    var parentId = selectedNodeId || (sceneData && sceneData.nodes ? sceneData.nodes.id : null);
    var result = await api('POST', '/api/node/paste', { parent_id: parentId });
    if (result && result.ok) {
      if (parentId) expandedNodes.add(parentId);
      await fetchScene(); await fetchSelected();
      logMessage('info', 'Pasted ' + (result.pasted || 0) + ' node(s)');
    }
  }
  async function doCut() {
    var ids = selectedNodeIds.size > 0 ? Array.from(selectedNodeIds) : (selectedNodeId ? [selectedNodeId] : []);
    if (ids.length === 0) return;
    await api('POST', '/api/node/cut', { node_ids: ids });
    selectedNodeId = null; selectedNodeData = null; selectedNodeIds.clear();
    renderInspectorEmpty(); await fetchScene();
    logMessage('info', 'Cut ' + ids.length + ' node(s)');
  }

  // ---- Settings ----
  var settingsDialogOpen = false;
  async function fetchSettings() {
    var data = await api('GET', '/api/settings');
    if (data) editorSettings = data;
    renderViewportOverlay();
  }
  async function updateSetting(key, value) {
    var body = {}; body[key] = value;
    var data = await api('POST', '/api/settings', body);
    if (data) editorSettings = data;
    renderViewportOverlay();
  }


  // ---- Polling ----
  function startPolling() {
    // pat-vl1mk: single-in-flight polling. Each named endpoint skips its tick
    // if the previous invocation has not settled, so async polls can never pile
    // up and jam the renderer main thread under sustained polling. Combined with
    // the AbortController timeout in api(), no poll can leak a pending request.
    var _pollInflight = {};
    function schedulePoll(name, intervalMs, fn) {
      setInterval(function() {
        if (_pollInflight[name]) return;
        _pollInflight[name] = true;
        Promise.resolve().then(fn).catch(function(){}).finally(function(){ _pollInflight[name] = false; });
      }, intervalMs);
    }
    schedulePoll('scene', 500, fetchScene);
    schedulePoll('viewport', 200, refreshViewport);
    schedulePoll('logs', 1000, fetchLogs);
    schedulePoll('sceneInfo', 2000, function() { updateSceneTab(); return fetchSceneInfo(); });
    schedulePoll('filesystem', 5000, fetchFileSystem);
    // Keeps the History dock live (undo/redo from menus/shortcuts, new edits).
    // refreshHistoryDock() no-ops unless the History tab is showing, so this
    // costs nothing while FileSystem is active.
    schedulePoll('history', 800, refreshHistoryDock);
    schedulePoll('panels', 2000, function() { updateSnapIndicator(); renderViewportOverlay(); if (editorMode === 'game') refreshGameStatus(); return Promise.all([fetchMonitorData(), fetchDebugData(), fetchProfilerData()]); });
    fetchSettings();
  }

  // ---- Right panel tabs (Inspector / Node) ----
  var currentRightTab = 'inspector';

  function setupRightPanelTabs() {
    var tabs = document.querySelectorAll('.right-panel-tab');
    var contents = document.querySelectorAll('.right-panel-content');
    for (var i = 0; i < tabs.length; i++) {
      tabs[i].addEventListener('click', function() {
        var tabName = this.getAttribute('data-rptab');
        currentRightTab = tabName;
        for (var j = 0; j < tabs.length; j++) tabs[j].classList.toggle('active', tabs[j].getAttribute('data-rptab') === tabName);
        for (var j = 0; j < contents.length; j++) contents[j].classList.toggle('active', contents[j].getAttribute('data-rptab') === tabName);
        if (tabName === 'node' && selectedNodeId !== null) fetchNodeDock();
      });
    }
  }

  // ---- Node Dock: Signals + Groups ----
  var nodeDockData = null;

  async function fetchNodeDock() {
    if (selectedNodeId === null) {
      renderNodeDockEmpty();
      return;
    }
    var data = await api('GET', '/api/node/signals?node_id=' + selectedNodeId);
    if (data) {
      nodeDockData = data;
      renderNodeDock(data);
    } else {
      renderNodeDockEmpty();
    }
  }

  function renderNodeDockEmpty() {
    document.getElementById('node-dock').innerHTML = '<div class="insp-empty">Select a node to view signals</div>';
  }

  // pat-9zrvd: current Signals-tab filter text + a shared lightweight context
  // menu for the signal/connection right-click menus. items: [{label, act, disabled?}].
  var nodeSignalFilter = '';
  // pat-3uujp: current Groups-tab filter text.
  var nodeGroupFilter = '';
  // pat-3uujp: every group used anywhere in the scene (the Scene Groups list),
  // sorted and de-duplicated, walked from the cached scene tree.
  function collectSceneGroups() {
    var out = {};
    function walk(n) {
      if (!n) return;
      (n.groups || []).forEach(function(g) { out[g] = true; });
      (n.children || []).forEach(walk);
    }
    if (sceneData && sceneData.nodes) walk(sceneData.nodes);
    return Object.keys(out).sort();
  }
  function showSimpleContextMenu(clientX, clientY, items) {
    var old = document.getElementById('simple-context-menu'); if (old) old.remove();
    var menu = document.createElement('div');
    menu.id = 'simple-context-menu';
    menu.style.cssText = 'position:fixed;top:' + clientY + 'px;left:' + clientX + 'px;background:var(--panel);border:1px solid var(--border);border-radius:3px;padding:4px 0;z-index:600;box-shadow:0 4px 12px rgba(0,0,0,0.5);min-width:150px;';
    for (var i = 0; i < items.length; i++) {
      (function(it) {
        var item = document.createElement('div');
        item.textContent = it.label;
        item.style.cssText = 'padding:4px 12px;font-size:12px;white-space:nowrap;' + (it.disabled ? 'opacity:0.4;cursor:default;' : 'cursor:pointer;');
        if (!it.disabled) {
          item.addEventListener('mouseenter', function() { item.style.background = 'var(--hover)'; });
          item.addEventListener('mouseleave', function() { item.style.background = ''; });
          item.addEventListener('click', function() { menu.remove(); it.act(); });
        }
        menu.appendChild(item);
      })(items[i]);
    }
    document.body.appendChild(menu);
    var dismiss = function() { menu.remove(); document.removeEventListener('click', dismiss); };
    setTimeout(function() { document.addEventListener('click', dismiss); }, 0);
  }

  function renderNodeDock(data) {
    var el = document.getElementById('node-dock');
    el.innerHTML = '';

    // Signals section
    var sigSection = createSection('Signals', 'node-signals');
    var sigBody = sigSection.querySelector('.insp-section-body');

    // pat-9zrvd: "Filter Signals" search box with a right-aligned Search icon,
    // matching Godot's ConnectionsDock search_box (set_right_icon(Search)).
    var sigSearchWrap = document.createElement('div');
    sigSearchWrap.className = 'signal-search-wrap';
    var sigSearch = document.createElement('input');
    sigSearch.type = 'text';
    sigSearch.id = 'signal-search';
    sigSearch.className = 'signal-search';
    sigSearch.placeholder = 'Filter Signals';
    sigSearch.value = nodeSignalFilter;
    var sigSearchIcon = document.createElement('span');
    sigSearchIcon.className = 'signal-search-icon';
    sigSearchIcon.innerHTML = '&#128269;'; // Search icon
    sigSearch.addEventListener('input', function() { nodeSignalFilter = this.value; renderNodeDock(data); });
    sigSearchWrap.appendChild(sigSearch);
    sigSearchWrap.appendChild(sigSearchIcon);
    sigBody.appendChild(sigSearchWrap);

    var sigFilterLower = (nodeSignalFilter || '').toLowerCase();
    var shownSignals = (data.signals || []).filter(function(s) {
      return !sigFilterLower || (s.name && s.name.toLowerCase().indexOf(sigFilterLower) >= 0);
    });

    if (shownSignals.length > 0) {
      for (var i = 0; i < shownSignals.length; i++) {
        var sig = shownSignals[i];
        var row = document.createElement('div');
        row.className = 'signal-row';

        var icon = document.createElement('span');
        icon.className = 'signal-icon ' + (sig.connected ? 'connected' : 'disconnected');
        icon.innerHTML = sig.connected ? '&#9889;&#8594;' : '&#9889;';

        var name = document.createElement('span');
        name.className = 'signal-name';
        name.textContent = sig.name;

        var connectBtn = document.createElement('button');
        connectBtn.className = 'signal-connect-btn';
        connectBtn.textContent = 'Connect...';
        connectBtn.addEventListener('click', (function(sigName) { return function() {
          openConnectDialog(sigName);
        }; })(sig.name));

        row.appendChild(icon);
        row.appendChild(name);
        row.appendChild(connectBtn);
        // pat-9zrvd: right-click a signal -> Connect.../Disconnect All/Copy
        // Name/Open Documentation (connections_dialog.cpp signal_menu).
        (function(sg) {
          row.addEventListener('contextmenu', function(ev) {
            ev.preventDefault(); ev.stopPropagation();
            showSimpleContextMenu(ev.clientX, ev.clientY, [
              { label: 'Connect...', act: function() { openConnectDialog(sg.name); } },
              { label: 'Disconnect All', disabled: !sg.connected, act: function() {
                  api('POST', '/api/signal/disconnect', { node_id: selectedNodeId, signal: sg.name }).then(function() { fetchNodeDock(); });
                } },
              { label: 'Copy Name', act: function() { fsCopyToClipboard(sg.name, 'Copied Signal Name'); } },
              { label: 'Open Documentation', act: function() { logMessage('info', 'Open Documentation: ' + (data['class'] || 'Node') + '.' + sg.name); if (typeof toggleHelpDialog === 'function') toggleHelpDialog(); } }
            ]);
          });
        })(sig);
        sigBody.appendChild(row);

        // pat-9zrvd: render each connection (slot) with its own right-click menu
        // -> Edit.../Go to Method/Disconnect (connections_dialog.cpp slot_menu).
        if (sig.connections && sig.connections.length) {
          for (var ci = 0; ci < sig.connections.length; ci++) {
            (function(conn, sgName) {
              var crow = document.createElement('div');
              crow.className = 'signal-connection-detail signal-connection-row';
              crow.textContent = '→ ' + (conn.method || '(method)');
              crow.addEventListener('contextmenu', function(ev) {
                ev.preventDefault(); ev.stopPropagation();
                showSimpleContextMenu(ev.clientX, ev.clientY, [
                  { label: 'Edit...', act: function() { openConnectDialog(sgName); } },
                  { label: 'Go to Method', act: function() { logMessage('info', 'Go to Method: ' + (conn.method || '')); showFsToast('Go to Method: ' + (conn.method || '')); } },
                  { label: 'Disconnect', act: function() { api('POST', '/api/signal/disconnect', { node_id: selectedNodeId, signal: sgName }).then(function() { fetchNodeDock(); }); } }
                ]);
              });
              sigBody.appendChild(crow);
            })(sig.connections[ci], sig.name);
          }
        }
      }
    } else {
      var empty = document.createElement('div');
      empty.className = 'insp-empty';
      empty.style.padding = '8px';
      empty.textContent = sigFilterLower ? 'No signals match the filter' : 'No signals for this node type';
      sigBody.appendChild(empty);
    }
    el.appendChild(sigSection);

    // Groups section
    // pat-3uujp: Groups tab matching Godot's GroupsEditor (groups_editor.cpp):
    // an "Add a new group" button + "Filter Groups" box (Search icon), then
    // Scene Groups / Global Groups tree headers with per-group membership
    // checkboxes, copy-name buttons, and a right-click menu (convert
    // scene<->global, rename, delete).
    var grpSection = createSection('Groups', 'node-groups');
    var grpBody = grpSection.querySelector('.insp-section-body');
    var groupsDiv = document.createElement('div');
    groupsDiv.className = 'groups-section';

    // Toolbar: Add Group button + Filter Groups box with a right Search icon.
    var grpToolbar = document.createElement('div');
    grpToolbar.className = 'groups-toolbar';
    var grpAddBtn = document.createElement('button');
    grpAddBtn.className = 'group-add-btn';
    grpAddBtn.id = 'group-add-btn';
    grpAddBtn.title = 'Add a new group.';
    grpAddBtn.innerHTML = '&#65291;';
    grpAddBtn.addEventListener('click', function() {
      var name = prompt('Add a new group:');
      if (name && name.trim()) api('POST', '/api/node/groups/add', { node_id: selectedNodeId, group: name.trim() }).then(function() { fetchNodeDock(); });
    });
    var grpFilterWrap = document.createElement('div');
    grpFilterWrap.className = 'group-filter-wrap';
    var grpFilter = document.createElement('input');
    grpFilter.type = 'text';
    grpFilter.id = 'group-filter';
    grpFilter.className = 'group-filter';
    grpFilter.placeholder = 'Filter Groups';
    grpFilter.value = nodeGroupFilter;
    var grpFilterIcon = document.createElement('span');
    grpFilterIcon.className = 'group-filter-icon';
    grpFilterIcon.innerHTML = '&#128269;';
    grpFilter.addEventListener('input', function() { nodeGroupFilter = this.value; renderNodeDock(data); });
    grpFilterWrap.appendChild(grpFilter);
    grpFilterWrap.appendChild(grpFilterIcon);
    grpToolbar.appendChild(grpAddBtn);
    grpToolbar.appendChild(grpFilterWrap);
    groupsDiv.appendChild(grpToolbar);

    var nodeGroups = data.groups || [];
    var grpFilterLower = (nodeGroupFilter || '').toLowerCase();
    // A single group row: [membership checkbox] name [copy-name] + RMB menu.
    function makeGroupRow(name, isMember, isGlobal) {
      var row = document.createElement('div');
      row.className = 'group-row';
      var cb = document.createElement('input');
      cb.type = 'checkbox';
      cb.className = 'group-check';
      cb.checked = isMember;
      cb.title = 'Toggle this node’s membership in the group';
      cb.addEventListener('change', function() {
        var ep = cb.checked ? '/api/node/groups/add' : '/api/node/groups/remove';
        api('POST', ep, { node_id: selectedNodeId, group: name }).then(function() { fetchNodeDock(); });
      });
      var label = document.createElement('span');
      label.className = 'group-row-name';
      label.textContent = name;
      var copyBtn = document.createElement('button');
      copyBtn.className = 'group-copy-btn';
      copyBtn.title = 'Copy group name';
      copyBtn.innerHTML = '&#128203;';
      copyBtn.addEventListener('click', function(e) { e.stopPropagation(); fsCopyToClipboard(name, 'Copied Group Name'); });
      row.appendChild(cb);
      row.appendChild(label);
      row.appendChild(copyBtn);
      row.addEventListener('contextmenu', function(ev) {
        ev.preventDefault(); ev.stopPropagation();
        showSimpleContextMenu(ev.clientX, ev.clientY, [
          { label: isGlobal ? 'Convert to Scene Group' : 'Convert to Global Group', act: function() { showFsToast((isGlobal ? 'Convert to Scene Group: ' : 'Convert to Global Group: ') + name); } },
          { label: 'Rename', act: function() {
              var nn = prompt('Rename group:', name);
              if (nn && nn.trim() && nn !== name) {
                api('POST', '/api/node/groups/remove', { node_id: selectedNodeId, group: name })
                  .then(function() { return api('POST', '/api/node/groups/add', { node_id: selectedNodeId, group: nn.trim() }); })
                  .then(function() { fetchNodeDock(); });
              }
            } },
          { label: 'Delete', act: function() {
              showConfirm('Delete group &quot;' + escapeHtml(name) + '&quot; and all its references?', { danger: true }).then(function(ok) {
                if (ok) api('POST', '/api/node/groups/remove', { node_id: selectedNodeId, group: name }).then(function() { fetchNodeDock(); });
              });
            } }
        ]);
      });
      return row;
    }

    // Scene Groups: every group used anywhere in the scene, checkbox = whether
    // this node is a member.
    var sceneHeader = document.createElement('div');
    sceneHeader.className = 'group-tree-header';
    sceneHeader.id = 'scene-groups-header';
    sceneHeader.textContent = 'Scene Groups';
    groupsDiv.appendChild(sceneHeader);
    var sceneGroups = collectSceneGroups();
    var anyScene = false;
    sceneGroups.forEach(function(g) {
      if (grpFilterLower && g.toLowerCase().indexOf(grpFilterLower) < 0) return;
      anyScene = true;
      groupsDiv.appendChild(makeGroupRow(g, nodeGroups.indexOf(g) >= 0, false));
    });
    if (!anyScene) {
      var es = document.createElement('div');
      es.className = 'group-empty';
      es.textContent = grpFilterLower ? '(no matching groups)' : '(no scene groups)';
      groupsDiv.appendChild(es);
    }

    // Global Groups: project-wide groups. Our engine does not yet source these,
    // so the header renders with an empty state (Godot always shows it).
    var globalHeader = document.createElement('div');
    globalHeader.className = 'group-tree-header';
    globalHeader.id = 'global-groups-header';
    globalHeader.textContent = 'Global Groups';
    groupsDiv.appendChild(globalHeader);
    var eg = document.createElement('div');
    eg.className = 'group-empty';
    eg.textContent = '(no global groups)';
    groupsDiv.appendChild(eg);

    grpBody.appendChild(groupsDiv);
    el.appendChild(grpSection);
  }

  // ---- Connect dialog ----
  var pendingConnectSignal = null;

  function openConnectDialog(signalName) {
    pendingConnectSignal = signalName;
    document.getElementById('connect-signal-name').value = signalName;
    var defaultMethod = '_on_' + (selectedNodeData ? selectedNodeData.name.toLowerCase().replace(/[^a-z0-9]/g, '_') : 'node') + '_' + signalName;
    document.getElementById('connect-method-name').value = defaultMethod;
    // pat-jwuv1.3: reset advanced flags/binds for the fresh connection.
    var cb = document.getElementById('connect-binds'); if (cb) cb.value = '';
    var cd = document.getElementById('connect-deferred'); if (cd) cd.checked = false;
    var co = document.getElementById('connect-oneshot'); if (co) co.checked = false;
    document.getElementById('connect-dialog-overlay').classList.add('open');
    document.getElementById('connect-method-name').focus();
  }

  function closeConnectDialog() {
    document.getElementById('connect-dialog-overlay').classList.remove('open');
    pendingConnectSignal = null;
  }

  function setupConnectDialog() {
    document.getElementById('connect-cancel').addEventListener('click', closeConnectDialog);
    document.getElementById('connect-confirm').addEventListener('click', function() {
      if (!pendingConnectSignal || !selectedNodeId) return;
      var method = document.getElementById('connect-method-name').value.trim();
      if (!method) return;
      // pat-jwuv1.3: carry the advanced connect flags + bound args.
      var bindsRaw = (document.getElementById('connect-binds') || {}).value || '';
      var binds = bindsRaw.split(',').map(function(s) { return s.trim(); }).filter(function(s) { return s.length; });
      api('POST', '/api/node/signals/connect', {
        node_id: selectedNodeId,
        signal: pendingConnectSignal,
        method: method,
        deferred: !!(document.getElementById('connect-deferred') || {}).checked,
        one_shot: !!(document.getElementById('connect-oneshot') || {}).checked,
        binds: binds
      }).then(function() {
        closeConnectDialog();
        fetchNodeDock();
      });
    });
    document.getElementById('connect-method-name').addEventListener('keydown', function(e) {
      if (e.key === 'Enter') document.getElementById('connect-confirm').click();
      if (e.key === 'Escape') closeConnectDialog();
    });
    document.getElementById('connect-dialog-overlay').addEventListener('click', function(e) {
      if (e.target === this) closeConnectDialog();
    });
  }

  // ---- Script panel ----
  var currentScriptPath = null;
  var scriptOriginalContent = '';
  var scriptModified = false;

  function highlightGDScript(line) {
    var result = '';
    var i = 0;
    while (i < line.length) {
      if (line[i] === '#') { result += '<span class="gd-comment">' + escapeHtml(line.substring(i)) + '</span>'; break; }
      if (line[i] === '"' || line[i] === "'") {
        var quote = line[i]; var end = line.indexOf(quote, i + 1);
        if (end === -1) end = line.length - 1;
        result += '<span class="gd-string">' + escapeHtml(line.substring(i, end + 1)) + '</span>';
        i = end + 1; continue;
      }
      if (line[i] === '@') {
        var annoMatch = line.substring(i).match(/^@[a-zA-Z_][a-zA-Z0-9_]*/);
        if (annoMatch) { result += '<span class="gd-annotation">' + escapeHtml(annoMatch[0]) + '</span>'; i += annoMatch[0].length; continue; }
      }
      if (line[i] === '$') {
        var npMatch = line.substring(i).match(/^\$[A-Za-z0-9_\/]+/);
        if (npMatch) { result += '<span class="gd-nodepath">' + escapeHtml(npMatch[0]) + '</span>'; i += npMatch[0].length; continue; }
      }
      if (line[i] === '-' && i + 1 < line.length && line[i+1] === '>') {
        result += '<span class="gd-arrow">-&gt;</span>'; i += 2; continue;
      }
      if (line[i] === ':' && i + 1 < line.length && line[i+1] === ' ') {
        var thMatch = line.substring(i).match(/^:\s+([A-Z][a-zA-Z0-9_]*|int|float|bool|void|String|Array|Dictionary|Vector2|Vector3|Color|NodePath|Variant)/);
        if (thMatch) { result += '<span class="gd-typehint">' + escapeHtml(thMatch[0]) + '</span>'; i += thMatch[0].length; continue; }
      }
      if (/[0-9]/.test(line[i]) && (i === 0 || /[\s(,=+\-*\/<>!&|^~\[]/.test(line[i-1]))) {
        var numMatch = line.substring(i).match(/^[0-9]+(\.[0-9]+)?/);
        if (numMatch) { result += '<span class="gd-number">' + escapeHtml(numMatch[0]) + '</span>'; i += numMatch[0].length; continue; }
      }
      if (/[a-zA-Z_]/.test(line[i])) {
        var wordMatch = line.substring(i).match(/^[a-zA-Z_][a-zA-Z0-9_]*/);
        if (wordMatch) {
          var word = wordMatch[0];
          var keywords = ['func','var','if','else','elif','for','while','return','class','extends','match','const','static','pass','break','continue','in','not','and','or','yield','await','class_name','preload','load','setget','tool'];
          var constants = ['self','super','true','false','null','PI','TAU','INF','NAN'];
          var declKeywords = ['signal','enum','export','onready'];
          var builtins = ['print','str','int','float','len','range','abs','min','max','clamp','lerp','sign','round','ceil','floor','sqrt','pow','sin','cos','tan'];
          if (declKeywords.indexOf(word) >= 0) {
            result += (word === 'signal' || word === 'enum') ? '<span class="gd-keyword">' + escapeHtml(word) + '</span>' : '<span class="gd-annotation">' + escapeHtml(word) + '</span>';
          } else if (keywords.indexOf(word) >= 0) { result += '<span class="gd-keyword">' + escapeHtml(word) + '</span>';
          } else if (constants.indexOf(word) >= 0) { result += '<span class="gd-constant">' + escapeHtml(word) + '</span>';
          } else if (builtins.indexOf(word) >= 0) { result += '<span class="gd-builtin">' + escapeHtml(word) + '</span>';
          } else if (/^[A-Z][a-zA-Z0-9]*$/.test(word) && word.length > 1) { result += '<span class="gd-classname">' + escapeHtml(word) + '</span>';
          } else { result += escapeHtml(word); }
          i += word.length; continue;
        }
      }
      result += escapeHtml(line[i]); i++;
    }
    return result;
  }

  function highlightFullContent(content) {
    var lines = content.split('\n'); var hl = [];
    for (var li = 0; li < lines.length; li++) hl.push(highlightGDScript(lines[li]));
    return hl.join('\n');
  }
  function updateLineNumbers(content) {
    var el = document.getElementById('script-line-numbers'); if (!el) return;
    var c = content.split('\n').length; var nums = [];
    for (var ln = 1; ln <= c; ln++) nums.push(String(ln));
    el.textContent = nums.join('\n');
  }
  function updateHighlight(content) {
    var el = document.getElementById('script-highlight'); if (!el) return;
    el.innerHTML = highlightFullContent(content) + '\n';
  }
  function markScriptModified(modified) {
    scriptModified = modified;
    var tabBtn = document.getElementById('script-tab-btn');
    if (tabBtn) tabBtn.textContent = modified ? 'Script *' : 'Script';
  }
  async function saveScript() {
    var textarea = document.getElementById('script-textarea');
    if (!textarea || !currentScriptPath) return;
    var content = textarea.value;
    var result = await api('POST', '/api/script/save', { path: currentScriptPath, content: content });
    if (result && result.ok) {
      scriptOriginalContent = content; markScriptModified(false);
      var indicator = document.getElementById('script-saved-indicator');
      if (indicator) { indicator.classList.add('visible'); setTimeout(function() { indicator.classList.remove('visible'); }, 1500); }
    }
  }
  // pat-nrwy2: edited-document history for the script_back/script_forward arrows
  // (Godot ScriptEditor history). Each opened script is pushed; the arrows walk
  // the stack and are enabled per position.
  var scriptDocHistory = [];
  var scriptDocHistoryIdx = -1;
  var scriptHistoryNavigating = false;
  function updateScriptHistoryArrows() {
    var back = document.getElementById('script-back');
    var fwd = document.getElementById('script-forward');
    if (back) back.disabled = scriptDocHistoryIdx <= 0;
    if (fwd) fwd.disabled = scriptDocHistoryIdx >= scriptDocHistory.length - 1;
  }
  function scriptHistoryGo(delta) {
    var target = scriptDocHistoryIdx + delta;
    if (target < 0 || target >= scriptDocHistory.length) return;
    scriptDocHistoryIdx = target;
    scriptHistoryNavigating = true;
    fetchScript(scriptDocHistory[scriptDocHistoryIdx]).then(function() { scriptHistoryNavigating = false; });
    updateScriptHistoryArrows();
  }
  async function fetchScript(path) {
    if (!path) return;
    currentScriptPath = path;
    // Record in the document history unless we're navigating it.
    if (!scriptHistoryNavigating) {
      if (scriptDocHistoryIdx < scriptDocHistory.length - 1) {
        scriptDocHistory = scriptDocHistory.slice(0, scriptDocHistoryIdx + 1);
      }
      if (scriptDocHistory[scriptDocHistory.length - 1] !== path) {
        scriptDocHistory.push(path);
        scriptDocHistoryIdx = scriptDocHistory.length - 1;
      }
      updateScriptHistoryArrows();
    }
    var data = await api('GET', '/api/script?path=' + encodeURIComponent(path));
    if (data && data.content !== undefined) { renderScript(data.content, data.path); }
    else { document.getElementById('script-panel').innerHTML = '<div class="script-empty">Could not load script: ' + escapeHtml(path) + '</div>'; }
  }
  function renderScript(content, path) {
    var el = document.getElementById('script-panel'); el.innerHTML = '';
    scriptOriginalContent = content; markScriptModified(false);
    var header = document.createElement('div'); header.className = 'script-header';
    var pathSpan = document.createElement('span'); pathSpan.className = 'script-path';
    pathSpan.textContent = path || 'Script'; pathSpan.title = path || ''; header.appendChild(pathSpan);
    var savedInd = document.createElement('span'); savedInd.className = 'script-saved-indicator';
    savedInd.id = 'script-saved-indicator'; savedInd.textContent = 'Saved'; header.appendChild(savedInd);
    var saveBtn = document.createElement('button'); saveBtn.className = 'script-save-btn';
    saveBtn.textContent = 'Save'; saveBtn.title = 'Save script (Ctrl+S)';
    saveBtn.addEventListener('click', function() { saveScript(); }); header.appendChild(saveBtn);
    var newBtn = document.createElement('button'); newBtn.className = 'script-new-btn';
    newBtn.textContent = '+ New'; newBtn.title = 'Create new script';
    newBtn.addEventListener('click', function() { createNewScript(); }); header.appendChild(newBtn);
    el.appendChild(header);
    var wrap = document.createElement('div'); wrap.className = 'script-editor-wrap';
    var lineNums = document.createElement('div'); lineNums.className = 'script-line-numbers';
    lineNums.id = 'script-line-numbers'; wrap.appendChild(lineNums);
    var hlLayer = document.createElement('div'); hlLayer.className = 'script-highlight-layer';
    hlLayer.id = 'script-highlight'; wrap.appendChild(hlLayer);
    var textarea = document.createElement('textarea'); textarea.className = 'script-textarea';
    textarea.id = 'script-textarea'; textarea.spellcheck = false; textarea.autocomplete = 'off';
    textarea.autocapitalize = 'off'; textarea.value = content; textarea.setAttribute('wrap', 'off');
    textarea.addEventListener('scroll', function() {
      hlLayer.style.transform = 'translate(' + (-textarea.scrollLeft) + 'px, ' + (-textarea.scrollTop) + 'px)';
      lineNums.style.transform = 'translateY(' + (-textarea.scrollTop) + 'px)';
    });
    textarea.addEventListener('input', function() {
      updateHighlight(textarea.value); updateLineNumbers(textarea.value);
      markScriptModified(textarea.value !== scriptOriginalContent);
    });
    textarea.addEventListener('keydown', function(e) {
      if (e.key === 'Tab') { e.preventDefault();
        var start = textarea.selectionStart; var end = textarea.selectionEnd; var val = textarea.value;
        textarea.value = val.substring(0, start) + '    ' + val.substring(end);
        textarea.selectionStart = textarea.selectionEnd = start + 4;
        textarea.dispatchEvent(new Event('input')); return;
      }
      if (e.key === 'Enter') { e.preventDefault();
        var cStart = textarea.selectionStart; var val = textarea.value;
        var lineStart = val.lastIndexOf('\n', cStart - 1) + 1;
        var currentLine = val.substring(lineStart, cStart);
        var indentMatch = currentLine.match(/^(\s*)/);
        var indent = indentMatch ? indentMatch[1] : '';
        if (currentLine.trimEnd().endsWith(':')) indent += '    ';
        textarea.value = val.substring(0, cStart) + '\n' + indent + val.substring(textarea.selectionEnd);
        textarea.selectionStart = textarea.selectionEnd = cStart + 1 + indent.length;
        textarea.dispatchEvent(new Event('input')); return;
      }
      if (e.ctrlKey && e.key === 's') { e.preventDefault(); saveScript(); return; }
    });
    wrap.appendChild(textarea); el.appendChild(wrap);
    updateHighlight(content); updateLineNumbers(content);
    // pat-f64ui: reflect the opened script in the list, members overview, and
    // the centered script-name label.
    updateScriptDocChrome(path, content);
  }

  // pat-f64ui: open-scripts list + members overview population.
  var activeScriptMethods = [];
  var scriptMembersAlphaSort = false;
  function scriptBaseName(p) { return (p || '').split('/').pop() || 'Script'; }
  function extractScriptMethods(content) {
    var methods = [];
    (content || '').split('\n').forEach(function(line) {
      var m = /^\s*(?:static\s+)?func\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(/.exec(line);
      if (m) methods.push(m[1]);
    });
    return methods;
  }
  function scrollToScriptMethod(name) {
    var ta = document.getElementById('script-textarea');
    if (!ta) return;
    var lines = ta.value.split('\n');
    for (var i = 0; i < lines.length; i++) {
      if (new RegExp('^\\s*(?:static\\s+)?func\\s+' + name + '\\s*\\(').test(lines[i])) {
        var pos = lines.slice(0, i).join('\n').length + (i > 0 ? 1 : 0);
        ta.focus(); ta.setSelectionRange(pos, pos);
        ta.scrollTop = Math.max(0, (i - 2) * 16);
        break;
      }
    }
  }
  function renderScriptMembers() {
    var host = document.getElementById('script-members');
    if (!host) return;
    host.innerHTML = '';
    var q = ((document.getElementById('script-method-filter') || {}).value || '').toLowerCase();
    var methods = activeScriptMethods.filter(function(n) { return !q || n.toLowerCase().indexOf(q) >= 0; });
    if (scriptMembersAlphaSort) methods = methods.slice().sort();
    methods.forEach(function(name) {
      var item = document.createElement('div');
      item.className = 'script-member-item';
      item.textContent = name + '()';
      item.addEventListener('click', function() { scrollToScriptMethod(name); });
      host.appendChild(item);
    });
  }
  function updateScriptDocChrome(path, content) {
    setScriptName(scriptBaseName(path));
    var existing = null;
    for (var i = 0; i < scriptListData.length; i++) {
      scriptListData[i].active = false;
      if (scriptListData[i].path === path) existing = scriptListData[i];
    }
    if (existing) { existing.active = true; }
    else { scriptListData.push({ path: path, name: scriptBaseName(path), active: true }); }
    renderScriptList();
    activeScriptMethods = extractScriptMethods(content);
    renderScriptMembers();
  }

  async function createNewScript() {
    var path = prompt('Script file path (e.g. res://scripts/player.gd):');
    if (!path) return; if (!path.endsWith('.gd')) path += '.gd';
    var template = 'extends Node2D\n\nfunc _ready():\n    pass\n\nfunc _process(delta):\n    pass\n';
    var result = await api('POST', '/api/script/save', { path: path, content: template });
    if (result && result.ok) {
      currentScriptPath = path; renderScript(template, path);
      if (selectedNodeId !== null) {
        await api('POST', '/api/property/set', { node_id: selectedNodeId, property: 'script', value: { type: 'String', value: path } });
        await fetchSelected();
      }
    }
  }
  async function doAttachScript(nodeId) {
    var path = prompt('Script file path (e.g. res://scripts/player.gd):');
    if (!path) return; if (!path.endsWith('.gd')) path += '.gd';
    var existing = await api('GET', '/api/script?path=' + encodeURIComponent(path));
    if (!existing || existing.error) {
      var template = 'extends Node2D\n\nfunc _ready():\n    pass\n\nfunc _process(delta):\n    pass\n';
      await api('POST', '/api/script/save', { path: path, content: template });
    }
    await api('POST', '/api/property/set', { node_id: nodeId, property: 'script', value: { type: 'String', value: path } });
    await fetchSelected();
  }
  function clearScript() {
    currentScriptPath = null; scriptOriginalContent = ''; markScriptModified(false);
    document.getElementById('script-panel').innerHTML = '<div class="script-empty">Select a node with a script to view its content</div>';
  }

  // ---- Animation panel (pat-8zg44) ----
  var currentAnimData = null;
  var animPlaying = false;
  var animRecording = false;
  var animPlaybackTimer = null;
  var ANIM_PIXELS_PER_SEC = 120;
  var selectedKeyframe = null; // { trackIdx, kfIdx }

  function setupAnimationPanel() {
    // Onion skinning toggle
    var onionBtn = document.getElementById('anim-onion-btn');
    if (onionBtn) {
      onionBtn.addEventListener('click', function() {
        onionBtn.classList.toggle('active');
        editorSettings.onion_skinning_enabled = onionBtn.classList.contains('active');
        logMessage('info', 'Onion skinning ' + (editorSettings.onion_skinning_enabled ? 'enabled' : 'disabled'));
      });
    }
    // AnimationTree dialog
    var treeBtn = document.getElementById('anim-tree-btn');
    if (treeBtn) {
      treeBtn.addEventListener('click', function() {
        document.getElementById('anim-tree-dialog').classList.add('open');
      });
    }
    var treeClose = document.getElementById('anim-tree-close');
    if (treeClose) {
      treeClose.addEventListener('click', function() {
        document.getElementById('anim-tree-dialog').classList.remove('open');
      });
    }

    var newBtn = document.getElementById('anim-new-btn');
    var delBtn = document.getElementById('anim-delete-btn');
    var playBtn = document.getElementById('anim-play-btn');
    var stopBtn = document.getElementById('anim-stop-btn');
    var recBtn = document.getElementById('anim-record-btn');
    var sel = document.getElementById('anim-select');
    var addTrackBtn = document.getElementById('anim-add-track-btn');
    var canvas = document.getElementById('anim-timeline-canvas');

    if (newBtn) newBtn.addEventListener('click', async function() {
      var name = prompt('Animation name:', 'New Animation');
      if (!name) return;
      var length = parseFloat(prompt('Duration (seconds):', '1.0')) || 1.0;
      await api('POST', '/api/animation/create', { name: name, length: length });
      await refreshAnimationList();
      sel.value = name;
      await loadAnimation(name);
    });

    if (delBtn) delBtn.addEventListener('click', async function() {
      if (!sel || !sel.value) return;
      if (!await showConfirm('Delete animation &quot;' + escapeHtml(sel.value) + '&quot;?', { danger: true })) return;
      await api('POST', '/api/animation/delete', { name: sel.value });
      currentAnimData = null;
      await refreshAnimationList();
      renderAnimTracks();
      renderTimeline();
    });

    if (playBtn) playBtn.addEventListener('click', async function() {
      if (!sel || !sel.value) return;
      await api('POST', '/api/animation/play', { name: sel.value });
      animPlaying = true;
      startPlaybackPolling();
    });

    if (stopBtn) stopBtn.addEventListener('click', async function() {
      await api('POST', '/api/animation/stop');
      animPlaying = false;
      stopPlaybackPolling();
      updateTimeDisplay(0);
      updatePlayhead(0);
    });

    if (recBtn) recBtn.addEventListener('click', async function() {
      animRecording = !animRecording;
      await api('POST', '/api/animation/record', { enabled: animRecording });
      recBtn.classList.toggle('active', animRecording);
      document.body.classList.toggle('anim-recording', animRecording);
    });

    if (sel) sel.addEventListener('change', async function() {
      if (sel.value) {
        await loadAnimation(sel.value);
      } else {
        currentAnimData = null;
        renderAnimTracks();
        renderTimeline();
      }
    });

    if (addTrackBtn) addTrackBtn.addEventListener('click', function() {
      if (!currentAnimData) { logMessage('warn', 'Select an animation first'); return; }
      showAddTrackDialog(currentAnimData.name);
    });

    // Timeline canvas click — add/select keyframe
    if (canvas) {
      canvas.addEventListener('mousedown', function(e) {
        if (!currentAnimData) return;
        var rect = canvas.getBoundingClientRect();
        var x = e.clientX - rect.left;
        var y = e.clientY - rect.top;
        var time = x / ANIM_PIXELS_PER_SEC;
        var trackIdx = Math.floor(y / 24);

        // Check if clicking on existing keyframe
        if (trackIdx >= 0 && trackIdx < currentAnimData.tracks.length) {
          var track = currentAnimData.tracks[trackIdx];
          for (var ki = 0; ki < track.keyframes.length; ki++) {
            var kfX = track.keyframes[ki].time * ANIM_PIXELS_PER_SEC;
            if (Math.abs(x - kfX) < 6) {
              selectedKeyframe = { trackIdx: trackIdx, kfIdx: ki };
              renderTimeline();
              return;
            }
          }
        }

        // Click on empty area — seek to that time
        if (time >= 0 && time <= currentAnimData.length) {
          api('POST', '/api/animation/seek', { time: time }).then(function() {
            updatePlayhead(time);
            updateTimeDisplay(time);
          });
        }
        selectedKeyframe = null;
        renderTimeline();
      });

      // Double-click to add keyframe at position
      canvas.addEventListener('dblclick', function(e) {
        if (!currentAnimData) return;
        var rect = canvas.getBoundingClientRect();
        var x = e.clientX - rect.left;
        var y = e.clientY - rect.top;
        var time = Math.max(0, Math.min(x / ANIM_PIXELS_PER_SEC, currentAnimData.length));
        var trackIdx = Math.floor(y / 24);
        if (trackIdx < 0 || trackIdx >= currentAnimData.tracks.length) return;
        var track = currentAnimData.tracks[trackIdx];
        // Sample existing value at nearest keyframe or use default
        var value = { type: 'Float', value: 0 };
        if (track.keyframes.length > 0) {
          value = track.keyframes[track.keyframes.length - 1].value;
        }
        api('POST', '/api/animation/keyframe/add', {
          animation: currentAnimData.name,
          track_node: track.node_path,
          track_property: track.property,
          time: time,
          value: typeof value === 'object' ? value : { type: 'Float', value: value }
        }).then(function() { loadAnimation(currentAnimData.name); });
      });

      // Right-click to delete selected keyframe
      canvas.addEventListener('contextmenu', function(e) {
        e.preventDefault();
        if (!selectedKeyframe || !currentAnimData) return;
        api('POST', '/api/animation/keyframe/remove', {
          animation: currentAnimData.name,
          track_index: selectedKeyframe.trackIdx,
          keyframe_index: selectedKeyframe.kfIdx
        }).then(function() {
          selectedKeyframe = null;
          loadAnimation(currentAnimData.name);
        });
      });
    }

    // ---- Blend preview controls ----
    var blendToolbar = document.getElementById('anim-blend-toolbar');
    var blendSelect = document.getElementById('anim-blend-select');
    var blendSlider = document.getElementById('anim-blend-slider');
    var blendLabel = document.getElementById('anim-blend-weight');

    if (sel) sel.addEventListener('change', function() {
      // Show/hide blend toolbar when an animation is selected
      if (blendToolbar) blendToolbar.style.display = sel.value ? 'flex' : 'none';
      // Reset blend when primary changes
      if (blendSelect) blendSelect.value = '';
      if (blendSlider) { blendSlider.value = 0; blendSlider.disabled = true; }
      if (blendLabel) blendLabel.textContent = '0%';
      api('POST', '/api/animation/blend', { secondary: null, weight: 0 });
    });

    if (blendSelect) blendSelect.addEventListener('change', async function() {
      var sec = blendSelect.value || null;
      if (blendSlider) blendSlider.disabled = !sec;
      if (!sec) {
        if (blendSlider) blendSlider.value = 0;
        if (blendLabel) blendLabel.textContent = '0%';
      }
      var w = sec ? parseInt(blendSlider.value) / 100 : 0;
      await api('POST', '/api/animation/blend', { secondary: sec, weight: w });
    });

    if (blendSlider) blendSlider.addEventListener('input', async function() {
      var w = parseInt(blendSlider.value) / 100;
      if (blendLabel) blendLabel.textContent = Math.round(w * 100) + '%';
      var sec = blendSelect ? blendSelect.value : null;
      if (sec) {
        await api('POST', '/api/animation/blend', { secondary: sec, weight: w });
      }
    });
  }

  async function loadAnimation(name) {
    var data = await api('GET', '/api/animation?name=' + encodeURIComponent(name));
    if (!data) return;
    currentAnimData = data;
    renderAnimTracks();
    renderTimeline();
    updateTimeDisplay(0);
    updatePlayhead(0);
    // Show blend toolbar when animation loaded
    var blendToolbar = document.getElementById('anim-blend-toolbar');
    if (blendToolbar) blendToolbar.style.display = 'flex';
  }

  async function refreshAnimationList() {
    var sel = document.getElementById('anim-select');
    if (!sel) return;
    var data = await api('GET', '/api/animations');
    if (!data || !Array.isArray(data)) return;
    var curVal = sel.value;
    sel.innerHTML = '<option value="">-- No Animation --</option>';
    data.forEach(function(a) {
      var opt = document.createElement('option');
      opt.value = a.name; opt.textContent = a.name + ' (' + a.length.toFixed(1) + 's)';
      sel.appendChild(opt);
    });
    if (curVal) sel.value = curVal;
    // Also populate blend dropdown (excluding current primary)
    var blendSel = document.getElementById('anim-blend-select');
    if (blendSel) {
      var blendVal = blendSel.value;
      blendSel.innerHTML = '<option value="">-- None --</option>';
      data.forEach(function(a) {
        if (a.name !== curVal) {
          var opt = document.createElement('option');
          opt.value = a.name; opt.textContent = a.name;
          blendSel.appendChild(opt);
        }
      });
      if (blendVal) blendSel.value = blendVal;
    }
  }

  function renderAnimTracks() {
    var container = document.getElementById('anim-tracks');
    if (!container) return;
    container.innerHTML = '';
    if (!currentAnimData || !currentAnimData.tracks || currentAnimData.tracks.length === 0) {
      container.innerHTML = '<div class="anim-empty">No tracks</div>';
      return;
    }
    var trackTypeIcons = { property: 'P', method: 'M', audio: 'A' };
    var trackTypeColors = { property: '#7cb3f0', method: '#e0a050', audio: '#80d080' };
    for (var i = 0; i < currentAnimData.tracks.length; i++) {
      var track = currentAnimData.tracks[i];
      var row = document.createElement('div');
      row.className = 'anim-track-row';
      // Track type badge
      var badge = document.createElement('span');
      badge.className = 'anim-track-type-badge';
      var tt = track.track_type || 'property';
      badge.textContent = trackTypeIcons[tt] || 'P';
      badge.style.color = trackTypeColors[tt] || '#7cb3f0';
      badge.title = tt.charAt(0).toUpperCase() + tt.slice(1) + ' track';
      row.appendChild(badge);
      var nodeSpan = document.createElement('span');
      nodeSpan.className = 'anim-track-node';
      nodeSpan.textContent = track.node_path || '?';
      var sep = document.createTextNode(':');
      var propSpan = document.createElement('span');
      propSpan.className = 'anim-track-prop';
      propSpan.textContent = track.property || '?';
      row.appendChild(nodeSpan);
      row.appendChild(sep);
      row.appendChild(propSpan);
      // Delete button
      var delBtn = document.createElement('button');
      delBtn.className = 'anim-track-delete-btn';
      delBtn.textContent = 'x';
      delBtn.title = 'Delete track';
      delBtn.dataset.trackIndex = i;
      delBtn.addEventListener('click', function(e) {
        e.stopPropagation();
        var idx = parseInt(this.dataset.trackIndex);
        showConfirm('Delete this track?', { danger: true }).then(function(ok) {
          if (ok) api('POST', '/api/animation/track/delete', {
            animation: currentAnimData.name,
            track_index: idx
          }).then(function() { loadAnimation(currentAnimData.name); });
        });
      });
      row.appendChild(delBtn);
      container.appendChild(row);
    }
  }

  function renderTimeline() {
    var canvas = document.getElementById('anim-timeline-canvas');
    if (!canvas) return;
    var ctx = canvas.getContext('2d');
    if (!ctx) return;

    var trackCount = (currentAnimData && currentAnimData.tracks) ? currentAnimData.tracks.length : 0;
    var duration = (currentAnimData && currentAnimData.length) ? currentAnimData.length : 1;
    var width = Math.max(600, Math.ceil(duration * ANIM_PIXELS_PER_SEC) + 40);
    var height = Math.max(120, trackCount * 24 + 24);
    canvas.width = width;
    canvas.height = height;

    // Background
    ctx.fillStyle = getComputedStyle(document.body).getPropertyValue('--bg').trim() || '#0d0d0d';
    ctx.fillRect(0, 0, width, height);

    // Time ruler at top
    ctx.fillStyle = getComputedStyle(document.body).getPropertyValue('--panel').trim() || '#161616';
    ctx.fillRect(0, 0, width, 20);
    ctx.strokeStyle = getComputedStyle(document.body).getPropertyValue('--border').trim() || '#2a2a2a';
    ctx.beginPath(); ctx.moveTo(0, 20); ctx.lineTo(width, 20); ctx.stroke();

    // Time markers
    ctx.fillStyle = getComputedStyle(document.body).getPropertyValue('--text-dim').trim() || '#888';
    ctx.font = '9px monospace';
    ctx.textAlign = 'center';
    var step = 0.5;
    if (duration > 5) step = 1.0;
    if (duration > 20) step = 2.0;
    for (var t = 0; t <= duration; t += step) {
      var tx = t * ANIM_PIXELS_PER_SEC;
      ctx.fillText(t.toFixed(1) + 's', tx, 13);
      ctx.beginPath();
      ctx.moveTo(tx, 16); ctx.lineTo(tx, 20);
      ctx.stroke();
    }

    // Sub-ticks
    for (var st = 0; st <= duration; st += step / 4) {
      var stx = st * ANIM_PIXELS_PER_SEC;
      ctx.beginPath();
      ctx.moveTo(stx, 18); ctx.lineTo(stx, 20);
      ctx.stroke();
    }

    if (!currentAnimData || !currentAnimData.tracks) return;

    // Track rows
    for (var ti = 0; ti < currentAnimData.tracks.length; ti++) {
      var rowY = 20 + ti * 24;
      // Alternating row background
      if (ti % 2 === 0) {
        ctx.fillStyle = 'rgba(255,255,255,0.02)';
        ctx.fillRect(0, rowY, width, 24);
      }
      // Track separator line
      ctx.strokeStyle = getComputedStyle(document.body).getPropertyValue('--border').trim() || '#2a2a2a';
      ctx.beginPath(); ctx.moveTo(0, rowY + 24); ctx.lineTo(width, rowY + 24); ctx.stroke();

      // Keyframes
      var track = currentAnimData.tracks[ti];
      for (var ki = 0; ki < track.keyframes.length; ki++) {
        var kf = track.keyframes[ki];
        var kx = kf.time * ANIM_PIXELS_PER_SEC;
        var ky = rowY + 12;

        // Check if selected
        var isSel = selectedKeyframe && selectedKeyframe.trackIdx === ti && selectedKeyframe.kfIdx === ki;

        // Draw diamond keyframe marker
        ctx.beginPath();
        ctx.moveTo(kx, ky - 5);
        ctx.lineTo(kx + 5, ky);
        ctx.lineTo(kx, ky + 5);
        ctx.lineTo(kx - 5, ky);
        ctx.closePath();

        var trackColors = { property: '#7cb3f0', method: '#e0a050', audio: '#80d080' };
        var trackColor = trackColors[track.track_type] || getComputedStyle(document.body).getPropertyValue('--accent').trim() || '#579eff';
        if (isSel) {
          ctx.fillStyle = '#ffffff';
          ctx.strokeStyle = trackColor;
          ctx.lineWidth = 2;
        } else {
          ctx.fillStyle = trackColor;
          ctx.strokeStyle = 'rgba(255,255,255,0.3)';
          ctx.lineWidth = 1;
        }
        ctx.fill();
        ctx.stroke();
        ctx.lineWidth = 1;

        // Draw connection line between keyframes (curved for bezier)
        if (ki > 0) {
          var prevKx = track.keyframes[ki - 1].time * ANIM_PIXELS_PER_SEC;
          var hasBezier = kf.transition && typeof kf.transition === 'object' && kf.transition.type === 'cubic_bezier';
          if (hasBezier) {
            ctx.strokeStyle = 'rgba(87,158,255,0.5)';
            ctx.lineWidth = 1.5;
            ctx.beginPath();
            ctx.moveTo(prevKx + 5, ky);
            var seg = kx - prevKx - 10;
            var cx1 = prevKx + 5 + seg * kf.transition.x1;
            var cy1 = ky - seg * 0.3 * kf.transition.y1;
            var cx2 = prevKx + 5 + seg * kf.transition.x2;
            var cy2 = ky - seg * 0.3 * kf.transition.y2;
            ctx.bezierCurveTo(cx1, cy1, cx2, cy2, kx - 5, ky);
            ctx.stroke();
            ctx.lineWidth = 1;
          } else {
            ctx.strokeStyle = 'rgba(87,158,255,0.3)';
            ctx.beginPath();
            ctx.moveTo(prevKx + 5, ky);
            ctx.lineTo(kx - 5, ky);
            ctx.stroke();
          }
        }
      }
    }

    // Duration end marker
    var endX = duration * ANIM_PIXELS_PER_SEC;
    ctx.strokeStyle = 'rgba(224,80,80,0.5)';
    ctx.setLineDash([4, 4]);
    ctx.beginPath(); ctx.moveTo(endX, 0); ctx.lineTo(endX, height); ctx.stroke();
    ctx.setLineDash([]);
  }

  function updatePlayhead(time) {
    var playhead = document.getElementById('anim-playhead');
    if (!playhead) return;
    playhead.style.left = (time * ANIM_PIXELS_PER_SEC) + 'px';
  }

  function updateTimeDisplay(currentTime) {
    var el = document.getElementById('anim-time-display');
    if (!el) return;
    var total = (currentAnimData && currentAnimData.length) ? currentAnimData.length : 0;
    el.textContent = currentTime.toFixed(2) + ' / ' + total.toFixed(2);
  }

  function showAddTrackDialog(animName) {
    // Remove any existing dialog
    var existing = document.getElementById('add-track-dialog-overlay');
    if (existing) existing.remove();
    // Create overlay
    var overlay = document.createElement('div');
    overlay.id = 'add-track-dialog-overlay';
    overlay.className = 'dialog-overlay';
    var dialog = document.createElement('div');
    dialog.className = 'dialog-box';
    dialog.innerHTML =
      '<div class="dialog-title">Add Track</div>' +
      '<div class="dialog-field">' +
        '<label>Track Type:</label>' +
        '<div class="dialog-track-type-btns">' +
          '<button class="track-type-btn selected" data-type="property">Property</button>' +
          '<button class="track-type-btn" data-type="method">Method</button>' +
          '<button class="track-type-btn" data-type="audio">Audio</button>' +
        '</div>' +
      '</div>' +
      '<div class="dialog-field">' +
        '<label>Node Path:</label>' +
        '<input type="text" id="add-track-node" placeholder="e.g. Player" value="" />' +
      '</div>' +
      '<div class="dialog-field" id="add-track-prop-field">' +
        '<label id="add-track-prop-label">Property:</label>' +
        '<input type="text" id="add-track-prop" placeholder="e.g. position" value="position" />' +
      '</div>' +
      '<div class="dialog-hint" id="add-track-hint">Animates a node property over time.</div>' +
      '<div class="dialog-actions">' +
        '<button id="add-track-cancel">Cancel</button>' +
        '<button id="add-track-ok" class="primary">Add</button>' +
      '</div>';
    overlay.appendChild(dialog);
    document.body.appendChild(overlay);
    // Track type selection
    var selectedType = 'property';
    var typeBtns = dialog.querySelectorAll('.track-type-btn');
    var propLabel = dialog.querySelector('#add-track-prop-label');
    var propInput = dialog.querySelector('#add-track-prop');
    var hint = dialog.querySelector('#add-track-hint');
    var hints = {
      property: 'Animates a node property over time.',
      method: 'Calls a method on the node at each keyframe.',
      audio: 'Triggers audio playback at keyframe times.'
    };
    var propLabels = { property: 'Property:', method: 'Method:', audio: 'Label:' };
    var propPlaceholders = { property: 'e.g. position', method: 'e.g. jump', audio: 'e.g. sfx_footstep' };
    var propDefaults = { property: 'position', method: 'call', audio: 'audio' };
    for (var bi = 0; bi < typeBtns.length; bi++) {
      typeBtns[bi].addEventListener('click', function() {
        for (var j = 0; j < typeBtns.length; j++) typeBtns[j].classList.remove('selected');
        this.classList.add('selected');
        selectedType = this.dataset.type;
        propLabel.textContent = propLabels[selectedType];
        propInput.placeholder = propPlaceholders[selectedType];
        propInput.value = propDefaults[selectedType];
        hint.textContent = hints[selectedType];
      });
    }
    // Cancel
    dialog.querySelector('#add-track-cancel').addEventListener('click', function() { overlay.remove(); });
    overlay.addEventListener('click', function(e) { if (e.target === overlay) overlay.remove(); });
    // OK
    dialog.querySelector('#add-track-ok').addEventListener('click', async function() {
      var nodePath = dialog.querySelector('#add-track-node').value.trim();
      var prop = dialog.querySelector('#add-track-prop').value.trim();
      if (!nodePath) { dialog.querySelector('#add-track-node').focus(); return; }
      if (!prop) { dialog.querySelector('#add-track-prop').focus(); return; }
      var res = await api('POST', '/api/animation/track/add', {
        animation: animName,
        node_path: nodePath,
        property: prop,
        track_type: selectedType
      });
      overlay.remove();
      if (res && res.ok) {
        logMessage('info', 'Added ' + selectedType + ' track: ' + nodePath + ':' + prop);
        await loadAnimation(animName);
      }
    });
    // Focus node path input
    setTimeout(function() { dialog.querySelector('#add-track-node').focus(); }, 50);
  }

  function startPlaybackPolling() {
    stopPlaybackPolling();
    animPlaybackTimer = setInterval(async function() {
      var status = await api('GET', '/api/animation/status');
      if (!status) return;
      if (status.playing) {
        updatePlayhead(status.current_time || 0);
        updateTimeDisplay(status.current_time || 0);
      } else {
        animPlaying = false;
        stopPlaybackPolling();
      }
      if (status.recording !== undefined) {
        animRecording = status.recording;
        var recBtn = document.getElementById('anim-record-btn');
        if (recBtn) recBtn.classList.toggle('active', animRecording);
        document.body.classList.toggle('anim-recording', animRecording);
      }
    }, 50);
  }

  function stopPlaybackPolling() {
    if (animPlaybackTimer) { clearInterval(animPlaybackTimer); animPlaybackTimer = null; }
  }

  // ---- Curve editor (pat-o51nk) ----
  var curveEditorState = null;
  var curveEditorDrag = null;

  var CURVE_PRESETS = [
    { name: 'Linear', x1: 0, y1: 0, x2: 1, y2: 1 },
    { name: 'Ease In', x1: 0.42, y1: 0, x2: 1, y2: 1 },
    { name: 'Ease Out', x1: 0, y1: 0, x2: 0.58, y2: 1 },
    { name: 'Ease In-Out', x1: 0.42, y1: 0, x2: 0.58, y2: 1 },
    { name: 'Ease In Quad', x1: 0.55, y1: 0.085, x2: 0.68, y2: 0.53 },
    { name: 'Ease Out Quad', x1: 0.25, y1: 0.46, x2: 0.45, y2: 0.94 },
    { name: 'Ease In Back', x1: 0.6, y1: -0.28, x2: 0.735, y2: 0.045 },
    { name: 'Ease Out Back', x1: 0.175, y1: 0.885, x2: 0.32, y2: 1.275 },
  ];

  function openCurveEditor(animation, trackIdx, kfIdx, x1, y1, x2, y2) {
    curveEditorState = { animation: animation, trackIdx: trackIdx, kfIdx: kfIdx,
      x1: x1 !== undefined ? x1 : 0.42, y1: y1 !== undefined ? y1 : 0,
      x2: x2 !== undefined ? x2 : 0.58, y2: y2 !== undefined ? y2 : 1 };
    updateCurveInputs();
    renderCurveCanvas();
    document.getElementById('curve-editor-overlay').classList.add('open');
  }

  function closeCurveEditor() {
    document.getElementById('curve-editor-overlay').classList.remove('open');
    curveEditorState = null;
  }

  function updateCurveInputs() {
    if (!curveEditorState) return;
    document.getElementById('curve-x1').value = curveEditorState.x1.toFixed(3);
    document.getElementById('curve-y1').value = curveEditorState.y1.toFixed(3);
    document.getElementById('curve-x2').value = curveEditorState.x2.toFixed(3);
    document.getElementById('curve-y2').value = curveEditorState.y2.toFixed(3);
  }

  function readCurveInputs() {
    if (!curveEditorState) return;
    curveEditorState.x1 = Math.min(1, Math.max(0, parseFloat(document.getElementById('curve-x1').value) || 0));
    curveEditorState.y1 = parseFloat(document.getElementById('curve-y1').value) || 0;
    curveEditorState.x2 = Math.min(1, Math.max(0, parseFloat(document.getElementById('curve-x2').value) || 0));
    curveEditorState.y2 = parseFloat(document.getElementById('curve-y2').value) || 0;
    renderCurveCanvas();
  }

  function applyCurvePreset(preset) {
    if (!curveEditorState) return;
    curveEditorState.x1 = preset.x1; curveEditorState.y1 = preset.y1;
    curveEditorState.x2 = preset.x2; curveEditorState.y2 = preset.y2;
    updateCurveInputs();
    renderCurveCanvas();
  }

  function renderCurveCanvas() {
    var canvas = document.getElementById('curve-editor-canvas');
    if (!canvas || !curveEditorState) return;
    var ctx = canvas.getContext('2d');
    var w = canvas.width, h = canvas.height;
    var pad = 16;
    var gw = w - pad * 2, gh = h - pad * 2;
    function toX(v) { return pad + v * gw; }
    function toY(v) { return pad + (1 - v) * gh; }

    ctx.clearRect(0, 0, w, h);

    // Grid
    ctx.strokeStyle = 'rgba(255,255,255,0.06)';
    ctx.lineWidth = 1;
    for (var i = 0; i <= 4; i++) {
      var frac = i / 4;
      ctx.beginPath(); ctx.moveTo(toX(frac), pad); ctx.lineTo(toX(frac), pad + gh); ctx.stroke();
      ctx.beginPath(); ctx.moveTo(pad, toY(frac)); ctx.lineTo(pad + gw, toY(frac)); ctx.stroke();
    }

    // Linear reference
    ctx.strokeStyle = 'rgba(255,255,255,0.15)';
    ctx.setLineDash([4, 4]);
    ctx.beginPath(); ctx.moveTo(toX(0), toY(0)); ctx.lineTo(toX(1), toY(1)); ctx.stroke();
    ctx.setLineDash([]);

    // Bezier curve
    var s = curveEditorState;
    ctx.strokeStyle = getComputedStyle(document.documentElement).getPropertyValue('--accent').trim() || '#579eff';
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(toX(0), toY(0));
    ctx.bezierCurveTo(toX(s.x1), toY(s.y1), toX(s.x2), toY(s.y2), toX(1), toY(1));
    ctx.stroke();

    // Control handle lines
    ctx.strokeStyle = 'rgba(255,255,255,0.3)';
    ctx.lineWidth = 1;
    ctx.beginPath(); ctx.moveTo(toX(0), toY(0)); ctx.lineTo(toX(s.x1), toY(s.y1)); ctx.stroke();
    ctx.beginPath(); ctx.moveTo(toX(1), toY(1)); ctx.lineTo(toX(s.x2), toY(s.y2)); ctx.stroke();

    // Control points
    ctx.fillStyle = '#e05050';
    ctx.beginPath(); ctx.arc(toX(s.x1), toY(s.y1), 5, 0, Math.PI * 2); ctx.fill();
    ctx.fillStyle = '#50c878';
    ctx.beginPath(); ctx.arc(toX(s.x2), toY(s.y2), 5, 0, Math.PI * 2); ctx.fill();

    // Endpoints
    ctx.fillStyle = 'rgba(255,255,255,0.5)';
    ctx.beginPath(); ctx.arc(toX(0), toY(0), 3, 0, Math.PI * 2); ctx.fill();
    ctx.beginPath(); ctx.arc(toX(1), toY(1), 3, 0, Math.PI * 2); ctx.fill();
  }

  function setupCurveEditor() {
    var canvas = document.getElementById('curve-editor-canvas');
    if (!canvas) return;
    var pad = 16;
    function fromCanvas(cx, cy) {
      var gw = canvas.width - pad * 2, gh = canvas.height - pad * 2;
      return { x: Math.min(1, Math.max(0, (cx - pad) / gw)), y: 1 - (cy - pad) / gh };
    }

    canvas.addEventListener('mousedown', function(e) {
      if (!curveEditorState) return;
      var rect = canvas.getBoundingClientRect();
      var scale = canvas.width / rect.width;
      var pos = fromCanvas((e.clientX - rect.left) * scale, (e.clientY - rect.top) * scale);
      var s = curveEditorState;
      var d1 = Math.hypot(pos.x - s.x1, pos.y - s.y1);
      var d2 = Math.hypot(pos.x - s.x2, pos.y - s.y2);
      curveEditorDrag = (d1 < 0.08 || d1 < d2) ? 'cp1' : 'cp2';
    });

    canvas.addEventListener('mousemove', function(e) {
      if (!curveEditorDrag || !curveEditorState) return;
      var rect = canvas.getBoundingClientRect();
      var scale = canvas.width / rect.width;
      var pos = fromCanvas((e.clientX - rect.left) * scale, (e.clientY - rect.top) * scale);
      if (curveEditorDrag === 'cp1') {
        curveEditorState.x1 = pos.x; curveEditorState.y1 = pos.y;
      } else {
        curveEditorState.x2 = pos.x; curveEditorState.y2 = pos.y;
      }
      updateCurveInputs();
      renderCurveCanvas();
    });

    window.addEventListener('mouseup', function() { curveEditorDrag = null; });

    // Preset buttons
    var presetsDiv = document.getElementById('curve-presets');
    if (presetsDiv) {
      CURVE_PRESETS.forEach(function(p) {
        var btn = document.createElement('button');
        btn.textContent = p.name;
        btn.addEventListener('click', function() { applyCurvePreset(p); });
        presetsDiv.appendChild(btn);
      });
    }

    // Input handlers
    ['curve-x1','curve-y1','curve-x2','curve-y2'].forEach(function(id) {
      var el = document.getElementById(id);
      if (el) el.addEventListener('input', readCurveInputs);
    });

    // Apply
    var applyBtn = document.getElementById('curve-apply-btn');
    if (applyBtn) {
      applyBtn.addEventListener('click', async function() {
        if (!curveEditorState) return;
        var s = curveEditorState;
        await api('POST', '/api/animation/keyframe/transition', {
          animation: s.animation, track_index: s.trackIdx, keyframe_index: s.kfIdx,
          transition: 'cubic_bezier', x1: s.x1, y1: s.y1, x2: s.x2, y2: s.y2
        });
        await loadAnimation(s.animation);
        closeCurveEditor();
        renderTimeline();
      });
    }

    // Cancel
    var cancelBtn = document.getElementById('curve-cancel-btn');
    if (cancelBtn) cancelBtn.addEventListener('click', closeCurveEditor);

    // Overlay close
    var overlay = document.getElementById('curve-editor-overlay');
    if (overlay) overlay.addEventListener('click', function(e) { if (e.target === overlay) closeCurveEditor(); });

    // Curve button in animation toolbar
    var curveBtn = document.getElementById('anim-curve-btn');
    if (curveBtn) {
      curveBtn.addEventListener('click', function() {
        if (!selectedKeyframe || !currentAnimData) {
          logMessage('warn', 'Select a keyframe first'); return;
        }
        var track = currentAnimData.tracks[selectedKeyframe.trackIdx];
        var kf = track.keyframes[selectedKeyframe.kfIdx];
        var t = kf.transition;
        var x1 = 0.42, y1 = 0, x2 = 0.58, y2 = 1;
        if (t && typeof t === 'object' && t.type === 'cubic_bezier') {
          x1 = t.x1; y1 = t.y1; x2 = t.x2; y2 = t.y2;
        }
        openCurveEditor(currentAnimData.name, selectedKeyframe.trackIdx, selectedKeyframe.kfIdx, x1, y1, x2, y2);
      });
    }
  }

  // ---- Settings dialog (pat-sbdts: categorized) ----
  function setupSettingsDialog() {
    var btn = document.getElementById('btn-settings');
    var dialog = document.getElementById('settings-dialog');
    if (!btn || !dialog) return;
    btn.addEventListener('click', function() { dialog.style.display = 'flex'; loadKeybindings(); loadPlugins(); });
    document.getElementById('settings-close').addEventListener('click', function() { dialog.style.display = 'none'; });

    // Tab switching
    var tabs = dialog.querySelectorAll('.eset-tab');
    var sections = dialog.querySelectorAll('.eset-section');
    function switchTab(tabId) {
      tabs.forEach(function(t) {
        if (t.getAttribute('data-tab') === tabId) {
          t.classList.add('active');
          t.style.color = '';
          t.style.borderBottomColor = '';
        } else {
          t.classList.remove('active');
          t.style.color = 'var(--text-dim)';
          t.style.borderBottomColor = 'transparent';
        }
      });
      sections.forEach(function(s) {
        s.style.display = s.getAttribute('data-tab') === tabId ? '' : 'none';
      });
    }
    tabs.forEach(function(t) {
      t.addEventListener('click', function() { switchTab(t.getAttribute('data-tab')); });
    });

    // Load general settings
    api('GET', '/api/settings').then(function(data) {
      if (!data) return;
      var snapCheck = document.getElementById('set-grid-snap');
      var snapSize = document.getElementById('set-snap-size');
      var gridVis = document.getElementById('set-grid-visible');
      var rulerVis = document.getElementById('set-rulers-visible');
      var fontSize = document.getElementById('set-font-size');
      if (snapCheck) snapCheck.checked = !!data.grid_snap_enabled;
      if (snapSize) snapSize.value = data.grid_snap_size || 8;
      if (gridVis) gridVis.checked = data.grid_visible !== false;
      if (rulerVis) rulerVis.checked = data.rulers_visible !== false;
      if (fontSize) fontSize.value = data.font_size || 'medium';
      var themeEl = document.getElementById('set-theme');
      if (themeEl && data.theme) { themeEl.value = data.theme; applyTheme(data.theme); }
      var fpsEl = document.getElementById('set-physics-fps');
      if (fpsEl && data.physics_fps) fpsEl.value = data.physics_fps;
    });

    // Save general+theme settings on change
    dialog.addEventListener('change', function(e) {
      var themeVal = (document.getElementById('set-theme') || {}).value || 'dark';
      applyTheme(themeVal);
      // Apply accent color
      var accentEl = document.getElementById('set-accent-color');
      if (accentEl) { document.documentElement.style.setProperty('--accent', accentEl.value); }
      var settings = {
        grid_snap_enabled: !!(document.getElementById('set-grid-snap') || {}).checked,
        grid_snap_size: parseInt((document.getElementById('set-snap-size') || {}).value) || 8,
        grid_visible: !!(document.getElementById('set-grid-visible') || {}).checked,
        rulers_visible: !!(document.getElementById('set-rulers-visible') || {}).checked,
        font_size: (document.getElementById('set-font-size') || {}).value || 'medium',
        theme: themeVal,
        physics_fps: parseInt((document.getElementById('set-physics-fps') || {}).value) || 60
      };
      api('POST', '/api/settings', settings);
      var snapEl = document.getElementById('status-snap');
      if (snapEl) snapEl.textContent = settings.grid_snap_enabled ? settings.grid_snap_size + 'px' : 'Off';
    });

    // Keybinding rendering
    function loadKeybindings() {
      api('GET', '/api/keybindings').then(function(data) {
        if (!data || !data.keybindings) return;
        var list = document.getElementById('eset-kb-list');
        if (!list) return;
        list.innerHTML = '';
        data.keybindings.forEach(function(kb) {
          var row = document.createElement('div');
          row.className = 'eset-kb-row';
          row.innerHTML = '<span class="kb-action">' + kb.action + '</span>' +
            '<span class="kb-desc">' + kb.description + '</span>' +
            '<span class="kb-keys" data-action="' + kb.action + '" title="Click to rebind">' + kb.keys + '</span>';
          list.appendChild(row);
        });
        // Rebind click handler
        list.querySelectorAll('.kb-keys').forEach(function(el) {
          el.addEventListener('click', function() {
            var action = el.getAttribute('data-action');
            el.textContent = 'Press key...';
            el.style.borderColor = 'var(--accent)';
            function capture(e) {
              e.preventDefault(); e.stopPropagation();
              var parts = [];
              if (e.ctrlKey) parts.push('Ctrl');
              if (e.shiftKey) parts.push('Shift');
              if (e.altKey) parts.push('Alt');
              if (e.key !== 'Control' && e.key !== 'Shift' && e.key !== 'Alt') {
                parts.push(e.key.length === 1 ? e.key.toUpperCase() : e.key);
              }
              var combo = parts.join('+');
              if (combo) {
                el.textContent = combo;
                el.style.borderColor = '';
                api('POST', '/api/keybindings', { action: action, keys: combo });
              }
              document.removeEventListener('keydown', capture, true);
            }
            document.addEventListener('keydown', capture, true);
          });
        });
      });
    }

    // Keybinding search
    var kbSearch = document.getElementById('eset-kb-search');
    if (kbSearch) {
      kbSearch.addEventListener('input', function() {
        var q = kbSearch.value.toLowerCase();
        var rows = document.querySelectorAll('#eset-kb-list .eset-kb-row');
        rows.forEach(function(row) {
          var text = row.textContent.toLowerCase();
          row.style.display = text.indexOf(q) >= 0 ? '' : 'none';
        });
      });
    }

    // Plugin rendering
    function loadPlugins() {
      api('GET', '/api/plugins').then(function(data) {
        if (!data) return;
        var list = document.getElementById('eset-plugin-list');
        if (!list) return;
        var plugins = data.plugins || [];
        if (plugins.length === 0) {
          list.innerHTML = '<div style="color:var(--text-dim);font-size:11px;padding:8px;">No plugins registered.</div>';
          return;
        }
        list.innerHTML = '';
        plugins.forEach(function(p) {
          var row = document.createElement('div');
          row.className = 'eset-plugin-row';
          row.innerHTML = '<input type="checkbox" ' + (p.enabled ? 'checked' : '') +
            ' data-plugin="' + p.name + '">' +
            '<span class="plugin-name">' + p.name + '</span>' +
            '<span class="plugin-desc">' + (p.description || '') + '</span>';
          row.querySelector('input').addEventListener('change', function() {
            api('POST', '/api/plugins/toggle', { name: p.name, enabled: this.checked });
          });
          list.appendChild(row);
        });
      });
    }
  }

  // pat-kj4 / pat-c4zlm: Project settings dialog with categorized property editing
  var psetActiveCat = 'application';

  function setupProjectSettingsDialog() {
    var dialog = document.getElementById('project-settings-dialog');
    if (!dialog) return;

    // Category tab switching
    var catTabs = dialog.querySelectorAll('.pset-cat');
    catTabs.forEach(function(tab) {
      tab.addEventListener('click', function() {
        psetActiveCat = tab.getAttribute('data-cat');
        catTabs.forEach(function(t) { t.classList.remove('active'); });
        tab.classList.add('active');
        psetShowCategory(psetActiveCat);
        if (psetActiveCat === 'input') renderInputMap(); // pat-db37w
      });
    });

    // pat-db37w: Input Map add-action.
    var addInputBtn = document.getElementById('pset-input-add');
    if (addInputBtn) addInputBtn.addEventListener('click', function() {
      var inp = document.getElementById('pset-input-new-action');
      var name = (inp.value || '').trim();
      if (!name) return;
      api('POST', '/api/input_map', { action: name, keys: [] }).then(function() { inp.value = ''; renderInputMap(); });
    });

    // Search/filter
    var searchInput = document.getElementById('pset-search');
    if (searchInput) {
      searchInput.addEventListener('input', function() {
        var q = searchInput.value.toLowerCase();
        var sections = dialog.querySelectorAll('.pset-section');
        if (!q) {
          psetShowCategory(psetActiveCat);
          return;
        }
        // Show all sections, filter rows
        sections.forEach(function(sec) { sec.style.display = ''; });
        var rows = dialog.querySelectorAll('.settings-row');
        rows.forEach(function(row) {
          var label = row.querySelector('.settings-label');
          var text = label ? label.textContent.toLowerCase() : '';
          row.style.display = text.indexOf(q) >= 0 ? '' : 'none';
        });
        // Hide sections with no visible rows
        sections.forEach(function(sec) {
          var visible = sec.querySelectorAll('.settings-row:not([style*=\"display: none\"])');
          if (visible.length === 0) sec.style.display = 'none';
        });
      });
    }

    // Close / cancel
    var closeBtn = document.getElementById('pset-close');
    if (closeBtn) closeBtn.addEventListener('click', function() { dialog.style.display = 'none'; });
    document.getElementById('pset-cancel').addEventListener('click', function() { dialog.style.display = 'none'; });

    // Save — collect all fields
    document.getElementById('pset-save').addEventListener('click', function() {
      var settings = psetCollectValues();
      api('POST', '/api/project_settings', settings).then(function() { dialog.style.display = 'none'; });
    });

    // Load current values on setup
    psetLoadValues();

    // Click backdrop to close
    dialog.addEventListener('click', function(e) { if (e.target === dialog) dialog.style.display = 'none'; });
  }

  function psetShowCategory(cat) {
    var sections = document.querySelectorAll('#pset-props .pset-section');
    sections.forEach(function(sec) {
      sec.style.display = sec.getAttribute('data-cat') === cat ? '' : 'none';
    });
    // Reset row visibility
    var rows = document.querySelectorAll('#pset-props .settings-row');
    rows.forEach(function(row) { row.style.display = ''; });
  }

  // pat-db37w: live Input Map editor (Project Settings > Input Map). Backed by
  // /api/input_map — each action lists its mapped keys with add/remove, and
  // actions themselves can be added/removed.
  // Extract the bound key names from an action's events (Godot InputMap model:
  // each event is {Key:"x"} | {MouseButton:n} | {JoyButton:n}).
  function inputActionKeys(action) {
    return (action.events || []).map(function(ev) {
      if (typeof ev === 'string') return ev;
      if (ev && ev.Key !== undefined) return ev.Key;
      if (ev && ev.MouseButton !== undefined) return 'Mouse ' + ev.MouseButton;
      if (ev && ev.JoyButton !== undefined) return 'Joy ' + ev.JoyButton;
      return '';
    }).filter(function(k) { return k !== ''; });
  }
  function renderInputMap() {
    var host = document.getElementById('pset-input-map');
    if (!host) return;
    api('GET', '/api/input_map').then(function(data) {
      host.innerHTML = '';
      // The model serializes actions as an object keyed by name.
      var actionsObj = (data && data.actions) || {};
      Object.keys(actionsObj).sort().forEach(function(name) {
        var keys = inputActionKeys(actionsObj[name]);
        var row = document.createElement('div');
        row.className = 'settings-row input-action-row';
        var label = document.createElement('span'); label.className = 'settings-label'; label.textContent = name;
        var val = document.createElement('div'); val.className = 'settings-value';
        keys.forEach(function(k) {
          var chip = document.createElement('span'); chip.className = 'input-key-chip';
          chip.textContent = (k === ' ' ? 'Space' : k);
          var x = document.createElement('span'); x.className = 'input-key-x'; x.textContent = '×';
          x.addEventListener('click', function() {
            var nk = keys.filter(function(kk) { return kk !== k; });
            api('POST', '/api/input_map', { action: name, keys: nk }).then(renderInputMap);
          });
          chip.appendChild(x); val.appendChild(chip);
        });
        var addKey = document.createElement('input'); addKey.type = 'text'; addKey.placeholder = '+ key'; addKey.className = 'input-key-add';
        addKey.addEventListener('keydown', function(e) {
          if (e.key === 'Enter' && addKey.value.trim()) {
            var nk = keys.concat([addKey.value.trim()]);
            api('POST', '/api/input_map', { action: name, keys: nk }).then(renderInputMap);
          }
        });
        val.appendChild(addKey);
        var del = document.createElement('span'); del.className = 'input-action-del'; del.textContent = '🗑'; del.title = 'Remove action';
        del.addEventListener('click', function() {
          api('POST', '/api/input_map', { action: name, remove: true }).then(renderInputMap);
        });
        val.appendChild(del);
        row.appendChild(label); row.appendChild(val); host.appendChild(row);
      });
    });
  }

  function psetCollectValues() {
    function val(id) { var el = document.getElementById(id); return el ? el.value : ''; }
    function numVal(id, def) { var v = parseFloat(val(id)); return isNaN(v) ? def : v; }
    function intVal(id, def) { var v = parseInt(val(id)); return isNaN(v) ? def : v; }
    function boolVal(id) { var el = document.getElementById(id); return el ? el.checked : false; }
    return {
      project_name: val('pset-name'),
      description: val('pset-desc'),
      icon: val('pset-icon'),
      main_scene: val('pset-main-scene'),
      resolution_w: intVal('pset-res-w', 1152),
      resolution_h: intVal('pset-res-h', 648),
      stretch_mode: val('pset-stretch-mode'),
      stretch_aspect: val('pset-stretch-aspect'),
      fullscreen: boolVal('pset-fullscreen'),
      vsync: boolVal('pset-vsync'),
      physics_fps: intVal('pset-physics-fps', 60),
      gravity: numVal('pset-gravity', 980.0),
      linear_damp: numVal('pset-linear-damp', 0.1),
      angular_damp: numVal('pset-angular-damp', 1.0),
      bus_layout: val('pset-bus-layout'),
      master_volume_db: numVal('pset-master-vol', 0.0),
      audio_input: boolVal('pset-audio-input'),
      renderer: val('pset-renderer'),
      anti_aliasing: val('pset-anti-aliasing'),
      environment_default: val('pset-env-default')
    };
  }

  function psetLoadValues() {
    api('GET', '/api/project_settings').then(function(data) {
      if (!data || !data.categories) return;
      // Build flat key->value map from categorized response
      var vals = {};
      data.categories.forEach(function(cat) {
        (cat.properties || []).forEach(function(p) { vals[p.key] = p.value; });
      });
      function setVal(id, key) { var el = document.getElementById(id); if (el) el.value = vals[key] != null ? vals[key] : ''; }
      function setBool(id, key) { var el = document.getElementById(id); if (el) el.checked = !!vals[key]; }
      setVal('pset-name', 'project_name');
      setVal('pset-desc', 'description');
      setVal('pset-icon', 'icon');
      setVal('pset-main-scene', 'main_scene');
      setVal('pset-res-w', 'resolution_w');
      setVal('pset-res-h', 'resolution_h');
      setVal('pset-stretch-mode', 'stretch_mode');
      setVal('pset-stretch-aspect', 'stretch_aspect');
      setBool('pset-fullscreen', 'fullscreen');
      setBool('pset-vsync', 'vsync');
      setVal('pset-physics-fps', 'physics_fps');
      setVal('pset-gravity', 'gravity');
      setVal('pset-linear-damp', 'linear_damp');
      setVal('pset-angular-damp', 'angular_damp');
      setVal('pset-bus-layout', 'bus_layout');
      setVal('pset-master-vol', 'master_volume_db');
      setBool('pset-audio-input', 'audio_input');
      setVal('pset-renderer', 'renderer');
      setVal('pset-anti-aliasing', 'anti_aliasing');
      setVal('pset-env-default', 'environment_default');
      // Store categories metadata for potential dynamic use
      window._psetCategories = data.categories;
    });
  }

  // ---- Export dialog (pat-4cexp) ----
  var exportPresets = [];
  var exportSelectedIdx = -1;

  function openExportDialog() {
    var dlg = document.getElementById('export-dialog');
    if (!dlg) return;
    dlg.style.display = 'flex';
    loadExportPresets();
  }

  function loadExportPresets() {
    api('GET', '/api/export/presets').then(function(data) {
      exportPresets = Array.isArray(data) ? data : [];
      renderExportPresetList();
      if (exportPresets.length > 0 && exportSelectedIdx < 0) { exportSelectedIdx = 0; }
      renderExportPresetForm();
    });
  }

  function renderExportPresetList() {
    var list = document.getElementById('export-preset-list');
    if (!list) return;
    list.innerHTML = '';
    for (var i = 0; i < exportPresets.length; i++) {
      var p = exportPresets[i];
      var el = document.createElement('div');
      el.style.cssText = 'padding:6px 12px;font-size:11px;cursor:pointer;border-left:3px solid ' + (i === exportSelectedIdx ? 'var(--accent)' : 'transparent') + ';background:' + (i === exportSelectedIdx ? 'rgba(87,158,255,0.1)' : 'transparent');
      el.textContent = p.name + ' (' + (p.platform || '?') + ')';
      el.setAttribute('data-idx', i);
      el.addEventListener('click', function() {
        exportSelectedIdx = parseInt(this.getAttribute('data-idx'));
        renderExportPresetList();
        renderExportPresetForm();
      });
      list.appendChild(el);
    }
  }

  function renderExportPresetForm() {
    var noPreset = document.getElementById('export-no-preset');
    var form = document.getElementById('export-preset-form');
    if (exportSelectedIdx < 0 || exportSelectedIdx >= exportPresets.length) {
      if (noPreset) noPreset.style.display = '';
      if (form) form.style.display = 'none';
      return;
    }
    if (noPreset) noPreset.style.display = 'none';
    if (form) form.style.display = '';
    var p = exportPresets[exportSelectedIdx];
    document.getElementById('exp-name').value = p.name || '';
    document.getElementById('exp-platform').textContent = p.platform || '';
    document.getElementById('exp-build').value = p.build_profile || 'release';
    document.getElementById('exp-path').value = p.export_path || '';
    document.getElementById('exp-app-name').value = p.app_name || '';
    document.getElementById('exp-include').value = (p.include_filters || []).join(', ');
    document.getElementById('exp-exclude').value = (p.exclude_filters || []).join(', ');
    var customList = document.getElementById('exp-custom-list');
    customList.innerHTML = '';
    var props = p.custom_properties || {};
    var keys = Object.keys(props).sort();
    for (var k = 0; k < keys.length; k++) {
      var key = keys[k];
      var row = document.createElement('div');
      row.className = 'settings-row';
      row.innerHTML = '<span class="settings-label">' + key + '</span><div class="settings-value"><input type="text" data-cprop="' + key + '" value="' + (props[key] || '') + '" style="width:100%;"></div>';
      customList.appendChild(row);
    }
  }

  function setupExportDialog() {
    var dlg = document.getElementById('export-dialog');
    if (!dlg) return;
    document.getElementById('export-close').addEventListener('click', function() { dlg.style.display = 'none'; });
    dlg.addEventListener('click', function(e) { if (e.target === dlg) dlg.style.display = 'none'; });

    document.getElementById('export-add-btn').addEventListener('click', function() {
      var platform = document.getElementById('export-add-platform').value;
      var labels = {windows:'Windows',linux:'Linux',macos:'macOS',web:'Web',android:'Android',ios:'iOS'};
      var name = (labels[platform] || platform) + ' Export';
      var count = exportPresets.filter(function(p){return p.name.indexOf(labels[platform]||platform)===0;}).length;
      if (count > 0) name += ' ' + (count + 1);
      var preset = { name: name, platform: platform, build_profile: 'release', export_path: '', app_name: '', include_filters: ['*'], exclude_filters: [], custom_properties: {} };
      api('POST', '/api/export/presets', preset).then(function() {
        exportSelectedIdx = exportPresets.length;
        loadExportPresets();
      });
    });

    document.getElementById('exp-delete').addEventListener('click', function() {
      if (exportSelectedIdx < 0 || exportSelectedIdx >= exportPresets.length) return;
      var name = exportPresets[exportSelectedIdx].name;
      api('POST', '/api/export/presets/delete', { name: name }).then(function() {
        exportSelectedIdx = Math.max(0, exportSelectedIdx - 1);
        if (exportPresets.length <= 1) exportSelectedIdx = -1;
        loadExportPresets();
      });
    });

    document.getElementById('exp-save').addEventListener('click', function() {
      if (exportSelectedIdx < 0 || exportSelectedIdx >= exportPresets.length) return;
      var customProps = {};
      document.querySelectorAll('#exp-custom-list input[data-cprop]').forEach(function(inp) {
        customProps[inp.getAttribute('data-cprop')] = inp.value;
      });
      var incStr = document.getElementById('exp-include').value;
      var excStr = document.getElementById('exp-exclude').value;
      var preset = {
        name: document.getElementById('exp-name').value,
        platform: exportPresets[exportSelectedIdx].platform,
        build_profile: document.getElementById('exp-build').value,
        export_path: document.getElementById('exp-path').value,
        app_name: document.getElementById('exp-app-name').value,
        include_filters: incStr ? incStr.split(',').map(function(s){return s.trim();}) : [],
        exclude_filters: excStr ? excStr.split(',').map(function(s){return s.trim();}) : [],
        custom_properties: customProps
      };
      api('POST', '/api/export/presets/update', preset).then(function() { loadExportPresets(); });
    });
  }

  // ---- Theme ----
  function applyTheme(theme) {
    if (theme === 'light') { document.body.classList.add('light'); } else { document.body.classList.remove('light'); }
  }

  // ---- Menu bar ----
  function setupMenuBar() {
    var items = document.querySelectorAll('.menu-item');
    var openMenu = null;

    items.forEach(function(item) {
      var btn = item.querySelector('button');
      if (btn) {
        btn.addEventListener('click', function(e) {
          var dd = item.querySelector('.menu-dropdown');
          var wasOpen = dd && dd.classList.contains('open');
          closeAllMenus();
          if (dd && !wasOpen) {
            dd.classList.add('open');
            btn.classList.add('open');
            openMenu = item;
          }
          e.stopPropagation();
        });
        // Hover to switch between menus when one is already open
        btn.addEventListener('mouseenter', function() {
          if (openMenu && openMenu !== item) {
            var dd = item.querySelector('.menu-dropdown');
            closeAllMenus();
            if (dd) {
              dd.classList.add('open');
              btn.classList.add('open');
              openMenu = item;
            }
          }
        });
      }
    });

    function closeAllMenus() {
      document.querySelectorAll('.menu-dropdown.open').forEach(function(d) { d.classList.remove('open'); });
      document.querySelectorAll('.menu-item > button.open').forEach(function(b) { b.classList.remove('open'); });
      openMenu = null;
    }

    document.addEventListener('click', closeAllMenus);

    // Handle both old .menu-dropdown-item and new .menu-action clicks
    document.querySelectorAll('.menu-dropdown-item, .menu-action').forEach(function(mi) {
      mi.addEventListener('click', function(e) {
        if (mi.classList.contains('disabled')) { e.stopPropagation(); return; }
        var action = mi.getAttribute('data-action');
        handleMenuAction(action);
        closeAllMenus();
        e.stopPropagation();
      });
    });
  }

  function handleMenuAction(action) {
    switch (action) {
      // -- Scene menu --
      case 'menu-new-scene':
      case 'scene-new':
        newScene();
        break;
      case 'menu-open-scene':
      case 'scene-open':
        openSceneDialog();
        break;
      case 'menu-save-scene':
      case 'scene-save':
        document.getElementById('btn-save').click();
        break;
      case 'menu-save-scene-as':
      case 'scene-save-as':
        saveSceneAs();
        break;
      case 'scene-close':
        closeCurrentScene();
        break;
      case 'scene-quit':
        logMessage('info', 'Quit requested');
        break;

      // -- Edit menu --
      case 'edit-undo':
        api('POST', '/api/undo').then(function() { fetchScene(); });
        break;
      case 'edit-redo':
        api('POST', '/api/redo').then(function() { fetchScene(); });
        break;
      case 'edit-cut':
        api('POST', '/api/node/cut').then(function() { fetchScene(); });
        break;
      case 'edit-copy':
        api('POST', '/api/node/copy');
        break;
      case 'edit-paste':
        api('POST', '/api/node/paste').then(function() { fetchScene(); });
        break;
      case 'edit-select-all':
        if (typeof selectAllNodes === 'function') selectAllNodes();
        break;
      case 'edit-deselect':
        if (typeof deselectAllNodes === 'function') deselectAllNodes();
        break;

      // -- Project menu --
      case 'project-open':
        openProjectDialog();
        break;
      case 'menu-project-settings':
      case 'project-settings':
        var psd = document.getElementById('project-settings-dialog');
        if (psd) psd.style.display = 'flex';
        break;
      case 'project-export':
        var exd = document.getElementById('export-dialog');
        if (exd) exd.style.display = 'flex';
        break;
      case 'project-install-android':
        logMessage('info', 'Android build template install not yet available');
        break;
      case 'project-refresh':
        fetchFileSystem();
        logMessage('info', 'Filesystem refreshed');
        break;

      // -- Debug menu --
      case 'debug-run':
        document.getElementById('btn-play').click();
        break;
      case 'debug-run-current':
        document.getElementById('btn-play-current').click();
        break;
      case 'debug-pause':
        document.getElementById('btn-pause').click();
        break;
      case 'debug-stop':
        document.getElementById('btn-stop').click();
        break;
      case 'debug-step':
        api('POST', '/api/runtime/step');
        break;
      case 'debug-break':
        logMessage('info', 'Break requested');
        break;
      case 'debug-keep-running':
        logMessage('info', 'Remote debug deploy not yet available');
        break;

      // -- Editor menu --
      case 'menu-editor-settings':
      case 'editor-settings':
        var esd = document.getElementById('settings-dialog');
        if (esd) esd.style.display = 'flex';
        break;
      case 'editor-layout-save':
        logMessage('info', 'Editor layout saved');
        break;
      case 'editor-layout-default':
        logMessage('info', 'Editor layout reset to default');
        break;
      case 'editor-toggle-fullscreen':
        if (document.fullscreenElement) { document.exitFullscreen(); }
        else { document.documentElement.requestFullscreen(); }
        break;
      case 'editor-toggle-console':
        var bp = document.getElementById('bottom-panel');
        if (bp) bp.style.display = bp.style.display === 'none' ? '' : 'none';
        break;

      // -- Help menu --
      case 'menu-shortcuts':
      case 'help-docs':
        toggleHelpDialog();
        break;
      case 'help-issues':
        logMessage('info', 'Bug reports: https://github.com/patinaengine/patina/issues');
        break;
      case 'menu-about':
      case 'help-about':
        alert('Patina Engine v0.1.0\nA Rust-native Godot-compatible game engine.\n\nhttps://patinaengine.com');
        break;
    }
  }


  // ---- Snap indicator (pat-zlv) ----
  function updateSnapIndicator() {
    var el = document.getElementById('snap-indicator');
    if (!el) return;
    if (editorSettings.grid_snap_enabled) {
      el.textContent = 'Snap: ' + editorSettings.grid_snap_size + 'px';
      el.classList.add('active');
    } else {
      el.classList.remove('active');
    }
  }

  // ---- Viewport overlay: grid, rulers, origin, guides (pat-zoh4r) ----
  var activeSnapGuides = [];

  function renderViewportOverlay() {
    var canvas = document.getElementById('viewport-overlay-canvas');
    if (!canvas) return;
    var container = document.getElementById('viewport-container');
    if (!container) return;
    var cw = container.clientWidth;
    var ch = container.clientHeight;
    if (cw === 0 || ch === 0) return;
    canvas.width = cw;
    canvas.height = ch;
    var ctx = canvas.getContext('2d');
    if (!ctx) return;
    ctx.clearRect(0, 0, cw, ch);

    // In 3D mode the perspective view (grid, axes, nodes) is rendered
    // server-side into the viewport image; the 2D grid/rulers/origin/Node2D
    // overlay must NOT paint over it (pat-3dnav). The projection label is now an
    // interactive DOM dropdown (#vp3d-view-menu), so only the nav gizmo is drawn.
    if (editorMode === '3d') {
      draw3DNavGizmo(ctx, cw);
      return;
    }

    var zoom = viewportZoom || 1.0;
    var panX = viewportPanX || 0;
    var panY = viewportPanY || 0;

    // Origin marker (world 0,0)
    var ox = cw / 2 + panX;
    var oy = ch / 2 + panY;
    ctx.strokeStyle = 'rgba(224,80,80,0.5)';
    ctx.lineWidth = 1;
    ctx.beginPath(); ctx.moveTo(ox, 0); ctx.lineTo(ox, ch); ctx.stroke();
    ctx.strokeStyle = 'rgba(80,200,120,0.5)';
    ctx.beginPath(); ctx.moveTo(0, oy); ctx.lineTo(cw, oy); ctx.stroke();

    // Grid lines
    if (editorSettings.grid_visible) {
      var gridSize = (editorSettings.grid_snap_size || 8) * zoom;
      if (gridSize >= 4) {
        ctx.strokeStyle = 'rgba(255,255,255,0.06)';
        ctx.lineWidth = 1;
        var startX = ox % gridSize;
        for (var gx = startX; gx < cw; gx += gridSize) {
          ctx.beginPath(); ctx.moveTo(Math.round(gx) + 0.5, 0); ctx.lineTo(Math.round(gx) + 0.5, ch); ctx.stroke();
        }
        var startY = oy % gridSize;
        for (var gy = startY; gy < ch; gy += gridSize) {
          ctx.beginPath(); ctx.moveTo(0, Math.round(gy) + 0.5); ctx.lineTo(cw, Math.round(gy) + 0.5); ctx.stroke();
        }
      }
    }

    // Rulers
    if (editorSettings.rulers_visible) {
      var rulerSize = 18;
      // Top ruler
      ctx.fillStyle = 'rgba(22,22,22,0.9)';
      ctx.fillRect(0, 0, cw, rulerSize);
      // Left ruler
      ctx.fillRect(0, 0, rulerSize, ch);
      // Ruler tick marks and labels
      ctx.fillStyle = 'rgba(136,136,136,0.8)';
      ctx.font = '9px monospace';
      ctx.textAlign = 'center';
      var tickStep = 50 * zoom;
      if (tickStep < 20) tickStep = 100 * zoom;
      if (tickStep < 10) tickStep = 200 * zoom;
      if (tickStep >= 10) {
        // Horizontal ruler
        var hStart = ox % tickStep;
        for (var rx = hStart; rx < cw; rx += tickStep) {
          var worldX = Math.round((rx - ox) / zoom);
          ctx.fillRect(Math.round(rx), rulerSize - 6, 1, 6);
          if (Math.abs(worldX) % 100 === 0 || tickStep >= 50) {
            ctx.fillText(worldX, rx, rulerSize - 7);
          }
        }
        // Vertical ruler
        ctx.textAlign = 'right';
        var vStart = oy % tickStep;
        for (var ry = vStart; ry < ch; ry += tickStep) {
          var worldY = Math.round((ry - oy) / zoom);
          ctx.fillRect(rulerSize - 6, Math.round(ry), 6, 1);
          ctx.save();
          ctx.translate(rulerSize - 7, ry);
          ctx.rotate(-Math.PI / 2);
          ctx.textAlign = 'center';
          ctx.fillText(worldY, 0, 0);
          ctx.restore();
        }
      }
      // Corner square
      ctx.fillStyle = 'rgba(22,22,22,0.9)';
      ctx.fillRect(0, 0, rulerSize, rulerSize);
      ctx.strokeStyle = 'rgba(42,42,42,1)';
      ctx.strokeRect(0, 0, rulerSize, rulerSize);
    }

    // Snap guides (alignment lines from smart snap)
    if (activeSnapGuides.length > 0) {
      ctx.strokeStyle = 'rgba(87,158,255,0.6)';
      ctx.lineWidth = 1;
      ctx.setLineDash([4, 4]);
      for (var i = 0; i < activeSnapGuides.length; i++) {
        var g = activeSnapGuides[i];
        var gPos = g.position * zoom;
        if (g.axis === 'x') {
          var gx2 = ox + gPos;
          ctx.beginPath(); ctx.moveTo(gx2, 0); ctx.lineTo(gx2, ch); ctx.stroke();
        } else {
          var gy2 = oy + gPos;
          ctx.beginPath(); ctx.moveTo(0, gy2); ctx.lineTo(cw, gy2); ctx.stroke();
        }
      }
      ctx.setLineDash([]);
    }

    // Y-Sort indicator (show when selected node is YSort-enabled)
    if (selectedNodeData && selectedNodeData.class_name === 'Node2D' && selectedNodeData.y_sort_enabled) {
      ctx.fillStyle = 'rgba(87,158,255,0.15)';
      ctx.fillRect(0, oy, cw, 1);
      ctx.fillStyle = 'rgba(87,158,255,0.6)';
      ctx.font = '10px monospace';
      ctx.textAlign = 'left';
      ctx.fillText('Y-Sort', 4, oy - 4);
    }
  }

  // Fetch snap guides from API during drag operations
  async function fetchSnapGuides() {
    var data = await api('GET', '/api/viewport/snap_guides');
    if (data && data.guides) {
      activeSnapGuides = data.guides;
    } else {
      activeSnapGuides = [];
    }
    renderViewportOverlay();
  }

  // ---- Script find/replace (pat-cgc) ----
  var scriptFindOpen = false;
  var scriptGotoOpen = false;

  function toggleScriptFind() {
    scriptFindOpen = !scriptFindOpen;
    var bar = document.getElementById('script-find-bar');
    if (bar) { bar.classList.toggle('open', scriptFindOpen); if (scriptFindOpen) { var inp = bar.querySelector('input'); if (inp) inp.focus(); } }
  }

  function toggleScriptGoto() {
    scriptGotoOpen = !scriptGotoOpen;
    var bar = document.getElementById('script-goto-bar');
    if (bar) { bar.classList.toggle('open', scriptGotoOpen); if (scriptGotoOpen) { var inp = bar.querySelector('input'); if (inp) inp.focus(); } }
  }

  function setupScriptFindReplace() {
    // Inject find bar into script panel (done dynamically when script loads)
  }

  // ---- Monitors panel (pat-lbu) ----
  // ---- Monitors (Godot EditorPerformanceProfiler parity) ----
  // Category + monitor list mirrors Godot's Performance monitors. `live` marks
  // the ones Patina currently computes; the rest report 0 (as Godot does when a
  // subsystem is idle) until the runtime instruments them (pat tracked).
  var MONITOR_SPEC = [
    {cat: 'Time', items: [
      {key: 'time/fps', label: 'FPS', unit: 'int'},
      {key: 'time/process', label: 'Process', unit: 'ms'},
      {key: 'time/physics_process', label: 'Physics Process', unit: 'ms'},
      {key: 'time/navigation_process', label: 'Navigation Process', unit: 'ms'}]},
    {cat: 'Memory', items: [
      {key: 'memory/static', label: 'Static', unit: 'bytes'},
      {key: 'memory/static_max', label: 'Static Max', unit: 'bytes'},
      {key: 'memory/msg_buf_max', label: 'Message Buffer Max', unit: 'bytes'}]},
    {cat: 'Object', items: [
      {key: 'object/objects', label: 'Objects', unit: 'int'},
      {key: 'object/resources', label: 'Resources', unit: 'int'},
      {key: 'object/nodes', label: 'Nodes', unit: 'int'},
      {key: 'object/orphan_nodes', label: 'Orphan Nodes', unit: 'int'}]},
    {cat: 'Raster', items: [
      {key: 'raster/total_objects_drawn', label: 'Total Objects Drawn', unit: 'int'},
      {key: 'raster/total_primitives_drawn', label: 'Total Primitives Drawn', unit: 'int'},
      {key: 'raster/total_draw_calls', label: 'Total Draw Calls', unit: 'int'}]},
    {cat: 'Video', items: [
      {key: 'video/video_mem_used', label: 'Video Mem Used', unit: 'bytes'},
      {key: 'video/texture_mem_used', label: 'Texture Mem Used', unit: 'bytes'},
      {key: 'video/buffer_mem_used', label: 'Buffer Mem Used', unit: 'bytes'}]},
    {cat: 'Physics 2D', items: [
      {key: 'physics_2d/active_objects', label: 'Active Objects', unit: 'int'},
      {key: 'physics_2d/collision_pairs', label: 'Collision Pairs', unit: 'int'},
      {key: 'physics_2d/islands', label: 'Islands', unit: 'int'}]},
    {cat: 'Physics 3D', items: [
      {key: 'physics_3d/active_objects', label: 'Active Objects', unit: 'int'},
      {key: 'physics_3d/collision_pairs', label: 'Collision Pairs', unit: 'int'},
      {key: 'physics_3d/islands', label: 'Islands', unit: 'int'}]},
    {cat: 'Navigation', items: [
      {key: 'navigation/active_maps', label: 'Active Maps', unit: 'int'},
      {key: 'navigation/regions', label: 'Regions', unit: 'int'},
      {key: 'navigation/agents', label: 'Agents', unit: 'int'},
      {key: 'navigation/links', label: 'Links', unit: 'int'},
      {key: 'navigation/polygons', label: 'Polygons', unit: 'int'},
      {key: 'navigation/edges', label: 'Edges', unit: 'int'}]},
    {cat: 'Audio', items: [
      {key: 'audio/driver_output_latency', label: 'Driver Output Latency', unit: 'ms'}]}
  ];
  var MONITOR_COLORS = ['#579eff','#8eef97','#ffcc66','#c88eff','#ff8888','#66cccc','#ff9e57','#b0b0b0'];
  var monitorChecked = {};   // key -> bool
  var monitorHistory = {};   // key -> [values]
  var monitorColorIdx = {};  // key -> color index (assigned on check)
  var monitorTreeBuilt = false;
  function monitorElId(key) { return 'mval-' + key.replace('/', '-'); }
  function monitorChkId(key) { return 'mchk-' + key.replace('/', '-'); }
  function fmtMonitor(unit, v) {
    if (unit === 'int') return String(Math.round(v));
    if (unit === 'ms') return v.toFixed(2) + ' ms';
    if (unit === 'bytes') {
      if (v < 1024) return Math.round(v) + ' B';
      if (v < 1048576) return (v / 1024).toFixed(1) + ' KiB';
      return (v / 1048576).toFixed(1) + ' MiB';
    }
    return String(v);
  }
  function buildMonitorTree() {
    var body = document.getElementById('monitor-tree-body');
    if (!body || monitorTreeBuilt) return;
    var html = '';
    MONITOR_SPEC.forEach(function(group) {
      html += '<div class="monitor-cat">' + group.cat + '</div>';
      group.items.forEach(function(m) {
        monitorHistory[m.key] = monitorHistory[m.key] || [];
        html += '<div class="monitor-row"><span class="mcol mcol-name">' +
          '<input type="checkbox" class="mcheck" id="' + monitorChkId(m.key) + '" data-mkey="' + m.key + '">' +
          escapeHtml(m.label) + '</span><span class="mcol mcol-value" id="' + monitorElId(m.key) + '">0</span></div>';
      });
    });
    body.innerHTML = html;
    body.querySelectorAll('.mcheck').forEach(function(cb) {
      cb.addEventListener('change', function() {
        var key = cb.getAttribute('data-mkey');
        monitorChecked[key] = cb.checked;
        if (cb.checked && monitorColorIdx[key] == null) {
          monitorColorIdx[key] = Object.keys(monitorColorIdx).length % MONITOR_COLORS.length;
        }
        drawMonitorGraph();
      });
    });
    monitorTreeBuilt = true;
  }

  async function fetchMonitorData() {
    buildMonitorTree();
    // pat-wnwio: live Performance counters drive every monitor (render, video,
    // physics, navigation, memory) — not just FPS/Process/Nodes.
    var mon = await api('GET', '/api/monitors');
    var vals = (mon && mon.monitors) || {};
    var fps = vals['time/fps'] || 0;
    // hidden legacy spans (kept so older code paths don't error)
    var fpsEl = document.getElementById('monitor-fps'); if (fpsEl) fpsEl.textContent = fps ? fps.toFixed(0) : '--';
    MONITOR_SPEC.forEach(function(group) {
      group.items.forEach(function(m) {
        var v = vals[m.key] || 0;
        var el = document.getElementById(monitorElId(m.key));
        if (el) el.textContent = fmtMonitor(m.unit, v);
        var hist = monitorHistory[m.key] || (monitorHistory[m.key] = []);
        hist.push(v);
        if (hist.length > 160) hist.shift();
      });
    });
    drawMonitorGraph();
  }

  function drawMonitorGraph() {
    var canvas = document.getElementById('monitor-graph-canvas');
    var empty = document.getElementById('monitor-graph-empty');
    if (!canvas) return;
    var keys = Object.keys(monitorChecked).filter(function(k) { return monitorChecked[k]; });
    if (keys.length === 0) { canvas.style.display = 'none'; if (empty) empty.style.display = 'flex'; return; }
    canvas.style.display = ''; if (empty) empty.style.display = 'none';
    var wrap = canvas.parentElement;
    canvas.width = Math.max(50, wrap.clientWidth); canvas.height = Math.max(50, wrap.clientHeight);
    var ctx = canvas.getContext('2d'); var w = canvas.width, h = canvas.height;
    ctx.clearRect(0, 0, w, h);
    var labelMap = {};
    MONITOR_SPEC.forEach(function(g) { g.items.forEach(function(m) { labelMap[m.key] = m; }); });
    var stripH = h / keys.length;
    keys.forEach(function(key, idx) {
      var y0 = idx * stripH;
      var m = labelMap[key];
      var color = MONITOR_COLORS[monitorColorIdx[key] || 0];
      var hist = monitorHistory[key] || [];
      var maxV = 1; for (var i = 0; i < hist.length; i++) maxV = Math.max(maxV, hist[i]);
      // strip separator
      ctx.strokeStyle = '#333'; ctx.lineWidth = 1;
      ctx.beginPath(); ctx.moveTo(0, y0 + 0.5); ctx.lineTo(w, y0 + 0.5); ctx.stroke();
      // plot line (leave room at top for the label)
      var top = y0 + 18, bot = y0 + stripH - 6, gh = Math.max(4, bot - top);
      ctx.strokeStyle = color; ctx.lineWidth = 1.5; ctx.beginPath();
      var step = w / Math.max(hist.length - 1, 1);
      for (var j = 0; j < hist.length; j++) {
        var x = j * step, yy = bot - (hist[j] / maxV) * gh;
        if (j === 0) ctx.moveTo(x, yy); else ctx.lineTo(x, yy);
      }
      ctx.stroke();
      // label + current value + max
      var cur = hist.length ? hist[hist.length - 1] : 0;
      ctx.fillStyle = color; ctx.font = 'bold 11px monospace';
      ctx.fillText(m.label + ': ' + fmtMonitor(m.unit, cur), 6, y0 + 13);
      ctx.fillStyle = '#888';
      ctx.fillText('max ' + fmtMonitor(m.unit, maxV), w - 110, y0 + 13);
    });
  }

  // ---- Profiler panel (pat-7wvfu) ----
  var PROFILER_FUNC_COLORS = ['#579eff','#8ebbff','#8eef97','#c88eff','#e05050','#ffcc66','#66cccc','#ff8888'];

  var lastProfilerData = null; // pat-wykfm: cache for mode/frame recompute
  async function fetchProfilerData() {
    var data = await api('GET', '/api/profiler');
    if (!data) return;
    lastProfilerData = data;
    var fpsEl = document.getElementById('profiler-fps');
    var avgEl = document.getElementById('profiler-avg');
    var maxEl = document.getElementById('profiler-max');
    if (fpsEl) fpsEl.textContent = data.avg_fps ? data.avg_fps.toFixed(0) : '--';
    if (avgEl) avgEl.textContent = data.avg_ms ? data.avg_ms.toFixed(1) + 'ms' : '--';
    if (maxEl) maxEl.textContent = data.max_ms ? data.max_ms.toFixed(1) + 'ms' : '--';

    var canvas = document.getElementById('profiler-graph-canvas');
    if (!canvas || !data.frames || data.frames.length === 0) return;
    var ctx = canvas.getContext('2d');
    var w = canvas.width, h = canvas.height;
    ctx.clearRect(0, 0, w, h);

    var maxTime = Math.max(33.3, data.max_ms || 33.3);

    // Draw reference lines (16.6ms = 60fps, 33.3ms = 30fps)
    ctx.strokeStyle = '#2a2a2a'; ctx.lineWidth = 1;
    var y60 = h - (16.6 / maxTime) * h;
    var y30 = h - (33.3 / maxTime) * h;
    ctx.beginPath(); ctx.moveTo(0, y60); ctx.lineTo(w, y60); ctx.stroke();
    ctx.beginPath(); ctx.moveTo(0, y30); ctx.lineTo(w, y30); ctx.stroke();
    ctx.fillStyle = '#555'; ctx.font = '9px monospace';
    ctx.fillText('60fps', 2, y60 - 2);
    ctx.fillText('30fps', 2, y30 - 2);

    // Collect all unique function names across frames
    var funcNames = {};
    data.frames.forEach(function(f) {
      (f.functions || []).forEach(function(fn) { funcNames[fn.name] = true; });
    });
    var funcList = Object.keys(funcNames);

    // Draw stacked bar chart
    var barWidth = Math.max(2, w / Math.max(data.frames.length, 1) - 1);
    var step = w / Math.max(data.frames.length, 1);

    data.frames.forEach(function(frame, i) {
      var x = i * step;
      var yOffset = h;
      var funcs = frame.functions || [];

      // Draw function breakdown as stacked bars
      for (var fi = 0; fi < funcs.length; fi++) {
        var barH = (funcs[fi].time_ms / maxTime) * h;
        var colorIdx = funcList.indexOf(funcs[fi].name) % PROFILER_FUNC_COLORS.length;
        ctx.fillStyle = PROFILER_FUNC_COLORS[colorIdx];
        yOffset -= barH;
        ctx.fillRect(x, yOffset, barWidth, barH);
      }

      // If no function breakdown, draw total as single bar
      if (funcs.length === 0 && frame.total_ms > 0) {
        var totalH = (frame.total_ms / maxTime) * h;
        ctx.fillStyle = PROFILER_FUNC_COLORS[0];
        ctx.fillRect(x, h - totalH, barWidth, totalH);
      }

      // GPU overlay (semi-transparent)
      if (frame.gpu_ms > 0) {
        var gpuH = (frame.gpu_ms / maxTime) * h;
        ctx.fillStyle = 'rgba(200,142,255,0.3)';
        ctx.fillRect(x, h - gpuH, barWidth, gpuH);
      }
    });

    // Function tree (Name | Time | Calls) — computed per the Measure/Time/Frame
    // controls (pat-wykfm).
    renderProfilerTree();
  }

  // pat-wykfm: render the profiler function tree from the cached data, honoring
  // the Measure dropdown (Frame Time / Average Time / Frame % / Physics Frame %),
  // the Time dropdown (Inclusive vs Self), and the Frame # scrub.
  function renderProfilerTree() {
    var tree = document.getElementById('profiler-func-breakdown');
    var data = lastProfilerData;
    if (!tree || !data || !data.frames || data.frames.length === 0) { if (tree) tree.innerHTML = ''; return; }
    var measure = (document.getElementById('profiler-measure') || {}).value || 'Frame Time (ms)';
    var timeMode = (document.getElementById('profiler-time') || {}).value || 'Inclusive';
    var useSelf = (timeMode === 'Self');
    // Frame # scrub: clamp the requested frame to the captured range; default last.
    var frameInput = document.getElementById('profiler-frame');
    var fIdx = data.frames.length - 1;
    if (frameInput && frameInput.value !== '' && !isNaN(parseInt(frameInput.value))) {
      fIdx = Math.max(0, Math.min(data.frames.length - 1, parseInt(frameInput.value)));
    }
    var frame = data.frames[fIdx];
    var funcs = frame.functions || [];
    function timeVal(fn) { return useSelf ? (fn.self_ms != null ? fn.self_ms : fn.time_ms) : fn.time_ms; }
    // Average across all frames for a function name (Self/Inclusive aware).
    function avgFor(name) {
      var sum = 0, n = 0;
      data.frames.forEach(function(fr) {
        var e = (fr.functions || []).find(function(f) { return f.name === name; });
        if (e) { sum += timeVal(e); n++; }
      });
      return n ? sum / n : 0;
    }
    function measured(fn) {
      var t = timeVal(fn);
      if (measure === 'Average Time (ms)') return avgFor(fn.name).toFixed(2);
      if (measure === 'Frame %') return frame.total_ms > 0 ? (t / frame.total_ms * 100).toFixed(1) + '%' : '0%';
      if (measure === 'Physics Frame %') return frame.physics_ms > 0 ? (t / frame.physics_ms * 100).toFixed(1) + '%' : '—';
      return t.toFixed(2); // Frame Time (ms)
    }
    tree.innerHTML = '';
    funcs.forEach(function(fn, idx) {
      var colorIdx = idx % PROFILER_FUNC_COLORS.length;
      var calls = fn.calls != null ? fn.calls : 1;
      var row = document.createElement('div');
      row.className = 'profiler-row';
      row.innerHTML = '<span class="pcol pcol-name"><input type="checkbox" class="pcheck" checked>' +
        '<span style="color:' + PROFILER_FUNC_COLORS[colorIdx] + '">' + escapeHtml(fn.name) + '</span></span>' +
        '<span class="pcol pcol-time">' + measured(fn) + '</span><span class="pcol pcol-calls">' + calls + '</span>';
      tree.appendChild(row);
    });
    if (frame.gpu_ms > 0) {
      var grow = document.createElement('div');
      grow.className = 'profiler-row';
      grow.innerHTML = '<span class="pcol pcol-name"><input type="checkbox" class="pcheck" checked>' +
        '<span style="color:rgba(200,142,255,0.9)">GPU</span></span>' +
        '<span class="pcol pcol-time">' + frame.gpu_ms.toFixed(2) + '</span><span class="pcol pcol-calls">1</span>';
      tree.appendChild(grow);
    }
  }

  // ---- Profiler toolbar (Godot EditorProfiler: Start/Stop, Clear) ----
  function setupProfilerToolbar() {
    var act = document.getElementById('profiler-activate');
    if (act) act.addEventListener('click', function() {
      var on = act.classList.toggle('active');
      act.textContent = on ? 'Stop' : 'Start';
      api('POST', on ? '/api/profiler/start' : '/api/profiler/stop');
    });
    var clr = document.getElementById('profiler-clear');
    if (clr) clr.addEventListener('click', function() {
      var t = document.getElementById('profiler-func-breakdown'); if (t) t.innerHTML = '';
      var c = document.getElementById('profiler-graph-canvas');
      if (c) { var x = c.getContext('2d'); if (x) x.clearRect(0, 0, c.width, c.height); }
      lastProfilerData = null;
      api('POST', '/api/profiler/clear');
    });
    // pat-wykfm: Measure / Time / Frame# controls recompute the tree from the
    // cached capture without a refetch.
    ['profiler-measure', 'profiler-time', 'profiler-frame'].forEach(function(id) {
      var el = document.getElementById(id);
      if (el) el.addEventListener(el.tagName === 'SELECT' ? 'change' : 'input', renderProfilerTree);
    });
  }

  // ---- Debugger panel (pat-lbu, pat-zf49m) ----
  function setupDebuggerPanel() {
    var btnContinue = document.getElementById('debug-btn-continue');
    var btnStepIn = document.getElementById('debug-btn-step-in');
    var btnStepOver = document.getElementById('debug-btn-step-over');
    var btnStepOut = document.getElementById('debug-btn-step-out');
    if (btnContinue) btnContinue.addEventListener('click', function() { api('POST', '/api/debug/continue'); });
    if (btnStepIn) btnStepIn.addEventListener('click', function() { api('POST', '/api/debug/step_in'); });
    if (btnStepOver) btnStepOver.addEventListener('click', function() { api('POST', '/api/debug/step_over'); });
    if (btnStepOut) btnStepOut.addEventListener('click', function() { api('POST', '/api/debug/step_out'); });
  }

  async function fetchDebugData() {
    var data = await api('GET', '/api/debug/state');
    if (!data) return;

    // Update status indicator
    var statusEl = document.getElementById('debug-status');
    if (statusEl) {
      var state = data.state || 'detached';
      statusEl.textContent = state.charAt(0).toUpperCase() + state.slice(1);
      statusEl.className = 'debug-status ' + state;
    }

    // Render call stack
    var stackEl = document.getElementById('debug-stack-frames');
    if (stackEl) {
      if (!data.frames || data.frames.length === 0) {
        stackEl.innerHTML = '<div class="insp-empty" style="padding:8px">No active debug session</div>';
      } else {
        stackEl.innerHTML = '';
        for (var i = 0; i < data.frames.length; i++) {
          var frame = document.createElement('div');
          frame.className = 'debug-stack-frame' + (i === 0 ? ' current' : '');
          var f = data.frames[i];
          frame.textContent = (f.function || '?') + ' — ' + (f.script || '?') + ':' + (f.line || 0);
          frame.setAttribute('data-frame-index', i);
          frame.addEventListener('click', (function(idx) {
            return function() {
              document.querySelectorAll('.debug-stack-frame').forEach(function(el) { el.classList.remove('current'); });
              this.classList.add('current');
              fetchDebugLocals(idx);
            };
          })(i));
          stackEl.appendChild(frame);
        }
      }
    }

    // Render breakpoints
    renderBreakpointList(data.breakpoints || []);

    // Render locals for top frame
    renderVariables('debug-variables', data.locals || [], 'Not paused');

    // Render globals
    renderVariables('debug-globals', data.globals || [], 'Not paused');
  }

  function renderBreakpointList(breakpoints) {
    var el = document.getElementById('debug-breakpoints-list');
    if (!el) return;
    if (breakpoints.length === 0) {
      el.innerHTML = '<div class="insp-empty" style="padding:8px">No breakpoints set</div>';
      return;
    }
    el.innerHTML = '';
    breakpoints.forEach(function(bp) {
      var item = document.createElement('div');
      item.className = 'debug-breakpoint-item';
      item.innerHTML =
        '<input type="checkbox" class="bp-enabled" checked>' +
        '<span class="bp-location">' + (bp.script || '?') + ':' + (bp.line || 0) + '</span>' +
        '<span class="bp-remove" title="Remove breakpoint">&#10005;</span>';
      var removeBtn = item.querySelector('.bp-remove');
      removeBtn.addEventListener('click', function() {
        api('POST', '/api/debug/remove_breakpoint', { script: bp.script, line: bp.line });
      });
      el.appendChild(item);
    });
  }

  function renderVariables(containerId, vars, emptyMsg) {
    var el = document.getElementById(containerId);
    if (!el) return;
    if (!vars || vars.length === 0) {
      el.innerHTML = '<div class="insp-empty" style="padding:8px">' + emptyMsg + '</div>';
      return;
    }
    el.innerHTML = '';
    vars.forEach(function(v) {
      var item = document.createElement('div');
      item.className = 'debug-var-item';
      item.innerHTML =
        '<span class="var-name">' + (v.name || '?') + '</span>' +
        '<span class="var-type">' + (v.type || '') + '</span>' +
        '<span class="var-value">' + (v.value || 'null') + '</span>';
      el.appendChild(item);
    });
  }

  async function fetchDebugLocals(frameIndex) {
    var data = await api('GET', '/api/debug/locals?frame=' + frameIndex);
    if (data) renderVariables('debug-variables', data.locals || [], 'No variables');
  }

  // ---- Mode buttons (pat-dj6) ----
  // Central main-screen views: the mode switch swaps the central area between
  // the 2D/3D viewport (pat-7i8u4), the script editor (pat-qtmbv), the game view
  // (pat-xwdlg), and the AssetLib browser (pat-4jaq7) — so the mode buttons
  // change the central view instead of being cosmetic. Exactly one is visible.
  var editorMode = '2d';
  function setCentralView(mode) {
    editorMode = mode;
    // Drop any in-progress 3D nav/gizmo interaction when changing the main view.
    nav3D.active = false; nav3D.gizmo = false; nav3D.gizmoPending = false;
    var vp = document.getElementById('viewport-panel');
    var sep = document.getElementById('script-editor-panel');
    var gp = document.getElementById('game-panel');
    var ap = document.getElementById('assetlib-panel');
    var panel = document.getElementById('script-panel');
    var host = document.getElementById('script-main-host');
    if (vp) vp.style.display = 'none';
    if (sep) sep.style.display = 'none';
    if (gp) gp.style.display = 'none';
    if (ap) ap.style.display = 'none';
    // Return the script editor to the bottom Script tab when not in script mode.
    if (mode !== 'script' && panel) {
      var bottomScript = document.querySelector('.bottom-content-tab[data-tab="script"]');
      if (bottomScript && panel.parentElement !== bottomScript) bottomScript.appendChild(panel);
    }
    if (mode === 'script') {
      if (sep) sep.style.display = 'flex';
      if (panel && host && panel.parentElement !== host) host.appendChild(panel);
    } else if (mode === 'game') {
      if (gp) gp.style.display = 'flex';
      refreshGameStatus();
    } else if (mode === 'assetlib') {
      if (ap) ap.style.display = 'flex';
      loadAssetLib();
    } else if (vp) {
      vp.style.display = '';
      // The panel was display:none in the other modes, so its size is only
      // known now that it's visible — report it so the render fills the panel.
      scheduleViewportResize();
      // Repaint the viewport overlay for the new mode so 2D chrome (grid,
      // rulers, origin) clears when entering 3D and returns when leaving it.
      renderViewportOverlay();
      // Pull the 3D camera state so the navigation gizmo is drawn immediately,
      // and sync the server's gizmo mode to the active transform tool.
      if (mode === '3d') { refresh3DState(); push3DGizmoMode(); }
    }
    updateViewMenu();   // show/hide the 3D view menu for the new mode
    updateVp3dToolbar(); // show/hide + sync the 3D spatial toolbar
    updateVp2dToolbar(); // pat-9yq5q: show the 2D canvas toolbar only in 2D mode
  }

  // Programmatically activate a main-screen mode (2D/3D/Script/Game/AssetLib):
  // sync the top-bar button highlight, persist the mode server-side, and swap the
  // central view. Shared by the top-bar buttons and the scene-open auto-select.
  // Apply a main-screen mode LOCALLY: sync the top-bar button highlight and swap
  // the central view, WITHOUT persisting to the server. Shared by the user-driven
  // switch (selectMode, which also POSTs) and by Play/Stop, which follow a mode
  // the server already set (api_runtime_play/stop) and so must not re-POST it.
  function applyMode(mode) {
    var btns = document.querySelectorAll('.mode-btn');
    for (var j = 0; j < btns.length; j++) btns[j].classList.toggle('active', btns[j].getAttribute('data-mode') === mode);
    setCentralView(mode);
  }
  function selectMode(mode) {
    api('POST', '/api/editor/mode', { mode: mode });
    applyMode(mode);
  }

  // Godot parity: opening a scene selects the main-screen editor that matches its
  // root — a 3D (Node3D) scene shows the spatial editor, a 2D/GUI scene the
  // canvas — so a freshly opened 3D scene lands where the camera controls
  // (orbit / right-drag freelook / wheel zoom) actually work, instead of the 2D
  // canvas. `/api/scene` reports the choice as `recommended_view`. It is applied
  // once per scene open (guarded by `autoViewPending`) and never overrides a mode
  // the user switched to manually afterwards.
  var autoViewPending = true;
  function maybeAutoSelectView(data) {
    if (!autoViewPending) return;
    autoViewPending = false;
    var rec = data && data.recommended_view;
    if (rec !== '2d' && rec !== '3d') return;   // ambiguous/empty scene: keep mode
    if (editorMode === rec) return;             // already in the right view
    selectMode(rec);
  }

  // pat-9yq5q: the 2D canvas toolbar shows only in 2D mode.
  function updateVp2dToolbar() {
    var tb = document.getElementById('vp2d-toolbar');
    if (tb) tb.style.display = (editorMode === '2d') ? 'flex' : 'none';
  }
  function setupVp2dToolbar() {
    // Godot's snap/skeleton/view dropdown menu items.
    var SNAP_MENU = [
      { label: 'Use Rotation Snap', check: false }, { label: 'Use Scale Snap', check: false },
      { label: 'Snap Relative', check: false }, { label: 'Use Pixel Snap', check: false }, { sep: true },
      { label: 'Snap to Parent', check: false }, { label: 'Snap to Node Anchor', check: false },
      { label: 'Snap to Node Sides', check: false }, { label: 'Snap to Node Center', check: false },
      { label: 'Snap to Other Nodes', check: false }, { label: 'Snap to Guides', check: true }, { sep: true },
      { label: 'Configure Snap...', check: false }
    ];
    var SKELETON_MENU = [
      { label: 'Show Bones', check: true }, { sep: true },
      { label: 'Make Bone2D Node(s) from Node(s)', check: false }, { label: 'Clear Bones', check: false }
    ];
    var VIEW_MENU = [
      { label: 'Grid', check: true }, { label: 'Show Helpers', check: true }, { label: 'Show Rulers', check: true },
      { label: 'Show Guides', check: true }, { label: 'Show Origin', check: true }, { label: 'Show Viewport', check: true },
      { label: 'Show Group And Lock Icons', check: true }, { sep: true },
      { label: 'Center Selection', check: false }, { label: 'Frame Selection', check: false },
      { label: 'Preview Canvas Scale', check: false }
    ];
    function openVp2dMenu(btn, items) {
      var old = document.getElementById('vp2d-active-dropdown'); if (old) { old.remove(); return; }
      var menu = document.createElement('div'); menu.className = 'vp2d-dropdown'; menu.id = 'vp2d-active-dropdown';
      var r = btn.getBoundingClientRect(); menu.style.left = r.left + 'px'; menu.style.top = (r.bottom + 2) + 'px';
      items.forEach(function(it) {
        if (it.sep) { var s = document.createElement('div'); s.className = 'vp2d-mi-sep'; menu.appendChild(s); return; }
        var mi = document.createElement('div'); mi.className = 'vp2d-mi';
        mi.innerHTML = '<span class="vp2d-check">' + (it.check ? '✓' : '') + '</span>' + escapeHtml(it.label);
        mi.addEventListener('click', function() {
          it.check = !it.check; logMessage('info', it.label); menu.remove();
        });
        menu.appendChild(mi);
      });
      document.body.appendChild(menu);
      var dismiss = function() { menu.remove(); document.removeEventListener('click', dismiss); };
      setTimeout(function() { document.addEventListener('click', dismiss); }, 0);
    }
    // Mode tools (list-select / pivot / pan / ruler) toggle active state.
    ['vp2d-list-select', 'vp2d-pivot', 'vp2d-pan', 'vp2d-ruler'].forEach(function(id) {
      var b = document.getElementById(id);
      if (b) b.addEventListener('click', function() {
        document.querySelectorAll('#vp2d-toolbar .vp2d-tool').forEach(function(o) { o.classList.toggle('active', o === b); });
      });
    });
    // Independent toggles (local space, smart/grid snap).
    ['vp2d-local-space', 'vp2d-smart-snap', 'vp2d-grid-snap'].forEach(function(id) {
      var b = document.getElementById(id);
      if (b) b.addEventListener('click', function() { b.classList.toggle('active'); });
    });
    // Lock / unlock / group / ungroup act on the selection.
    var lockActs = { 'vp2d-lock': 'Lock', 'vp2d-unlock': 'Unlock', 'vp2d-group': 'Group', 'vp2d-ungroup': 'Ungroup' };
    Object.keys(lockActs).forEach(function(id) {
      var b = document.getElementById(id);
      if (b) b.addEventListener('click', function() { logMessage('info', lockActs[id] + ' selected node'); });
    });
    // Menus.
    var sc = document.getElementById('vp2d-snap-config'); if (sc) sc.addEventListener('click', function(e) { e.stopPropagation(); openVp2dMenu(sc, SNAP_MENU); });
    var sk = document.getElementById('vp2d-skeleton'); if (sk) sk.addEventListener('click', function(e) { e.stopPropagation(); openVp2dMenu(sk, SKELETON_MENU); });
    var vm = document.getElementById('vp2d-view'); if (vm) vm.addEventListener('click', function(e) { e.stopPropagation(); openVp2dMenu(vm, VIEW_MENU); });
    // Zoom widget — drive the existing viewport zoom controls.
    function zoomClick(srcId) { var z = document.getElementById(srcId); if (z) z.click(); syncVp2dZoom(); }
    var zo = document.getElementById('vp2d-zoom-out'); if (zo) zo.addEventListener('click', function() { zoomClick('zoom-out'); });
    var zi = document.getElementById('vp2d-zoom-in'); if (zi) zi.addEventListener('click', function() { zoomClick('zoom-in'); });
    var zr = document.getElementById('vp2d-zoom-reset'); if (zr) zr.addEventListener('click', function() { zoomClick('zoom-reset'); });
    syncVp2dZoom();
  }
  function syncVp2dZoom() {
    var src = document.getElementById('zoom-label');
    var dst = document.getElementById('vp2d-zoom-label');
    if (src && dst) dst.textContent = src.textContent;
  }

  // ---- Game view (pat-xwdlg): embed the running game render ----
  function refreshGameStatus() {
    api('GET', '/api/runtime/status').then(function(s) {
      var running = !!(s && s.running), paused = !!(s && s.paused);
      var img = document.getElementById('game-screen-img');
      var idle = document.getElementById('game-idle');
      var st = document.getElementById('game-status');
      if (img && idle) {
        if (running) { img.style.display = ''; idle.style.display = 'none'; }
        else { img.style.display = 'none'; idle.style.display = ''; }
      }
      if (st) {
        if (!running) st.textContent = 'Press play to start the game.';
        else if (paused) st.textContent = 'Paused';
        else st.textContent = 'Running' + (s && s.fps != null ? ' — ' + Math.round(s.fps) + ' FPS' : '');
      }
    });
  }

  // ---- AssetLib browser (pat-4jaq7) ----
  var assetLibData = null;
  var assetLibPage = 0;
  var assetLibWired = false;
  var ASSET_PAGE_SIZE = 8;
  function loadAssetLib() {
    wireAssetLibFilters();
    if (assetLibData) { renderAssetLib(); return; }
    api('GET', '/api/assetlib/list').then(function(d) {
      assetLibData = (d && d.assets) || [];
      var sel = document.getElementById('assetlib-category');
      if (sel && sel.options.length <= 1) {
        var cats = {};
        assetLibData.forEach(function(a) { if (a.category) cats[a.category] = 1; });
        Object.keys(cats).sort().forEach(function(c) {
          var o = document.createElement('option'); o.value = c; o.textContent = c; sel.appendChild(o);
        });
      }
      renderAssetLib();
    });
  }
  // Wire the filter controls so switching sort/category/support/search reloads the list.
  function wireAssetLibFilters() {
    if (assetLibWired) return;
    assetLibWired = true;
    ['assetlib-search', 'assetlib-sort', 'assetlib-category', 'assetlib-site', 'assetlib-support'].forEach(function(id) {
      var el = document.getElementById(id);
      if (!el) return;
      var ev = el.tagName === 'INPUT' ? 'input' : 'change';
      el.addEventListener(ev, function() { assetLibPage = 0; renderAssetLib(); });
    });
    var imp = document.getElementById('assetlib-import');
    if (imp) imp.addEventListener('click', function() { logMessage('info', 'Import asset (.zip) — not yet implemented'); });
    var plug = document.getElementById('assetlib-plugins');
    if (plug) plug.addEventListener('click', function() { logMessage('info', 'Plugins view — not yet implemented'); });
  }
  function getFilteredAssets() {
    var q = (document.getElementById('assetlib-search').value || '').toLowerCase();
    var cat = (document.getElementById('assetlib-category') || {}).value || '';
    var sup = (document.getElementById('assetlib-support') || {}).value || '';
    var sort = (document.getElementById('assetlib-sort') || {}).value || 'recent';
    var items = assetLibData.filter(function(a) {
      if (q && (a.title || '').toLowerCase().indexOf(q) < 0 && (a.author || '').toLowerCase().indexOf(q) < 0) return false;
      if (cat && a.category !== cat) return false;
      if (sup && a.support !== sup) return false;
      return true;
    });
    if (sort === 'name') items.sort(function(a, b) { return (a.title || '').localeCompare(b.title || ''); });
    else if (sort === 'rating') items.sort(function(a, b) { return (b.rating || 0) - (a.rating || 0); });
    return items;
  }
  function renderAssetLib() {
    var grid = document.getElementById('assetlib-grid');
    if (!grid || !assetLibData) return;
    var items = getFilteredAssets();
    var pageCount = Math.max(1, Math.ceil(items.length / ASSET_PAGE_SIZE));
    if (assetLibPage >= pageCount) assetLibPage = 0;
    var pageItems = items.slice(assetLibPage * ASSET_PAGE_SIZE, (assetLibPage + 1) * ASSET_PAGE_SIZE);
    grid.innerHTML = '';
    if (!items.length) { grid.innerHTML = '<div class="assetlib-empty">No assets match your search.</div>'; renderAssetPagination(1); return; }
    pageItems.forEach(function(a) {
      var row = document.createElement('div'); row.className = 'asset-row'; row.title = a.title || '';
      var icon = document.createElement('div'); icon.className = 'asset-row-icon'; icon.textContent = a.icon || '📦'; row.appendChild(icon);
      var body = document.createElement('div'); body.className = 'asset-row-body';
      var t = document.createElement('div'); t.className = 'asset-row-title'; t.textContent = a.title || 'Untitled'; body.appendChild(t);
      if (a.category) { var cat = document.createElement('div'); cat.className = 'asset-row-cat'; cat.textContent = a.category; body.appendChild(cat); }
      var auth = document.createElement('div'); auth.className = 'asset-row-author';
      auth.textContent = (a.author || '') + (a.license ? '    ' + a.license : '');
      body.appendChild(auth);
      row.appendChild(body);
      grid.appendChild(row);
    });
    renderAssetPagination(pageCount);
  }
  function renderAssetPagination(pageCount) {
    var pag = document.getElementById('assetlib-pagination');
    if (!pag) return;
    pag.innerHTML = '';
    if (pageCount <= 1) { pag.style.display = 'none'; return; }
    pag.style.display = 'flex';
    function pageBtn(label, page, opts) {
      var b = document.createElement('button');
      b.className = 'assetlib-page-btn' + (opts && opts.active ? ' active' : '');
      b.textContent = label;
      if (opts && opts.disabled) b.disabled = true;
      else b.addEventListener('click', function() { assetLibPage = page; renderAssetLib(); });
      return b;
    }
    pag.appendChild(pageBtn('First', 0, { disabled: assetLibPage === 0 }));
    pag.appendChild(pageBtn('Previous', assetLibPage - 1, { disabled: assetLibPage === 0 }));
    for (var p = 0; p < pageCount; p++) pag.appendChild(pageBtn(String(p + 1), p, { active: p === assetLibPage }));
    pag.appendChild(pageBtn('Next', assetLibPage + 1, { disabled: assetLibPage === pageCount - 1 }));
    pag.appendChild(pageBtn('Last', pageCount - 1, { disabled: assetLibPage === pageCount - 1 }));
  }

  function setupModeButtons() {
    var btns = document.querySelectorAll('.mode-btn');
    for (var i = 0; i < btns.length; i++) {
      btns[i].addEventListener('click', function() {
        // A manual mode switch is the user's choice — stop any pending scene-open
        // auto-select from overriding it on the next /api/scene refresh.
        autoViewPending = false;
        selectMode(this.getAttribute('data-mode'));
      });
    }
    var newMain = document.getElementById('script-new-main');
    if (newMain) newMain.addEventListener('click', function() { createNewScript(); });
    var gpb = document.getElementById('game-play-btn');
    if (gpb) gpb.addEventListener('click', function() { api('POST', '/api/runtime/play', {}).then(refreshGameStatus); });
    var gpause = document.getElementById('game-pause-btn');
    if (gpause) gpause.addEventListener('click', function() { api('POST', '/api/runtime/pause', {}).then(refreshGameStatus); });
    var gstop = document.getElementById('game-stop-btn');
    if (gstop) gstop.addEventListener('click', function() { api('POST', '/api/runtime/stop', {}).then(refreshGameStatus); });
    ['assetlib-search', 'assetlib-sort', 'assetlib-category', 'assetlib-support'].forEach(function(id) {
      var el = document.getElementById(id);
      if (el) { el.addEventListener('input', renderAssetLib); el.addEventListener('change', renderAssetLib); }
    });
  }

  // ---- Scene tab close button (pat-dj6 / pat-e0heb) ----
  // Close handling is now integrated into renderSceneTabs().
  function setupSceneTabClose() {
    // No-op: multi-tab close is handled in renderSceneTabs event listeners.
  }

  // ---- Command Palette (pat-ugb0p) ----
  var cmdPaletteCommands = [];
  var cmdPaletteIndex = 0;

  function openCommandPalette() {
    var el = document.getElementById('command-palette');
    var input = document.getElementById('cmd-search');
    el.classList.add('open');
    input.value = '';
    cmdPaletteIndex = 0;
    loadCommandList('');
    setTimeout(function() { input.focus(); }, 50);
  }

  function closeCommandPalette() {
    document.getElementById('command-palette').classList.remove('open');
  }

  function fuzzyMatch(query, text) {
    if (!query) return { match: true, score: 0, indices: [] };
    var qi = 0, indices = [], score = 0, consecutive = 0;
    var lq = query.toLowerCase(), lt = text.toLowerCase();
    for (var ti = 0; ti < lt.length && qi < lq.length; ti++) {
      if (lt[ti] === lq[qi]) {
        indices.push(ti);
        consecutive++;
        score += consecutive + (ti === 0 ? 5 : 0) + (text[ti] === text[ti].toUpperCase() && ti > 0 ? 2 : 0);
        qi++;
      } else {
        consecutive = 0;
      }
    }
    if (qi < lq.length) return { match: false, score: 0, indices: [] };
    return { match: true, score: score, indices: indices };
  }

  function highlightMatch(text, indices) {
    if (!indices.length) return text;
    var result = '', idx = new Set(indices);
    for (var i = 0; i < text.length; i++) {
      if (idx.has(i)) result += '<mark>' + text[i] + '</mark>';
      else result += text[i];
    }
    return result;
  }

  async function loadCommandList(query) {
    if (!cmdPaletteCommands.length) {
      var data = await api('GET', '/api/commands');
      if (data && data.commands) cmdPaletteCommands = data.commands;
    }
    var results = [];
    for (var i = 0; i < cmdPaletteCommands.length; i++) {
      var c = cmdPaletteCommands[i];
      var m = fuzzyMatch(query, c.label);
      if (m.match) {
        results.push({ cmd: c, score: m.score, indices: m.indices });
      }
    }
    results.sort(function(a, b) { return b.score - a.score; });
    cmdPaletteIndex = 0;
    renderCommandResults(results);
  }

  function renderCommandResults(results) {
    var container = document.getElementById('cmd-results');
    if (!results.length) {
      container.innerHTML = '<div class="cmd-empty">No matching commands</div>';
      return;
    }
    var html = '';
    for (var i = 0; i < results.length; i++) {
      var r = results[i];
      var cls = i === cmdPaletteIndex ? 'cmd-item active' : 'cmd-item';
      html += '<div class="' + cls + '" data-cmd="' + r.cmd.id + '">';
      html += '<span class="cmd-label">' + highlightMatch(r.cmd.label, r.indices) + '</span>';
      html += '<span class="cmd-cat">' + r.cmd.category + '</span>';
      html += '</div>';
    }
    container.innerHTML = html;

    var items = container.querySelectorAll('.cmd-item');
    for (var j = 0; j < items.length; j++) {
      items[j].addEventListener('click', function() {
        executeCommandFromPalette(this.getAttribute('data-cmd'));
      });
    }
  }

  async function executeCommandFromPalette(cmdId) {
    closeCommandPalette();
    var result = await api('POST', '/api/command/execute', { command: cmdId });
    if (!result) return;
    if (result.action === 'client') {
      handleClientCommand(cmdId);
    } else {
      // Server-side command was executed; refresh
      await fetchScene();
      if (selectedNodeId) await fetchSelected();
    }
  }

  function handleClientCommand(cmdId) {
    switch (cmdId) {
      case 'save_scene': document.getElementById('btn-save').click(); break;
      case 'load_scene': document.getElementById('btn-load').click(); break;
      case 'new_scene': location.reload(); break;
      case 'add_node': openAddNodeDialog(); break;
      case 'duplicate_node':
        if (selectedNodeId !== null) doDuplicate(selectedNodeId);
        break;
      case 'rename_node':
        if (selectedNodeId !== null) doRename(selectedNodeId);
        break;
      case 'copy_nodes': doCopy(); break;
      case 'paste_nodes': doPaste(); break;
      case 'cut_nodes': doCut(); break;
      case 'undo':
        api('POST', '/api/undo').then(function() { fetchScene(); if (selectedNodeId) fetchSelected(); });
        break;
      case 'redo':
        api('POST', '/api/redo').then(function() { fetchScene(); if (selectedNodeId) fetchSelected(); });
        break;
      case 'select_tool': setToolMode('select'); break;
      case 'move_tool': setToolMode('move'); break;
      case 'rotate_tool': setToolMode('rotate'); break;
      case 'scale_tool': setToolMode('scale'); break;
      case 'zoom_in':
        viewportZoom = Math.min(16.0, viewportZoom * 1.25);
        api('POST', '/api/viewport/zoom', { zoom: viewportZoom }); updateZoomIndicator(); break;
      case 'zoom_out':
        viewportZoom = Math.max(0.1, viewportZoom / 1.25);
        api('POST', '/api/viewport/zoom', { zoom: viewportZoom }); updateZoomIndicator(); break;
      case 'zoom_reset':
        viewportZoom = 1.0; viewportPanX = 0; viewportPanY = 0;
        api('POST', '/api/viewport/zoom', { zoom: 1.0 }); api('POST', '/api/viewport/pan', { x: 0, y: 0 }); updateZoomIndicator(); break;
      case 'toggle_grid':
        editorSettings.grid_visible = !editorSettings.grid_visible;
        api('POST', '/api/settings', editorSettings); renderViewportOverlay(); break;
      case 'toggle_rulers':
        editorSettings.rulers_visible = !editorSettings.rulers_visible;
        api('POST', '/api/settings', editorSettings); renderViewportOverlay(); break;
      case 'toggle_snap':
        editorSettings.grid_snap_enabled = !editorSettings.grid_snap_enabled;
        api('POST', '/api/settings', editorSettings); renderViewportOverlay(); break;
      case 'open_settings':
        var esd = document.getElementById('settings-dialog');
        if (esd) esd.style.display = 'flex'; break;
      case 'open_project_settings':
        var psd = document.getElementById('project-settings-dialog');
        if (psd) { psd.style.display = 'flex'; psetLoadValues(); } break;
      case 'open_help': toggleHelpDialog(); break;
      case 'search_nodes':
        var si = document.getElementById('scene-search');
        if (si) { si.focus(); si.select(); } break;
      case 'play_scene':
        document.getElementById('btn-play') && document.getElementById('btn-play').click(); break;
      case 'stop_scene':
        document.getElementById('btn-stop') && document.getElementById('btn-stop').click(); break;
      case 'toggle_theme':
        document.body.classList.toggle('light');
        editorSettings.theme = document.body.classList.contains('light') ? 'light' : 'dark';
        api('POST', '/api/settings', editorSettings); break;
    }
  }

  function setupCommandPalette() {
    var el = document.getElementById('command-palette');
    var input = document.getElementById('cmd-search');

    // Close on backdrop click
    el.addEventListener('click', function(e) {
      if (e.target === el) closeCommandPalette();
    });

    // Search input
    input.addEventListener('input', function() {
      loadCommandList(input.value.trim());
    });

    // Keyboard navigation
    input.addEventListener('keydown', function(e) {
      var items = document.querySelectorAll('#cmd-results .cmd-item');
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        cmdPaletteIndex = Math.min(cmdPaletteIndex + 1, items.length - 1);
        updateCmdSelection(items);
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        cmdPaletteIndex = Math.max(cmdPaletteIndex - 1, 0);
        updateCmdSelection(items);
      } else if (e.key === 'Enter') {
        e.preventDefault();
        if (items[cmdPaletteIndex]) {
          executeCommandFromPalette(items[cmdPaletteIndex].getAttribute('data-cmd'));
        }
      } else if (e.key === 'Escape') {
        e.preventDefault();
        closeCommandPalette();
      }
    });

    // Global shortcut: Ctrl+Shift+P
    document.addEventListener('keydown', function(e) {
      if (e.ctrlKey && e.shiftKey && e.key === 'P') {
        e.preventDefault();
        if (document.getElementById('command-palette').classList.contains('open')) {
          closeCommandPalette();
        } else {
          openCommandPalette();
        }
      }
    });
  }

  function updateCmdSelection(items) {
    for (var i = 0; i < items.length; i++) {
      items[i].classList.toggle('active', i === cmdPaletteIndex);
    }
    if (items[cmdPaletteIndex]) {
      items[cmdPaletteIndex].scrollIntoView({ block: 'nearest' });
    }
  }

  // ---- Init ----
  setupViewport();
  setupToolbar();
  setupToolMode();
  setupContextMenu();
  setupSearch();
  setupKeyboardShortcuts();
  setupBottomPanel();
  setupShaderEditor();
  setupVp2dToolbar();
  setupRightPanelTabs();
  setupConnectDialog();
  setupAddNodeDialog();
  setupPlayButtons();
  setupAnimationPanel();
  setupCurveEditor();
  refreshAnimationList();
  setupSettingsDialog();
  setupProjectSettingsDialog();
  setupExportDialog();
  setupHelpDialog();
  setupConfirmDialog();
  setupSaveSceneDialog();
  setupLeftDivider();
  setupInspectorDivider();
  setupHistoryDock();
  setupFileSystemChrome();
  setupSceneDockTabs();
  setupScriptToolbar();
  setupGameToolbar();
  setupOutputPanel();
  setupAudioPanel();
  setupViewMenu();
  setupVp3dToolbar();
  setupViewportResize();
  setupDebuggerSubtabs();
  setupMenuBar();
  setupDebuggerPanel();
  setupProfilerToolbar();
  buildMonitorTree();
  setupVramTab();
  setupMiscTab();
  setupNetworkProfilerTab();
  setupModeButtons();
  setupSceneTabClose();
  setupCommandPalette();
  logMessage('info', 'Editor ready');
  fetchScene();
  fetchSelected();
  refreshViewport();
  fetchLogs();
  fetchSceneInfo();
  fetchFileSystem();
  updateSceneTab();
  startPolling();
})();
</script>

<!-- Curve editor overlay (pat-o51nk) -->
<div id="curve-editor-overlay" class="curve-editor-overlay">
  <div class="curve-editor-popup">
    <h3>Curve Editor</h3>
    <div class="curve-canvas-wrap">
      <canvas id="curve-editor-canvas" width="200" height="200"></canvas>
    </div>
    <div id="curve-presets" class="curve-presets"></div>
    <div class="curve-inputs">
      <label>x1</label><input id="curve-x1" type="number" step="0.01" min="0" max="1">
      <label>y1</label><input id="curve-y1" type="number" step="0.01">
      <label>x2</label><input id="curve-x2" type="number" step="0.01" min="0" max="1">
      <label>y2</label><input id="curve-y2" type="number" step="0.01">
    </div>
    <div class="curve-actions">
      <button id="curve-cancel-btn">Cancel</button>
      <button id="curve-apply-btn" style="border-color:var(--accent);color:var(--accent)">Apply</button>
    </div>
  </div>
</div>

</body>
</html>
"##;
