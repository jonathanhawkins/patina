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
  width: 240px; min-width: 160px; background: var(--panel);
  border-right: 1px solid var(--border); display: flex; flex-direction: column; flex-shrink: 0;
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

/* Status bar */
#statusbar {
  display: flex; align-items: center; gap: 16px; padding: 4px 10px;
  background: var(--panel); border-top: 1px solid var(--border); font-size: 11px;
  color: var(--text-dim); flex-shrink: 0;
}
#statusbar .accent { color: var(--accent); }

/* Scrollbar styling */
::-webkit-scrollbar { width: 6px; height: 6px; }
::-webkit-scrollbar-track { background: var(--bg); }
::-webkit-scrollbar-thumb { background: var(--border); border-radius: 3px; }
::-webkit-scrollbar-thumb:hover { background: #444; }
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
  <div class="add-menu">
    <button id="btn-add" title="Add Node">+ Add Node &#9662;</button>
    <div class="add-menu-dropdown" id="add-dropdown">
      <div data-class="Node">Node</div>
      <div data-class="Node2D">Node2D</div>
      <div data-class="Node3D">Node3D</div>
      <div data-class="Sprite2D">Sprite2D</div>
      <div data-class="Camera2D">Camera2D</div>
      <div data-class="Control">Control</div>
      <div data-class="Label">Label</div>
      <div data-class="Button">Button</div>
    </div>
  </div>
  <button id="btn-delete" title="Delete Node (Del)">&#10005; Delete</button>
  <div class="sep"></div>
  <button id="btn-undo" title="Undo (Ctrl+Z)">&#8630; Undo</button>
  <button id="btn-redo" title="Redo (Ctrl+Y)">&#8631; Redo</button>
  <div class="sep"></div>
  <button id="btn-save" title="Save Scene (Ctrl+S)">&#128190; Save</button>
  <button id="btn-load" title="Load Scene">&#128194; Load</button>
  <span id="scene-file-indicator"></span>
</div>

<!-- Context menu -->
<div id="context-menu">
  <div class="ctx-item" data-action="rename">Rename<span class="ctx-shortcut">F2</span></div>
  <div class="ctx-item" data-action="duplicate">Duplicate<span class="ctx-shortcut">Ctrl+D</span></div>
  <div class="ctx-item" data-action="delete">Delete<span class="ctx-shortcut">Del</span></div>
  <div class="ctx-separator"></div>
  <div class="ctx-item" data-action="add-child">Add Child Node</div>
  <div class="ctx-separator"></div>
  <div class="ctx-item" data-action="move-up">Move Up</div>
  <div class="ctx-item" data-action="move-down">Move Down</div>
</div>

<!-- Main area -->
<div id="main">
  <!-- Scene tree -->
  <div id="scene-panel">
    <div class="panel-header">Scene Tree</div>
    <input type="text" id="scene-search" placeholder="Filter nodes..." autocomplete="off">
    <div id="scene-tree"></div>
  </div>

  <!-- Center: viewport + bottom panel -->
  <div id="center-area">
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
        <button id="bottom-toggle" title="Toggle panel">&#9650;</button>
      </div>
      <div id="bottom-panel-content">
        <div class="bottom-content-tab active" data-tab="output">
          <div id="output-log"></div>
        </div>
        <div class="bottom-content-tab" data-tab="scene-info">
          <div id="scene-info"></div>
        </div>
      </div>
    </div>
  </div>

  <!-- Inspector -->
  <div id="inspector-panel">
    <div class="panel-header">Inspector</div>
    <div id="inspector">
      <div class="insp-empty">Select a node to inspect</div>
    </div>
  </div>
</div>

<!-- Status bar -->
<div id="statusbar">
  <span>Selected: <span class="accent" id="status-selected">None</span></span>
  <span>Path: <span id="status-path">&mdash;</span></span>
  <span>Nodes: <span id="status-nodes">0</span></span>
</div>

<script>
(function() {
  'use strict';

  // State
  var selectedNodeId = null;
  var selectedNodeData = null;
  var sceneData = null;
  var expandedNodes = new Set();
  var searchFilter = '';
  var contextNodeId = null;
  var currentToolMode = 'select';
  var collapsedSections = {};
  var lastLogCount = 0;

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
    if (name.startsWith('script_') || name.startsWith('metadata/')) return 'Script';
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
    row.className = 'tree-row' + (node.id === selectedNodeId ? ' selected' : '');
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

    row.addEventListener('click', (function(nid) { return function() { selectNode(nid); }; })(node.id));

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
    document.getElementById('add-dropdown').classList.toggle('open');
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
    if (selectedNodeId === null) { renderInspectorEmpty(); return; }
    var data = await api('GET', '/api/selected');
    if (data) {
      selectedNodeData = data;
      renderInspector(data);
      document.getElementById('status-selected').textContent = data.name || 'None';
      document.getElementById('status-path').textContent = data.path || '\u2014';
    } else {
      renderInspectorEmpty();
    }
  }

  // ---- Inspector ----
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
    } else if (type === 'NodePath' || prop.name === 'texture' || prop.name.indexOf('path') >= 0) {
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
      api('POST', '/api/viewport/drag_start', c);
    });
    document.addEventListener('mousemove', function(e) {
      if (dragStartX === 0 && dragStartY === 0) return;
      if (!viewportImg) return;
      var dx = e.clientX - dragStartX;
      var dy = e.clientY - dragStartY;
      if (!isDragging && (Math.abs(dx) > DRAG_THRESHOLD || Math.abs(dy) > DRAG_THRESHOLD)) isDragging = true;
      if (isDragging) api('POST', '/api/viewport/drag', viewportCoords(e));
    });
    document.addEventListener('mouseup', function(e) {
      if (dragStartX === 0 && dragStartY === 0) return;
      if (!viewportImg) return;
      var c = viewportCoords(e);
      if (isDragging) {
        api('POST', '/api/viewport/drag_end', c).then(function() {
          fetchScene(); if (selectedNodeId) fetchSelected();
        });
      } else {
        api('POST', '/api/viewport/click', c).then(function(result) {
          if (result && result.selected) selectedNodeId = result.selected;
          else selectedNodeId = null;
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
  }

  // ---- Toolbar actions ----
  function setupToolbar() {
    var btnAdd = document.getElementById('btn-add');
    var dropdown = document.getElementById('add-dropdown');
    btnAdd.addEventListener('click', function(e) { e.stopPropagation(); dropdown.classList.toggle('open'); });
    document.addEventListener('click', function() { dropdown.classList.remove('open'); });

    dropdown.querySelectorAll('[data-class]').forEach(function(item) {
      item.addEventListener('click', async function(e) {
        e.stopPropagation(); dropdown.classList.remove('open');
        var className = item.getAttribute('data-class');
        var name = prompt('Node name:', className);
        if (!name) return;
        var parentId = selectedNodeId || (sceneData && sceneData.nodes ? sceneData.nodes.id : null);
        if (parentId === null) return;
        await api('POST', '/api/node/add', { parent_id: parentId, name: name, class_name: className });
        if (selectedNodeId) expandedNodes.add(selectedNodeId);
        await fetchScene();
      });
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

      if (e.key === 'Delete' && selectedNodeId !== null) { e.preventDefault(); doDelete(selectedNodeId); return; }
      if (e.key === 'F2' && selectedNodeId !== null) { e.preventDefault(); doRename(selectedNodeId); return; }
      if (e.ctrlKey && e.key === 'd' && selectedNodeId !== null) { e.preventDefault(); doDuplicate(selectedNodeId); return; }
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
    });
  }

  // ---- Polling ----
  function startPolling() {
    setInterval(fetchScene, 500);
    setInterval(refreshViewport, 200);
    setInterval(fetchLogs, 1000);
    setInterval(fetchSceneInfo, 2000);
  }

  // ---- Init ----
  setupViewport();
  setupToolbar();
  setupToolMode();
  setupContextMenu();
  setupSearch();
  setupKeyboardShortcuts();
  setupBottomPanel();
  fetchScene();
  fetchSelected();
  refreshViewport();
  fetchLogs();
  fetchSceneInfo();
  startPolling();
})();
</script>
</body>
</html>
"##;
