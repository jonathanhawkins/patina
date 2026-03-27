# Editor Architecture Plan

## Current State (Phase 8 Baseline)

The `gdeditor` crate provides a browser-based editor shell (~18,500 LOC) with:

### Modules

| Module | Purpose | LOC |
|--------|---------|-----|
| `editor_server` | HTTP REST API (TcpListener), scene/node/viewport endpoints | ~5k |
| `editor_ui` | HTML/CSS/JS generation for the browser frontend | ~3k |
| `scene_editor` | Scene editing operations (add/delete/move nodes) | ~2k |
| `scene_renderer` | Viewport frame rendering for the editor preview | ~1.5k |
| `inspector` | Property inspection panel with change callbacks | ~1.5k |
| `dock` | Scene tree dock, property dock panels | ~1k |
| `filesystem` | Project file system browsing and resource discovery | ~1k |
| `import` | Resource import pipeline (.tscn, .tres importers) | ~1k |
| `settings` | Editor settings, themes, project settings | ~800 |
| `texture_cache` | Texture loading and caching for the viewport | ~700 |
| `lib` | Central Editor state, EditorCommand, EditorPlugin trait | ~500 |

### Architecture Layers

```
Browser (HTML/CSS/JS)
    |
    v  HTTP REST
EditorServer (TcpListener)
    |
    v  Editor API
Editor { SceneTree, UndoRedo, Selection }
    |
    v  Engine API
gdscene::SceneTree + gdrender2d::SoftwareRenderer
```

### Current Capabilities

1. **Scene tree panel** - displays node hierarchy, supports selection
2. **Viewport** - renders scene via SoftwareRenderer, serves as BMP/PNG
3. **Inspector** - shows properties for selected node, supports editing
4. **Node operations** - add, delete, reparent, rename nodes
5. **Undo/redo** - command-based undo stack for all operations
6. **Save/Load** - .tscn round-trip via PackedScene + TscnSaver
7. **Animation** - basic animation playback in editor preview
8. **Plugin trait** - EditorPlugin for extensibility

### REST API Surface

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/editor` | GET | Main editor HTML page |
| `/api/scene` | GET | Scene tree as JSON |
| `/api/node/:id` | GET | Node details |
| `/api/node` | POST | Add node |
| `/api/node/:id` | DELETE | Delete node |
| `/api/node/:id/property` | PUT | Set property |
| `/api/viewport` | GET | Rendered frame (BMP/PNG) |
| `/api/save` | POST | Save scene to .tscn |
| `/api/load` | POST | Load scene from .tscn |
| `/api/undo` | POST | Undo last command |
| `/api/redo` | POST | Redo last undone command |

## Feature Gate

**No new editor features until runtime parity exits are green.**

The editor is maintenance-only:
- Bug fixes allowed
- Server stability improvements allowed
- Smoke test maintenance allowed
- New features blocked until oracle parity >= 98%

## Phase 8 Architecture Goals

When the feature gate lifts, the following architectural improvements are planned:

### 1. WebSocket Live Updates
Replace polling-based viewport refresh with WebSocket push for:
- Real-time viewport updates on scene changes
- Live property inspector updates on selection change
- Animation preview at target framerate

### 2. Multi-Document Support
Currently the editor manages a single scene. Multi-document requires:
- Tab-based scene switching
- Independent undo stacks per document
- Cross-scene resource references

### 3. 3D Viewport
Extend the viewport to support 3D scenes using the `gdrender3d` pipeline:
- Camera orbit controls (orbit, pan, zoom)
- Gizmo overlays (translate, rotate, scale handles)
- Grid and axis visualization

### 4. Asset Browser
Extend `EditorFileSystem` into a full asset browser:
- Thumbnail generation for resources
- Drag-and-drop from browser to scene tree
- Import settings per resource type

### 5. Script Editor Integration
Connect to `gdscript-interop` for in-editor scripting:
- Syntax-highlighted script display
- Variable/signal inspection at runtime
- Breakpoint-like frame stepping

## Dependency Graph

```
gdeditor
  +-- gdscene (scene tree, packed scenes, lifecycle)
  +-- gdrender2d (software renderer for viewport)
  +-- gdvariant (property serialization)
  +-- gdcore (math types, errors)
  +-- gdresource (resource loading for import pipeline)
```

## Testing Strategy

| Layer | Test Type | Location |
|-------|-----------|----------|
| Unit | Module-level `#[cfg(test)]` | `gdeditor/src/*.rs` |
| Smoke | HTTP round-trip tests | `tests/editor_smoke_test.rs` |
| Integration | Full editor workflow | `tests/editor_461_revalidation_test.rs` |

All tests must pass headless (no browser required).
