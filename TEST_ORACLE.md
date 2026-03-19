# TEST_ORACLE.md - Upstream Oracle Strategy

This document defines how upstream Godot serves as the source of truth for expected behavior in the Patina Engine, what outputs to capture, the rules governing oracle usage, fixture format standards, and version pinning strategy.

---

## Principle

> Upstream Godot is the reference implementation. The Rust runtime must match its observable behavior for all supported fixture classes. Internal implementation is free to differ.

---

## What to Capture from Upstream Godot

For every fixture class, the oracle system must produce machine-readable outputs covering:

### Scene Tree Structure
- Full node hierarchy (parent-child relationships)
- Node types and class names
- Node names and paths
- Group memberships
- Scene instancing relationships

### Node and Property Values
- All exported properties with their values
- Property types and metadata
- Default value comparisons
- Property change notifications

### Signal Emission Order
- Which signals fire during lifecycle operations
- The exact order of signal emissions
- Signal parameters at emission time
- Connection state (connected handlers, flags)

### Notifications and Lifecycle Events
- Notification dispatch order (NOTIFICATION_ENTER_TREE, NOTIFICATION_READY, NOTIFICATION_PROCESS, etc.)
- Notification parameters
- Lifecycle callback ordering (_enter_tree, _ready, _process, _exit_tree)
- Order relative to parent/child/sibling nodes

### Resource Serialization
- .tres serialization output (text format)
- Binary resource serialization output
- Roundtrip fidelity: load then save produces identical output
- Resource UID and path resolution results

### Import Pipeline Outputs
- Import settings and their effects
- Generated files from import
- Reimport determinism

### Render Snapshots
- Frame captures at defined points in fixture execution
- Viewport contents as PNG/raw pixel data
- Defined comparison thresholds (not pixel-perfect)
- Camera and viewport configuration at capture time

### Physics Traces
- Body positions and velocities at each simulation step
- Collision events (bodies involved, contact points, normals)
- Area overlap events
- Deterministic simulation step outputs

### Timing and Frame Progression
- Frame count at defined checkpoints
- Delta time sequences
- Process vs. physics process ordering
- Idle vs. physics frame interleaving

---

## Oracle Rules

### Rule 1: Observable Behavior is the Contract

The Rust implementation is allowed to differ internally from upstream Godot. Only observable behavior -- outputs that can be captured and compared by the oracle system -- constitutes the compatibility contract.

### Rule 2: Every Test States What It Checks

Every compatibility test must explicitly declare what observable behavior it verifies. Vague tests ("it works") are not acceptable. Each test documents:
- The fixture being executed
- The specific outputs being compared
- The comparison method and thresholds
- The upstream Godot version used to generate the golden output

### Rule 3: Ambiguity is Documented

When upstream Godot behavior is ambiguous, version-sensitive, or appears to be a bug rather than a contract, document it in this file under the "Known Ambiguities" section. Do not silently choose an interpretation.

### Rule 4: Golden Outputs are Versioned

All golden outputs (expected results from upstream Godot) are stored in version control under `fixtures/`. They are regenerated only when the pinned upstream version changes.

### Rule 5: Visual Demos are Not Parity

A visual demo that "looks right" is not evidence of compatibility. Parity is measured by oracle comparison, not human visual inspection. Render diff tests with defined thresholds are the standard.

### Rule 6: Determinism Where Godot is Deterministic

If upstream Godot produces deterministic output for a fixture (same input always yields same output), the Rust runtime must also produce deterministic and matching output. Nondeterministic behavior must be explicitly flagged in test metadata.

---

## Fixture Format Standards

### Directory Structure

```
fixtures/
  scenes/          # .tscn fixture files
  projects/        # Complete mini-projects for integration testing
  resources/       # .tres resource fixtures
  imports/         # Import pipeline test cases
  physics/         # Physics simulation fixtures
  render/          # Render comparison fixtures
  golden/          # Expected outputs from upstream Godot
    scenes/        # Golden scene tree dumps
    signals/       # Golden signal traces
    properties/    # Golden property dumps
    resources/     # Golden resource serialization outputs
    render/        # Golden render snapshots
    physics/       # Golden physics traces
```

### Fixture File Naming

```
<category>_<description>_<variant>.tscn
```

Examples:
- `scene_simple_hierarchy_01.tscn`
- `signal_custom_emission_order_01.tscn`
- `resource_texture_roundtrip_01.tres`
- `physics_static_collision_01.tscn`

### Fixture Metadata

Each fixture includes a metadata header (as a comment or companion .json file):

```json
{
  "fixture_id": "scene_simple_hierarchy_01",
  "category": "scene",
  "description": "Simple three-level node hierarchy with mixed types",
  "captures": ["scene_tree", "properties", "notifications"],
  "deterministic": true,
  "upstream_version": "4.x.x-stable",
  "created": "2026-03-18",
  "notes": ""
}
```

---

## Golden Output Format

Golden outputs are JSON files with a standard envelope:

```json
{
  "fixture_id": "scene_simple_hierarchy_01",
  "upstream_version": "4.x.x-stable",
  "upstream_commit": "<git-sha>",
  "capture_type": "scene_tree",
  "generated_at": "2026-03-18T00:00:00Z",
  "data": {
    // Capture-type-specific structured data
  }
}
```

### Capture-Specific Data Formats

**Scene Tree**: Nested JSON objects mirroring the node hierarchy. Each node contains `name`, `class`, `path`, `children` (recursive), and `properties` (tagged Variant values). Example:

```json
{
  "nodes": [
    {
      "name": "Root",
      "class": "Node",
      "path": "/root/Root",
      "children": [
        {
          "name": "Player",
          "class": "Node2D",
          "path": "/root/Root/Player",
          "children": [],
          "properties": {
            "position": { "type": "Vector2", "value": [100.0, 200.0] }
          }
        }
      ],
      "properties": {}
    }
  ]
}
```

**Properties**: Each property value uses the gdvariant tagged JSON format: `{ "type": "<VariantType>", "value": <typed-value> }`. Supported types: `Nil`, `Bool`, `Int`, `Float`, `String`, `Vector2` (array of 2 floats), `Vector3` (array of 3 floats), `Color` (array of 4 floats), `Rect2`, `Transform2D`, `Array`, `Dictionary`.

**Resources**: Object with `class_name`, `properties` (tagged Variant map), and `subresources` (keyed by sub-resource ID, each with `class_name` and `properties`). Example:

```json
{
  "class_name": "Theme",
  "properties": {
    "name": { "type": "String", "value": "MyTheme" }
  },
  "subresources": {
    "StyleBoxFlat_001": {
      "class_name": "StyleBoxFlat",
      "properties": {
        "bg_color": { "type": "Color", "value": [0.2, 0.3, 0.4, 1.0] }
      }
    }
  }
}
```

**Full Fixture Capture**: Combined oracle captures produced by `tools/oracle/run_fixture.gd`
include final `scene_tree`, final `properties`, ordered `signal_trace`,
ordered `notification_trace`, and per-frame `frame_trace` snapshots for
scenes that need frame-by-frame contract comparison.

**Signals**: Ordered array of signal emission events with signal name, emitter path, and parameters.

**Notifications**: Ordered array of notification events with notification ID, target path, and frame number.

**Render Snapshots**: PNG file path + metadata (viewport size, camera config, frame number).

**Physics Traces**: Array of per-step records with body states (position, velocity, rotation) and collision events.

### Float Comparison Strategy

Golden files for scene trees and resources use float tolerance (epsilon = 1e-6) when comparing `Vector2`, `Vector3`, `Color`, and `Float` variant values. This accounts for f32/f64 conversion artifacts. Non-float types use exact comparison.

---

## Version Pinning Strategy

### Upstream Godot Pin

- Upstream Godot is added as a git submodule pinned to a specific commit.
- The pinned version is recorded in this file and in the submodule configuration.
- All golden outputs are generated against the pinned version.

### Version Upgrade Process

When upgrading the pinned upstream version:

1. Update the submodule to the new commit/tag.
2. Regenerate all golden outputs using the oracle tools.
3. Run the full compatibility test suite against the new golden outputs.
4. Review any behavioral changes (tests that now fail or produce different output).
5. Document changes in this file under "Version History."
6. Update COMPAT_MATRIX.md if any subsystem status changes.

### Current Pin

- **Upstream version**: `4.5.1-stable`
- **Upstream commit**: `f62fdbde15035c5576dad93e586201f4d41ef0cb`
- **Pin date**: `2026-03-19`
- **Submodule path**: `upstream/godot`
- **Source remote**: `https://github.com/godotengine/godot.git`

### Sync and Update Commands

Use the pinned upstream checkout for all oracle generation work:

```sh
git submodule update --init --recursive
```

When intentionally updating the oracle pin:

```sh
git -C upstream/godot fetch --tags
git -C upstream/godot checkout <tag-or-commit>
git add upstream/godot .gitmodules
```

After changing the pin:

1. Regenerate all golden outputs.
2. Run the compatibility suites that consume those goldens.
3. Record the new version, commit, and date in this section.

---

## Known Ambiguities

This section documents cases where upstream Godot behavior is ambiguous, version-sensitive, or appears to be a bug.

| ID | Area | Description | Upstream Behavior | Our Decision | Notes |
|----|------|-------------|-------------------|-------------|-------|
| -- | -- | (none yet) | -- | -- | -- |

Ambiguities will be added here as they are discovered during fixture development and compatibility testing.

---

## Oracle Tooling

The following tools (in `tools/oracle/`) generate golden outputs:

| Tool | Purpose | Output Format |
|------|---------|---------------|
| Scene tree dumper | Captures full node hierarchy | JSON |
| Property dumper | Captures all node properties | JSON |
| Signal tracer | Records signal emission events | JSON |
| Notification tracer | Records notification dispatch | JSON |
| Resource roundtripper | Tests load-save fidelity | JSON + .tres |
| Render snapshotter | Captures viewport contents | PNG + JSON metadata |
| Physics tracer | Records simulation step data | JSON |

These tools run inside upstream Godot (via GDScript or GDExtension) and produce outputs in the golden format defined above.
