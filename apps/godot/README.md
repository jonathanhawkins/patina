# apps/godot — GDExtension Compatibility Lab

This directory is the **GDExtension compatibility lab** for Patina. It contains a godot-rust extension (`patina_lab`) that runs inside a pinned Godot 4 project and emits machine-readable probe output for oracle comparison.

---

## Purpose

The lab serves three goals:

1. **API probing** — emit ClassDB-level signatures (class list, parent class, property list, method list, signals) so Patina's object model can be validated against upstream Godot.
2. **Resource probing** — capture resource metadata, property enumeration, and subresource references for representative `.tres`/`.res` fixtures so Patina's resource loader can be validated.
3. **Smoke testing** — run scene tree, signal, and property operations and verify the output format that Patina tests consume.

All probe output is printed to stdout as `PATINA_PROBE:<json>` lines, which the oracle harness in `tools/` can capture and diff.

---

## Probes

### `scene_probe.rs`

Walks the scene tree rooted at a given `Node` and emits a JSON envelope:

```json
{
  "fixture_id": "smoke_probe",
  "capture_type": "scene_tree",
  "data": {
    "root": {
      "name": "...",
      "class": "...",
      "path": "...",
      "owner": "...",
      "script_path": "...",
      "process_mode": 0,
      "unique_name_in_owner": false,
      "children": [...]
    }
  }
}
```

**Covers:** node names, class names, scene paths, owner path, script path, process mode, unique name flag, tree structure.

---

### `property_probe.rs`

Enumerates all properties via `get_property_list()` for a node:

```json
{
  "fixture_id": "smoke_probe",
  "capture_type": "properties",
  "data": {
    "node_name": "...",
    "node_class": "...",
    "property_count": 42,
    "properties": [
      { "name": "position", "type": 5, "hint": 0, "hint_string": "", "usage": 4102, "class_name": "" }
    ]
  }
}
```

**Covers:** full ClassDB property enumeration — name, Variant type, hint, hint_string, usage flags, class_name.

---

### `signal_probe.rs`

Tests signal connect/emit/callback ordering AND enumerates all signals via `get_signal_list()`:

```json
{
  "fixture_id": "smoke_probe",
  "capture_type": "signals",
  "data": {
    "node_name": "...",
    "node_class": "...",
    "ordering_events": ["before_connect", "after_connect", "after_emit"],
    "signal_count": 5,
    "signals": [
      { "name": "ready", "args": [] },
      { "name": "tree_entered", "args": [] }
    ]
  }
}
```

**Covers:** signal connect/emit ordering, full signal list with argument metadata.

---

### `resource_probe.rs`

Loads a resource by path and emits metadata including full property enumeration and subresource references:

```json
{
  "fixture_id": "resource_probe",
  "capture_type": "resource_metadata",
  "data": {
    "resource_class": "PackedScene",
    "resource_path": "res://scenes/smoke_probe.tscn",
    "resource_name": "",
    "property_count": 5,
    "properties": [...],
    "subresource_count": 1,
    "subresources": [
      { "property": "script", "class": "GDScript", "path": "res://scripts/smoke_probe.gd" }
    ]
  }
}
```

**Covers:** resource class, path, name, full property list, subresource graph.

---

### `classdb_probe.rs`

Dumps ClassDB metadata for 17 core Godot classes that match Patina's `classdb_parity_test.rs`:

```json
{
  "fixture_id": "classdb_probe",
  "capture_type": "classdb",
  "data": {
    "class": "Node",
    "parent": "Object",
    "method_count": 85,
    "methods": [{ "name": "add_child", "args": [...], "return_type": 0 }],
    "property_count": 8,
    "properties": [{ "name": "name", "type": 4, "hint": 0, "hint_string": "", "usage": 4102 }],
    "signal_count": 3,
    "signals": [{ "name": "ready", "args": [] }]
  }
}
```

**Core classes probed:** Node, Node2D, Node3D, Sprite2D, Camera2D, AnimationPlayer, Control, Label, Button, RigidBody2D, StaticBody2D, CharacterBody2D, Area2D, CollisionShape2D, Timer, TileMap, CPUParticles2D.

**Covers:** parent class, all methods with argument types, all properties with type/hint/usage, all signals with argument metadata.

---

## Running the Lab

> **Prerequisite:** A Godot 4.x binary matching the version in `project.godot` must be installed, and godot-rust must compile against the local GDExtension API headers.

```bash
# Build the extension
cargo build --release

# Copy the .gdextension + shared lib into the Godot project
# (see patina_lab.gdextension for path config)

# Run headless and capture probe output
godot --headless --path apps/godot 2>&1 | grep PATINA_PROBE
```

Probe lines can be parsed with `jq` or the oracle tooling in `tools/`.

---

## Probe Status

| Probe | Status | Feeds |
|---|---|---|
| `scene_probe` | Implemented | `gdscene` oracle tests |
| `property_probe` | Implemented | `gdobject` property tests |
| `signal_probe` | Implemented | `gdobject` signal tests |
| `resource_probe` | Implemented | `gdresource` oracle tests |
| `classdb_probe` | Implemented | `gdobject` ClassDB parity |

All probes compile against godot-rust 0.2 and emit machine-readable JSON. Running them requires a local Godot 4 binary with matching GDExtension API.
