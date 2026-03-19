# apps/godot — GDExtension Compatibility Lab

This directory is the **GDExtension compatibility lab** for Patina. It contains a godot-rust extension (`patina_lab`) that runs inside a pinned Godot 4 project and emits machine-readable probe output for oracle comparison.

---

## Purpose

The lab serves three goals:

1. **API probing** — emit ClassDB-level signatures (class list, parent class, property list, method list) so Patina's object model can be validated against upstream Godot.
2. **Resource probing** — capture resource metadata and roundtrip behavior for representative `.tres`/`.res` fixtures so Patina's resource loader can be validated.
3. **Smoke testing** — run basic scene tree, signal, and property operations and verify the output format that Patina tests consume.

All probe output is printed to stdout as `PATINA_PROBE:<json>` lines, which the oracle harness in `tools/` can capture and diff.

---

## Existing Probes

### `scene_probe.rs`

**Status: Implemented**

Walks the scene tree rooted at a given `Node` and emits a JSON envelope:

```json
{
  "fixture_id": "smoke_probe",
  "capture_type": "scene_tree",
  "data": { "root": { "name": "...", "class": "...", "path": "...", "children": [...] } }
}
```

**What it covers:** node names, class names, scene paths, tree structure.
**What it does not cover:** node properties, script variables, sub-resources.

---

### `property_probe.rs`

**Status: Implemented**

Emits property metadata for the `PatinaSmokeProbe` node:

```json
{
  "fixture_id": "smoke_probe",
  "capture_type": "properties",
  "data": { "properties": { "probe_label": { "type": "String", "value": "..." }, ... } }
}
```

**What it covers:** `@var`-exported properties and their runtime values.
**What it does not cover:** full `ClassDB.get_property_list()` enumeration (deferred — see below).

---

### `signal_probe.rs`

**Status: Implemented**

Connects, emits, and records events for `probe_signal`:

```json
{
  "fixture_id": "smoke_probe",
  "capture_type": "signals",
  "data": { "events": ["before_connect", "after_connect", "after_emit"] }
}
```

**What it covers:** signal connect, emit, and callback ordering.
**What it does not cover:** disconnecting signals, multi-argument signals, cross-node signal wiring.

---

## Deferred Probes

The following probes are specified but not yet implemented. They require compiling `patina_lab` against the pinned Godot 4 binary (which requires a local Godot install with the matching GDExtension API).

### ClassDB API Probe (pat-9eb)

**Goal:** Emit machine-readable ClassDB metadata for a representative set of runtime classes.

**Planned output per class:**
```json
{
  "fixture_id": "classdb_probe",
  "capture_type": "classdb",
  "data": {
    "class": "Node",
    "parent": "Object",
    "methods": [{ "name": "add_child", "args": [...], "return_type": "..." }],
    "properties": [{ "name": "name", "type": "String", ... }],
    "signals": [{ "name": "ready", "args": [] }]
  }
}
```

**Acceptance:** Output is reproducible across runs and consumed by at least one regression test in `engine-rs/`.

**Blocked by:** Local Godot binary + godot-rust compilation against pinned API (`pat-1wt` prerequisite).

---

### Resource Metadata Probe (pat-a41)

**Goal:** Load a representative `.tres` fixture and emit metadata + roundtrip hash.

**Planned output:**
```json
{
  "fixture_id": "resource_probe",
  "capture_type": "resource_metadata",
  "data": {
    "resource_class": "SpriteFrames",
    "resource_path": "res://fixtures/sprite_frames.tres",
    "properties": { ... },
    "roundtrip_sha256": "..."
  }
}
```

**Acceptance:** Probe output feeds the oracle path in `engine-rs/crates/gdresource/`.

**Blocked by:** Same Godot binary dependency as ClassDB probe.

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

## Current State Summary

| Probe | Status | Feeds |
|---|---|---|
| `scene_probe` | Implemented | `gdscene` oracle tests |
| `property_probe` | Implemented | `gdobject` property tests |
| `signal_probe` | Implemented | `gdobject` signal tests |
| ClassDB API probe | **Deferred** (pat-9eb) | `gdobject` ClassDB parity |
| Resource metadata probe | **Deferred** (pat-a41) | `gdresource` oracle tests |

Full GDExtension lab execution is deferred pending a CI-available Godot binary. When unblocked, implement deferred probes and wire output into the golden comparison pipeline.
