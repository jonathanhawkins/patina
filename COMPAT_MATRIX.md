# COMPAT_MATRIX.md - Compatibility Status Tracking

This document tracks the implementation and compatibility status of each Patina Engine subsystem relative to upstream Godot behavior.

**Last updated**: Phase 0 (all subsystems at initial status)

---

## Status Definitions

| Status | Meaning |
|--------|---------|
| **Not Started** | No implementation work has begun |
| **In Progress** | Active implementation underway; not yet testable against fixtures |
| **Partial** | Initial implementation exists; limited fixture coverage; gaps remain |
| **Complete** | Oracle-backed parity tests passing for all supported fixtures in this area |

---

## Compatibility Matrix

| Subsystem | Crate | Status | Fixture Coverage | Parity Rate | Notes |
|-----------|-------|--------|-----------------|-------------|-------|
| Core Runtime | `gdcore` | Partial | -- | -- | Math types, IDs, strings, error handling; implemented, no fixtures yet |
| Variant System | `gdvariant` | Partial | -- | -- | Variant enum, type conversions, serialization; implemented, no fixtures yet |
| Object Model | `gdobject` | Not Started | -- | -- | ClassDB, properties, method binding; stub only |
| Signals | `gdobject` | Not Started | -- | -- | Declaration, connection, emission, ordering |
| Notifications | `gdobject` | Not Started | -- | -- | Dispatch, ordering, lifecycle notifications |
| Resources | `gdresource` | Not Started | -- | -- | Load/save, cache, UID/path, .tres/.tscn parsing; stub only |
| Scene System | `gdscene` | Not Started | -- | -- | Node hierarchy, SceneTree, lifecycle, PackedScene; stub only |
| 2D Rendering | `gdrender2d` | Not Started | -- | -- | Sprites, draw ops, transforms, viewports; stub only |
| 2D Physics | `gdphysics2d` | Not Started | -- | -- | Shapes, collision detection, bodies, spaces; stub only |
| Audio | `gdaudio` | Not Started | -- | -- | Playback, buses, mixing; stub only |
| Input | `gdplatform` | Not Started | -- | -- | Keyboard, mouse, gamepad events; stub only |
| Platform | `gdplatform` | Not Started | -- | -- | Windowing, timing, OS integration; stub only |
| 3D Runtime | -- | Not Started | -- | -- | Deferred to Phase 6; no crate yet |
| Scripting Interop | `gdscript-interop` | Not Started | -- | -- | GDScript-compatible API surface; stub only |
| Editor | `gdeditor` | Not Started | -- | -- | Deferred to Phase 8; stub only |

---

## Platform Support Matrix

| Platform | Status | Notes |
|----------|--------|-------|
| Linux (x86_64) | Not Started | Primary CI and development target |
| macOS (aarch64) | Not Started | Developer workstation target |
| macOS (x86_64) | Not Started | Developer workstation target |
| Windows (x86_64) | Not Started | Deferred to Phase 7 |
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
