# Godot 4.6.1 Release Delta Audit

**Scope**: Changes from Godot `4.5.1-stable` to `4.6.1-stable` that affect Patina's implemented subsystems.
**Patina pin status**: Moving from `4.5.1-stable` to `4.6.1-stable`.
**Patina oracle parity at repin**: 90.5% (57/63 comparisons) across 9 fixture scenes.
**Date**: 2026-03-20

This audit answers: *What did Godot change in 4.6.x that Patina must react to, and how urgent is each item?*

It is not a copy of Godot release notes. Each entry is filtered against Patina's implemented subsystem scope and rated for impact.

---

## How to Read This Audit

**Category**:
- `breaking` — changes observable behavior in ways that will break existing oracle-backed tests if unaddressed
- `behavioral-change` — changes observable behavior but does not necessarily break existing tests (e.g., a new signal emission, corrected timing)
- `new-api` — a new method, property, or class that Patina does not yet need to implement for current parity but may need for future coverage
- `cosmetic` — editor/tooling/display changes, zero Patina runtime impact

**Patina impact**:
- `needs-fix` — Patina must change to maintain oracle parity or avoid regressing tests
- `already-compatible` — Patina's current implementation is consistent with the 4.6 behavior
- `not-yet-implemented` — Patina does not implement this subsystem/feature yet; no regression risk, but a coverage gap
- `monitor` — likely compatible today; re-check when related fixtures are broadened

---

## Subsystem Audit Table

| # | Delta | Subsystem | Category | Patina Impact | Action |
|---|-------|-----------|----------|---------------|--------|
| 1 | `SceneTree::change_scene_to_node()` added | Scene system | new-api | not-yet-implemented | Add to scope backlog when scene-switching fixtures are created |
| 2 | NodePath hash function bug fixed (identical paths could produce different hashes) | Core / NodePath | behavioral-change | already-compatible | Patina's `NodePath` uses Rust's derived `Hash` on the parsed struct fields; correct by construction |
| 3 | `AnimationPlayer` now emits `animation_finished` for **every** animation end, including looping | Animation / Signals | behavioral-change | monitor | Patina's `AnimationPlayer` does not yet emit `animation_finished`; no existing test depends on it, so no regression now, but a gap to close before animation signal fixtures are added |
| 4 | `Camera2D` resets are now accepted only **after** entering the tree | Scene / Lifecycle | behavioral-change | not-yet-implemented | `Camera2D` is not in Patina's active 2D slice; no risk to current tests |
| 5 | `Quaternion` now correctly initializes to identity under `Variant` default (was undefined) | Variant / Math | breaking | already-compatible | Patina's `Quaternion` is stored as a typed `Variant` arm; default is not relied upon by any current oracle test. No action needed. |
| 6 | Signals with underscore prefix are hidden from autocomplete and docs (no runtime change) | Signals | cosmetic | already-compatible | This is a tooling/documentation change only; Patina's signal dispatch is unaffected |
| 7 | `StateMachinePlayback` sets Start state as default in constructor | Animation / State | behavioral-change | not-yet-implemented | AnimationTree/StateMachine is not in Patina's scope; skip |
| 8 | `ClassDB` class list sorting regression fixed (4.6.1 patch) | Object model | behavioral-change | needs-fix | Patina's `ClassDB` must return class names in stable sorted order. The `classdb_parity_test` covers this path. Verify sort is deterministic and consistent with 4.6.1. |
| 9 | `Geometry2D` arc tolerance scaling removed; arc subdivisions now fixed-count | 2D Physics / Math | behavioral-change | monitor | Patina uses Rapier for physics, not Godot's `Geometry2D`; only relevant if Patina exposes `Geometry2D` script API directly |
| 10 | Ghost collision fix in `Geometry2D` segment intersection | 2D Physics | behavioral-change | monitor | Same as above; Rapier-based physics is not affected, but note if `move_and_collide` edge cases appear |
| 11 | `AnimationLibrary` serialization format changed (no longer uses raw Dictionary encoding) | Resources | behavioral-change | not-yet-implemented | Patina does not parse `AnimationLibrary` resources yet; no impact to current fixtures |
| 12 | Resource sharing corrected when duplicating instanced scenes | Scene / Resources | behavioral-change | monitor | Patina's `packed_scene_edge_cases_test` covers instanced scene duplication; verify no regression when golden outputs are regenerated against 4.6.1 |
| 13 | `Control` mouse focus and keyboard focus are now decoupled (separate styling) | GUI / Control | behavioral-change | not-yet-implemented | Control nodes are not in Patina's 2D measured slice; skip |
| 14 | `Control::pivot_offset_ratio` property added | GUI / Control | new-api | not-yet-implemented | Skip until GUI/Control scope is opened |
| 15 | Glow blending now occurs before tonemapping; default glow blend mode changed to `screen` | 3D Rendering | behavioral-change | not-yet-implemented | 3D rendering is deferred; skip |
| 16 | Jolt Physics becomes the default for **new** 3D projects | 3D Physics | behavioral-change | not-yet-implemented | 3D is deferred; existing projects are unaffected; skip |
| 17 | `Object::script` member removed from internal API | Object model | breaking | already-compatible | Patina stores scripts in a separate `HashMap` keyed by `NodeId`, not as an object member. Already architecturally decoupled. |
| 18 | `NavigationServer` gains a `Dummy` backend to disable navigation | Navigation | new-api | not-yet-implemented | Navigation is not in Patina's current scope |
| 19 | Multi-threaded node processing (`_process_groups_thread`) added to SceneTree | Scene system | new-api | not-yet-implemented | Patina processes nodes single-threaded; no regression. This is a gap for future threading work. |
| 20 | NodePath `EditorProperty` used incorrect scene root (4.6.1 patch fix) | Editor | behavioral-change | not-yet-implemented | Editor is maintenance-only until runtime parity exits clear |
| 21 | Unique node IDs (persistent internal identifiers) added to Node | Scene system | new-api | monitor | Patina uses `NodeId` (ObjectId-backed u64) already. Verify semantics match Godot's `get_instance_id()` contract in tests. |
| 22 | `change_scene_to_node()` validation: node must not already be in the tree | Scene system | behavioral-change | not-yet-implemented | No `change_scene_to_node` implementation in Patina yet |
| 23 | GDExtension API parameters can now be marked `required` (nullable prevention) | GDExtension | new-api | not-yet-implemented | Patina does not use GDExtension at runtime |
| 24 | `AtlasTexture` size is now rounded consistently | Resources / Rendering | behavioral-change | not-yet-implemented | AtlasTexture is not in Patina's current scope |
| 25 | Script creation dialog used wrong base type (4.6.1 patch fix) | Editor | behavioral-change | not-yet-implemented | Editor-only; skip for now |

---

## Priority-Ordered Action List

### Immediate (before first 4.6.1 oracle run)

**1. ClassDB sort order (row 8)**

Patina's `ClassDB::class_list()` must return classes in consistent sorted order. Godot 4.6.1 fixed a regression in class list sorting. If Patina's oracle goldens are regenerated against 4.6.1, the class list comparison in `classdb_parity_test` will use the corrected order. Patina already sorts class names alphabetically in the test fixture, but verify the runtime path (`ClassDB::class_list`) also returns a deterministic sorted vec before running the oracle update.

**Action**: Confirm `ClassDB::class_list()` is sorted. Add or harden a unit test that asserts the returned vec is in lexicographic order. This is low-risk and a single-function check.

### Deferred (before animation signal fixtures are added)

**2. `AnimationPlayer::animation_finished` signal (row 3)**

Godot 4.6 ensures `animation_finished` fires for every animation completion including looping modes. Patina's `AnimationPlayer` does not currently emit this signal. No existing test depends on it, so there is no regression today. However, any future oracle fixture that exercises animation playback will see a gap here.

**Action**: When animation signal fixtures are created (not now), implement `animation_finished` emission in `AnimationPlayer::advance()` and verify against 4.6.1 oracle output.

### Monitor (no action now, review when fixtures expand)

**3. Instanced scene resource sharing (row 12)**

The fix to resource sharing when duplicating instanced scenes is subtle. Patina's `packed_scene_edge_cases_test` covers some of this ground. When oracle goldens are regenerated against 4.6.1, run this test suite first to confirm no regression.

**4. Unique node ID semantics (row 21)**

Patina's `NodeId` is already an internal unique identifier (u64). Verify that when the 4.6.1 oracle exposes `get_instance_id()` in any fixture, Patina's returned values are structurally compatible (type, not value — instance IDs are not expected to match, only the shape/type).

---

## Out-of-Scope Changes (Confirmed Skip)

The following 4.6.x changes are explicitly outside Patina's current milestone scope. They are listed here to document the decision, not as open items.

| Change | Why skipped |
|--------|-------------|
| Jolt Physics as 3D default | 3D deferred |
| Screen Space Reflection overhaul | 3D rendering deferred |
| IK framework (8 new SkeletonModifier3D classes) | 3D animation deferred |
| AgX tonemapper parameters | 3D rendering deferred |
| Octahedral probe maps | 3D rendering deferred |
| Android/XR platform changes | Platform not in scope |
| Delta-encoded patch PCKs | Export tooling not in scope |
| Tracy/Perfetto profiler integration | Profiler tooling not in scope |
| C# translation parser | C# not in scope |
| LSP BBCode-to-Markdown improvements | Tooling only |
| Editor theme (Modern default) | Editor maintenance-only |
| Movable editor docks | Editor maintenance-only |
| GridMap Bresenham painting | TileMap/GridMap not in scope |
| Advanced joypad LED/haptic API | Platform not in scope |
| Faster GPU texture import | Import pipeline not in scope |

---

## Oracle Regeneration Notes

When the upstream submodule is repinned to `4.6.1-stable` and golden outputs are regenerated:

1. **Run `classdb_parity_test` first** — the ClassDB sort fix (row 8) means the expected class list may be in a different order against 4.6.1 than against 4.5.1. This test should pass without changes if Patina's sort is already correct, or reveal a gap if not.

2. **Run `packed_scene_edge_cases_test` and `instancing_ownership_test`** — the instanced scene resource sharing fix (row 12) may produce marginally different property values in edge cases. Compare carefully.

3. **Run `oracle_parity_test` and `oracle_regression_test`** against the new 4.6.1 golden set. Expect the 9-scene fixture corpus to continue passing at or above 90.5%. Any new failures should be isolated to the behavioral changes documented above.

4. **No new golden fixtures need to be created as part of this repin** — the existing 9-scene corpus exercises the implemented subsystems. New fixtures (animation signals, Camera2D, Control) can be deferred until those subsystems move from `not-yet-implemented` to `Measured`.

---

## Summary Assessment

| Category | Count | Notes |
|----------|-------|-------|
| Needs-fix items | 1 | ClassDB sort order (low risk, single function) |
| Already-compatible | 5 | Object::script removal, Quaternion default, NodePath hash, underscore signals, signal decoupling |
| Monitor (re-check after golden regen) | 4 | Geometry2D ghost collision, instanced scene resources, AnimationPlayer signal, unique node IDs |
| Not-yet-implemented (skip now) | 15 | All 3D, GUI/Control, AnimationLibrary, Camera2D, navigation, threads |

**Headline conclusion**: Godot 4.6.1 introduces no breaking changes to Patina's currently-measured subsystem slice. The one `needs-fix` item (ClassDB sort order) is a low-risk single-function verification. The repin is safe to proceed once that check passes and oracle goldens are regenerated.
