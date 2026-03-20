# Godot 4.6.1 Release-Delta Audit for Patina

Audit date: 2026-03-20
Audited release: Godot 4.6.1-stable (commit 14d19694e0c88a3f9e82d899a0400f27a24c176e)
Previous Patina pin: Godot 4.5.1-stable

This audit identifies behavior changes between 4.5.1 and 4.6.1 that affect
Patina's compatibility surface. It covers two release deltas:

- 4.5.1 -> 4.6.0 (feature release)
- 4.6.0 -> 4.6.1 (maintenance release)

## Summary

Godot 4.6.1 is a maintenance release with mostly bug fixes on top of 4.6.0.
The 4.6.0 release introduced significant changes to rendering, physics defaults
(Jolt), and internal optimizations. For Patina's current 2D runtime scope, most
changes are either irrelevant (3D, editor) or low-risk (internal optimizations).
A small number of changes require verification or action.

## Patina-Impacting Deltas

### HIGH IMPACT: Requires verification or action

| Change | PR | Category | Impact |
|---|---|---|---|
| NodePath hash function fix (similar paths no longer collide) | GH-115473 | Core (4.6.1) | Patina's NodePath hashing must match. Verify `nodepath_resolution_test` still passes against new oracle. |
| ClassDB class list sorting regression fix | GH-115923 | Core (4.6.1) | ClassDB iteration order may differ from 4.5.1. Verify `classdb_parity_test` against new oracle. |
| Fix non-tool script check when emitting signals | GH-104340 | Core (4.6.0) | Signal emission behavior for non-tool scripts changed. Verify `signal_dispatch_parity_test` and `signal_trace_parity_test`. |
| Fix rotation/scale order in `CanvasItem::draw_set_transform` | GH-111476 | 2D (4.6.0) | Transform order change affects 2D rendering. Verify render goldens. |
| Camera2D limit checks for inverted boundaries fix | GH-111651 | 2D (4.6.0) | Camera2D behavior change. Verify `render_camera_viewport_test`. |
| Camera2D accepts resets only after entering tree | GH-112810 | 2D (4.6.0) | Camera2D lifecycle ordering change. Verify scene tree lifecycle tests. |
| Don't redraw invisible CanvasItems | GH-90401 | Rendering (4.6.0) | 2D render optimization changes draw behavior. Verify render goldens show no regressions. |
| Fix resource shared when duplicating instanced scene | GH-64487 | Core (4.6.0) | Resource duplication semantics changed. Verify `instancing_ownership_test`. |
| Remove `load_steps` from resource_format_text | GH-103352 | Core (4.6.0) | .tscn parsing change. Verify scene loading still works (this affects resource format, not Patina's parser). |
| Fix Jolt transform updates sometimes discarded | GH-115364 | Physics (4.6.1) | Physics transform sync fix. Verify `physics_playground` oracle parity. |

### MEDIUM IMPACT: Should verify but low risk

| Change | PR | Category | Impact |
|---|---|---|---|
| Initialize Quaternion variant with identity | GH-84658 | Core (4.6.0) | Default Quaternion value change. Patina does not heavily use Quaternion in 2D scope, but class_defaults may be affected. |
| Add `change_scene_to_node()` | GH-85762 | Core (4.6.0) | New API. No impact unless oracle outputs reference it. |
| Add `DUPLICATE_INTERNAL_STATE` flag | GH-57121 | Core (4.6.0) | New duplication flag. No impact on existing behavior but may appear in class defaults. |
| Optimize GDScriptInstance::notification | GH-94118 | GDScript (4.6.0) | Performance optimization only, no behavior change. |
| Prevent shallow scripts from leaking into ResourceCache | GH-109345 | GDScript (4.6.0) | Resource cache behavior change. Verify `cache_regression_test`. |
| Elide unnecessary copies in CONSTRUCT_TYPED_* opcodes | GH-110717 | GDScript (4.6.0) | VM optimization. No behavior change expected. |
| Add draw_ellipse methods | GH-85080 | Rendering (4.6.0) | New API. No impact on existing behavior. |
| Fix modifier order in keycode string generation | GH-108260 | Input (4.6.0) | Input string representation change. Verify `input_map_loading_test` if action names depend on keycode strings. |
| Allow all gamepad devices for built-in ui_* actions | GH-110823 | Input (4.6.0) | Input action binding change. Verify `input_action_coverage_test`. |
| Jolt is now default for new projects | GH-105737 | Physics (4.6.0) | Default physics engine changed. Patina uses its own physics; only affects oracle if oracle was regenerated with Jolt defaults. |
| MultiMesh 2D physics interpolation added | GH-107666 | Physics (4.6.0) | New feature. No impact on existing behavior. |
| Fix bug in ManifoldBetweenTwoFaces | GH-110507 | Physics (4.6.0) | Physics collision fix. May affect physics_playground oracle values. |
| Supplement scene instantiation for Editable Children | GH-81530 | Core (4.6.0) | Scene instancing semantics change. Verify `packed_scene_edge_cases_test`. |

### LOW IMPACT: No Patina action expected

| Change | PR | Category | Reason |
|---|---|---|---|
| Tween.kill propagation to subtweens | GH-108227 | Animation (4.6.0) | Patina does not implement Tween. |
| AnimationPlayer emits animation_finished for every animation | GH-110508 | Animation (4.6.0) | AnimationPlayer not in Patina scope. |
| Audio: pause when game is paused | GH-104420 | Audio (4.6.0) | Audio is stub-only in Patina. |
| Audio: random pitch now in semitones | GH-103742 | Audio (4.6.0) | Audio is stub-only in Patina. |
| All 3D changes | Various | 3D | 3D is deferred scope. |
| All C# changes | Various | C# | C# not in Patina scope. |
| All editor-only changes | Various | Editor | Editor changes don't affect runtime behavior. |
| All platform-specific changes | Various | Platforms | Patina uses its own platform layer. |
| GDExtension: free script/extension instance before object deconstructing | GH-110907 | GDExtension (4.6.0) | Affects GDExtension lifecycle, not Patina runtime. |
| LSP changes | Various | GDScript | LSP is editor tooling, not runtime. |
| Rendering: all 3D-specific rendering changes | Various | Rendering | 3D renderer not in Patina scope. |

### CHANGES BETWEEN 4.6.0 AND 4.6.1 (Maintenance)

The 4.6.1 release is small (approximately 30 entries). Patina-relevant items:

1. **Core: NodePath hash fix** (GH-115473) - HIGH. Similar paths no longer hash identically.
2. **Core: ClassDB sorting fix** (GH-115923) - HIGH. Class iteration order restored.
3. **Physics: Jolt transform update fix** (GH-115364) - MEDIUM. Physics transform sync.
4. **Rendering: sky/volumetric fog fixes** (GH-115874, GH-116107) - NONE. 3D only.
5. **Rendering: MSAA sample selection** (GH-115124) - NONE. GPU pipeline only.
6. **GDScript: LSP fixes** (GH-115671, GH-115672) - NONE. Editor tooling only.
7. **Particles: revert curve range change** (GH-116140) - NONE. Particles not in scope.

## Action Items

1. **NodePath hashing**: Compare Patina's NodePath hash implementation against
   upstream 4.6.1. If Patina uses its own hash, ensure it does not produce
   collisions for similar paths. Run `nodepath_resolution_test` against
   regenerated oracle.

2. **ClassDB ordering**: Verify `classdb_parity_test` passes with regenerated
   oracle outputs. The sorting fix may change property enumeration order.

3. **2D transform order**: Verify render goldens are still correct. The
   `draw_set_transform` rotation/scale order fix (GH-111476) may change
   rendered output for scenes that use both rotation and scale.

4. **Camera2D lifecycle**: Verify Camera2D reset behavior in lifecycle tests.
   The "accepts resets only after entering tree" change (GH-112810) may affect
   camera initialization ordering.

5. **Signal emission for non-tool scripts**: Verify signal parity tests pass.
   The fix (GH-104340) changes when signals are emitted from non-tool scripts.

6. **Physics oracle values**: The Jolt default (GH-105737) and collision fix
   (GH-110507) may change physics_playground oracle outputs. Verify after
   oracle regeneration.

7. **Resource duplication**: Verify instancing/ownership tests against new oracle.
   Resource sharing behavior changed (GH-64487).

## Conclusion

The 4.5.1-to-4.6.1 delta is dominated by 3D, editor, and platform changes
that do not affect Patina's 2D runtime scope. The changes that do matter are:

- A small number of Core fixes (NodePath hash, ClassDB sort, resource duplication)
- 2D rendering fixes (transform order, Camera2D lifecycle, invisible CanvasItem optimization)
- Signal emission behavior for non-tool scripts
- Physics collision and transform fixes

All of these should be caught by oracle parity tests once P0 oracle regeneration
is complete. No code changes are required in Patina preemptively -- the correct
approach is to regenerate oracle outputs, run parity tests, and fix any
regressions that surface.
