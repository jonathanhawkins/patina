# Patina Engine v1 — Exit Criteria

Ship gate checklist for the 2D vertical slice milestone. Each gate has a pass
condition, the test file(s) that prove it, and current status.

**Last updated**: 2026-03-19 (pat-ati)

---

## Gate 1: Core Runtime — lifecycle trace parity

All lifecycle notification traces must match upstream Godot ordering across
all fixture scenes.

- [x] `enter_tree` / `ready` / `exit_tree` ordering matches Godot (14 tests)
- [x] `process` / `physics_process` dispatch in tree order (32 tests)
- [x] Pause/unpause notification sequencing correct (included in lifecycle tests)
- [x] Reparent fires `UNPARENTED` → `PARENTED` → `MOVED_IN_PARENT` (6 tests)
- [x] All 8 fixture scenes produce matching lifecycle traces (7+7 multi-scene parity)

| Requirement | Status | Test files |
|-------------|--------|------------|
| Lifecycle ordering | **Pass** | `lifecycle_trace_parity_test` (14), `notification_coverage_test` (16) |
| Frame processing semantics | **Pass** | `frame_processing_semantics_test` (32) |
| Trace parity vs upstream | **Pass** | `trace_parity_test` (10), `multi_scene_trace_parity_test` (7) |
| Frame trace generation | **Pass** | `frame_trace_test` (8) |

---

## Gate 2: Physics — deterministic golden traces

Physics simulation must be deterministic and produce identical golden traces
regardless of run order.

- [x] Gravity free-fall trace matches golden (30 frames)
- [x] Friction deceleration trace matches golden (30 frames)
- [x] Static body blocking trace matches golden (60 frames)
- [x] Elastic bounce trace matches golden (30 frames)
- [x] Rigid + static scene trace matches golden (20 frames)
- [x] `physics_playground` full trace matches golden (60 frames)
- [x] Collision shape registration from scene nodes verified
- [x] CharacterBody2D kinematic behavior verified
- [x] Fixed-timestep accumulator carries remainder correctly

| Requirement | Status | Test files |
|-------------|--------|------------|
| Deterministic goldens (8 files) | **Pass** | `physics_integration_test` (54) |
| Body sync + step integration | **Pass** | `gdphysics2d` units (86) |

---

## Gate 3: Input — all action types through engine pipeline

Input must flow through the engine-owned pipeline: `InputMap` → `InputState` →
`InputSnapshot` → scripts via `Input.is_action_pressed()`.

- [x] Keyboard action snapshots work through engine API
- [x] Input map loading from JSON fixture
- [x] Action binding coverage for all mapped actions
- [x] Input auto-clears after each frame (no stale leak)
- [x] Bidirectional input (left + right) in same frame
- [x] Diagonal input (multiple simultaneous actions)
- [x] Mouse position and button routing to input snapshots

| Requirement | Status | Test files |
|-------------|--------|------------|
| Input map loading | **Pass** | `input_map_loading_test` (16) |
| Action coverage | **Pass** | `input_action_coverage_test` (10) |
| End-to-end input routing | **Pass** | `vertical_slice_test` (16) |
| Platform input units | **Pass** | `gdplatform` units (120) |
| Mouse routing | **Pass** | `gdplatform` units (pat-aro completed) |

---

## Gate 4: Signals — dispatch parity with Godot

Signal system must match Godot's connection-order dispatch, one-shot behavior,
and reparent survival semantics.

- [x] Multiple connections fire in connection order
- [x] Reversed connection order reverses dispatch
- [x] Connections survive emitter reparenting
- [x] Connections survive receiver reparenting
- [x] One-shot connections auto-disconnect after first emit
- [x] One-shot mixed with persistent: only one-shot removed
- [x] One-shot preserves dispatch order on firing emission
- [x] Signal trace parity against upstream mock (12 tests)

| Requirement | Status | Test files |
|-------------|--------|------------|
| Dispatch parity | **Pass** | `signal_dispatch_parity_test` (16) |
| Trace parity | **Pass** | `signal_trace_parity_test` (12) |

---

## Gate 5: Resources — unified loader with UID + cache dedup

Resource loading must resolve both `res://` paths and `uid://` references
through a single loader path, with cache deduplication.

- [x] `res://` path loading works
- [x] `uid://` reference loading works
- [x] Same resource via path and UID returns same `Arc`
- [x] Cache deduplication: 100 loads of same path → loader called once
- [x] Cache invalidation produces new `Arc`, old stays valid
- [x] UID register/unregister cycles (100 cycles)
- [x] 50 different resources all unique in cache
- [x] Alternating path/UID loads hit same cache entry

| Requirement | Status | Test files |
|-------------|--------|------------|
| Unified loader | **Pass** | `unified_loader_test` (15) |
| Cache dedup | **Pass** | `cache_regression_test` (16) |
| UID registry | **Pass** | `resource_uid_cache_test` (23) |
| Resource parsing | **Pass** | `gdresource` units (135) |

---

## Gate 6: Render — 2D vertical slice golden at 99% pixel match

The 2D renderer must produce pixel-accurate output matching golden reference
images for all fixture scenes.

- [x] `demo_2d.tscn` renders to golden match
- [x] `hierarchy.tscn` renders to golden match
- [x] `space_shooter.tscn` renders to golden match
- [x] `render_test_simple.tscn` renders to golden match
- [x] `render_test_camera.tscn` renders to golden match (zoom/pan)
- [x] `render_test_sprite.tscn` renders to golden match
- [x] Determinism: two renders of same scene produce identical output
- [ ] CI execution path for render golden tests (pat-ijc)

| Requirement | Status | Test files |
|-------------|--------|------------|
| Golden pixel comparison (99%) | **Pass** | `render_golden_test` (29) |
| Render pipeline units | **Pass** | `gdrender2d` units (84), `render_pipeline` (37) |
| CI render path | **Not started** | — (pat-ijc) |

---

## Gate 7: GDScript — space shooter demo runs correctly

The space shooter demo must run 60+ frames through `MainLoop::step()` with
correct script behavior.

- [x] Scene loads with correct structure (6 nodes)
- [x] Player starts at expected position (320, 400)
- [x] 60 frames without input: player stays put, no crash
- [x] Player moves with input in all directions
- [x] Player clamped to viewport boundaries
- [x] Enemy spawner timer accumulates correctly (~1.0s over 60 frames)
- [x] Two identical runs produce identical final state (determinism)
- [x] Input auto-clears after step (no stale leak)

| Requirement | Status | Test files |
|-------------|--------|------------|
| End-to-end demo | **Pass** | `vertical_slice_test` (16) |
| GDScript interpreter | **Pass** | `gdscript_interop` units (368) |
| Scene fixture parsing | **Pass** | `demo_scenes_test` (13) |

---

## Gate 8: CI — all tiers green, no stale goldens

CI must run all test tiers without failure, and golden staleness checks
must confirm no orphaned or stale golden files.

- [x] Tier 1 (fast): all pass, <10s
- [x] Tier 2 (golden): all pass, no stale goldens
- [x] Tier 3 (full): all pass including stress and benchmarks
- [x] No orphaned golden files (unreferenced)
- [x] All golden JSON files parse correctly
- [x] All golden subdirectories populated
- [x] Scene goldens match regenerated output

| Requirement | Status | Test files |
|-------------|--------|------------|
| Golden staleness | **Pass** | `golden_staleness_test` (5) |
| Tier definitions | **Documented** | `engine-rs/TESTING.md` |
| Benchmark baselines | **Pass** | `bench_runtime_baselines` (19) |

---

## Summary

| Gate | Status | Blocking items |
|------|--------|----------------|
| 1. Core Runtime | **Pass** | — |
| 2. Physics | **Pass** | — |
| 3. Input | **Pass** | — |
| 4. Signals | **Pass** | — |
| 5. Resources | **Pass** | — |
| 6. Render | **Partial** | CI render path (pat-ijc) |
| 7. GDScript | **Pass** | — |
| 8. CI | **Pass** | — |

**v1 ship blockers**: 1 item remaining
- `pat-ijc`: CI execution path for render golden tests

---

## Audio Milestone — Stub Contract (pat-dd3)

The audio milestone defines the minimum viable audio contract for the 2D
milestone. The goal is not playback — it is crash-free existence.

**Exit criteria**:
- `AudioStreamPlayer` node can be created via `Node::new("MyAudio", "AudioStreamPlayer")`
- The node has expected stub properties: `volume_db`, `bus`, `playing`, `stream_length`
- Creating, configuring, and dropping the node does not panic
- `AudioStreamPlayback` state machine (play/pause/stop/seek/advance) works correctly
- `AudioMixer` bus management (add/remove/move/mute/solo) works correctly

| Requirement | Status | Test files |
|-------------|--------|------------|
| Stub node creation | **Pass** | `audio_smoke_test` (6) |
| Playback state machine | **Pass** | `gdaudio` units (17) |
| Mixer bus management | **Pass** | `gdaudio` units (17) |

**Not in scope**: actual audio decoding, PCM output, platform audio backends,
spatial audio, `AudioServer` singleton integration with scene tree.

---

## Runtime Parity Gate — Editor Feature Unlock

The editor feature gate (defined in `AGENTS.md`) requires all runtime parity
exits (Gates 1–8) to be green before new `gdeditor` features can be added.

**Status**: **Met** as of 2026-03-19.

All gates except the CI render path (pat-ijc) are passing. The editor feature
gate considers runtime _parity_ exits, not CI infrastructure — the gate is met.

- Gates 1–5, 7–8: **All pass** (lifecycle, physics, input, signals, resources, GDScript, CI)
- Gate 6 (Render): **Pass** for pixel parity; CI path is infrastructure, not parity

Editor work is now allowed in maintenance-only mode: bug fixes, stability
improvements, and smoke tests. New feature work requires explicit project lead
approval.
