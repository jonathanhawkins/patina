# Patina Bead Backlog

This backlog converts the current gap analysis into claim-ready beads with an explicit critical path.

The main conclusion is simple:

1. Build the oracle and GDExtension lab properly.
2. Close headless runtime behavioral parity.
3. Finish the measured 2D vertical slice.
4. Freeze later-phase drift until those exits are met.

Current status references:

- Overall oracle parity is still low at 32.2% in [COMPAT_DASHBOARD.md](/Users/bone/dev/games/patina/COMPAT_DASHBOARD.md).
- The v1 target remains 95%+ fixture parity in [PORT_SCOPE.md](/Users/bone/dev/games/patina/PORT_SCOPE.md).
- The 2D slice, platform, and editor are still marked partial/in progress in [COMPAT_MATRIX.md](/Users/bone/dev/games/patina/COMPAT_MATRIX.md).

## Critical Path

`B001 -> B002 -> B003 -> B004 -> B005/B006/B007 -> B008/B009 -> B010/B011/B012 -> B013`

## Beads

### B001 - Pin upstream Godot and record the oracle version

Depends on: none

Why it matters:
The plan and oracle spec require a pinned upstream source of truth. The repo currently has no `upstream/` checkout, no `.gitmodules`, and `TEST_ORACLE.md` still contains placeholder pin fields.

Likely file targets:

- `.gitmodules`
- `upstream/godot/`
- [TEST_ORACLE.md](/Users/bone/dev/games/patina/TEST_ORACLE.md)
- [README.md](/Users/bone/dev/games/patina/README.md)

Acceptance criteria:

- Upstream Godot exists as a pinned submodule.
- Pinned version, commit, and pin date are recorded in `TEST_ORACLE.md`.
- Local setup docs explain how to sync and update the pin.
- Golden outputs are explicitly tied to that pin.

### B002 - Create `tools/oracle/` capture tooling

Depends on: `B001`

Why it matters:
The oracle spec calls for scene/property/signal/notification/resource/render/physics capture tools. Today `tools/` only contains an ad hoc comparison shell script.

Likely file targets:

- `tools/oracle/scene_tree_dumper.*`
- `tools/oracle/property_dumper.*`
- `tools/oracle/signal_tracer.*`
- `tools/oracle/notification_tracer.*`
- `tools/oracle/resource_roundtrip.*`
- `tools/oracle/render_capture.*`
- `tools/oracle/physics_tracer.*`
- `tools/oracle/run_fixture.*`

Acceptance criteria:

- Each capture type in `TEST_ORACLE.md` has a concrete tool entrypoint.
- Tools emit the documented golden envelope.
- A single command can generate goldens for at least one scene fixture and one resource fixture.

### B003 - Stand up `apps/godot/` as the GDExtension compatibility lab

Depends on: `B001`

Why it matters:
Phase 2 requires Rust running inside real Godot to validate lifecycle, signal, resource, and API assumptions before deeper runtime replacement.

Likely file targets:

- `apps/godot/project.godot`
- `apps/godot/Cargo.toml`
- `apps/godot/src/lib.rs`
- `apps/godot/src/scene_probe.rs`
- `apps/godot/src/resource_probe.rs`
- `apps/godot/src/signal_probe.rs`
- `apps/godot/src/api_probe.rs`

Acceptance criteria:

- `apps/godot/` builds as a GDExtension project.
- Godot can load it.
- Smoke probes emit machine-readable outputs for scene tree, property snapshots, and signal order.

### B004 - Replace manual parity plumbing with generated oracle artifacts

Depends on: `B002`, `B003`

Why it matters:
Current parity coverage still relies on embedded fixtures and normalization-heavy comparisons instead of a reproducible upstream-backed pipeline.

Likely file targets:

- [engine-rs/tests/oracle_parity_test.rs](/Users/bone/dev/games/patina/engine-rs/tests/oracle_parity_test.rs)
- [engine-rs/tests/oracle_regression_test.rs](/Users/bone/dev/games/patina/engine-rs/tests/oracle_regression_test.rs)
- `fixtures/golden/scenes/*`
- `fixtures/golden/signals/*`
- `fixtures/golden/properties/*`
- `fixtures/golden/resources/*`
- `fixtures/golden/render/*`
- `fixtures/golden/physics/*`

Acceptance criteria:

- Tests consume generated goldens from `fixtures/golden/`.
- Each test states the observable behavior it verifies.
- Tests fail clearly when goldens are stale or missing.

### B005 - Replace closure-only signals with scene-aware dispatch

Depends on: `B004`

Why it matters:
Signals are still one of the weakest measured areas. Script-created connections currently record too little runtime information to invoke target methods robustly.

Likely file targets:

- [engine-rs/crates/gdobject/src/signal.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdobject/src/signal.rs)
- [engine-rs/crates/gdscene/src/scene_tree.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdscene/src/scene_tree.rs)
- [engine-rs/crates/gdscene/src/scripting.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdscene/src/scripting.rs)
- [engine-rs/crates/gdscene/src/packed_scene.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdscene/src/packed_scene.rs)

Acceptance criteria:

- Scene-connected signals invoke target node/script methods in registration order.
- Cross-node `connect()` and `emit_signal()` work without closure hacks.
- Tests cover duplicate connections, disconnect, argument passing, and lifecycle-triggered emissions.

### B006 - Add a global lifecycle and signal ordering trace

Depends on: `B004`

Why it matters:
The runtime needs a single ordered event stream for enter/ready/process/physics/exit plus signal emissions. Current tests infer order indirectly.

Likely file targets:

- [engine-rs/crates/gdscene/src/lifecycle.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdscene/src/lifecycle.rs)
- [engine-rs/crates/gdscene/src/scene_tree.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdscene/src/scene_tree.rs)
- [engine-rs/crates/gdobject/src/notification.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdobject/src/notification.rs)
- [engine-rs/tests/oracle_regression_test.rs](/Users/bone/dev/games/patina/engine-rs/tests/oracle_regression_test.rs)

Acceptance criteria:

- Runtime captures a total ordered trace for notifications, script callbacks, and signal emissions.
- Oracle tests compare that trace directly.

### B007 - Make frame processing semantics match Godot contracts

Depends on: `B004`

Why it matters:
`SceneTree::process_frame()` is still explicitly a simplified stub, and the current loop semantics drift from Godot on accumulated script state.

Likely file targets:

- [engine-rs/crates/gdscene/src/scene_tree.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdscene/src/scene_tree.rs#L651)
- [engine-rs/crates/gdscene/src/main_loop.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdscene/src/main_loop.rs)
- [engine-rs/crates/patina-runner/src/main.rs](/Users/bone/dev/games/patina/engine-rs/crates/patina-runner/src/main.rs)
- [engine-rs/tests/project_loading_test.rs](/Users/bone/dev/games/patina/engine-rs/tests/project_loading_test.rs)

Acceptance criteria:

- Fixed-timestep physics, process ordering, pause behavior, and script sequencing are specified and tested.
- Oracle fixtures validate frame-by-frame property evolution.

### B008 - Fix script variable sync and cross-node resolution

Depends on: `B005`, `B006`, `B007`

Why it matters:
Cross-node access and script state sync are still called out as known gaps in the dashboard.

Likely file targets:

- [engine-rs/crates/gdscene/src/scene_tree.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdscene/src/scene_tree.rs)
- [engine-rs/crates/gdscene/src/scripting.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdscene/src/scripting.rs)
- [engine-rs/crates/gdscript-interop/src/interpreter.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdscript-interop/src/interpreter.rs)
- [engine-rs/crates/gdscript-interop/src/bindings.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdscript-interop/src/bindings.rs)

Acceptance criteria:

- Script-to-node sync uses explicit live values.
- `get_node`, `get_parent`, sibling and relative traversal use one consistent resolution model.
- Fixtures cover parent, child, sibling, absolute, relative, and missing-path behavior.

### B009 - Integrate resource UID and cache behavior into real loading paths

Depends on: `B004`

Why it matters:
The code has UID and cache primitives, but runtime loading does not yet behave like one coherent Godot resource system.

Likely file targets:

- [engine-rs/crates/gdresource/src/loader.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdresource/src/loader.rs)
- [engine-rs/crates/gdresource/src/uid.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdresource/src/uid.rs)
- [engine-rs/crates/gdresource/src/cache.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdresource/src/cache.rs)
- [engine-rs/crates/gdresource/src/project.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdresource/src/project.rs)
- [engine-rs/crates/gdscene/src/packed_scene.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdscene/src/packed_scene.rs)

Acceptance criteria:

- Loading by `res://` path and UID resolve consistently.
- Ext-resource and subresource references round-trip.
- Integration tests prove deduplication and stable identities across repeated loads.

### B010 - Wire the 2D renderer into scene-driven fixtures

Depends on: `B004`, `B007`

Why it matters:
Rendering exists as an isolated software renderer, but the roadmap requires scene-driven, oracle-measured output.

Likely file targets:

- [engine-rs/crates/gdrender2d/src/renderer.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdrender2d/src/renderer.rs)
- [engine-rs/crates/gdserver2d/src/server.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdserver2d/src/server.rs)
- [engine-rs/tests/demo_2d_test.rs](/Users/bone/dev/games/patina/engine-rs/tests/demo_2d_test.rs)
- [engine-rs/tests/platformer_test.rs](/Users/bone/dev/games/patina/engine-rs/tests/platformer_test.rs)
- `fixtures/golden/render/*`

Acceptance criteria:

- At least one `.tscn` scene produces golden-compared render output.
- Camera, transforms, layer ordering, texture draws, and visibility are validated end-to-end.

### B011 - Connect physics to scene nodes and fixed-step lifecycle

Depends on: `B007`

Why it matters:
`gdphysics2d` has local simulation logic, but it is not yet integrated as a Godot-like scene/runtime subsystem.

Likely file targets:

- [engine-rs/crates/gdphysics2d/src/world.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdphysics2d/src/world.rs)
- [engine-rs/crates/gdscene/src/node2d.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdscene/src/node2d.rs)
- [engine-rs/crates/gdscene/src/scene_tree.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdscene/src/scene_tree.rs)
- [engine-rs/tests/platformer_test.rs](/Users/bone/dev/games/patina/engine-rs/tests/platformer_test.rs)
- `fixtures/golden/physics/*`

Acceptance criteria:

- Basic body nodes advance through a fixed timestep from the scene runtime.
- Node transforms and physics body positions stay synchronized.
- A deterministic physics trace fixture compares against golden output.

### B012 - Replace demo-local input and loop orchestration with engine-owned runtime flow

Depends on: `B007`, `B010`, `B011`

Why it matters:
The examples currently carry too much runtime orchestration. The engine needs one owned flow for input, process, physics, and render.

Likely file targets:

- [engine-rs/crates/gdplatform/src/input.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdplatform/src/input.rs)
- [engine-rs/crates/gdplatform/src/display.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdplatform/src/display.rs)
- [engine-rs/crates/gdplatform/src/winit_backend.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdplatform/src/winit_backend.rs)
- [engine-rs/examples/platformer_demo.rs](/Users/bone/dev/games/patina/engine-rs/examples/platformer_demo.rs)
- [engine-rs/examples/space_shooter_live.rs](/Users/bone/dev/games/patina/engine-rs/examples/space_shooter_live.rs)

Acceptance criteria:

- Runtime ingests input and exposes it through a stable engine-facing API.
- Example/demo code no longer recreates the core loop contract.
- Keyboard and mouse basics are covered by integration tests.

### B013 - Add measured 2D vertical-slice fixtures and update status docs from evidence

Depends on: `B010`, `B011`, `B012`

Why it matters:
The 2D slice is not complete until it is measured. Demos are not parity.

Likely file targets:

- `fixtures/scenes/*`
- `fixtures/golden/render/*`
- `fixtures/golden/physics/*`
- [COMPAT_DASHBOARD.md](/Users/bone/dev/games/patina/COMPAT_DASHBOARD.md)
- [COMPAT_MATRIX.md](/Users/bone/dev/games/patina/COMPAT_MATRIX.md)

Acceptance criteria:

- One representative 2D fixture set runs end-to-end.
- Render outputs and physics traces compare to golden outputs with documented thresholds.
- Dashboard and matrix report measured status instead of aspiration.

## Deferred / Freeze

### B014 - Freeze new editor feature expansion until runtime parity exits are met

Why it matters:
The repo already contains substantial editor/server work, but the project rules explicitly say editor work should wait until runtime milestones are stable.

Likely file targets:

- [engine-rs/crates/gdeditor/src/lib.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdeditor/src/lib.rs)
- [engine-rs/tests/editor_test.rs](/Users/bone/dev/games/patina/engine-rs/tests/editor_test.rs)

Acceptance criteria:

- New `gdeditor` feature work is blocked behind runtime and 2D parity exits.
- Only compatibility-preserving maintenance continues.

### B015 - Stop counting 3D-adjacent scaffolding as 2D milestone progress

Why it matters:
There is already 3D-adjacent surface area in crates named for 2D work. That increases maintenance surface before the 2D slice is closed.

Likely file targets:

- [engine-rs/crates/gdserver2d/src/lib.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdserver2d/src/lib.rs)
- [engine-rs/crates/gdphysics2d/src/lib.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdphysics2d/src/lib.rs)
- milestone and compatibility docs

Acceptance criteria:

- 2D milestone reporting excludes unfinished 3D scaffolding.
- Deferred 3D work is labeled clearly as Phase 6+.

### B016 - Reframe examples as fixture feeders, not proof of completion

Why it matters:
Examples should feed compatibility work, not act as substitutes for it.

Likely file targets:

- [engine-rs/examples/platformer_demo.rs](/Users/bone/dev/games/patina/engine-rs/examples/platformer_demo.rs)
- [engine-rs/examples/space_shooter_live.rs](/Users/bone/dev/games/patina/engine-rs/examples/space_shooter_live.rs)
- [engine-rs/examples/editor.rs](/Users/bone/dev/games/patina/engine-rs/examples/editor.rs)

Acceptance criteria:

- Each example maps to a measurable fixture or compatibility target.
- Example-specific orchestration is reduced where it duplicates engine runtime responsibilities.

## Immediate Start Recommendation

Claim these first:

1. `B002` if the upstream pin is already locally available.
2. Otherwise `B003` for oracle-runner cleanup and reproducibility work that can start now.
3. Then `B004` only after generated artifacts exist.
