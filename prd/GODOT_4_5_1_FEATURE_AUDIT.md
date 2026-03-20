# Godot 4.5.1 Feature Audit

This audit answers one question:

Can Patina honestly claim the pinned `Godot 4.5.1-stable` target is complete enough to repin to `4.6.1`?

Short answer: close, but not yet fully proven.

The repo contains substantial functionality and a large test surface. Oracle property parity is now **90.5%** across the 9-scene fixture corpus (up from 37.4% after `class_defaults.rs` filtering, bare var sync, and `self.position.x` fixes). Seven of nine scenes are at 100% parity; remaining gaps are in `physics_playground` (66.7%) and `test_scripts` (80.0%). The 2D slice is strong but broader subsystems (platform, audio, editor) are still claimed or deferred.

## Current Source Of Truth

- Behavioral oracle target: pinned upstream Godot `4.5.1-stable`
- Scope source: `PORT_SCOPE.md`
- Status source: `COMPAT_MATRIX.md`, `COMPAT_DASHBOARD.md`
- Execution source: `prd/BEAD_EXECUTION_MAP.md`
- Remaining critical path beads:
  - `pat-i5c` runtime/frame semantics
  - `pat-gnt` generate upstream `frame_trace` golden
  - `pat-9j5` compare Patina frame traces to upstream

## Audit Rule

Subsystem labels are interpreted conservatively:

- `Ready`: enough measured evidence exists for the current scoped slice, and this subsystem should not block a repin.
- `Almost`: strong implementation and tests exist, but there is still a material gap between subsystem confidence and repo-level oracle confidence.
- `Not Ready`: the subsystem is either explicitly only claimed/deferred, or the current evidence is not strong enough to support a repin.

## Headline Finding

The main mismatch is this:

- `COMPAT_MATRIX.md` marks many subsystems as `Measured`
- the same file still reports `37.4%` overall oracle parity

That means Patina is best described as:

- strong and real in a measured 2D/runtime slice
- not yet broadly validated enough to claim the pinned `4.5.1` target is fully complete

## Subsystem Audit

| Subsystem | Repo claim | Evidence | Audit read | Repin status |
|---|---|---|---|---|
| Core runtime | Measured | `frame_processing_semantics_test.rs`, `frame_trace_test.rs`, `oracle_parity_test.rs`, `oracle_regression_test.rs` | Runtime semantics are implemented and tested, but repo-level oracle parity is still low. The runtime is measured in slices, not broadly proven. | Not Ready |
| Object model | Measured | `classdb_parity_test.rs`, `object_property_reflection_test.rs` | Reflection and ClassDB coverage exist, but broader property/default-value coverage still appears incomplete. | Almost |
| Scene system | Measured | `instancing_ownership_test.rs`, `packed_scene_edge_cases_test.rs`, `frame_processing_semantics_test.rs`, `trace_parity_test.rs`, `multi_scene_trace_parity_test.rs` | Real scene/instancing/trace behavior exists, but breadth outside the current fixture set is still under-validated. | Not Ready |
| Signals | Measured | `signal_dispatch_parity_test.rs`, `signal_trace_parity_test.rs` | Good targeted parity coverage, but not enough to offset the low end-to-end oracle score. | Almost |
| Notifications | Measured | `notification_coverage_test.rs`, `lifecycle_trace_parity_test.rs` | Lifecycle and notification ordering are covered, but still tied to the current fixture corpus. | Almost |
| Resources | Measured | `cache_regression_test.rs`, `unified_loader_test.rs`, `resource_uid_cache_test.rs` | Loader/cache/UID paths are real and tested; remaining risk is breadth and exact parity in edge cases. | Almost |
| Packed scenes / NodePath | Measured under scene system | `packed_scene_edge_cases_test.rs`, `instancing_ownership_test.rs`, `nodepath_resolution_test.rs` | Strong targeted coverage, but not enough evidence for broad Godot-complete behavior. | Almost |
| GDScript interop | Measured | `demo_scenes_test.rs`, `gdscript_v1_features_test.rs` | Interop is substantial, but script-visible frame evolution and property parity still depend on the unresolved oracle trace gap. | Not Ready |
| Trace parity | Measured | `trace_parity_test.rs`, `multi_scene_trace_parity_test.rs`, `frame_trace_test.rs` | The right test shape exists. The unresolved issue is upstream-backed frame trace generation and comparison. | Not Ready |
| Oracle parity | Measured | `oracle_parity_test.rs`, `oracle_regression_test.rs` | This is the controlling metric. Current repo docs still show `37.4%` overall parity. | Not Ready |
| 2D rendering | Measured | `render_golden_test.rs`, `render_draw_ordering_test.rs`, `render_camera_viewport_test.rs`, `render_sprite_property_test.rs`, `render_vertical_slice_test.rs` | Strong evidence for the current 2D slice. This is fixture-driven, not a claim of broad renderer completeness. | Ready for slice-scoped repin |
| 2D physics | Measured | `physics_integration_test.rs`, `platformer_test.rs`, `vertical_slice_test.rs` | Deterministic, tested vertical-slice physics exists. Remaining likely gaps are breadth, not absence. | Ready for slice-scoped repin |
| Input | Measured | `input_map_loading_test.rs`, `input_action_coverage_test.rs`, `platform_backend_test.rs` | Input map and snapshot routing look solid for current scope. | Ready for slice-scoped repin |
| Platform / windowing | Claimed | `window_lifecycle_test.rs`, `PLATFORM_ROADMAP.md` | Useful runtime/editor shell support exists, but the repo still classifies this as claimed rather than parity-measured. | Not Ready |
| Audio | Claimed | `gdaudio` crate tests, `AUDIO_MILESTONE.md` | Audio is a tested stub/utility layer, not a parity-complete subsystem. `cargo test -p gdaudio` currently passes 51 tests, but docs still say 17 stub tests. | Not Ready |
| Editor | Claimed | `editor_test.rs`, `editor_smoke_test.rs`, `gdeditor` unit tests | Large maintenance surface exists, but there is no editor parity target and the repo already states editor work is maintenance-only until runtime exits are met. | Not Ready |
| 3D runtime | Deferred | scope docs | Explicitly out of current milestone scope. | Deferred |

## What Is Actually Working

The following can be stated with confidence:

- Patina is not a stub project. Core runtime, scenes, resources, scripting, render, physics, and input all have substantial code and dedicated tests.
- The 2D slice is real:
  - scene loading
  - script execution
  - input mapping
  - deterministic 2D physics
  - render goldens
- Audio and editor are real codebases, but they are not parity-complete deliverables.

## Operational Readiness Notes

These items are not Godot feature subsystems, but they do affect whether a repin report will be believable.

| Area | Current read | What to fix before reporting a repin |
|---|---|---|
| CI | Mostly in better shape than the docs imply. The repo already has a render-goldens CI path, while some docs still describe that as missing. | Reconcile `EXIT_CRITERIA.md` and any remaining stale CI notes with the actual workflow. |
| Benchmarks | Benchmark tests and docs exist, but the committed artifact path described in docs is not clearly populated. | Either commit the expected benchmark result artifacts or narrow the docs to describe only the existing framework/tests. |
| Status docs | This is the main reporting risk. Audio counts are stale, and subsystem labels still read stronger than the repo-level oracle parity number supports. | Reconcile `COMPAT_MATRIX.md`, `COMPAT_DASHBOARD.md`, and bead state before claiming completion. |

## What Is Not Yet Proven

The following should not be claimed yet:

- full Godot `4.5.1` behavioral completion
- broad oracle-backed parity across the supported fixture corpus
- platform/windowing parity
- audio parity
- editor parity

## Repin Gate

Do not repin to `4.6.1` until the current pinned target is reconciled at the oracle layer.

Minimum acceptable gate:

1. `pat-i5c` is closed
2. `pat-gnt` is closed
3. `pat-9j5` is closed

Recommended gate:

1. `pat-i5c` is closed
2. `pat-gnt` is closed
3. `pat-9j5` is closed
4. `pat-b16` is closed
5. `pat-x8u` is closed

Why this gate:

- it closes frame/process semantics
- it makes upstream `frame_trace` artifacts real
- it compares Patina traces directly to upstream
- it reduces the risk that a version bump hides unresolved runtime-ordering bugs

## Required Reconciliation Before Any Repin

Before opening a `4.6.1` repin bead, do this audit pass:

1. Reconcile `br` state with code reality
   - close any bead that is actually complete
   - reopen or split any bead whose acceptance criteria are still not met

2. Reconcile docs with evidence
   - if a subsystem is only measured in a narrow slice, say that explicitly
   - update stale counts, especially audio

3. Reconcile oracle claims with artifacts
   - verify upstream-generated `frame_trace` artifacts exist
   - verify parity tests consume those artifacts
   - verify the published parity percentage reflects the current fixture corpus

## Recommendation

Patina should be described today as:

- a real, tested, Rust-native Godot-compatible runtime with a meaningful measured 2D slice
- not yet fully closed against the pinned `Godot 4.5.1` oracle target

The next move is not “add more beads” and not “repin now.”

The next move is:

1. finish the remaining oracle/runtime critical path
2. reconcile bead state with the docs and tests
3. then repin deliberately from `4.5.1` to `4.6.1`
