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
  --bg: #0d0d0d; --panel: #161616; --border: #2a2a2a;
  --text: #e0e0e0; --text-dim: #888; --accent: #d4a574;
  --selected: #2a1f14; --hover: #1a1a1a; --error: #e05050;
  --icon-node: #999; --icon-node2d: #8ebbff; --icon-sprite2d: #8eef97;
  --icon-camera2d: #c88eff; --icon-control: #8eef97; --icon-label: #8eef97;
  --icon-button: #8eef97; --icon-node3d: #fc7f7f;
  --vec-x: #e05050; --vec-y: #50c878;
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

/* Toolbar */
#toolbar {
  display: flex; align-items: center; gap: 6px; padding: 6px 10px;
  background: var(--panel); border-bottom: 1px solid var(--border); flex-shrink: 0;
}
#toolbar .sep { width: 1px; height: 20px; background: var(--border); margin: 0 4px; }
#toolbar .brand { color: var(--accent); font-weight: bold; font-size: 14px; margin-right: 8px; }
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
#scene-search {
  margin: 4px 6px; padding: 4px 8px; font-size: 12px; border-radius: 3px;
  background: var(--bg); color: var(--text); border: 1px solid var(--border);
}
#scene-search:focus { border-color: var(--accent); }
#scene-search::placeholder { color: var(--text-dim); }
#scene-tree { flex: 1; overflow: auto; padding: 4px 0; }
.tree-node { user-select: none; }
.tree-row {
  display: flex; align-items: center; padding: 2px 8px; cursor: pointer;
  white-space: nowrap; gap: 4px; position: relative; border: 1px solid transparent;
}
.tree-row:hover { background: var(--hover); }
.tree-row.selected { background: var(--selected); color: var(--accent); }
.tree-row.drag-over-above { border-top: 2px solid var(--accent); }
.tree-row.drag-over-inside { background: rgba(212,165,116,0.15); border: 1px dashed var(--accent); }
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
#viewport-panel .panel-header {
  padding: 6px 10px; font-weight: bold; font-size: 11px; text-transform: uppercase;
  color: var(--text-dim); border-bottom: 1px solid var(--border); letter-spacing: 0.5px;
}
#viewport-container { flex: 1; display: flex; align-items: center; justify-content: center; overflow: hidden; padding: 8px; }
#viewport-img {
  max-width: 100%; max-height: 100%; image-rendering: pixelated; background: #111;
  border: 1px solid var(--border);
}
#viewport-placeholder {
  color: var(--text-dim); font-size: 14px; text-align: center;
}

/* Inspector panel */
#inspector-panel {
  width: 300px; min-width: 200px; background: var(--panel);
  border-left: 1px solid var(--border); display: flex; flex-direction: column; flex-shrink: 0;
}
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
.vec2-input { flex: 1; min-width: 40px; }

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

/* Bottom panel */
#bottom-panel {
  background: var(--panel); border-top: 1px solid var(--border);
  display: flex; flex-direction: column; flex-shrink: 0;
  min-height: 30px; transition: height 0.15s;
}
#bottom-panel.collapsed { height: 30px !important; }
#bottom-panel-header {
  display: flex; align-items: center; gap: 0; border-bottom: 1px solid var(--border);
  flex-shrink: 0; height: 30px;
}
.bottom-tab {
  padding: 5px 14px; font-size: 11px; cursor: pointer; color: var(--text-dim);
  border: none; background: transparent; border-bottom: 2px solid transparent;
  font: inherit; text-transform: uppercase; letter-spacing: 0.5px;
}
.bottom-tab:hover { color: var(--text); background: transparent; border-color: transparent; }
.bottom-tab.active { color: var(--accent); border-bottom-color: var(--accent); }
#bottom-toggle {
  margin-left: auto; padding: 4px 8px; font-size: 10px; cursor: pointer;
  color: var(--text-dim); background: transparent; border: none;
}
#bottom-toggle:hover { color: var(--text); background: transparent; border: none; }
#bottom-panel-content { flex: 1; overflow: auto; padding: 6px 10px; font-size: 12px; }
.bottom-content-tab { display: none; }
.bottom-content-tab.active { display: block; }
#output-log { font-family: monospace; font-size: 11px; line-height: 1.5; }
.log-entry { padding: 1px 0; }
.log-entry .log-time { color: var(--text-dim); margin-right: 8px; }
.log-entry .log-msg { color: var(--text); }
.log-entry.log-warn .log-msg { color: #e0c050; }
.log-entry.log-error .log-msg { color: var(--error); }
#scene-info { line-height: 1.8; }
.scene-info-row { display: flex; gap: 8px; }
.scene-info-label { color: var(--text-dim); width: 120px; }

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
#fs-tree { flex: 1; overflow: auto; padding: 4px 0; }
.fs-node { user-select: none; }
.fs-row {
  display: flex; align-items: center; padding: 2px 8px; cursor: pointer;
  white-space: nowrap; gap: 4px; font-size: 12px;
}
.fs-row:hover { background: var(--hover); }
.fs-row.selected { background: var(--selected); color: var(--accent); }
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
#add-node-dialog {
  display: none; position: fixed; top: 0; left: 0; width: 100%; height: 100%;
  background: rgba(0,0,0,0.5); z-index: 300; align-items: center; justify-content: center;
}
#add-node-dialog.open { display: flex; }
#add-node-dialog-inner {
  background: var(--panel); border: 1px solid var(--border); border-radius: 6px;
  width: 420px; max-height: 500px; display: flex; flex-direction: column;
  box-shadow: 0 8px 32px rgba(0,0,0,0.6);
}
#add-node-dialog-header {
  padding: 10px 14px; font-weight: bold; font-size: 13px; border-bottom: 1px solid var(--border);
  display: flex; justify-content: space-between; align-items: center;
}
#add-node-dialog-header button {
  background: transparent; border: none; color: var(--text-dim); cursor: pointer; font-size: 16px; padding: 0 4px;
}
#add-node-dialog-header button:hover { color: var(--text); background: transparent; border: none; }
#add-node-search {
  margin: 8px 12px 4px 12px; padding: 6px 10px; font-size: 12px; border-radius: 3px;
  background: var(--bg); color: var(--text); border: 1px solid var(--border);
}
#add-node-search:focus { border-color: var(--accent); }
#add-node-list {
  flex: 1; overflow: auto; padding: 4px 0; min-height: 200px; max-height: 340px;
}
.add-node-category {
  padding: 4px 14px 2px 14px; font-size: 10px; font-weight: bold; text-transform: uppercase;
  color: var(--text-dim); letter-spacing: 0.5px;
}
.add-node-item {
  padding: 4px 14px 4px 24px; cursor: pointer; font-size: 12px;
  display: flex; align-items: center; gap: 6px;
}
.add-node-item:hover { background: var(--hover); }
.add-node-item.selected { background: var(--selected); color: var(--accent); }
.add-node-item .node-type-icon { width: 16px; text-align: center; font-size: 12px; }
#add-node-description {
  padding: 8px 14px; border-top: 1px solid var(--border); font-size: 11px;
  color: var(--text-dim); min-height: 40px; line-height: 1.4;
}
#add-node-dialog-footer {
  padding: 8px 14px; border-top: 1px solid var(--border); display: flex;
  justify-content: flex-end; gap: 6px;
}

/* Play buttons */
.play-buttons {
  display: flex; align-items: center; gap: 2px; margin-left: auto;
}
.play-btn {
  padding: 4px 8px; font-size: 14px; min-width: 32px; text-align: center; border-radius: 3px;
}
.play-btn:hover { border-color: var(--accent); }
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

/* Inspector/Node tabs */
.right-panel-tabs {
  display: flex; border-bottom: 1px solid var(--border); flex-shrink: 0;
}
.right-panel-tab {
  padding: 5px 14px; font-size: 11px; cursor: pointer; color: var(--text-dim);
  border: none; background: transparent; border-bottom: 2px solid transparent;
  font: inherit; text-transform: uppercase; letter-spacing: 0.5px;
}
.right-panel-tab:hover { color: var(--text); background: transparent; border-color: transparent; }
.right-panel-tab.active { color: var(--accent); border-bottom-color: var(--accent); }
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
.script-editor {
  font-family: 'SF Mono', 'Cascadia Code', 'Consolas', monospace;
  font-size: 12px; line-height: 1.6; padding: 0; margin: 0;
  overflow: auto; background: var(--bg); flex: 1;
}
.script-line {
  display: flex; white-space: pre;
}
.script-line-number {
  width: 40px; text-align: right; padding-right: 8px; color: var(--text-dim);
  user-select: none; flex-shrink: 0; border-right: 1px solid var(--border);
  margin-right: 8px;
}
.script-line-content { flex: 1; }
.script-empty { color: var(--text-dim); font-style: italic; padding: 20px; text-align: center; }
/* GDScript syntax highlighting */
.gd-keyword { color: #569cd6; }
.gd-string { color: #6a9955; }
.gd-comment { color: #6a6a6a; font-style: italic; }
.gd-number { color: #d19a66; }
.gd-builtin { color: #dcdcaa; }
.gd-nodepath { color: #c586c0; }

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
</style>
</head>
<body>

<!-- Toolbar -->
<div id="toolbar">
  <span class="brand">Patina</span>
  <div class="sep"></div>
  <button class="tool-btn active" data-tool="select" title="Select (Q)">Q</button>
  <button class="tool-btn" data-tool="move" title="Move (W)">W</button>
  <button class="tool-btn" data-tool="rotate" title="Rotate (E)">E</button>
  <button class="tool-btn" data-tool="scale" title="Scale (S)">S</button>
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
  <div class="play-buttons">
    <button class="play-btn play-main" id="btn-play" title="Play (F5)">&#9654;</button>
    <button class="play-btn pause-btn" id="btn-pause" title="Pause (F7)">&#9208;</button>
    <button class="play-btn stop-btn" id="btn-stop" title="Stop (F8)">&#9209;</button>
    <button class="play-btn play-current" id="btn-play-current" title="Play Current Scene (F6)">&#9654;&#9998;</button>
  </div>
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
  <div class="ctx-separator"></div>
  <div class="ctx-item" data-action="move-up">Move Up</div>
  <div class="ctx-item" data-action="move-down">Move Down</div>
</div>

<!-- Add Node Dialog -->
<div id="add-node-dialog">
  <div id="add-node-dialog-inner">
    <div id="add-node-dialog-header">
      <span>Create New Node</span>
      <button id="add-node-close" title="Close">&times;</button>
    </div>
    <input type="text" id="add-node-search" placeholder="Search node type..." autocomplete="off">
    <div id="add-node-list"></div>
    <div id="add-node-description">Select a node type to see its description.</div>
    <div id="add-node-dialog-footer">
      <button id="add-node-cancel">Cancel</button>
      <button id="add-node-create" style="border-color:var(--accent);color:var(--accent)">Create</button>
    </div>
  </div>
</div>

<!-- Main area -->
<div id="main">
  <!-- Left panel: Scene tree + FileSystem -->
  <div id="left-panel">
    <div id="scene-panel">
      <div class="panel-header">Scene Tree</div>
      <input type="text" id="scene-search" placeholder="Filter nodes..." autocomplete="off">
      <div id="scene-tree"></div>
    </div>
    <div id="left-divider"></div>
    <div id="filesystem-panel">
      <div class="panel-header"><span>FileSystem</span><span class="fs-path">res://</span></div>
      <div id="fs-tree"></div>
    </div>
  </div>

  <!-- Center: viewport + bottom panel -->
  <div id="center-area">
    <!-- Scene tabs -->
    <div id="scene-tabs">
      <div class="scene-tab active" id="scene-tab-current">Untitled</div>
    </div>
    <!-- Viewport -->
    <div id="viewport-panel">
      <div class="panel-header">Viewport</div>
      <div id="viewport-container">
        <div id="viewport-placeholder">No frame available</div>
      </div>
    </div>

    <!-- Bottom panel -->
    <div id="bottom-resize-handle"></div>
    <div id="bottom-panel" style="height: 150px;">
      <div id="bottom-panel-header">
        <button class="bottom-tab active" data-tab="output">Output</button>
        <button class="bottom-tab" data-tab="scene-info">Scene Info</button>
        <button class="bottom-tab" data-tab="script">Script</button>
        <button class="bottom-tab" data-tab="animation">Animation</button>
        <button id="bottom-toggle" title="Toggle panel">&#9650;</button>
      </div>
      <div id="bottom-panel-content">
        <div class="bottom-content-tab active" data-tab="output">
          <div id="output-log"></div>
        </div>
        <div class="bottom-content-tab" data-tab="scene-info">
          <div id="scene-info"></div>
        </div>
        <div class="bottom-content-tab" data-tab="script">
          <div id="script-panel">
            <div class="script-empty">Select a node with a script to view its content</div>
          </div>
        </div>
        <div class="bottom-content-tab" data-tab="animation">
          <div id="animation-panel">
            <div class="anim-toolbar">
              <select id="anim-select"><option value="">-- No Animation --</option></select>
              <button id="anim-new-btn" title="New Animation">+</button>
              <button id="anim-delete-btn" title="Delete Animation">&#x2715;</button>
              <span class="anim-sep"></span>
              <button id="anim-record-btn" class="anim-record" title="Toggle Recording">&#9679;</button>
              <button id="anim-play-btn" title="Play">&#9654;</button>
              <button id="anim-stop-btn" title="Stop">&#9632;</button>
              <span id="anim-time-display">0.00 / 0.00</span>
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
      </div>
    </div>
  </div>

  <!-- Inspector / Node dock -->
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
    <div class="connect-dialog-buttons">
      <button id="connect-cancel">Cancel</button>
      <button id="connect-confirm" style="border-color:var(--accent);color:var(--accent)">Connect</button>
    </div>
  </div>
</div>


<!-- Settings dialog -->
<div id="settings-dialog">
  <div id="settings-dialog-inner">
    <h3>Editor Settings</h3>
    <div class="settings-row"><span class="settings-label">Grid Snap</span><div class="settings-value"><input type="checkbox" id="set-grid-snap"> <label for="set-grid-snap">Enabled</label></div></div>
    <div class="settings-row"><span class="settings-label">Snap Size</span><div class="settings-value"><select id="set-snap-size"><option value="8">8</option><option value="16">16</option><option value="32">32</option><option value="64">64</option></select></div></div>
    <div class="settings-row"><span class="settings-label">Grid Visible</span><div class="settings-value"><input type="checkbox" id="set-grid-visible" checked></div></div>
    <div class="settings-row"><span class="settings-label">Rulers Visible</span><div class="settings-value"><input type="checkbox" id="set-rulers-visible" checked></div></div>
    <div class="settings-row"><span class="settings-label">Font Size</span><div class="settings-value"><select id="set-font-size"><option value="small">Small</option><option value="medium" selected>Medium</option><option value="large">Large</option></select></div></div>
    <div style="text-align:right;margin-top:12px"><button id="settings-close">Close</button></div>
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
  var collapsedSections = {};
  var lastLogCount = 0;

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
    modulate: { type: 'Color', value: [1, 1, 1, 1] },
    self_modulate: { type: 'Color', value: [1, 1, 1, 1] },
    offset: { type: 'Vector2', value: [0, 0] },
    flip_h: { type: 'Bool', value: false },
    flip_v: { type: 'Bool', value: false },
    zoom: { type: 'Vector2', value: [1, 1] },
    current: { type: 'Bool', value: false }
  };

  // Class-specific default properties that should always show
  var CLASS_DEFAULT_PROPS = {
    Node2D: ['position', 'rotation', 'scale', 'visible', 'z_index'],
    Sprite2D: ['position', 'rotation', 'scale', 'visible', 'z_index', 'texture', 'offset', 'flip_h', 'flip_v', 'modulate'],
    Camera2D: ['position', 'rotation', 'scale', 'visible', 'z_index', 'zoom', 'current'],
    Control: ['position', 'rotation', 'scale', 'visible', 'anchor_left', 'anchor_top', 'anchor_right', 'anchor_bottom', 'offset_left', 'offset_top', 'offset_right', 'offset_bottom', 'size_flags_horizontal', 'size_flags_vertical'],
    Label: ['position', 'rotation', 'scale', 'visible', 'text', 'modulate'],
    Button: ['position', 'rotation', 'scale', 'visible', 'text', 'modulate']
  };

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
    try {
      var resp = await fetch(path, opts);
      var text = await resp.text();
      if (!text || text === 'null') return null;
      return JSON.parse(text);
    } catch (e) {
      return null;
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
  function nodeMatchesFilter(node, filter) {
    if (!filter) return true;
    var lower = filter.toLowerCase();
    return node.name && node.name.toLowerCase().indexOf(lower) >= 0;
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
    iconWrapper.innerHTML = classIconHtml(node['class']);
    var icon = iconWrapper.firstChild;

    var name = document.createElement('span');
    name.className = 'tree-name';
    name.textContent = node.name;

    if (searchFilter && nodeMatchesFilter(node, searchFilter)) {
      name.style.color = 'var(--accent)';
      name.style.fontWeight = 'bold';
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

    row.appendChild(toggle);
    row.appendChild(icon);
    row.appendChild(name);
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
      if (treeDragNodeId === null || treeDragNodeId === nid) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = 'move';
      var rect = row.getBoundingClientRect();
      var y = e.clientY - rect.top;
      var h = rect.height;
      clearDragIndicators();
      if (y < h * 0.25) { row.classList.add('drag-over-above'); treeDragZone = 'above'; }
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
      if (treeDragNodeId === null || treeDragNodeId === nid) return;
      if (treeDragZone === 'inside') {
        api('POST', '/api/node/reparent', { node_id: treeDragNodeId, new_parent_id: nid })
          .then(function() { expandedNodes.add(nid); fetchScene(); });
      } else {
        var targetParent = findNodeParent(sceneData.nodes, nid);
        if (targetParent) {
          api('POST', '/api/node/reparent', { node_id: treeDragNodeId, new_parent_id: targetParent.id })
            .then(function() { fetchScene(); });
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
    }
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
    }
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
    if (!confirm('Delete selected node?')) return;
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
    await api('POST', '/api/node/select', { node_id: id });
    refreshTree();
    await fetchSelected();
  }

  async function fetchSelected() {
    if (selectedNodeIds.size > 1) { renderInspectorMulti(selectedNodeIds.size); return; }
    if (selectedNodeId === null) {
      renderInspectorEmpty();
      renderNodeDockEmpty();
      clearScript();
      return;
    }
    var data = await api('GET', '/api/selected');
    if (data) {
      selectedNodeData = data;
      renderInspector(data);
      document.getElementById('status-selected').textContent = data.name || 'None';
      document.getElementById('status-path').textContent = data.path || '\u2014';
      // Refresh node dock if visible.
      if (currentRightTab === 'node') fetchNodeDock();
      // Check for script property and load it.
      var scriptPath = null;
      if (data.properties) {
        for (var pi = 0; pi < data.properties.length; pi++) {
          if (data.properties[pi].name === 'script') {
            var sv = data.properties[pi].value;
            if (sv && sv.value && typeof sv.value === 'string') scriptPath = sv.value;
            break;
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
  function renderInspectorMulti(count) {
    document.getElementById('inspector').innerHTML = '<div class="insp-empty">' + count + ' nodes selected</div>';
    document.getElementById('status-selected').textContent = count + ' nodes';
    document.getElementById('status-path').textContent = '\u2014';
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

    // Node info section
    var infoSection = createSection('Node', 'node-info');
    var infoBody = infoSection.querySelector('.insp-section-body');

    // Name (editable)
    var nameRow = document.createElement('div');
    nameRow.className = 'insp-row';
    nameRow.innerHTML = '<div class="insp-label">Name</div>';
    var nameVal = document.createElement('div');
    nameVal.className = 'insp-value';
    var nameInput = document.createElement('input');
    nameInput.type = 'text';
    nameInput.value = data.name || '';
    nameInput.addEventListener('change', function() {
      if (nameInput.value && nameInput.value !== data.name) {
        api('POST', '/api/node/rename', { node_id: data.id, new_name: nameInput.value })
          .then(function() { fetchScene(); fetchSelected(); });
      }
    });
    nameVal.appendChild(nameInput);
    nameRow.appendChild(nameVal);
    infoBody.appendChild(nameRow);

    // Class (readonly)
    var classRow = document.createElement('div');
    classRow.className = 'insp-row';
    classRow.innerHTML = '<div class="insp-label">Class</div><div class="insp-value"><span class="insp-readonly">' +
      escapeHtml(data['class'] || 'Unknown') + '</span></div>';
    infoBody.appendChild(classRow);
    el.appendChild(infoSection);

    // Build property map
    var propMap = {};
    if (data.properties) {
      for (var i = 0; i < data.properties.length; i++) {
        propMap[data.properties[i].name] = data.properties[i];
      }
    }

    // Add class-specific defaults
    var cls = data['class'] || '';
    var defaults = CLASS_DEFAULT_PROPS[cls] || [];
    if (!CLASS_DEFAULT_PROPS[cls] && (cls.indexOf('2D') >= 0 || cls.indexOf('2d') >= 0)) {
      defaults = CLASS_DEFAULT_PROPS['Node2D'] || [];
    }
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

    // Group by category
    var categories = {};
    var propNames = Object.keys(propMap);
    for (var pi = 0; pi < propNames.length; pi++) {
      var prop = propMap[propNames[pi]];
      if (prop.type === 'Nil') continue;
      var cat = getPropCategory(prop.name);
      if (cat === 'Internal') continue;
      if (!categories[cat]) categories[cat] = [];
      categories[cat].push(prop);
    }

    // Render categories
    for (var ci = 0; ci < CATEGORY_ORDER.length; ci++) {
      var catName = CATEGORY_ORDER[ci];
      var catProps = categories[catName];
      if (!catProps || catProps.length === 0) continue;

      var section = createSection(catName, 'cat-' + catName);
      var body = section.querySelector('.insp-section-body');

      catProps.sort(function(a, b) { return a.name.localeCompare(b.name); });
      for (var cpi = 0; cpi < catProps.length; cpi++) {
        body.appendChild(createPropertyRow(data.id, catProps[cpi]));
      }
      el.appendChild(section);
    }
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

  function createPropertyRow(nodeId, prop) {
    var row = document.createElement('div');
    row.className = 'insp-row';
    var label = document.createElement('div');
    label.className = 'insp-label';
    label.textContent = prop.name;
    label.title = prop.name;
    row.appendChild(label);

    var val = document.createElement('div');
    val.className = 'insp-value';

    var type = prop.type;
    var v = prop.value && prop.value.value;

    if (type === 'String') {
      var input = document.createElement('input');
      input.type = 'text';
      input.value = v != null ? String(v) : '';
      input.addEventListener('change', function() {
        setProperty(nodeId, prop.name, { type: 'String', value: input.value });
      });
      val.appendChild(input);
    } else if (type === 'Int') {
      if (prop.name === 'body_type') {
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
      var arr = Array.isArray(v) ? v : [0, 0];
      var vec = document.createElement('div');
      vec.className = 'vec2-editor';
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
        return { field: field, input: inp };
      }
      var xf = makeVec2Field('x', 0);
      var yf = makeVec2Field('y', 1);
      function sendVec2() {
        setProperty(nodeId, prop.name, { type: 'Vector2', value: [parseFloat(xf.input.value)||0, parseFloat(yf.input.value)||0] });
      }
      xf.input.addEventListener('change', sendVec2);
      yf.input.addEventListener('change', sendVec2);
      vec.appendChild(xf.field); vec.appendChild(yf.field);
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
      var npBtn = document.createElement('button');
      npBtn.className = 'nodepath-pick'; npBtn.textContent = '...';
      npBtn.title = 'Pick node (not yet implemented)';
      npDiv.appendChild(npInput); npDiv.appendChild(npBtn);
      val.appendChild(npDiv);
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

  function setupViewport() {
    var container = document.getElementById('viewport-container');
    viewportImg = document.createElement('img');
    viewportImg.id = 'viewport-img';
    viewportImg.style.display = 'none';
    viewportImg.draggable = false;
    viewportImg.addEventListener('mousedown', function(e) {
      e.preventDefault();
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
        }
      });
    });
    document.addEventListener('mousemove', function(e) {
      if (isBoxSelecting && boxSelectStart) {
        var x = Math.min(boxSelectStart.x, e.clientX);
        var y = Math.min(boxSelectStart.y, e.clientY);
        var w = Math.abs(e.clientX - boxSelectStart.x);
        var h = Math.abs(e.clientY - boxSelectStart.y);
        boxSelectOverlay.style.left = x + 'px'; boxSelectOverlay.style.top = y + 'px';
        boxSelectOverlay.style.width = w + 'px'; boxSelectOverlay.style.height = h + 'px';
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
      if (isBoxSelecting && boxSelectStart) {
        isBoxSelecting = false;
        if (boxSelectOverlay) boxSelectOverlay.style.display = 'none';
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

    // Zoom with mouse wheel
    container.addEventListener('wheel', function(e) {
      e.preventDefault();
      var delta = e.deltaY > 0 ? -0.1 : 0.1;
      viewportZoom = Math.max(0.1, Math.min(16.0, viewportZoom + delta * viewportZoom));
      api('POST', '/api/viewport/zoom', { zoom: viewportZoom });
      updateZoomIndicator();
    }, { passive: false });

    // Pan with middle-mouse or Shift+left drag
    container.addEventListener('mousedown', function(e) {
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
    });
    document.getElementById('zoom-out').addEventListener('click', function() {
      viewportZoom = Math.max(0.1, viewportZoom / 1.25);
      api('POST', '/api/viewport/zoom', { zoom: viewportZoom });
      updateZoomIndicator();
    });
    document.getElementById('zoom-reset').addEventListener('click', function() {
      viewportZoom = 1.0; viewportPanX = 0; viewportPanY = 0;
      api('POST', '/api/viewport/zoom', { zoom: 1.0 });
      api('POST', '/api/viewport/pan', { x: 0, y: 0 });
      updateZoomIndicator();
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
  }

  function updateZoomIndicator() {
    var label = document.getElementById('zoom-label');
    if (label) label.textContent = Math.round(viewportZoom * 100) + '%';
    var szl = document.getElementById('status-zoom');
    if (szl) szl.textContent = Math.round(viewportZoom * 100) + '%';
  }

  function refreshViewport() {
    if (!viewportImg) return;
    var img = new Image();
    img.onload = function() {
      viewportImg.src = img.src;
      viewportImg.style.display = 'block';
      var ph = document.getElementById('viewport-placeholder');
      if (ph) ph.style.display = 'none';
    };
    img.onerror = function() {
      viewportImg.style.display = 'none';
      var ph = document.getElementById('viewport-placeholder');
      if (ph) ph.style.display = 'block';
    };
    img.src = '/api/viewport/png?t=' + Date.now();
  }

  // ---- Tool mode ----
  function setupToolMode() {
    var toolBtns = document.querySelectorAll('.tool-btn');
    for (var i = 0; i < toolBtns.length; i++) {
      toolBtns[i].addEventListener('click', function() {
        currentToolMode = this.getAttribute('data-tool');
        for (var j = 0; j < toolBtns.length; j++) {
          toolBtns[j].classList.toggle('active', toolBtns[j].getAttribute('data-tool') === currentToolMode);
        }
      });
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
  }

  // ---- Bottom panel ----
  function setupBottomPanel() {
    var panel = document.getElementById('bottom-panel');
    var toggleBtn = document.getElementById('bottom-toggle');
    var tabs = document.querySelectorAll('.bottom-tab');
    var contents = document.querySelectorAll('.bottom-content-tab');
    var resizeHandle = document.getElementById('bottom-resize-handle');

    toggleBtn.addEventListener('click', function() {
      panel.classList.toggle('collapsed');
      toggleBtn.innerHTML = panel.classList.contains('collapsed') ? '&#9660;' : '&#9650;';
    });

    for (var i = 0; i < tabs.length; i++) {
      tabs[i].addEventListener('click', function() {
        var tabName = this.getAttribute('data-tab');
        for (var j = 0; j < tabs.length; j++) tabs[j].classList.toggle('active', tabs[j].getAttribute('data-tab') === tabName);
        for (var j = 0; j < contents.length; j++) contents[j].classList.toggle('active', contents[j].getAttribute('data-tab') === tabName);
        if (panel.classList.contains('collapsed')) {
          panel.classList.remove('collapsed');
          toggleBtn.innerHTML = '&#9650;';
        }
      });
    }

    // Resize
    var isResizing = false;
    var startY = 0;
    var startH = 0;
    resizeHandle.addEventListener('mousedown', function(e) {
      isResizing = true; startY = e.clientY; startH = panel.offsetHeight; e.preventDefault();
    });
    document.addEventListener('mousemove', function(e) {
      if (!isResizing) return;
      var newH = Math.max(30, Math.min(400, startH + (startY - e.clientY)));
      panel.style.height = newH + 'px';
    });
    document.addEventListener('mouseup', function() { isResizing = false; });
  }

  async function fetchLogs() {
    var data = await api('GET', '/api/logs');
    if (!data || !Array.isArray(data)) return;
    if (data.length === lastLogCount) return;
    lastLogCount = data.length;
    var logEl = document.getElementById('output-log');
    logEl.innerHTML = '';
    for (var i = data.length - 1; i >= 0; i--) {
      var entry = data[i];
      var div = document.createElement('div');
      div.className = 'log-entry' + (entry.level === 'warn' ? ' log-warn' : '') + (entry.level === 'error' ? ' log-error' : '');
      var time = new Date(entry.timestamp);
      var timeStr = time.toLocaleTimeString();
      div.innerHTML = '<span class="log-time">[' + escapeHtml(timeStr) + ']</span><span class="log-msg">' + escapeHtml(entry.message) + '</span>';
      logEl.appendChild(div);
    }
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

  // ---- Add Node Dialog ----
  var NODE_TYPES = {
    'Node': { category: 'Node', desc: 'Base class for all scene objects. A node can contain other nodes as children.' },
    'Node2D': { category: '2D', desc: 'A 2D game object. Base node for 2D game entities with position, rotation, and scale.' },
    'Sprite2D': { category: '2D', desc: 'Displays a 2D texture. Can be used as a visual representation for game objects.' },
    'AnimatedSprite2D': { category: '2D', desc: 'Sprite node that contains multiple textures as animation frames.' },
    'Camera2D': { category: '2D', desc: 'Camera node for 2D scenes. Controls the viewport view.' },
    'Light2D': { category: '2D', desc: 'Casts light in a 2D environment. Can be used for dynamic lighting effects.' },
    'CanvasModulate': { category: '2D', desc: 'Applies a color tint to the entire canvas. Useful for day/night cycles.' },
    'CharacterBody2D': { category: 'Physics 2D', desc: 'Specialized 2D physics body for characters controlled by script.' },
    'RigidBody2D': { category: 'Physics 2D', desc: 'A 2D physics body that is moved by the physics engine simulation.' },
    'StaticBody2D': { category: 'Physics 2D', desc: 'A 2D physics body that cannot be moved. Used for walls and floors.' },
    'Area2D': { category: 'Physics 2D', desc: 'A 2D area that detects overlapping bodies and areas.' },
    'CollisionShape2D': { category: 'Physics 2D', desc: 'Provides collision shape for 2D physics bodies.' },
    'Control': { category: 'UI', desc: 'Base class for all UI-related nodes. Handles input events and anchoring.' },
    'Button': { category: 'UI', desc: 'A standard themed button that can contain text and an icon.' },
    'Label': { category: 'UI', desc: 'Displays plain text. Supports wrapping and alignment.' },
    'TextEdit': { category: 'UI', desc: 'A multi-line text editing control with syntax highlighting support.' },
    'LineEdit': { category: 'UI', desc: 'A single-line text input field.' },
    'Panel': { category: 'UI', desc: 'A UI panel that draws a background style box.' },
    'TextureRect': { category: 'UI', desc: 'Displays a texture inside a UI layout. Supports stretch modes.' },
    'VBoxContainer': { category: 'UI', desc: 'Arranges child controls vertically.' },
    'HBoxContainer': { category: 'UI', desc: 'Arranges child controls horizontally.' },
    'GridContainer': { category: 'UI', desc: 'Arranges child controls in a grid pattern.' },
    'ScrollContainer': { category: 'UI', desc: 'A container that provides scrollbars when content exceeds bounds.' },
    'TabContainer': { category: 'UI', desc: 'A container with tabs at the top for switching between child controls.' },
    'AudioStreamPlayer': { category: 'Audio', desc: 'Plays audio non-positionally. Useful for background music and UI sounds.' },
    'AudioStreamPlayer2D': { category: 'Audio', desc: 'Plays audio with 2D positional effects.' },
    'Timer': { category: 'Other', desc: 'Counts down a specified interval and emits a signal when it reaches 0.' },
    'AnimationPlayer': { category: 'Other', desc: 'Plays animations. Can animate any property of any node.' },
    'NavigationAgent2D': { category: 'Other', desc: 'Provides navigation and pathfinding for 2D characters.' },
    'Node3D': { category: 'Other', desc: 'Base node for 3D game entities with 3D transform.' }
  };

  var CATEGORY_DISPLAY_ORDER = ['Node', '2D', 'Physics 2D', 'UI', 'Audio', 'Other'];
  var addNodeSelectedType = null;

  function openAddNodeDialog() {
    addNodeSelectedType = null;
    document.getElementById('add-node-search').value = '';
    renderAddNodeList('');
    document.getElementById('add-node-dialog').classList.add('open');
    setTimeout(function() { document.getElementById('add-node-search').focus(); }, 50);
  }

  function closeAddNodeDialog() {
    document.getElementById('add-node-dialog').classList.remove('open');
    addNodeSelectedType = null;
  }

  function renderAddNodeList(filter) {
    var list = document.getElementById('add-node-list');
    list.innerHTML = '';
    var lower = filter.toLowerCase();
    var byCategory = {};
    var types = Object.keys(NODE_TYPES);
    for (var i = 0; i < types.length; i++) {
      var t = types[i];
      if (lower && t.toLowerCase().indexOf(lower) < 0) continue;
      var cat = NODE_TYPES[t].category;
      if (!byCategory[cat]) byCategory[cat] = [];
      byCategory[cat].push(t);
    }
    for (var ci = 0; ci < CATEGORY_DISPLAY_ORDER.length; ci++) {
      var catName = CATEGORY_DISPLAY_ORDER[ci];
      var items = byCategory[catName];
      if (!items || items.length === 0) continue;
      var catEl = document.createElement('div');
      catEl.className = 'add-node-category';
      catEl.textContent = catName;
      list.appendChild(catEl);
      for (var j = 0; j < items.length; j++) {
        (function(typeName) {
          var item = document.createElement('div');
          item.className = 'add-node-item' + (typeName === addNodeSelectedType ? ' selected' : '');
          var icon = document.createElement('span');
          icon.className = 'node-type-icon';
          icon.innerHTML = classIconHtml(typeName).replace('tree-icon', 'node-type-icon');
          var nameSpan = document.createElement('span');
          nameSpan.textContent = typeName;
          item.appendChild(icon);
          item.appendChild(nameSpan);
          item.addEventListener('click', function() {
            addNodeSelectedType = typeName;
            list.querySelectorAll('.add-node-item').forEach(function(el) { el.classList.remove('selected'); });
            item.classList.add('selected');
            document.getElementById('add-node-description').textContent = NODE_TYPES[typeName].desc;
          });
          item.addEventListener('dblclick', function() {
            addNodeSelectedType = typeName;
            createSelectedNode();
          });
          list.appendChild(item);
        })(items[j]);
      }
    }
    // Auto-select first if filter narrows
    if (filter && !addNodeSelectedType) {
      var first = list.querySelector('.add-node-item');
      if (first) first.click();
    }
  }

  async function createSelectedNode() {
    if (!addNodeSelectedType) return;
    var name = prompt('Node name:', addNodeSelectedType);
    if (!name) return;
    var parentId = selectedNodeId || (sceneData && sceneData.nodes ? sceneData.nodes.id : null);
    if (parentId === null) return;
    await api('POST', '/api/node/add', { parent_id: parentId, name: name, class_name: addNodeSelectedType });
    if (selectedNodeId) expandedNodes.add(selectedNodeId);
    closeAddNodeDialog();
    await fetchScene();
  }

  function setupAddNodeDialog() {
    document.getElementById('add-node-search').addEventListener('input', function() {
      renderAddNodeList(this.value.trim());
    });
    document.getElementById('add-node-search').addEventListener('keydown', function(e) {
      if (e.key === 'Enter') { e.preventDefault(); createSelectedNode(); }
      if (e.key === 'Escape') { e.preventDefault(); closeAddNodeDialog(); }
    });
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
    var ext = entry.name.split('.').pop();
    if (ext === 'tscn') return '\uD83D\uDCC4';
    if (ext === 'gd') return '\uD83D\uDCDC';
    if (ext === 'tres') return '\uD83D\uDCE6';
    return '\uD83D\uDCC4';
  }

  function renderFsTree(entries, depth, container) {
    if (!entries) return;
    for (var i = 0; i < entries.length; i++) {
      var entry = entries[i];
      var node = document.createElement('div');
      node.className = 'fs-node';
      node.style.paddingLeft = (depth * 16) + 'px';

      var row = document.createElement('div');
      row.className = 'fs-row';

      var toggle = document.createElement('span');
      toggle.className = 'fs-toggle';
      if (entry.is_dir) {
        var isExpanded = fsExpandedDirs.has(entry.path);
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
          row.addEventListener('click', function() {
            var ext = e.name.split('.').pop();
            if (ext === 'tscn') {
              api('POST', '/api/scene/load', { path: e.path.replace('res://', '') }).then(function() {
                selectedNodeId = null; selectedNodeData = null;
                expandedNodes.clear(); renderInspectorEmpty();
                fetchScene(); fetchSceneInfo();
              });
            }
          });
        })(entry);
      }

      node.appendChild(row);
      container.appendChild(node);

      if (entry.is_dir && entry.children && fsExpandedDirs.has(entry.path)) {
        var childContainer = document.createElement('div');
        renderFsTree(entry.children, depth + 1, childContainer);
        container.appendChild(childContainer);
      }
    }
  }

  function refreshFsTree() {
    var el = document.getElementById('fs-tree');
    el.innerHTML = '';
    if (fsData && fsData.files) {
      renderFsTree(fsData.files, 0, el);
    }
  }

  async function fetchFileSystem() {
    var data = await api('GET', '/api/filesystem');
    if (data) {
      fsData = data;
      refreshFsTree();
    }
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

  // ---- Scene tabs ----
  function updateSceneTab() {
    var tab = document.getElementById('scene-tab-current');
    if (!tab) return;
    var info = sceneData;
    var sceneInfo = document.getElementById('scene-info');
    // Use scene file name or "Untitled"
    var el = document.getElementById('scene-file-indicator');
    var name = 'Untitled';
    if (el && el.textContent && el.textContent.replace(/^\*\s*/, '').trim()) {
      name = el.textContent.replace(/^\*\s*/, '').trim();
    }
    var modified = el && el.querySelector('.modified');
    tab.innerHTML = escapeHtml(name) + (modified ? '<span class="modified-indicator"> *</span>' : '');
  }

  // ---- Runtime state ----
  var runtimeRunning = false, runtimePaused = false, runtimeFrameCount = 0, runtimeFps = 0, runtimeStatusInterval = null;

  function updatePlayButtonStates() {
    var bp = document.getElementById('btn-play'), bpa = document.getElementById('btn-pause'), bs = document.getElementById('btn-stop');
    if (runtimeRunning) {
      bp.style.borderColor = 'var(--accent)'; bp.style.background = 'rgba(80,200,120,0.15)';
      bs.style.borderColor = '#e05050';
      bpa.style.borderColor = runtimePaused ? 'var(--accent)' : ''; bpa.style.background = runtimePaused ? 'rgba(224,192,80,0.15)' : '';
    } else { bp.style.borderColor = ''; bp.style.background = ''; bpa.style.borderColor = ''; bpa.style.background = ''; bs.style.borderColor = ''; }
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
  async function pollRuntimeStatus() {
    if (!runtimeRunning) return;
    var r = await api('GET', '/api/runtime/status');
    if (r) { runtimeRunning = r.running; runtimePaused = r.paused; runtimeFrameCount = r.frame_count; runtimeFps = r.fps; updatePlayButtonStates(); showPlayingOverlay(runtimeRunning); updateRuntimeStatusBar(); if (!runtimeRunning) { setRuntimeEditingDisabled(false); stopRuntimePolling(); } }
  }
  function startRuntimePolling() { if (runtimeStatusInterval) return; runtimeStatusInterval = setInterval(pollRuntimeStatus, 200); }
  function stopRuntimePolling() { if (runtimeStatusInterval) { clearInterval(runtimeStatusInterval); runtimeStatusInterval = null; } }

  // ---- Play buttons ----
  function setupPlayButtons() {
    document.getElementById('btn-play').addEventListener('click', async function() {
      if (runtimeRunning) return;
      var r = await api('POST', '/api/runtime/play');
      if (r && r.ok) { runtimeRunning = true; runtimePaused = false; runtimeFrameCount = 0; logMessage('info', 'Play started (F5)'); updatePlayButtonStates(); showPlayingOverlay(true); updateRuntimeStatusBar(); setRuntimeEditingDisabled(true); startRuntimePolling(); }
    });
    document.getElementById('btn-pause').addEventListener('click', async function() {
      if (!runtimeRunning) return;
      var r = await api('POST', '/api/runtime/pause');
      if (r && r.ok) { runtimePaused = r.paused; logMessage('info', runtimePaused ? 'Paused (F7)' : 'Resumed (F7)'); updatePlayButtonStates(); showPlayingOverlay(true); }
    });
    document.getElementById('btn-stop').addEventListener('click', async function() {
      var r = await api('POST', '/api/runtime/stop');
      if (r && r.ok) { runtimeRunning = false; runtimePaused = false; runtimeFrameCount = 0; logMessage('info', 'Stopped (F8)'); updatePlayButtonStates(); showPlayingOverlay(false); updateRuntimeStatusBar(); setRuntimeEditingDisabled(false); stopRuntimePolling(); }
    });
    document.getElementById('btn-play-current').addEventListener('click', async function() {
      if (runtimeRunning) return;
      var r = await api('POST', '/api/runtime/play');
      if (r && r.ok) { runtimeRunning = true; runtimePaused = false; runtimeFrameCount = 0; logMessage('info', 'Play Current Scene (F6)'); updatePlayButtonStates(); showPlayingOverlay(true); updateRuntimeStatusBar(); setRuntimeEditingDisabled(true); startRuntimePolling(); }
    });
  }

  function logMessage(level, message) {
    // Add a log entry to the output panel directly
    var logEl = document.getElementById('output-log');
    var div = document.createElement('div');
    div.className = 'log-entry';
    var time = new Date();
    var timeStr = time.toLocaleTimeString();
    div.innerHTML = '<span class="log-time">[' + escapeHtml(timeStr) + ']</span><span class="log-msg">' + escapeHtml(message) + '</span>';
    logEl.insertBefore(div, logEl.firstChild);
  }

  // ---- Toolbar actions ----
  function setupToolbar() {
    var btnAdd = document.getElementById('btn-add');
    btnAdd.addEventListener('click', function(e) { e.stopPropagation(); openAddNodeDialog(); });

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
      var path = prompt('Save path:', 'scene.tscn');
      if (!path) return;
      var result = await api('POST', '/api/scene/save', { path: path });
      if (result && result.ok) {
        var btn = document.getElementById('btn-save');
        btn.style.borderColor = 'var(--accent)';
        setTimeout(function(){ btn.style.borderColor = ''; }, 500);
        fetchSceneInfo();
      }
    });

    document.getElementById('btn-load').addEventListener('click', async function() {
      var path = prompt('Load path:');
      if (!path) return;
      await api('POST', '/api/scene/load', { path: path });
      selectedNodeId = null; selectedNodeData = null;
      expandedNodes.clear(); renderInspectorEmpty();
      await fetchScene(); fetchSceneInfo();
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

      // Tool mode shortcuts
      if (e.key === 'q' || e.key === 'Q') { setToolMode('select'); return; }
      if (e.key === 'w' || e.key === 'W') { setToolMode('move'); return; }
      if (e.key === 'e' || e.key === 'E') { setToolMode('rotate'); return; }

      // Play shortcuts
      if (e.key === 'F5') { e.preventDefault(); document.getElementById('btn-play').click(); return; }
      if (e.key === 'F6') { e.preventDefault(); document.getElementById('btn-play-current').click(); return; }
      if (e.key === 'F7') { e.preventDefault(); document.getElementById('btn-pause').click(); return; }
      if (e.key === 'F8') { e.preventDefault(); document.getElementById('btn-stop').click(); return; }
    });
  }

  // ---- Multi-delete ----
  async function doDeleteMulti() {
    if (selectedNodeIds.size === 0) return;
    if (!confirm('Delete ' + selectedNodeIds.size + ' selected nodes?')) return;
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
  }
  async function updateSetting(key, value) {
    var body = {}; body[key] = value;
    var data = await api('POST', '/api/settings', body);
    if (data) editorSettings = data;
  }


  // ---- Polling ----
  function startPolling() {
    setInterval(fetchScene, 500);
    setInterval(refreshViewport, 200);
    setInterval(fetchLogs, 1000);
    setInterval(function() { fetchSceneInfo(); updateSceneTab(); }, 2000);
    setInterval(fetchFileSystem, 5000);
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

  function renderNodeDock(data) {
    var el = document.getElementById('node-dock');
    el.innerHTML = '';

    // Signals section
    var sigSection = createSection('Signals', 'node-signals');
    var sigBody = sigSection.querySelector('.insp-section-body');

    if (data.signals && data.signals.length > 0) {
      for (var i = 0; i < data.signals.length; i++) {
        var sig = data.signals[i];
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
        sigBody.appendChild(row);
      }
    } else {
      var empty = document.createElement('div');
      empty.className = 'insp-empty';
      empty.style.padding = '8px';
      empty.textContent = 'No signals for this node type';
      sigBody.appendChild(empty);
    }
    el.appendChild(sigSection);

    // Groups section
    var grpSection = createSection('Groups', 'node-groups');
    var grpBody = grpSection.querySelector('.insp-section-body');
    var groupsDiv = document.createElement('div');
    groupsDiv.className = 'groups-section';

    if (data.groups && data.groups.length > 0) {
      for (var gi = 0; gi < data.groups.length; gi++) {
        var tag = document.createElement('span');
        tag.className = 'group-tag';
        tag.textContent = data.groups[gi];
        var removeBtn = document.createElement('span');
        removeBtn.className = 'group-remove';
        removeBtn.innerHTML = '&#10005;';
        removeBtn.title = 'Remove group';
        removeBtn.addEventListener('click', (function(group) { return function() {
          api('POST', '/api/node/groups/remove', { node_id: selectedNodeId, group: group })
            .then(function() { fetchNodeDock(); });
        }; })(data.groups[gi]));
        tag.appendChild(removeBtn);
        groupsDiv.appendChild(tag);
      }
    }

    var addRow = document.createElement('div');
    addRow.className = 'group-add-row';
    var addInput = document.createElement('input');
    addInput.type = 'text';
    addInput.placeholder = 'New group name...';
    var addBtn = document.createElement('button');
    addBtn.textContent = 'Add';
    addBtn.addEventListener('click', function() {
      var groupName = addInput.value.trim();
      if (!groupName) return;
      api('POST', '/api/node/groups/add', { node_id: selectedNodeId, group: groupName })
        .then(function() { addInput.value = ''; fetchNodeDock(); });
    });
    addInput.addEventListener('keydown', function(e) {
      if (e.key === 'Enter') addBtn.click();
    });
    addRow.appendChild(addInput);
    addRow.appendChild(addBtn);
    groupsDiv.appendChild(addRow);
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
      api('POST', '/api/node/signals/connect', {
        node_id: selectedNodeId,
        signal: pendingConnectSignal,
        method: method
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

  function highlightGDScript(line) {
    var result = '';
    var i = 0;
    while (i < line.length) {
      if (line[i] === '#') {
        result += '<span class="gd-comment">' + escapeHtml(line.substring(i)) + '</span>';
        break;
      }
      if (line[i] === '"' || line[i] === "'") {
        var quote = line[i];
        var end = line.indexOf(quote, i + 1);
        if (end === -1) end = line.length - 1;
        result += '<span class="gd-string">' + escapeHtml(line.substring(i, end + 1)) + '</span>';
        i = end + 1;
        continue;
      }
      if (line[i] === '$') {
        var match = line.substring(i).match(/^\$[A-Za-z0-9_/]+/);
        if (match) {
          result += '<span class="gd-nodepath">' + escapeHtml(match[0]) + '</span>';
          i += match[0].length;
          continue;
        }
      }
      if (/[0-9]/.test(line[i]) && (i === 0 || /[\s(,=+\-*/<>!&|^~\[]/.test(line[i-1]))) {
        var numMatch = line.substring(i).match(/^[0-9]+(\.[0-9]+)?/);
        if (numMatch) {
          result += '<span class="gd-number">' + escapeHtml(numMatch[0]) + '</span>';
          i += numMatch[0].length;
          continue;
        }
      }
      if (/[a-zA-Z_]/.test(line[i])) {
        var wordMatch = line.substring(i).match(/^[a-zA-Z_][a-zA-Z0-9_]*/);
        if (wordMatch) {
          var word = wordMatch[0];
          var keywords = ['func','var','if','else','elif','for','while','return','class','extends','signal','enum','match','const','static','onready','export','pass','break','continue','in','not','and','or','true','false','null','self','yield','await','class_name','preload','load'];
          var builtins = ['print','str','int','float','len','range','abs','min','max','clamp','lerp','sign','round','ceil','floor','sqrt','pow','sin','cos','tan'];
          if (keywords.indexOf(word) >= 0) {
            result += '<span class="gd-keyword">' + escapeHtml(word) + '</span>';
          } else if (builtins.indexOf(word) >= 0) {
            result += '<span class="gd-builtin">' + escapeHtml(word) + '</span>';
          } else {
            result += escapeHtml(word);
          }
          i += word.length;
          continue;
        }
      }
      result += escapeHtml(line[i]);
      i++;
    }
    return result;
  }

  async function fetchScript(path) {
    if (!path || path === currentScriptPath) return;
    currentScriptPath = path;
    var data = await api('GET', '/api/script?path=' + encodeURIComponent(path));
    if (data && data.content !== undefined) {
      renderScript(data.content, data.path);
    } else {
      document.getElementById('script-panel').innerHTML = '<div class="script-empty">Could not load script: ' + escapeHtml(path) + '</div>';
    }
  }

  function renderScript(content, path) {
    var el = document.getElementById('script-panel');
    el.innerHTML = '';

    var header = document.createElement('div');
    header.style.cssText = 'padding:4px 8px;font-size:11px;color:var(--text-dim);border-bottom:1px solid var(--border);';
    header.textContent = path || 'Script';
    el.appendChild(header);

    var editor = document.createElement('div');
    editor.className = 'script-editor';

    var lines = content.split('\n');
    for (var i = 0; i < lines.length; i++) {
      var lineDiv = document.createElement('div');
      lineDiv.className = 'script-line';

      var lineNum = document.createElement('span');
      lineNum.className = 'script-line-number';
      lineNum.textContent = String(i + 1);

      var lineContent = document.createElement('span');
      lineContent.className = 'script-line-content';
      lineContent.innerHTML = highlightGDScript(lines[i]);

      lineDiv.appendChild(lineNum);
      lineDiv.appendChild(lineContent);
      editor.appendChild(lineDiv);
    }
    el.appendChild(editor);
  }

  function clearScript() {
    currentScriptPath = null;
    document.getElementById('script-panel').innerHTML = '<div class="script-empty">Select a node with a script to view its content</div>';
  }


  // ---- Settings Dialog ----
  function setupSettingsDialog() {
    document.getElementById('btn-settings').addEventListener('click', function() {
      var d = document.getElementById('settings-dialog');
      d.classList.add('open');
      document.getElementById('set-grid-snap').checked = editorSettings.grid_snap_enabled;
      document.getElementById('set-snap-size').value = String(editorSettings.grid_snap_size);
      document.getElementById('set-grid-visible').checked = editorSettings.grid_visible;
      document.getElementById('set-rulers-visible').checked = editorSettings.rulers_visible;
      document.getElementById('set-font-size').value = editorSettings.font_size;
    });
    document.getElementById('settings-close').addEventListener('click', function() {
      document.getElementById('settings-dialog').classList.remove('open');
    });
    document.getElementById('settings-dialog').addEventListener('click', function(e) {
      if (e.target === this) this.classList.remove('open');
    });
    document.getElementById('set-grid-snap').addEventListener('change', function() {
      updateSetting('grid_snap_enabled', this.checked);
      document.getElementById('status-snap').textContent = this.checked ? editorSettings.grid_snap_size + 'px' : 'Off';
    });
    document.getElementById('set-snap-size').addEventListener('change', function() {
      updateSetting('grid_snap_size', parseInt(this.value));
      if (editorSettings.grid_snap_enabled) document.getElementById('status-snap').textContent = this.value + 'px';
    });
    document.getElementById('set-grid-visible').addEventListener('change', function() { updateSetting('grid_visible', this.checked); });
    document.getElementById('set-rulers-visible').addEventListener('change', function() { updateSetting('rulers_visible', this.checked); });
    document.getElementById('set-font-size').addEventListener('change', function() {
      updateSetting('font_size', this.value);
      var sizes = { small: '11px', medium: '13px', large: '15px' };
      document.body.style.fontSize = sizes[this.value] || '13px';
    });
  }


  // ---- Animation Panel ----
  var currentAnimName = null;
  var animPlaying = false;
  var animPlayInterval = null;

  function setupAnimationPanel() {
    var select = document.getElementById('anim-select');
    var newBtn = document.getElementById('anim-new-btn');
    var delBtn = document.getElementById('anim-delete-btn');
    var recordBtn = document.getElementById('anim-record-btn');
    var playBtn = document.getElementById('anim-play-btn');
    var stopBtn = document.getElementById('anim-stop-btn');
    var addTrackBtn = document.getElementById('anim-add-track-btn');

    select.addEventListener('change', function() {
      currentAnimName = this.value || null;
      refreshAnimationPanel();
    });

    newBtn.addEventListener('click', function() {
      var name = prompt('Animation name:', 'NewAnimation');
      if (!name) return;
      var length = parseFloat(prompt('Length (seconds):', '1.0')) || 1.0;
      api('POST', '/api/animation/create', { name: name, length: length }).then(function() {
        refreshAnimationList().then(function() {
          document.getElementById('anim-select').value = name;
          currentAnimName = name;
          refreshAnimationPanel();
        });
      });
    });

    delBtn.addEventListener('click', function() {
      if (!currentAnimName) return;
      if (!confirm('Delete animation "' + currentAnimName + '"?')) return;
      api('POST', '/api/animation/delete', { name: currentAnimName }).then(function() {
        currentAnimName = null;
        refreshAnimationList();
        refreshAnimationPanel();
      });
    });

    recordBtn.addEventListener('click', function() {
      var isActive = recordBtn.classList.toggle('active');
      document.body.classList.toggle('anim-recording', isActive);
      api('POST', '/api/animation/record', { enabled: isActive });
    });

    playBtn.addEventListener('click', function() {
      if (!currentAnimName) return;
      api('POST', '/api/animation/play', { name: currentAnimName }).then(function() {
        startAnimPlayback();
      });
    });

    stopBtn.addEventListener('click', function() {
      api('POST', '/api/animation/stop', {}).then(function() {
        stopAnimPlayback();
      });
    });

    addTrackBtn.addEventListener('click', function() {
      if (!currentAnimName) { alert('Select an animation first'); return; }
      var nodeName = prompt('Node name (e.g. Player):');
      if (!nodeName) return;
      var prop = prompt('Property (e.g. position):');
      if (!prop) return;
      api('POST', '/api/animation/keyframe/add', {
        animation: currentAnimName, track_node: nodeName, track_property: prop,
        time: 0.0, value: { type: 'Float', value: 0 }
      }).then(function() { refreshAnimationPanel(); });
    });

    // Timeline canvas click for scrubbing
    var canvas = document.getElementById('anim-timeline-canvas');
    canvas.addEventListener('mousedown', function(e) { scrubTimeline(e, canvas); });
    canvas.addEventListener('mousemove', function(e) {
      if (e.buttons === 1) scrubTimeline(e, canvas);
    });
    canvas.addEventListener('dblclick', function(e) { addKeyframeAtClick(e, canvas); });
  }

  function scrubTimeline(e, canvas) {
    if (!currentAnimName) return;
    var rect = canvas.getBoundingClientRect();
    var x = e.clientX - rect.left;
    api('GET', '/api/animation?name=' + encodeURIComponent(currentAnimName)).then(function(anim) {
      if (!anim) return;
      var time = (x / canvas.width) * anim.length;
      time = Math.max(0, Math.min(anim.length, time));
      api('POST', '/api/animation/seek', { time: time }).then(function() {
        refreshAnimationPanel();
        refreshViewport();
      });
    });
  }

  function addKeyframeAtClick(e, canvas) {
    if (!currentAnimName) return;
    var rect = canvas.getBoundingClientRect();
    var x = e.clientX - rect.left;
    var y = e.clientY - rect.top;
    api('GET', '/api/animation?name=' + encodeURIComponent(currentAnimName)).then(function(anim) {
      if (!anim || !anim.tracks.length) return;
      var time = (x / canvas.width) * anim.length;
      var trackHeight = 24;
      var trackIdx = Math.floor(y / trackHeight);
      if (trackIdx >= anim.tracks.length) return;
      var track = anim.tracks[trackIdx];
      var val = prompt('Value for ' + track.node_path + '.' + track.property + ' at t=' + time.toFixed(2) + ':');
      if (val === null) return;
      var numVal = parseFloat(val);
      api('POST', '/api/animation/keyframe/add', {
        animation: currentAnimName, track_node: track.node_path, track_property: track.property,
        time: time, value: { type: 'Float', value: isNaN(numVal) ? 0 : numVal }
      }).then(function() { refreshAnimationPanel(); });
    });
  }

  function startAnimPlayback() {
    animPlaying = true;
    if (animPlayInterval) clearInterval(animPlayInterval);
    animPlayInterval = setInterval(function() {
      api('GET', '/api/animation/status').then(function(status) {
        if (!status || !status.playing) { stopAnimPlayback(); return; }
        var newTime = status.current_time + (1.0 / 30.0);
        api('POST', '/api/animation/seek', { time: newTime }).then(function() {
          refreshAnimTimeline();
          refreshViewport();
        });
      });
    }, 1000 / 30);
  }

  function stopAnimPlayback() {
    animPlaying = false;
    if (animPlayInterval) { clearInterval(animPlayInterval); animPlayInterval = null; }
    refreshAnimTimeline();
  }

  async function refreshAnimationList() {
    var data = await api('GET', '/api/animations');
    if (!data) return;
    var select = document.getElementById('anim-select');
    var cur = select.value;
    select.innerHTML = '<option value="">-- No Animation --</option>';
    for (var i = 0; i < data.length; i++) {
      var opt = document.createElement('option');
      opt.value = data[i].name;
      opt.textContent = data[i].name + ' (' + data[i].length.toFixed(1) + 's)';
      select.appendChild(opt);
    }
    if (cur && data.some(function(a) { return a.name === cur; })) {
      select.value = cur;
    }
  }

  async function refreshAnimationPanel() {
    if (!currentAnimName) {
      document.getElementById('anim-tracks').innerHTML = '<div class="anim-empty">No animation selected</div>';
      drawTimeline(null, 0);
      document.getElementById('anim-time-display').textContent = '0.00 / 0.00';
      return;
    }
    var anim = await api('GET', '/api/animation?name=' + encodeURIComponent(currentAnimName));
    if (!anim) return;
    var status = await api('GET', '/api/animation/status');
    var curTime = (status && status.current_time) || 0;

    // Track list
    var tracksEl = document.getElementById('anim-tracks');
    if (anim.tracks.length === 0) {
      tracksEl.innerHTML = '<div class="anim-empty">No tracks. Click + Add Track.</div>';
    } else {
      var html = '';
      for (var i = 0; i < anim.tracks.length; i++) {
        html += '<div class="anim-track-row">' +
          '<span class="anim-track-node">' + escapeHtml(anim.tracks[i].node_path) + '</span>' +
          '<span class="anim-track-prop">.' + escapeHtml(anim.tracks[i].property) + '</span>' +
          '</div>';
      }
      tracksEl.innerHTML = html;
    }

    // Time display
    document.getElementById('anim-time-display').textContent =
      curTime.toFixed(2) + ' / ' + anim.length.toFixed(2);

    drawTimeline(anim, curTime);
  }

  function refreshAnimTimeline() {
    if (!currentAnimName) return;
    Promise.all([
      api('GET', '/api/animation?name=' + encodeURIComponent(currentAnimName)),
      api('GET', '/api/animation/status')
    ]).then(function(results) {
      var anim = results[0];
      var status = results[1];
      if (!anim) return;
      var curTime = (status && status.current_time) || 0;
      document.getElementById('anim-time-display').textContent =
        curTime.toFixed(2) + ' / ' + anim.length.toFixed(2);
      drawTimeline(anim, curTime);
    });
  }

  function drawTimeline(anim, currentTime) {
    var canvas = document.getElementById('anim-timeline-canvas');
    var ctx = canvas.getContext('2d');
    var w = canvas.width;
    var h = canvas.height;
    ctx.clearRect(0, 0, w, h);

    if (!anim) return;

    var length = anim.length || 1;
    var trackHeight = 24;

    // Background
    ctx.fillStyle = '#111';
    ctx.fillRect(0, 0, w, h);

    // Time ruler
    ctx.fillStyle = '#1a1a1a';
    ctx.fillRect(0, 0, w, 16);
    ctx.strokeStyle = '#333';
    ctx.fillStyle = '#666';
    ctx.font = '9px monospace';
    var step = 0.5;
    if (length > 5) step = 1.0;
    if (length > 20) step = 5.0;
    for (var t = 0; t <= length; t += step) {
      var x = (t / length) * w;
      ctx.beginPath();
      ctx.moveTo(x, 0);
      ctx.lineTo(x, 16);
      ctx.stroke();
      ctx.fillText(t.toFixed(1), x + 2, 12);
    }

    // Track rows
    for (var i = 0; i < anim.tracks.length; i++) {
      var y = 16 + i * trackHeight;
      ctx.strokeStyle = '#2a2a2a';
      ctx.beginPath();
      ctx.moveTo(0, y + trackHeight);
      ctx.lineTo(w, y + trackHeight);
      ctx.stroke();

      // Keyframe diamonds
      var kfs = anim.tracks[i].keyframes;
      for (var j = 0; j < kfs.length; j++) {
        var kx = (kfs[j].time / length) * w;
        var ky = y + trackHeight / 2;
        ctx.fillStyle = '#d4a574';
        ctx.beginPath();
        ctx.moveTo(kx, ky - 5);
        ctx.lineTo(kx + 5, ky);
        ctx.lineTo(kx, ky + 5);
        ctx.lineTo(kx - 5, ky);
        ctx.closePath();
        ctx.fill();
      }
    }

    // Playhead
    var phX = (currentTime / length) * w;
    ctx.strokeStyle = '#d4a574';
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(phX, 0);
    ctx.lineTo(phX, h);
    ctx.stroke();
    ctx.lineWidth = 1;

    // Update playhead div
    var playhead = document.getElementById('anim-playhead');
    playhead.style.left = phX + 'px';
  }

  // ---- Init ----
  setupViewport();
  setupToolbar();
  setupToolMode();
  setupContextMenu();
  setupSearch();
  setupKeyboardShortcuts();
  setupBottomPanel();
  setupRightPanelTabs();
  setupConnectDialog();
  setupAddNodeDialog();
  setupPlayButtons();
  setupAnimationPanel();
  refreshAnimationList();
  setupSettingsDialog();
  setupLeftDivider();
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
</body>
</html>
"##;
