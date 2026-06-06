# Editor Architecture Plan

## Current State (Phase 8)

The `gdeditor` crate provides a browser-based editor shell with 40 modules.
The editor feature gate was **lifted on 2026-03-19** after runtime parity
exits passed (100% oracle parity, 71/71 scenes). Editor feature work is
now the primary focus.

### Module Inventory

| Module | Purpose |
|--------|---------|
| `lib` | Central Editor state, EditorCommand, EditorPlugin trait |
| `editor_server` | HTTP REST API (TcpListener), scene/node/viewport endpoints |
| `editor_ui` | HTML/CSS/JS generation for the browser frontend |
| `scene_editor` | Scene editing operations (add/delete/move/reparent nodes) |
| `scene_renderer` | Viewport frame rendering for the editor preview |
| `inspector` | Property inspection panel with change callbacks, plugin registry |
| `dock` | Scene tree dock, property dock, plugin dock panels |
| `filesystem` | Project file system browsing and resource discovery |
| `import` | Resource import pipeline (tscn, tres, fbx, gltf, obj importers) |
| `import_settings` | Per-resource import configuration UI |
| `settings` | Editor settings, themes, project settings |
| `editor_settings_dialog` | Settings dialog with tabs, key bindings, plugin info |
| `project_settings_dialog` | Project settings editing dialog |
| `texture_cache` | Texture loading and caching for the viewport |
| `animation_editor` | Timeline, keyframe editing, playback, onion skinning |
| `curve_editor` | Bezier curve editing for animation easing |
| `script_editor` | GDScript editing with syntax highlighting |
| `script_completion` | Autocompletion for GDScript |
| `script_gutter` | Line numbers, breakpoints, bookmarks in script editor |
| `shader_editor` | Shader code editing |
| `find_replace` | Find and replace across editor panels |
| `command_palette` | Ctrl+P command palette for quick actions |
| `editor_menu` | Main menu bar (File, Edit, Scene, Project, Debug, Help) |
| `editor_plugin` | EditorPlugin trait and plugin lifecycle |
| `create_dialog` | Node creation dialog (class browser) |
| `signal_dialog` | Signal connection editing dialog |
| `group_dialog` | Node group management dialog |
| `export_dialog` | Export presets, platform selection, build profiles |
| `output_panel` | Output/log panel for editor messages |
| `profiler_panel` | Performance profiler visualization |
| `viewport_2d` | 2D viewport with pan, zoom, grid |
| `viewport_3d` | 3D viewport with camera orbit, gizmos, grid, environment preview |
| `environment_preview` | 3D environment preview rendering |
| `theme_editor` | Theme editing and live preview |
| `tilemap_editor` | TileMap painting and tileset editing |
| `asset_drag_drop` | Drag-and-drop from asset browser to scene tree |
| `undo_redo` | Command-based undo/redo stack |
| `vcs` | Version control integration (git status, diff) |
| `recent_items` | Recently opened scenes/files tracking |
| `project_manager` | Project listing and creation |

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
gdscene::SceneTree + gdrender2d + gdserver3d + gdrender3d
```

### Current Capabilities

1. **Scene tree panel** - displays node hierarchy, supports selection and reparenting
2. **2D Viewport** - renders scene via SoftwareRenderer, pan/zoom/grid
3. **3D Viewport** - camera orbit, gizmos (translate/rotate/scale), grid, environment preview
4. **Inspector** - property editing with typed editors, plugin-based extensibility
5. **Node operations** - add, delete, reparent, rename, duplicate nodes
6. **Undo/redo** - command-based undo stack for all operations
7. **Save/Load** - .tscn round-trip via PackedScene + TscnSaver
8. **Animation editor** - timeline, keyframe editing, playback, onion skinning
9. **Script editor** - syntax highlighting, autocompletion, find/replace
10. **Shader editor** - shader code editing
11. **Command palette** - quick action search (Ctrl+P)
12. **Export dialog** - platform presets, build profiles
13. **TileMap editor** - tile painting and tileset editing
14. **Theme editor** - theme property editing with live preview
15. **VCS integration** - git status and diff display
16. **Plugin trait** - EditorPlugin for extensibility

### REST API Surface

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/editor` | GET | Main editor HTML page |
| `/api/scene` | GET | Scene tree as JSON |
| `/api/node/:id` | GET | Node details |
| `/api/node` | POST | Add node |
| `/api/node/:id` | DELETE | Delete node |
| `/api/property/set` | POST | Set node property |
| `/api/viewport` | GET | Rendered frame (BMP/PNG) |
| `/api/scene/save` | POST | Save scene to .tscn |
| `/api/scene/load` | POST | Load scene from .tscn |
| `/api/undo` | POST | Undo last command |
| `/api/redo` | POST | Redo last undone command |

## Dependency Graph

```
gdeditor
  +-- gdscene (scene tree, packed scenes, lifecycle)
  +-- gdrender2d (software renderer for 2D viewport)
  +-- gdserver3d (3D rendering server for 3D viewport)
  +-- gdvariant (property serialization)
  +-- gdcore (math types, errors)
  +-- gdobject (class_db, signals)
  +-- gdresource (resource loading for import pipeline)
  +-- gdscript-interop (script parsing for script editor)
  +-- gdplatform (export, platform targets)
```

## Subsystem Boundaries

The editor is organized into five concrete subsystems. Each subsystem groups
related modules and defines a clear responsibility boundary.

### Subsystem 1: Editor Shell (server + browser frontend)

**Modules:** `editor_server`, `editor_ui`
**Boundary:** Owns the HTTP server, HTML/CSS/JS generation, and REST API
routing. All browser communication flows through this subsystem. No engine
state mutation happens here — it delegates to the Editor API layer.

### Subsystem 2: Scene Editing Core

**Modules:** `scene_editor`, `scene_renderer`, `dock`, `create_dialog`,
`signal_dialog`, `group_dialog`, `undo_redo`
**Boundary:** Owns scene tree manipulation (add/delete/reparent/duplicate
nodes), selection state, the undo/redo command stack, and scene tree dock
rendering. Reads from `gdscene::SceneTree` but does not own the scene tree
itself — it operates through `Editor` commands.

### Subsystem 3: Inspection and Properties

**Modules:** `inspector`, `settings`, `editor_settings_dialog`,
`project_settings_dialog`
**Boundary:** Owns property display, typed property editors, change
callbacks, and settings persistence. Does not own the property values
themselves — it reads/writes through the Variant system (`gdvariant`).

### Subsystem 4: Viewports and Rendering

**Modules:** `viewport_2d`, `viewport_3d`, `environment_preview`,
`texture_cache`
**Boundary:** Owns viewport state (camera, pan/zoom, gizmos, grid) and
frame rendering. Delegates actual rendering to `gdrender2d` (2D software
renderer) and `gdserver3d` (3D rendering server). Does not own scene data.

### Subsystem 5: Tooling and Specialized Editors

**Modules:** `animation_editor`, `curve_editor`, `script_editor`,
`script_completion`, `script_gutter`, `shader_editor`, `find_replace`,
`command_palette`, `editor_menu`, `export_dialog`, `output_panel`,
`profiler_panel`, `theme_editor`, `tilemap_editor`, `import`,
`import_settings`, `asset_drag_drop`, `vcs`, `editor_plugin`,
`editor_interface`, `editor_compat`
**Boundary:** Each specialized editor or tool is self-contained. They access
the scene and engine through the Editor API — they do not reach into engine
internals directly. The `editor_plugin` module defines the extensibility
trait; `editor_interface` provides the compatibility layer for Godot's
`EditorInterface` API.

### Cross-Cutting Concerns

| Concern | Owner | Notes |
|---------|-------|-------|
| Undo/redo | `undo_redo` | Command-based, per-document stack |
| Selection | `Editor` (lib) | Single-selection model, broadcast to inspector/dock |
| Filesystem | `filesystem` | Read-only project browsing, feeds import pipeline |
| Plugin lifecycle | `editor_plugin` | Trait-based, registered at startup |
| Recent items | `recent_items` | Persisted across sessions |
| Project management | `project_manager` | Project listing and creation |

## Architecture Goals (Post-V1)

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

### 3. Asset Browser
Extend `EditorFileSystem` into a full asset browser:
- Thumbnail generation for resources
- Drag-and-drop from browser to scene tree (asset_drag_drop module exists)
- Import settings per resource type (import_settings module exists)

### 4. Debugger Integration
Connect script editor to runtime debugger:
- Variable/signal inspection at runtime
- Breakpoint-like frame stepping (script_gutter has breakpoint support)
- Output panel for runtime logs (output_panel module exists)

### 5. Plugin Ecosystem
Expand EditorPlugin trait:
- Plugin discovery and loading
- Plugin settings and configuration
- Plugin dock panels (PluginDockManager exists)

## Deferred Scope

The following capabilities are explicitly **not in scope** for the current
editor milestone. They may be addressed in future phases.

| Capability | Reason Deferred | Prerequisite |
|------------|-----------------|--------------|
| Native desktop editor (non-browser) | Current architecture is browser-served; native UI would require a new frontend layer | Stable WebSocket protocol |
| Visual shader editor (node graph) | Requires a graph-based UI framework not yet built | Shader language parity |
| Full GDScript debugger (breakpoints, stepping) | Script interpreter lacks debug hooks | `gdscript-interop` debug protocol |
| Plugin marketplace / download | Requires network infrastructure and trust model | Plugin ecosystem maturity |
| Collaborative editing (multi-user) | Requires CRDT or OT layer for scene merging | Multi-document support |
| Android/iOS editor preview | Mobile platform targets not yet stable | Platform export parity |
| Full import pipeline parity (FBX, glTF, all formats) | Only subset of importers implemented | Resource loader completeness |
| Editor localization (i18n) | No translation infrastructure | Stable UI string surface |

## Testing Strategy

| Layer | Test Type | Location |
|-------|-----------|----------|
| Unit | Module-level `#[cfg(test)]` | `gdeditor/src/*.rs` |
| Smoke | HTTP round-trip tests | `tests/editor_smoke_test.rs` |
| Parity | Editor DOM structure tests | `tests/editor_dom_parity_test.rs` |
| Layout | Main window layout validation | `tests/editor_main_window_layout_test.rs` |
| Menu | Menu bar parity | `tests/editor_menu_parity_test.rs` |
| Plugin | Plugin API validation | `tests/editor_plugin_api_tool_script_test.rs` |
| Inspector | Property editors | `tests/property_inspector_*.rs` |
| Script | Script editor features | `tests/script_editor_*.rs` |
| Theme | Theme editor | `tests/theme_editor_*.rs` |
| TileMap | TileMap editor | `tests/tilemap_editor_*.rs` |
| Integration | Full editor workflow | `tests/editor_461_revalidation_test.rs` |

All tests must pass headless (no browser required).
