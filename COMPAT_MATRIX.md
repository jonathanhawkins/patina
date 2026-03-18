# COMPAT_MATRIX.md - Compatibility Status Tracking

This document tracks the implementation and compatibility status of each Patina Engine subsystem relative to upstream Godot behavior.

**Last updated**: Phase 0 (all subsystems at initial status)

---

## Status Definitions

| Status | Meaning |
|--------|---------|
| **Not Started** | No implementation work has begun |
| **In Progress** | Active implementation underway; not yet testable |
| **Alpha** | Basic functionality works; limited fixture coverage; API may change |
| **Beta** | Core functionality stable; broad fixture coverage; API mostly stable |
| **Stable** | Full fixture coverage; parity tests passing; API frozen for the release |

---

## Compatibility Matrix

| Subsystem | Crate | Status | Fixture Coverage | Parity Rate | Notes |
|-----------|-------|--------|-----------------|-------------|-------|
| Core Runtime | `gdcore` | Not Started | -- | -- | Math types, IDs, strings, error handling |
| Variant System | `gdvariant` | Not Started | -- | -- | Variant enum, type conversions, serialization |
| Object Model | `gdobject` | Not Started | -- | -- | ClassDB, properties, method binding |
| Signals | `gdobject` | Not Started | -- | -- | Declaration, connection, emission, ordering |
| Notifications | `gdobject` | Not Started | -- | -- | Dispatch, ordering, lifecycle notifications |
| Resources | `gdresource` | Not Started | -- | -- | Load/save, cache, UID/path, .tres/.tscn parsing |
| Scene System | `gdscene` | Not Started | -- | -- | Node hierarchy, SceneTree, lifecycle, PackedScene |
| 2D Rendering | `gdrender2d` | Not Started | -- | -- | Sprites, draw ops, transforms, viewports |
| 2D Physics | `gdphysics2d` | Not Started | -- | -- | Shapes, collision detection, bodies, spaces |
| Audio | `gdaudio` | Not Started | -- | -- | Playback, buses, mixing |
| Input | `gdplatform` | Not Started | -- | -- | Keyboard, mouse, gamepad events |
| Platform | `gdplatform` | Not Started | -- | -- | Windowing, timing, OS integration |
| 3D Runtime | -- | Not Started | -- | -- | Deferred to Phase 6 |
| Scripting Interop | `gdscript-interop` | Not Started | -- | -- | GDScript-compatible API surface |
| Editor | `gdeditor` | Not Started | -- | -- | Deferred to Phase 8 |

---

## Platform Support Matrix

| Platform | Status | Notes |
|----------|--------|-------|
| Linux (x86_64) | Not Started | Primary CI target |
| macOS (x86_64) | Not Started | Development platform |
| macOS (aarch64) | Not Started | Development platform |
| Windows (x86_64) | Not Started | Broad user base |
| Android | Not Started | Deferred to Phase 7+ |
| iOS | Not Started | Deferred to Phase 7+ |
| Web (WASM) | Not Started | Deferred to Phase 7+ |

---

## Update Protocol

This matrix is updated when:

1. A subsystem transitions between status levels.
2. New fixture coverage is added for a subsystem.
3. Parity test results change materially.
4. A new subsystem row is added.

Each update should include the date and the bead or PR that prompted the change.
