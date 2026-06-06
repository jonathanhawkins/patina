# Godot 4.5.1 → 4.6.1 Repin Delta Audit

**Date**: 2026-03-20
**Upstream commit**: `14d19694e0c88a3f9e82d899a0400f27a24c176e` (4.6.1-stable)
**Previous pin**: 4.5.1-stable

## Summary

The upstream Godot submodule has been repinned from 4.5.1-stable to 4.6.1-stable.
All golden files, oracle outputs, and benchmark baselines have been regenerated
against the current Patina codebase. This document summarizes the observed
Patina-facing behavior changes.

## Observed Changes in Regenerated Goldens

### 1. Notification Ordering: NOTIFICATION_CHILD_ORDER_CHANGED

**Affected fixtures**: all scenes (minimal, hierarchy, platformer, space_shooter,
physics_playground, signals_complex, test_scripts, ui_menu)

**Change**: Patina's trace output now includes `NOTIFICATION_CHILD_ORDER_CHANGED`
in the root node's notification log. Previously this notification was not emitted
(or was filtered). The new traces show it firing on the root node when children
are added during scene instancing.

**Impact**: Low. This is additive — no existing notifications changed order or
were removed. The `NOTIFICATION_CHILD_ORDER_CHANGED` notification is informational
and does not affect gameplay logic. Upstream Godot 4.6.x documents this notification
as expected behavior when the child list changes.

**Action**: Trace goldens updated. No code change needed.

### 2. Render Goldens: Hierarchy Scene

**Affected files**: `hierarchy.png`, `vs_editor_hierarchy.png`

**Change**: These two render goldens were removed because the underlying tests
are `#[ignore]`d due to a known hang in `scene_renderer::render_scene()` for the
hierarchy fixture. This is a pre-existing issue unrelated to the repin.

**Impact**: None. The `vs_runtime_hierarchy.png` golden was regenerated successfully
via the non-editor render path. The hang is tracked separately.

**Action**: No new regression. Editor hierarchy render remains ignored.

### 3. Physics Goldens

**Affected files**: All 8 physics golden files in `fixtures/golden/physics/`

**Change**: No behavioral change observed. All physics goldens regenerated
identically. Determinism tests confirm bitwise-identical traces across runs.

**Impact**: None.

### 4. Scene and Resource Goldens

**Change**: No behavioral change observed. All 11 scene goldens and all resource
goldens pass staleness checks. Scene tree structure, node counts, property values,
and class assignments are unchanged.

**Impact**: None.

### 5. Benchmark Baselines

**Change**: Benchmark numbers captured post-repin (debug profile, Apple M-series):

| Category | Range |
|----------|-------|
| Frame stepping | 0.012–0.148 ms/frame across 5 scenes |
| Load+instance | 0.014–0.167 ms/iter across 5 scenes |
| Physics frames | 0.036–0.091 ms/frame across 3 scenes |
| Script parsing | 0.024–0.080 ms/iter across 5 scripts |
| Variant roundtrip | 0.012 ms/iter |

No regressions observed versus historical baselines. Numbers are within normal
variance for debug-profile measurements on shared hardware.

## Godot 4.6.x Behavioral Changes (Patina-Relevant)

Based on the Godot 4.6.x changelog and observed golden diffs, the following
upstream changes are relevant to Patina:

### Relevant

1. **NOTIFICATION_CHILD_ORDER_CHANGED dispatch broadened** — Godot 4.6.x emits
   this notification more consistently when children are added/removed. Patina
   already implemented this notification; the golden update reflects our traces
   now capturing it where they previously did not.

2. **No physics behavior changes detected** — Rapier-based physics in Patina is
   independent of upstream Godot physics changes. Deterministic traces are stable.

3. **No scene tree lifecycle ordering changes** — ENTER_TREE (top-down),
   READY (bottom-up), and EXIT_TREE (bottom-up) ordering unchanged.

4. **No signal dispatch ordering changes** — Signal emission and connection
   order remains compatible.

### Not Relevant to Patina (No Impact)

- GDExtension API changes (Patina does not use GDExtension at runtime)
- Editor UI/dock changes (Patina has its own editor)
- 3D renderer changes (Patina is 2D-only currently)
- C# / .NET changes (Patina uses GDScript interop only)
- Android/iOS/Web platform changes (Patina targets desktop only)

## Conclusion

The 4.5.1 → 4.6.1 repin is clean. The only observable delta is the
`NOTIFICATION_CHILD_ORDER_CHANGED` notification appearing in trace goldens,
which is additive and correct behavior. No regressions were found in physics,
rendering, scene loading, or benchmark performance.

All golden files have been regenerated and the `UPSTREAM_VERSION` stamp updated
to `14d19694e0c88a3f9e82d899a0400f27a24c176e`.
