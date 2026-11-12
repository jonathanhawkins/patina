# Editor-Facing Compatibility Layer Specification

Date: 2026-04-07
Bead: `pat-6m9ky`
Source audit: `prd/PHASE8_EDITOR_PARITY_AUDIT.md`

## Purpose

This document is the formal definition of Patina's **minimal editor-facing
compatibility layer**. It enumerates every public API surface that constitutes
the layer, maps each to measured test evidence, and draws an explicit boundary
between what is inside the layer and what is outside.

The compatibility layer is the set of Godot-compatible API names, type aliases,
adapter structs, and singleton surfaces that allow GDExtension plugins and
editor scripts to target Patina using familiar Godot 4 API names.

## Scope

The compatibility layer covers exactly two modules in `gdeditor`:

1. **`editor_compat`** --- Godot-compatible type aliases, enum mappings,
   adapter structs, and free functions.
2. **`editor_interface`** --- Godot 4 `EditorInterface` singleton equivalent.

Everything else in `gdeditor` is implementation (editor shell, tooling,
specialized editors). Those modules may use the compatibility layer, but they
are not part of it.

## API Surface: `editor_compat`

### Type Aliases

| Alias | Maps To | Godot Equivalent |
|-------|---------|------------------|
| `EditorInspector` | `inspector::InspectorPanel` | `EditorInspector` |
| `EditorProperty` | `inspector::CustomPropertyEditor` | `EditorProperty` |
| `EditorInspectorPluginManager` | `inspector::InspectorPluginRegistry` | `EditorInspectorPlugin` (registry) |
| `EditorUndoRedoManager` | `Editor` | `EditorUndoRedoManager` |

### Enum Mappings

| Enum | Variants | Godot Equivalent |
|------|----------|------------------|
| `PropertyUsageFlags` | `Editor`, `Storage`, `Default`, `NoEditor`, `ReadOnly` | `PropertyUsageFlags` (subset) |

### Structs

| Struct | Purpose | Godot Equivalent |
|--------|---------|------------------|
| `EditorPropertyInfo` | Property metadata (name, type, hint, usage) | `Dictionary` property info |
| `EditorSelection` | Selection wrapper with Godot-named methods | `EditorSelection` |
| `EditorInterfaceCompat` | Read-only accessors for editor subsystems | `EditorInterface` (lightweight) |
| `UndoRedoCompat` | Undo/redo with Godot-named methods | `UndoRedo` |

### Free Functions

| Function | Purpose | Godot Equivalent |
|----------|---------|------------------|
| `validate_property_value` | Type validation with Godot coercion rules | `_validate_property` |
| `get_property_category` | Property categorization | Inspector category logic |

### Test Evidence

| API | Test File | Classification |
|-----|-----------|----------------|
| Type aliases | `editor_compat.rs` unit tests (`type_aliases_are_correct`) | Measured |
| `PropertyUsageFlags` | `editor_compat.rs` unit tests (`property_usage_flags_*`) | Measured |
| `EditorPropertyInfo` | `editor_compat.rs` unit tests (`editor_property_info_*`) | Measured |
| `EditorSelection` | `editor_compat.rs` unit tests (`editor_selection_*`) | Measured |
| `EditorInterfaceCompat` | `editor_compat.rs` unit tests (`editor_interface_compat_accessors`) | Measured |
| `UndoRedoCompat` | `editor_compat.rs` unit tests (`undo_redo_compat`) | Measured |
| `validate_property_value` | `editor_compat.rs` unit tests (`validate_property_value_*`) | Measured |
| `get_property_category` | `editor_compat.rs` unit tests (`get_property_category_maps_correctly`) | Measured |

## API Surface: `editor_interface`

### Struct: `EditorInterface`

The full `EditorInterface` singleton with Godot 4-compatible method names.

#### Editor Access

| Method | Godot Equivalent |
|--------|------------------|
| `get_editor()` | `EditorInterface.get_editor()` (Patina-specific) |
| `get_editor_mut()` | (mutable variant) |
| `get_editor_settings()` | `EditorInterface.get_editor_settings()` |
| `get_editor_settings_mut()` | (mutable variant) |
| `get_file_system_dock()` | `EditorInterface.get_file_system_dock()` |
| `get_file_system_dock_mut()` | (mutable variant) |
| `get_inspector()` | `EditorInterface.get_inspector()` |
| `get_inspector_mut()` | (mutable variant) |
| `get_edited_scene_root()` | `EditorInterface.get_edited_scene_root()` |
| `get_edited_scene_root_mut()` | (mutable variant) |

#### Selection

| Method | Godot Equivalent |
|--------|------------------|
| `get_selection()` | `EditorInterface.get_selection()` |
| `select_node(id)` | `EditorSelection.add_node()` |
| `deselect()` | `EditorSelection.clear()` |

#### Scene Operations

| Method | Godot Equivalent |
|--------|------------------|
| `get_current_path()` | `EditorInterface.get_current_path()` |
| `set_current_path(path)` | (internal) |
| `save_scene(path)` | `EditorInterface.save_scene()` |
| `open_scene_from_path(path)` | `EditorInterface.open_scene_from_path()` |
| `reload_scene_from_disk()` | `EditorInterface.reload_scene_from_disk()` |

#### Command Execution

| Method | Godot Equivalent |
|--------|------------------|
| `execute_command(cmd)` | `UndoRedo.commit_action()` (via Editor) |
| `undo()` | `UndoRedo.undo()` |
| `redo()` | `UndoRedo.redo()` |

#### UI State

| Method | Godot Equivalent |
|--------|------------------|
| `is_distraction_free_mode_enabled()` | `EditorInterface.is_distraction_free_mode_enabled()` |
| `set_distraction_free_mode(bool)` | `EditorInterface.set_distraction_free_mode()` |
| `is_bottom_panel_visible()` | (Patina-specific) |
| `set_bottom_panel_visible(bool)` | (Patina-specific) |
| `get_editor_theme()` | `EditorInterface.get_editor_theme()` |

#### File System

| Method | Godot Equivalent |
|--------|------------------|
| `scan_file_system()` | `EditorInterface.get_resource_filesystem().scan()` |
| `get_project_root()` | (Patina-specific) |

#### Plugin

| Method | Godot Equivalent |
|--------|------------------|
| `get_plugin_names()` | (Patina-specific) |
| `is_scene_modified()` | `EditorInterface.is_scene_modified()` (Patina: undo depth > 0) |

### ClassDB Registrations

| Class | Methods | Evidence |
|-------|---------|----------|
| `EditorPlugin` | `get_editor_interface`, `add_control_to_dock`, `add_custom_type` | `editor_interface_compat_test.rs::classdb_editor_plugin_registered` |
| `EditorInterface` | `get_editor_settings`, `get_selection`, `get_inspector`, `open_scene_from_path`, `save_scene` | `editor_interface_compat_test.rs::classdb_editor_interface_registered` |

### Test Evidence

| API | Test File | Classification |
|-----|-----------|----------------|
| Selection round-trip | `editor_interface.rs` unit tests, `editor_interface_compat_test.rs` | Measured |
| Command execution + undo/redo | `editor_interface.rs` unit tests, `editor_interface_compat_test.rs` | Measured |
| Settings mutation | `editor_interface_compat_test.rs::editor_interface_settings_mutation` | Measured |
| Scene path tracking | `editor_interface_compat_test.rs::editor_interface_scene_path_tracking` | Measured |
| Distraction-free mode | `editor_interface_compat_test.rs::editor_interface_distraction_free_toggle` | Measured |
| ClassDB registrations | `editor_interface_compat_test.rs::classdb_*` | Measured |
| Project root | `editor_interface_compat_test.rs::editor_interface_project_root` | Measured |

## What Is NOT In The Compatibility Layer

The following are editor implementation, not compatibility surface:

- **Editor shell** (`editor_server`, `editor_ui`) --- browser-served HTTP
  interface; tested separately via `editor_smoke_test.rs`
- **Specialized editors** (script, shader, animation, theme, tilemap) ---
  tooling modules with their own tested slices
- **Inspector internals** (`inspector` module) --- the compat layer aliases
  inspector types but does not own inspector logic
- **Dock panels** (`dock`) --- scene tree dock, property dock
- **Import pipeline** (`import`, `import_settings`) --- resource importers
- **Viewport rendering** (`viewport_2d`, `viewport_3d`) --- canvas/3D rendering
- **All other `gdeditor` modules** --- implementation details

These modules have their own measured slices documented in
`prd/PHASE8_EDITOR_PARITY_AUDIT.md` under the tooling milestone inventory.
They are not part of the compatibility layer claim.

## Boundary Rule

A type, function, or method is part of the compatibility layer if and only if:

1. It lives in `editor_compat` or `editor_interface`, AND
2. It maps a Godot API name to a Patina equivalent, AND
3. It has a corresponding unit or integration test

If a new Godot-compatible surface is added, it must be added to both the code
module and this specification, with a test citation.

## Validation

The compatibility layer surface is validated by:

- `engine-rs/crates/gdeditor/src/editor_compat.rs` unit tests (13 tests)
- `engine-rs/crates/gdeditor/src/editor_interface.rs` unit tests (8 tests)
- `engine-rs/tests/editor_interface_compat_test.rs` integration tests (8 tests)
- `engine-rs/tests/editor_compat_layer_surface_test.rs` surface validation test

The surface validation test (`editor_compat_layer_surface_test.rs`) checks that
the public API inventory matches this specification. If a new type or method is
added to the compatibility modules without updating the spec and the surface
test, the test will fail.
