# PORT_SCOPE.md - Patina Engine v1 Scope Definition

This document defines what counts as success for v1 of the Patina Engine, what is deferred to later milestones, supported fixture classes, initial platform targets, and compatibility boundaries.

---

## Target Outcome for v1

A Rust runtime that can:

1. **Load scenes and resources** -- Parse and instantiate Godot scene (.tscn) and resource (.tres) files for a supported subset of node and resource types.
2. **Run a subset of SceneTree behavior** -- Execute the SceneTree lifecycle including node enter/ready/process flow, parent-child relationships, and scene instancing for supported node types.
3. **Support a core object model** -- Implement Variant types, object registration, metadata, signals, notifications, and reference-counted lifetime management.
4. **Execute deterministic compatibility fixtures** -- Run a defined corpus of fixture scenes and produce machine-readable outputs that can be compared against upstream Godot oracle outputs.
5. **Render a first meaningful 2D slice** -- Display a working 2D rendering path including sprites, basic draw operations, transforms, and a frame loop.
6. **Demonstrate measurable Godot parity** -- Pass automated parity tests against upstream Godot for all supported fixture classes, with quantified compatibility metrics.

---

## Deferred Items (Not in v1)

The following are explicitly out of scope for v1 and will be addressed in later milestones:

| Item | Rationale | Target Phase |
|------|-----------|-------------|
| Full editor parity | Editor depends on stable runtime; runtime-first strategy | Phase 8+ |
| All Godot subsystems | Staged approach; only core + 2D in v1 | Phase 5+ |
| All platforms | Desktop-first; mobile, web, console later | Phase 7+ |
| 3D runtime | 2D vertical slice first; 3D requires stable foundation | Phase 6 |
| GDScript execution | Scripting interop is separate workstream | Phase 5+ |
| Full import pipeline | Only subset needed for fixture execution | Phase 5+ |
| Animation system (full) | Basic support only if needed for 2D fixtures | Phase 5+ |
| Networking/multiplayer | Not required for v1 parity goals | Phase 5+ |
| Plugin/addon system | Depends on editor and scripting layers | Phase 8+ |
| Visual shader editor | Editor-facing; deferred with other editor work | Phase 8+ |

---

## Supported Fixture Classes

v1 must support deterministic execution and oracle comparison for these fixture classes:

### Scene Fixtures
- Simple node hierarchies (Node, Node2D, Control)
- Parent-child relationships and tree traversal
- Scene instancing (PackedScene)
- Node lifecycle: enter_tree, ready, process, exit_tree

### Resource Fixtures
- Resource loading from .tres files
- Resource serialization roundtrips
- Resource caching and UID/path resolution
- Basic resource types (Texture2D, Font, Theme subset)

### Signal Fixtures
- Signal declaration and connection
- Signal emission and handler invocation
- Signal emission ordering
- Built-in signals (tree_entered, ready, tree_exiting)

### Object Model Fixtures
- Variant type conversions
- Object property get/set
- Notification dispatch and ordering
- Reference counting lifecycle

### 2D Rendering Fixtures
- Sprite2D rendering
- Basic CanvasItem draw operations
- Transform2D hierarchy
- Viewport and camera basics
- Render snapshot comparison within defined diff thresholds

### 2D Physics Fixtures (Basic)
- Static body collision detection
- Basic shape queries
- Deterministic simulation traces

---

## Initial Platform Targets

v1 targets desktop platforms only:

| Platform | Priority | Notes |
|----------|----------|-------|
| Linux (x86_64) | Primary | CI and development platform |
| macOS (x86_64, aarch64) | Primary | Development platform |
| Windows (x86_64) | Primary | Broad user base |

### Deferred Platforms

| Platform | Target Phase |
|----------|-------------|
| Android | Phase 7+ |
| iOS | Phase 7+ |
| Web (WASM) | Phase 7+ |
| Console (Switch, PlayStation, Xbox) | Phase 7+ |

---

## Compatibility Boundaries

### What "Parity" Means

- **Observable behavior parity**: The Rust runtime produces the same observable outputs as upstream Godot for supported fixture classes.
- **Not source-level parity**: Internal implementation is free to differ. We port contracts, not implementation details.
- **Not pixel-perfect rendering**: Render outputs must be within defined diff thresholds, not identical at the pixel level.
- **Deterministic where Godot is deterministic**: If upstream Godot produces deterministic output for a fixture, the Rust runtime must also produce deterministic output that matches.

### Oracle Relationship

- Upstream Godot (pinned version) is the single source of truth for expected behavior.
- Every compatibility test must state what observable behavior it checks.
- When upstream behavior is ambiguous or version-sensitive, it is documented in TEST_ORACLE.md.

### Version Pinning

- Upstream Godot is pinned to a specific commit/tag.
- All fixtures and golden outputs are generated against the pinned version.
- Version upgrades require re-generation of all golden outputs and explicit review.

### Boundary Conditions

- Error behavior: Match upstream error behavior where practical; document divergences.
- Edge cases: Not all upstream edge cases need to be matched in v1; document known gaps.
- Performance: Must be "competitive" with upstream Godot on supported workloads; not required to be faster in v1.
- Memory: Must not have unbounded memory growth; absolute footprint targets deferred to benchmarks.

---

## Success Criteria Summary

v1 is complete when:

1. All supported fixture classes have oracle-backed parity tests.
2. Parity tests pass at an agreed threshold (target: 95%+ of fixture corpus).
3. At least one representative 2D project or fixture set runs end-to-end.
4. Render outputs are within agreed diff thresholds.
5. The runtime builds and runs on all three desktop platforms.
6. Performance baselines are established and documented in BENCHMARKS.md.
7. The compatibility matrix (COMPAT_MATRIX.md) reflects actual measured status.
