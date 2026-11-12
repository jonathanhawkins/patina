# Changelog

All notable changes to the Patina Engine will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Full 2D runtime vertical slice (scenes, resources, physics, rendering)
- 3D runtime slice (gdserver3d, gdrender3d, gdphysics3d)
- Oracle parity testing framework with golden comparison
- Editor server with HTTP API
- GDScript interop layer
- CI pipeline with format, lint, test, and release build checks
- Release workflow triggered on version tags

## [0.1.0] - 2026-03-28

### Added
- Initial workspace structure with 16 crates
- Core types: StringName, NodePath, GString, RID, Error
- Variant system covering all 28 Godot 4 types
- Object model with ClassDB, signals, property reflection
- Resource loading (.tscn, .tres, .res) with UID registry
- Scene tree with node lifecycle hooks
- 2D physics with deterministic tick and golden traces
- 2D rendering with canvas items and pixel-diff validation
- 3D math layer (Vector3, Quaternion, Basis, Transform3D, AABB)
- 3D node support (Node3D, Camera3D, MeshInstance3D, Light3D)
- Platform abstraction with headless mode for CI
- 10 representative 3D fixture scenes with oracle outputs
- Oracle parity at 81.4% (180/221 properties matching)
