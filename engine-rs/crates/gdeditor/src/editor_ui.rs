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

/* Main layout */
#main { display: flex; flex: 1; overflow: hidden; }

/* Scene tree panel */
#scene-panel {
  width: 220px; min-width: 160px; background: var(--panel);
  border-right: 1px solid var(--border); display: flex; flex-direction: column; flex-shrink: 0;
}
#scene-panel .panel-header {
  padding: 6px 10px; font-weight: bold; font-size: 11px; text-transform: uppercase;
  color: var(--text-dim); border-bottom: 1px solid var(--border); letter-spacing: 0.5px;
}
#scene-tree { flex: 1; overflow: auto; padding: 4px 0; }
.tree-node { user-select: none; }
.tree-row {
  display: flex; align-items: center; padding: 2px 8px; cursor: pointer;
  white-space: nowrap; gap: 4px;
}
.tree-row:hover { background: var(--hover); }
.tree-row.selected { background: var(--selected); color: var(--accent); }
.tree-toggle { width: 14px; text-align: center; font-size: 10px; color: var(--text-dim); flex-shrink: 0; }
.tree-icon { font-size: 11px; color: var(--text-dim); flex-shrink: 0; width: 14px; text-align: center; }
.tree-name { flex: 1; overflow: hidden; text-overflow: ellipsis; }
.tree-children { display: none; }
.tree-children.expanded { display: block; }

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
  width: 280px; min-width: 200px; background: var(--panel);
  border-left: 1px solid var(--border); display: flex; flex-direction: column; flex-shrink: 0;
}
#inspector-panel .panel-header {
  padding: 6px 10px; font-weight: bold; font-size: 11px; text-transform: uppercase;
  color: var(--text-dim); border-bottom: 1px solid var(--border); letter-spacing: 0.5px;
}
#inspector { flex: 1; overflow: auto; padding: 8px; }
.insp-section { margin-bottom: 12px; }
.insp-section-header {
  font-weight: bold; font-size: 11px; text-transform: uppercase; color: var(--text-dim);
  margin-bottom: 6px; padding-bottom: 4px; border-bottom: 1px solid var(--border); letter-spacing: 0.5px;
}
.insp-row { display: flex; align-items: center; margin-bottom: 4px; gap: 6px; }
.insp-label { width: 80px; font-size: 12px; color: var(--text-dim); flex-shrink: 0; overflow: hidden; text-overflow: ellipsis; }
.insp-value { flex: 1; display: flex; gap: 4px; align-items: center; }
.insp-value input[type="text"], .insp-value input[type="number"] { width: 100%; }
.insp-value .vec-label { font-size: 11px; color: var(--text-dim); min-width: 10px; }
.insp-value .vec-input { flex: 1; min-width: 40px; }
.insp-readonly { color: var(--text-dim); font-style: italic; font-size: 12px; }
.insp-empty { color: var(--text-dim); font-style: italic; padding: 20px 0; text-align: center; }

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
  <button id="btn-delete" title="Delete Node">&#10005; Delete</button>
  <div class="sep"></div>
  <button id="btn-undo" title="Undo">&#8630; Undo</button>
  <button id="btn-redo" title="Redo">&#8631; Redo</button>
  <div class="sep"></div>
  <button id="btn-save" title="Save Scene">&#128190; Save</button>
  <button id="btn-load" title="Load Scene">&#128194; Load</button>
</div>

<!-- Main area -->
<div id="main">
  <!-- Scene tree -->
  <div id="scene-panel">
    <div class="panel-header">Scene Tree</div>
    <div id="scene-tree"></div>
  </div>

  <!-- Viewport -->
  <div id="viewport-panel">
    <div class="panel-header">Viewport</div>
    <div id="viewport-container">
      <div id="viewport-placeholder">No frame available</div>
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
  <span>Path: <span id="status-path">—</span></span>
  <span>Nodes: <span id="status-nodes">0</span></span>
</div>

<script>
(function() {
  'use strict';

  // State
  let selectedNodeId = null;
  let selectedNodeData = null;
  let sceneData = null;
  let expandedNodes = new Set();

  // ---- API helpers ----
  async function api(method, path, body) {
    const opts = { method };
    if (body !== undefined) {
      opts.headers = { 'Content-Type': 'application/json' };
      opts.body = JSON.stringify(body);
    }
    try {
      const resp = await fetch(path, opts);
      const text = await resp.text();
      if (!text || text === 'null') return null;
      return JSON.parse(text);
    } catch (e) {
      return null;
    }
  }

  // ---- Scene tree ----
  function countNodes(node) {
    if (!node || !node.children) return 1;
    return 1 + node.children.reduce((s, c) => s + countNodes(c), 0);
  }

  function classIcon(cls) {
    if (!cls) return '●';
    const c = cls.toLowerCase();
    if (c.includes('2d') || c === 'sprite2d' || c === 'camera2d') return '◆';
    if (c === 'control' || c === 'label' || c === 'button') return '□';
    return '●';
  }

  function renderTree(node, depth, container) {
    if (!node) return;
    const div = document.createElement('div');
    div.className = 'tree-node';
    div.style.paddingLeft = (depth * 14) + 'px';

    const hasChildren = node.children && node.children.length > 0;
    const isExpanded = expandedNodes.has(node.id);

    const row = document.createElement('div');
    row.className = 'tree-row' + (node.id === selectedNodeId ? ' selected' : '');

    const toggle = document.createElement('span');
    toggle.className = 'tree-toggle';
    toggle.textContent = hasChildren ? (isExpanded ? '▼' : '▶') : '';
    if (hasChildren) {
      toggle.addEventListener('click', function(e) {
        e.stopPropagation();
        if (expandedNodes.has(node.id)) expandedNodes.delete(node.id);
        else expandedNodes.add(node.id);
        refreshTree();
      });
    }

    const icon = document.createElement('span');
    icon.className = 'tree-icon';
    icon.textContent = classIcon(node.class);

    const name = document.createElement('span');
    name.className = 'tree-name';
    name.textContent = node.name;

    row.appendChild(toggle);
    row.appendChild(icon);
    row.appendChild(name);

    row.addEventListener('click', function() {
      selectNode(node.id);
    });

    div.appendChild(row);
    container.appendChild(div);

    if (hasChildren && isExpanded) {
      const childContainer = document.createElement('div');
      childContainer.className = 'tree-children expanded';
      for (const child of node.children) {
        renderTree(child, depth + 1, childContainer);
      }
      container.appendChild(childContainer);
    }
  }

  function refreshTree() {
    const el = document.getElementById('scene-tree');
    el.innerHTML = '';
    if (sceneData && sceneData.nodes) {
      renderTree(sceneData.nodes, 0, el);
      document.getElementById('status-nodes').textContent = countNodes(sceneData.nodes);
    }
  }

  async function fetchScene() {
    const data = await api('GET', '/api/scene');
    if (data) {
      sceneData = data;
      // Auto-expand root on first load
      if (expandedNodes.size === 0 && data.nodes) {
        expandedNodes.add(data.nodes.id);
      }
      refreshTree();
    }
  }

  // ---- Selection ----
  async function selectNode(id) {
    selectedNodeId = id;
    await api('POST', '/api/node/select', { node_id: id });
    refreshTree();
    await fetchSelected();
  }

  async function fetchSelected() {
    if (selectedNodeId === null) {
      renderInspectorEmpty();
      return;
    }
    const data = await api('GET', '/api/selected');
    if (data) {
      selectedNodeData = data;
      renderInspector(data);
      document.getElementById('status-selected').textContent = data.name || 'None';
      document.getElementById('status-path').textContent = data.path || '—';
    } else {
      renderInspectorEmpty();
    }
  }

  // ---- Inspector ----
  function renderInspectorEmpty() {
    document.getElementById('inspector').innerHTML = '<div class="insp-empty">Select a node to inspect</div>';
    document.getElementById('status-selected').textContent = 'None';
    document.getElementById('status-path').textContent = '—';
  }

  function renderInspector(data) {
    const el = document.getElementById('inspector');
    el.innerHTML = '';

    // Node info section
    const infoSection = document.createElement('div');
    infoSection.className = 'insp-section';
    const infoHeader = document.createElement('div');
    infoHeader.className = 'insp-section-header';
    infoHeader.textContent = 'Node';
    infoSection.appendChild(infoHeader);

    // Name (editable)
    const nameRow = document.createElement('div');
    nameRow.className = 'insp-row';
    nameRow.innerHTML = '<div class="insp-label">Name</div>';
    const nameVal = document.createElement('div');
    nameVal.className = 'insp-value';
    const nameInput = document.createElement('input');
    nameInput.type = 'text';
    nameInput.value = data.name || '';
    nameInput.readOnly = true;  // Name editing not yet supported by API
    nameVal.appendChild(nameInput);
    nameRow.appendChild(nameVal);
    infoSection.appendChild(nameRow);

    // Class (readonly)
    const classRow = document.createElement('div');
    classRow.className = 'insp-row';
    classRow.innerHTML = '<div class="insp-label">Class</div><div class="insp-value"><span class="insp-readonly">' +
      (data.class || 'Unknown') + '</span></div>';
    infoSection.appendChild(classRow);

    el.appendChild(infoSection);

    // Properties section
    if (data.properties && data.properties.length > 0) {
      const propSection = document.createElement('div');
      propSection.className = 'insp-section';
      const propHeader = document.createElement('div');
      propHeader.className = 'insp-section-header';
      propHeader.textContent = 'Properties';
      propSection.appendChild(propHeader);

      for (const prop of data.properties) {
        if (prop.type === 'Nil') continue;
        const row = createPropertyRow(data.id, prop);
        propSection.appendChild(row);
      }
      el.appendChild(propSection);
    }
  }

  function createPropertyRow(nodeId, prop) {
    const row = document.createElement('div');
    row.className = 'insp-row';
    const label = document.createElement('div');
    label.className = 'insp-label';
    label.textContent = prop.name;
    label.title = prop.name;
    row.appendChild(label);

    const val = document.createElement('div');
    val.className = 'insp-value';

    const type = prop.type;
    const v = prop.value && prop.value.value;

    if (type === 'String') {
      const input = document.createElement('input');
      input.type = 'text';
      input.value = v != null ? String(v) : '';
      input.addEventListener('change', function() {
        setProperty(nodeId, prop.name, { type: 'String', value: input.value });
      });
      val.appendChild(input);
    } else if (type === 'Int') {
      const input = document.createElement('input');
      input.type = 'number';
      input.step = '1';
      input.value = v != null ? v : 0;
      input.addEventListener('change', function() {
        setProperty(nodeId, prop.name, { type: 'Int', value: parseInt(input.value) || 0 });
      });
      val.appendChild(input);
    } else if (type === 'Float') {
      const input = document.createElement('input');
      input.type = 'number';
      input.step = '0.1';
      input.value = v != null ? v : 0;
      input.addEventListener('change', function() {
        setProperty(nodeId, prop.name, { type: 'Float', value: parseFloat(input.value) || 0 });
      });
      val.appendChild(input);
    } else if (type === 'Bool') {
      const input = document.createElement('input');
      input.type = 'checkbox';
      input.checked = !!v;
      input.addEventListener('change', function() {
        setProperty(nodeId, prop.name, { type: 'Bool', value: input.checked });
      });
      val.appendChild(input);
    } else if (type === 'Vector2') {
      const arr = Array.isArray(v) ? v : [0, 0];
      const xl = document.createElement('span');
      xl.className = 'vec-label';
      xl.textContent = 'x';
      const xi = document.createElement('input');
      xi.type = 'number'; xi.step = '0.1'; xi.className = 'vec-input';
      xi.value = arr[0] != null ? arr[0] : 0;
      const yl = document.createElement('span');
      yl.className = 'vec-label';
      yl.textContent = 'y';
      const yi = document.createElement('input');
      yi.type = 'number'; yi.step = '0.1'; yi.className = 'vec-input';
      yi.value = arr[1] != null ? arr[1] : 0;
      function sendVec2() {
        setProperty(nodeId, prop.name, { type: 'Vector2', value: [parseFloat(xi.value)||0, parseFloat(yi.value)||0] });
      }
      xi.addEventListener('change', sendVec2);
      yi.addEventListener('change', sendVec2);
      val.appendChild(xl); val.appendChild(xi);
      val.appendChild(yl); val.appendChild(yi);
    } else if (type === 'Vector3') {
      const arr = Array.isArray(v) ? v : [0, 0, 0];
      ['x','y','z'].forEach(function(axis, i) {
        const al = document.createElement('span');
        al.className = 'vec-label';
        al.textContent = axis;
        const ai = document.createElement('input');
        ai.type = 'number'; ai.step = '0.1'; ai.className = 'vec-input';
        ai.value = arr[i] != null ? arr[i] : 0;
        ai.addEventListener('change', function() {
          const vals = [];
          val.querySelectorAll('.vec-input').forEach(function(inp) { vals.push(parseFloat(inp.value)||0); });
          setProperty(nodeId, prop.name, { type: 'Vector3', value: vals });
        });
        val.appendChild(al); val.appendChild(ai);
      });
    } else if (type === 'Color') {
      const input = document.createElement('input');
      input.type = 'color';
      if (Array.isArray(v) && v.length >= 3) {
        const r = Math.round((v[0]||0)*255), g = Math.round((v[1]||0)*255), b = Math.round((v[2]||0)*255);
        input.value = '#' + [r,g,b].map(function(c){return c.toString(16).padStart(2,'0');}).join('');
      }
      input.addEventListener('change', function() {
        const hex = input.value;
        const r = parseInt(hex.slice(1,3),16)/255;
        const g = parseInt(hex.slice(3,5),16)/255;
        const b = parseInt(hex.slice(5,7),16)/255;
        setProperty(nodeId, prop.name, { type: 'Color', value: [r,g,b,1.0] });
      });
      val.appendChild(input);
    } else {
      const span = document.createElement('span');
      span.className = 'insp-readonly';
      span.textContent = type + ': ' + JSON.stringify(v);
      val.appendChild(span);
    }

    row.appendChild(val);
    return row;
  }

  async function setProperty(nodeId, property, value) {
    await api('POST', '/api/property/set', { node_id: nodeId, property: property, value: value });
  }

  // ---- Viewport ----
  let viewportImg = null;
  let viewportTimer = null;

  let isDragging = false;
  let dragStartX = 0;
  let dragStartY = 0;
  const DRAG_THRESHOLD = 3;

  function viewportCoords(e) {
    const rect = viewportImg.getBoundingClientRect();
    const scaleX = viewportImg.naturalWidth / rect.width;
    const scaleY = viewportImg.naturalHeight / rect.height;
    return { x: Math.round((e.clientX - rect.left) * scaleX), y: Math.round((e.clientY - rect.top) * scaleY) };
  }

  function setupViewport() {
    const container = document.getElementById('viewport-container');
    viewportImg = document.createElement('img');
    viewportImg.id = 'viewport-img';
    viewportImg.style.display = 'none';
    viewportImg.draggable = false;

    viewportImg.addEventListener('mousedown', function(e) {
      e.preventDefault();
      const c = viewportCoords(e);
      dragStartX = e.clientX;
      dragStartY = e.clientY;
      isDragging = false;
      api('POST', '/api/viewport/drag_start', c);
    });

    document.addEventListener('mousemove', function(e) {
      if (dragStartX === 0 && dragStartY === 0) return;
      if (!viewportImg) return;
      const dx = e.clientX - dragStartX;
      const dy = e.clientY - dragStartY;
      if (!isDragging && (Math.abs(dx) > DRAG_THRESHOLD || Math.abs(dy) > DRAG_THRESHOLD)) {
        isDragging = true;
      }
      if (isDragging) {
        const c = viewportCoords(e);
        api('POST', '/api/viewport/drag', c);
      }
    });

    document.addEventListener('mouseup', function(e) {
      if (dragStartX === 0 && dragStartY === 0) return;
      if (!viewportImg) return;
      const c = viewportCoords(e);
      if (isDragging) {
        api('POST', '/api/viewport/drag_end', c).then(function() {
          fetchScene();
          if (selectedNodeId) fetchSelected();
        });
      } else {
        api('POST', '/api/viewport/click', c).then(function(result) {
          if (result && result.selected) {
            selectedNodeId = result.selected;
          } else {
            selectedNodeId = null;
          }
          refreshTree();
          fetchSelected();
          fetchScene();
        });
      }
      isDragging = false;
      dragStartX = 0;
      dragStartY = 0;
    });

    container.appendChild(viewportImg);
  }

  function refreshViewport() {
    if (!viewportImg) return;
    const img = new Image();
    img.onload = function() {
      viewportImg.src = img.src;
      viewportImg.style.display = 'block';
      const placeholder = document.getElementById('viewport-placeholder');
      if (placeholder) placeholder.style.display = 'none';
    };
    img.onerror = function() {
      viewportImg.style.display = 'none';
      const placeholder = document.getElementById('viewport-placeholder');
      if (placeholder) placeholder.style.display = 'block';
    };
    img.src = '/api/viewport?t=' + Date.now();
  }

  // ---- Toolbar actions ----
  function setupToolbar() {
    // Add node dropdown
    const btnAdd = document.getElementById('btn-add');
    const dropdown = document.getElementById('add-dropdown');
    btnAdd.addEventListener('click', function(e) {
      e.stopPropagation();
      dropdown.classList.toggle('open');
    });
    document.addEventListener('click', function() { dropdown.classList.remove('open'); });

    dropdown.querySelectorAll('[data-class]').forEach(function(item) {
      item.addEventListener('click', async function(e) {
        e.stopPropagation();
        dropdown.classList.remove('open');
        const className = item.getAttribute('data-class');
        const name = prompt('Node name:', className);
        if (!name) return;
        const parentId = selectedNodeId || (sceneData && sceneData.nodes ? sceneData.nodes.id : null);
        if (parentId === null) return;
        await api('POST', '/api/node/add', { parent_id: parentId, name: name, class_name: className });
        if (selectedNodeId) expandedNodes.add(selectedNodeId);
        await fetchScene();
      });
    });

    // Delete
    document.getElementById('btn-delete').addEventListener('click', async function() {
      if (selectedNodeId === null) return;
      if (!confirm('Delete selected node?')) return;
      await api('POST', '/api/node/delete', { node_id: selectedNodeId });
      selectedNodeId = null;
      selectedNodeData = null;
      renderInspectorEmpty();
      await fetchScene();
    });

    // Undo / Redo
    document.getElementById('btn-undo').addEventListener('click', async function() {
      await api('POST', '/api/undo');
      await fetchScene();
      if (selectedNodeId) await fetchSelected();
    });
    document.getElementById('btn-redo').addEventListener('click', async function() {
      await api('POST', '/api/redo');
      await fetchScene();
      if (selectedNodeId) await fetchSelected();
    });

    // Save
    document.getElementById('btn-save').addEventListener('click', async function() {
      const path = prompt('Save path:', 'scene.tscn');
      if (!path) return;
      const result = await api('POST', '/api/scene/save', { path: path });
      if (result && result.ok) {
        // Briefly flash the button to indicate success
        const btn = document.getElementById('btn-save');
        btn.style.borderColor = 'var(--accent)';
        setTimeout(function(){ btn.style.borderColor = ''; }, 500);
      }
    });

    // Load
    document.getElementById('btn-load').addEventListener('click', async function() {
      const path = prompt('Load path:');
      if (!path) return;
      await api('POST', '/api/scene/load', { path: path });
      selectedNodeId = null;
      selectedNodeData = null;
      expandedNodes.clear();
      renderInspectorEmpty();
      await fetchScene();
    });
  }

  // ---- Polling loops ----
  function startPolling() {
    // Scene tree: every 500ms
    setInterval(fetchScene, 500);
    // Viewport: every 200ms
    setInterval(refreshViewport, 200);
  }

  // ---- Init ----
  setupViewport();
  setupToolbar();
  fetchScene();
  fetchSelected();
  refreshViewport();
  startPolling();
})();
</script>
</body>
</html>
"##;
