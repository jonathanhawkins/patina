# COMPAT_MATRIX.md - Compatibility Status Tracking

This document tracks the implementation and compatibility status of each Patina Engine subsystem relative to upstream Godot behavior.

**Last updated**: 2026-07-12 (fresh-eyes source audit — see warning below)

---

## ⚠️ Fresh-Eyes Audit 2026-07-11 — READ FIRST

A 15-subsystem code audit against the vendored Godot 4.6.1 source
(**[docs/PARITY_AUDIT_2026-07-11.md](docs/PARITY_AUDIT_2026-07-11.md)**, epic `pat-u3l8c`,
117 beads) found that the per-subsystem "Measured / ~N%" rows below systematically overstate
runtime parity. Unit-test counts measure **library code in isolation**; `MainLoop::step`
wires in only the 2D physics server. Concretely, at runtime today:

- **3D physics is never stepped** (a RigidBody3D never falls in a running game).
- **Audio plays through the cpal output** — `gdscene::audio_server_bridge` pumps `AudioServer::mix` (decode → mixer → effects → bus-send routing → device), so the earlier "nothing pumps `mix()`" claim is stale. The remaining audio gaps are that **GDScript cannot yet drive `AudioServer`** (no interpreter singleton for `set_bus_volume_db` etc.) and `default_bus_layout.tres` is not auto-loaded at boot.
- **Scene nodes emit no 2D draw commands** (Play renders class-glyph icons, not sprites);
  rotated sprites rasterize as AABBs; TileMaps render as a dot.
- **3D materials never sample textures; no skinning** reaches either renderer.
- **Custom `.gdshader` neither loads nor executes** on any path.
- **2D physics has no gravity** and no rotated-shape collision.
- **Navigation/particles/AnimationTree/GUI layout/PathFollow run only in their own tests.**
- **No GUI control is functional** (no draw-from-tree, input, signals, TTF fonts).
- **Real exported Godot projects cannot load** (binary `.res` stub, PCK layout mismatch,
  no `.ctex`, wrong UID codec).
- **The browser target is structurally absent** (no wasm build anywhere; the "browser demo"
  is BMP frames streamed from a native server to an `<img>` tag).
- **GDScript rejects `const`, `*=`, `is`, `as`, `assert`**; match arms bind instead of compare.

A subsystem may only claim runtime parity when an **app-level test drives it through
`MainLoop::step` / `patina-runner` on a `.tscn` scene**. The rows below remain accurate as
*library/unit-test inventories* until re-scored per this bar.

---

## Status Definitions

| Status | Meaning |
|--------|---------|
| **Measured** | Automated tests compare Patina output against Godot oracle fixtures or deterministic goldens. Test files are cited. **Caveat (2026-07-11 audit): unless the tests are app-level, this measures the library, not the running engine — see warning above.** |
| **Claimed** | Code exists and appears to work, but no dedicated parity test proves it matches Godot behavior. |
| **Deferred** | Not started or explicitly out of scope for the current milestone. |

---

## Compatibility Matrix

> Each row cites the specific test files that prove the subsystem's status. A subsystem is "Measured" only if those tests exist and pass. "Claimed" means code is present but lacks parity evidence.

| Subsystem | Crate | Status | Tests | Goldens | Parity | Test files |
|-----------|-------|--------|-------|---------|--------|------------|
| Core Runtime | `gdcore` | **Measured** | 828 | — | ~100% | `gdcore` unit tests (math, IDs, strings, compare3d, perf_comparison, crash_triage, memory_profiler) |
| Variant System | `gdvariant` | **Measured** | 131 | — | ~100% | `gdvariant` unit tests (enum, conversion, serialization, fuzz) |
| Object Model | `gdobject` | **Measured** | 75 + integration | — | ~80% | `gdobject` units (75), `object_property_reflection_test`, `classdb_parity_test` |
| Signals | `gdobject` | **Measured** | included in gdobject | — | ~60% | `signal_dispatch_parity_test`, `signal_trace_parity_test` |
| Notifications | `gdobject` | **Measured** | included in gdobject | — | ~85% | `notification_coverage_test`, `lifecycle_trace_parity_test` |
| Resources | `gdresource` | **Measured** | 319 + integration | 5 | ~95% | `gdresource` units (319), `cache_regression_test`, `unified_loader_test`, `resource_uid_cache_test` |
| Scene System | `gdscene` | **Measured** | 959 + integration | 37 | ~90% | `gdscene` units (959), `golden_tests`, `instancing_ownership_test`, `packed_scene_edge_cases_test`, `frame_processing_semantics_test` |
| GDScript Interop | `gdscript-interop` | **Measured** | 464 + integration | — | ~85% | `gdscript_interop` units (464), `demo_scenes_test` |
| Trace Parity | `gdscene` | **Measured** | integration | 23 | — | `trace_parity_test`, `multi_scene_trace_parity_test`, `frame_trace_test` |
| Oracle Parity | `gdscene` | **Measured** | integration | 119 oracle outputs | 100% | `oracle_parity_test`, `oracle_regression_test` — measured against Godot 4.6.1 |
| 2D Rendering | `gdrender2d` | **Measured** | 168 + integration | 64 | Golden-based | `gdrender2d` units (168), `render_pipeline`, `render_golden_test` |
| 2D Rendering Server | `gdserver2d` | **Measured** | 108 | — | Measured | `gdserver2d` units (108) — draw ordering, canvas, sprite server |
| 2D Physics | `gdphysics2d` | **Measured** | 111 + integration | 17 | Deterministic | `gdphysics2d` units (111), `physics_integration_test` |
| Input / Platform Stable Layer | `gdplatform` | **Measured** | 504 + integration | — | Measured for bounded slice | `gdplatform` units (504) — input, timers, OS info, target metadata, export config; `platform_first_stable_layer_test`, `platform_targets_validation_test`, `ci_build_matrix_platform_test`, `startup_runtime_packaging_flow_test` |
| 2D Vertical Slice | all | **Measured** | integration | — | End-to-end | `vertical_slice_test`: scene→scripts→input→physics→process→render |
| Audio | `gdaudio` | **Claimed** | 100+ | — | Partial | Platform output (cpal, pumped by `audio_server_bridge`), WAV/OGG/**MP3** decode, biquad filter / compressor / limiter / spectrum effect chain, bus volume/mute/solo, **bus-to-bus send routing in `mix()`**, and **bus-layout load** (`load_bus_layout_str`). Remaining: the script-facing `AudioServer` singleton (GDScript `set_bus_volume_db` etc.), boot-time `default_bus_layout` auto-load, streaming, and Godot oracle parity. See `AUDIO_MILESTONE.md` + [scorecard](prd/PORT_1TO1_SCORECARD_2026-07-11.md). |
| Editor-Facing Compatibility Layer | `gdeditor` | **Measured** | 1963 + integration | — | Measured for bounded slice | `gdeditor` units (1963), `editor_smoke_test`, `editor_461_revalidation_test`, `editor_interface_compat_test`, `editor_menu_parity_test`, `editor_systems_parity_test` — browser editor shell, compatibility layer, selected tooling slices |
| 3D Rendering | `gdrender3d` | **Measured** | 95 + integration | 64 render | Golden-based | `gdrender3d` units (95), `render_3d_parity_test`, `comparison_tooling_3d_test` — software renderer, framebuffer comparison, diff imaging |
| 3D Rendering Server | `gdserver3d` | **Measured** | 338 | — | Measured | `gdserver3d` units (338) — mesh, camera, lighting, viewport, material |
| 3D Physics | `gdphysics3d` | **Measured** | 124 + integration | — | Deterministic | `gdphysics3d` units (124) — rigid body, collision, trace comparison |
| 3D Comparison Tooling | `gdcore` | **Measured** | 44 | — | Tooling | `compare3d` units (34), `comparison_tooling_3d_test` (10) — scene tree, physics trace, render, unified parity reports |
| Runner | `patina-runner` | **Measured** | 80 | — | N/A | `patina-runner` unit tests (CLI runner, project loading, scene execution) |
| Multiplayer / Networking | `gdplatform` | **Claimed — Patina-only transport** | — | — | NOT Godot-wire-compatible | `enet_multiplayer` / `tcp_multiplayer` / `webrtc_peer` units — see the "Multiplayer / Network Transport" note below |

**Total test count**: ~15,700 across workspace (6,355 crate unit tests + 9,338 integration tests in 393 test files)
**Total golden files**: 150 (17 physics, 23 traces, 37 scenes, 5 resources, 64 render, 3 signals, 1 version)
**Oracle output files**: 115 (measured against Godot 4.6.1)

## Multiplayer / Network Transport (Patina-only) — pat-u3l8c.77

Patina's multiplayer transport is **Patina-only and is NOT wire-compatible with
Godot's networking**. A Patina client cannot connect to a stock Godot 4.6 server
(or vice-versa) over the network. This is a documented, deliberate scope
decision; the alternative — a real ENet protocol implementation plus the Godot
`NETWORK_COMMAND_REMOTE_CALL` byte layout proven against a Godot 4.6 server
fixture — is **deferred**.

**Why it is not compatible today:**

- **ENet framing** — `gdplatform::ENetMultiplayerPeer` (`enet_multiplayer.rs`)
  uses a bespoke `PEN1` 4-byte-magic UDP framing, not the real ENet protocol
  Godot's `ENetMultiplayerPeer` speaks. The packets are not interchangeable.
- **RPC wire protocol** — Patina RPC uses a proprietary `PRPC` magic plus a path
  hash and method-name strings, rather than Godot's command bytes
  (`NETWORK_COMMAND_REMOTE_CALL`) and the node-path-cache handshake
  (`CONFIRM_PATH` / `SIMPLIFY_PATH`, `scene_rpc_interface.cpp`).

**Canonical peer:** `gdplatform::ENetMultiplayerPeer` is the single canonical UDP
multiplayer peer. `TcpMultiplayerPeer` is a separate TCP transport, and the
ClassDB `ENetMultiplayerPeer` registration is a compatibility *alias* that
delegates to `TcpMultiplayerPeer` for class-reflection only — it does not change
the on-wire framing. Same-Patina peers (ENet↔ENet, TCP↔TCP) interoperate
correctly; cross-implementation and cross-engine (Godot) interop are out of scope
until the real ENet + Godot command layout is bound.

---

## Measured vs Claimed vs Deferred

### Measured (has test evidence proving Godot parity)

| Subsystem | Evidence |
|-----------|----------|
| Core Runtime | 828 unit tests; math, IDs, strings, 3D comparison tooling, crash triage, memory profiler |
| Variant System | 131 unit tests; all variant types serialize/deserialize correctly, fuzz testing |
| Object Model | 75 unit tests + integration tests for reflection + ClassDB parity |
| Signals | Integration tests: dispatch parity + trace parity |
| Notifications | Integration tests: coverage + lifecycle traces |
| Resources | 319 unit tests + integration; parsing, caching, UID, unified loading; 5 resource goldens |
| Scene System | 959 unit tests + integration; hierarchy, lifecycle, instancing, packed scenes, frame processing; 37 scene goldens |
| GDScript Interop | 464 unit tests + integration; tokenizer, parser, interpreter, built-ins, bindings |
| Trace Parity | Integration tests against 23 trace goldens (patina vs upstream mock) |
| Oracle Parity | Integration tests against 119 oracle output files (Godot 4.6.1); 100% property parity in the 9-scene report |
| 2D Rendering | 168 unit tests + integration + 64 render goldens (pixel-level comparison) |
| 2D Rendering Server | 108 unit tests; draw ordering, canvas, sprite server |
| 2D Physics | 111 unit tests + integration + 17 physics goldens (deterministic trace comparison) |
| Input / Platform Stable Layer | 504 unit tests + integration; input map loading, action coverage, snapshot routing, headless windowing, timers, target metadata, startup/packaging flow |
| 2D Vertical Slice | Integration end-to-end tests: full pipeline from scene load to frame output |
| Editor-Facing Compatibility Layer | 1963 unit tests + integration; browser editor shell, editor menus, compatibility layer, script editor, inspector, animation/theme/tilemap tooling, selected editor systems |
| 3D Rendering | 95 unit tests + integration; software renderer, framebuffer comparison, diff imaging; 64 render goldens |
| 3D Rendering Server | 338 unit tests; mesh, camera, lighting, viewport, material |
| 3D Physics | 124 unit tests + integration; rigid body, collision, trace comparison |
| 3D Comparison Tooling | 44 tests; scene tree diff, physics trace comparison, render comparison, unified parity reports (JSON + text) |

### Claimed (code exists, no Godot parity test)

| Subsystem | What exists | What's missing |
|-----------|-------------|----------------|
| Audio | Tests covering bus routing, the send graph, effects, the playback state machine, and WAV/OGG/MP3 decode | Real cpal output exists (`audio_server_bridge` pumps `mix()`); remaining gaps are the script-facing `AudioServer` singleton and Godot audio oracle parity. See `AUDIO_MILESTONE.md`. |

### Deferred (not in current milestone)

| Subsystem | Milestone | Notes |
|-----------|-----------|-------|
| 3D Oracle Parity | Phase 6+ | 3D comparison tooling exists; oracle outputs for 3D scenes not yet captured from Godot |
| Audio (oracle parity) | Audio milestone | Decode→mix→effects→cpal pipeline implemented; remaining gate is Godot-vs-Patina audible parity plus the script-facing `AudioServer` singleton. See EXIT_CRITERIA.md audio gate (pat-dd3). |
| XR Server | Phase 7+ | Not started |

---

## Oracle Parity Summary

Measured against **Godot 4.6.1** (v4.6.1.stable.official.14d19694e). Re-measured 2026-11-03 after the oracle outputs were regenerated with a local Godot 4.6.1: **45 oracle scenes** have a Godot property capture (`fixtures/oracle_outputs/*_properties.json`, 119 oracle output files in all), and the property-parity report (`oracle_regression_test::golden_all_scenes_property_parity_report`) compares 9 golden scenes, 71 property comparisons:

- **Overall**: 100% (71/71 property comparisons match in the 9-scene parity report)
- **Node structure**: 100% (all nodes present with correct names/classes across all measured scenes)
- **Explicit properties**: 100% for all measured scenes (positions, collision masks, modulate all match)
- **Script-exported properties**: Closed — all script-exported variables verified across space_shooter, test_scripts, and other scenes
- **3D Scene Trees**: 5 3D fixtures measured (minimal_3d, hierarchy_3d, indoor_3d, multi_light_3d, physics_3d_playground)

Test files: `oracle_parity_test.rs`, `oracle_regression_test.rs`, `comparison_tooling_3d_test.rs`

---

## Golden File Inventory

| Category | Count | Location | Validated by |
|----------|-------|----------|-------------|
| Physics traces | 17 | `fixtures/golden/physics/` | `physics_integration_test` |
| Lifecycle traces | 23 | `fixtures/golden/traces/` | `trace_parity_test`, `multi_scene_trace_parity_test` |
| Scene trees | 37 | `fixtures/golden/scenes/` | `golden_tests`, `oracle_parity_test` |
| Resources | 5 | `fixtures/golden/resources/` | `golden_tests` |
| Render images | 64 | `fixtures/golden/render/` | `render_golden_test`, `render_pipeline`, `render_3d_parity_test` |
| Signals | 3 | `fixtures/golden/signals/` | Signal parity tests |
| Oracle outputs | 115 | `fixtures/oracle_outputs/` | `oracle_parity_test`, `oracle_regression_test` |
| **Total golden** | **150** | `fixtures/golden/` | `golden_staleness_test` (cross-cutting checks) |

---

## Platform Support Matrix

| Platform | Status | Notes |
|----------|--------|-------|
| macOS (aarch64) | **Measured** | Primary dev target for the current headless/runtime slice; native shell parity remains narrower |
| Linux (x86_64) | Implemented, not yet measured | Target metadata and CI matrix coverage are green; native X11/Wayland behaviour is model-level only, and `linux_x11_wayland_platform_test.rs` has 6 rows red at HEAD |
| macOS (x86_64) | Implemented, not yet measured | Target metadata is green; native Cocoa behaviour beyond the bounded slice is model-level only (`macos_platform_layer_test.rs`) |
| Windows (x86_64) | Implemented, not yet measured | Platform-layer and runtime-smoke tests are green but model-level; `windows_platform_win32_backend_test.rs` has 7 rows red at HEAD |
| Android | Deferred | Phase 7+ |
| iOS | Deferred | Phase 7+ |
| Web (WASM) | Deferred | Phase 7+ |

---

### Per-OS Native Platform-Layer Classification

Phase 7's platform work is classified per OS and per surface, because "the
platform layer is implemented" and "this OS's native shell behaves like Godot's"
are different claims and only the first is backed by tests.

Vocabulary, deliberately narrower than the matrix statuses above:

* **Headless/model** — the surface is exercised through Patina's headless
  backend or an OS-API model. The test runs on any host, so it proves the model
  agrees with itself; it does not prove a real OS shell agrees.
* **Native-runtime** — a test drives the real OS windowing/input stack on that
  OS. **No row below is classified this way**, because no test in the repo does
  it. The row stays absent rather than aspirational.
* **Deferred** — not attempted in this phase.

#### Linux

| Surface | Classification | Evidence | Caveat |
|---------|----------------|----------|--------|
| Target metadata and toolchain | Headless/model | `engine-rs/tests/platform_suite/platform_targets_validation_test.rs` | green at HEAD |
| Stable-layer composition (window/input/timing) | Headless/model | `engine-rs/tests/platform_suite/platform_first_stable_layer_test.rs`, `engine-rs/tests/platform_suite/platform_backend_test.rs` | green at HEAD |
| Native X11/Wayland windowing | Headless/model | `engine-rs/tests/misc_suite_d/linux_x11_wayland_platform_test.rs` | ⚠️ 6 rows red at HEAD — protocol/model coverage only, no X11 or Wayland server is contacted |

#### macOS

| Surface | Classification | Evidence | Caveat |
|---------|----------------|----------|--------|
| Target metadata and toolchain | Headless/model | `engine-rs/tests/platform_suite/platform_targets_validation_test.rs` | green at HEAD |
| Stable-layer composition (window/input/timing) | Headless/model | `engine-rs/tests/platform_suite/platform_first_stable_layer_test.rs`, `engine-rs/tests/platform_suite/platform_backend_test.rs` | green at HEAD; this is the primary dev host, so "runs here" is not per-OS evidence |
| Native Cocoa windowing and menu bar | Headless/model | `engine-rs/tests/misc_suite_d/macos_platform_layer_test.rs` | green at HEAD — API/model coverage only, no NSApplication is started |

#### Windows

| Surface | Classification | Evidence | Caveat |
|---------|----------------|----------|--------|
| Target metadata and toolchain | Headless/model | `engine-rs/tests/platform_suite/platform_targets_validation_test.rs` | green at HEAD |
| Runtime startup, input and audio smoke | Headless/model | `engine-rs/tests/platform_suite/windows_runtime_smoke_test.rs` | green at HEAD, but a MODEL test — it asserts modelled DPI/window/theme state and passes on macOS |
| Native Win32 windowing backend | Headless/model | `engine-rs/tests/misc_suite_g/windows_platform_layer_test.rs`, `engine-rs/tests/misc_suite_g/windows_platform_win32_backend_test.rs` | ⚠️ win32 backend test has 7 rows red at HEAD; no HWND is created |

Red-row counts are from `docs/test-debt/2026-10-30-failing-tests-at-HEAD.txt`,
which drifts within the day — a caveat above means **re-measure**, not
"permanently red".

## Measured Scene Inventory

### Oracle-Measured Scenes

31 oracle scenes measured against Godot 4.6.1, with 21 scenes having full parity coverage:

| Scene | Fixture | Oracle Golden | Status |
|-------|---------|---------------|--------|
| `minimal.tscn` | `fixtures/scenes/minimal.tscn` | `oracle_outputs/minimal_tree.json` | Measured |
| `simple_2d.tscn` | `fixtures/scenes/simple_2d.tscn` | `oracle_outputs/simple_2d_tree.json` | Measured |
| `hierarchy.tscn` | `fixtures/scenes/hierarchy.tscn` | `oracle_outputs/hierarchy_tree.json` | Measured |
| `simple_hierarchy.tscn` | `fixtures/scenes/simple_hierarchy.tscn` | `oracle_outputs/simple_hierarchy_tree.json` | Measured |
| `with_properties.tscn` | `fixtures/scenes/with_properties.tscn` | `oracle_outputs/with_properties_tree.json` | Measured |
| `platformer.tscn` | `fixtures/scenes/platformer.tscn` | `oracle_outputs/platformer_tree.json` | Measured |
| `space_shooter.tscn` | `fixtures/scenes/space_shooter.tscn` | `oracle_outputs/space_shooter_tree.json` | Measured |
| `physics_playground.tscn` | `fixtures/scenes/physics_playground.tscn` | `oracle_outputs/physics_playground_tree.json` | Measured |
| `physics_playground_extended.tscn` | `fixtures/scenes/physics_playground_extended.tscn` | `oracle_outputs/physics_playground_extended_tree.json` | Measured |
| `signal_test.tscn` | `fixtures/scenes/signal_test.tscn` | `oracle_outputs/signal_test_tree.json` | Measured |
| `signal_instantiation.tscn` | `fixtures/scenes/signal_instantiation.tscn` | `oracle_outputs/signal_instantiation_tree.json` | Measured |
| `signals_complex.tscn` | `fixtures/scenes/signals_complex.tscn` | `oracle_outputs/signals_complex_tree.json` | Measured |
| `ui_menu.tscn` | `fixtures/scenes/ui_menu.tscn` | `oracle_outputs/ui_menu_tree.json` | Measured |
| `unique_name_resolution.tscn` | `fixtures/scenes/unique_name_resolution.tscn` | `oracle_outputs/unique_name_resolution_tree.json` | Measured |
| `character_body_test.tscn` | `fixtures/scenes/character_body_test.tscn` | `oracle_outputs/character_body_test_tree.json` | Measured |
| `test_scripts.tscn` | — | `oracle_outputs/test_scripts_tree.json` | Measured |
| `minimal_3d.tscn` | `fixtures/scenes/minimal_3d.tscn` | `oracle_outputs/minimal_3d_tree.json` | Measured |
| `hierarchy_3d.tscn` | `fixtures/scenes/hierarchy_3d.tscn` | `oracle_outputs/hierarchy_3d_tree.json` | Measured |
| `indoor_3d.tscn` | `fixtures/scenes/indoor_3d.tscn` | `oracle_outputs/indoor_3d_tree.json` | Measured |
| `multi_light_3d.tscn` | `fixtures/scenes/multi_light_3d.tscn` | `oracle_outputs/multi_light_3d_tree.json` | Measured |
| `physics_3d_playground.tscn` | `fixtures/scenes/physics_3d_playground.tscn` | `oracle_outputs/physics_3d_playground_tree.json` | Measured |

Test files: `oracle_parity_test.rs` (32 tests), `oracle_regression_test.rs` (43 tests), `v1_acceptance_gate_test.rs`

---

## Phase 9 Hardening Artifact Classification

Source of truth: `prd/PHASE9_HARDENING_AUDIT.md` (2026-03-29). That audit defines
the four labels used below — `Measured`, `Implemented, not yet measured`,
`Deferred`, `Missing` — and the rule that keeps this table honest: *"keep 'doc
exists' separate from 'doc is validated by tests'."*

This table applies that rule one step further than the audit could. A deliverable
counts as `Measured` only if its evidence test is **green at HEAD**. Three of the
six are not, so they are classified by what the repo can currently prove, not by
what the artifact set implies.

So the split below is between **measured test-backed** artifacts and **docs-only**
claims: a `Measured` row means a green test asserts the behaviour, while
`Implemented, not yet measured` means the artifact and its documentation exist but
the test that would validate them is currently failing. A docs-only claim is not
evidence, which is the whole point of the audit's separation rule.

| Deliverable | Classification | Evidence (paths current as of 2026-10-30) | Gap | Bead |
|-------------|----------------|-------------------------------------------|-----|------|
| Benchmark dashboards | Measured for the committed dashboard/tooling slice | `engine-rs/tests/misc_suite_a/benchmark_dashboard_test.rs`, `engine-rs/tests/misc_suite_e/perf_benchmark_ci_gate_test.rs`, `docs/BENCHMARK_BASELINES.md` | missing breadth — a checked-in dashboard framework, not a published external service | `pat-5jwj9` |
| Crash triage process | Measured for the local process/model slice | `engine-rs/tests/crash_suite/crash_triage_process_test.rs`, `engine-rs/tests/crash_suite/crash_triage_auto_issue_test.rs`, `docs/TRIAGE_PROCESS.md` | missing breadth — severity, queueing, regression labelling and auto-issue generation are validated; operational response is not | `pat-t8hgz` |
| Contributor onboarding | Measured | `engine-rs/tests/contributor_suite/contributor_onboarding_docs_test.rs`, `engine-rs/tests/contributor_suite/contributor_onboarding_validation_test.rs`, `docs/contributor-onboarding.md` | none | onboarding bead |
| Fuzz/property tests | Implemented, not yet measured | `engine-rs/tests/fuzz_suite/fuzz_property_coverage_test.rs` (3 rows red at HEAD: `all_fuzz_modules_exist`, `fuzz_modules_contain_tests`, `fuzz_modules_meet_minimum_test_counts`) | the coverage gate itself is failing, so current fuzz breadth is unproven — do not read it as exhaustive engine-wide coverage | `pat-3pstd`, `pat-21oab` |
| Release train | Implemented, not yet measured | `engine-rs/tests/release_suite/release_train_workflow_test.rs` (2 rows red at HEAD: `integration_test_count_floor`, `parity_tests_cover_key_subsystems`), `docs/RELEASE_PROCESS.md` | docs-overclaim — the audit already flagged that the docs imply automation the artifacts do not guarantee; the contract tests that would prove the rest are red | `pat-d59t7`, `pat-21oab` |
| Migration guide | Implemented, not yet measured | `engine-rs/tests/misc_suite_d/migration_guide_validation_test.rs` (3 rows red at HEAD: `guide_has_porting_walkthrough`, `limitations_mention_key_missing_features`, `porting_walkthrough_has_compatibility_checklist`), `docs/migration-guide.md` | the guide exists but its structure/scope validation is failing, so its stated coverage is unverified | `pat-1b7i6`, `pat-21oab` |

Red-row counts are from `docs/test-debt/2026-10-30-failing-tests-at-HEAD.txt`
(653 failures across 114 targets, found by the first full workspace test run in
months). That snapshot is a point-in-time measurement and drifts within the day,
so a row above means **re-measure before trusting it**, not "permanently red" —
`docs/test-debt/2026-10-30-reverified-against-prebuilt-binaries.md` records the
re-measurement method and which targets are trustworthy.

⚠️ The audit's own `Sources To Compare` list points every test at
`engine-rs/tests/<name>.rs`. Those paths no longer resolve: `engine-rs/tests` was
reorganised into suite directories, so each former standalone file is now a module
inside `tests/<suite>/main.rs`. The paths in this table were each located on disk
and are the current ones.

## Editor Tooling (Phase 8)

### Selected Editor Tooling Classification

Source of truth: `prd/PHASE8_EDITOR_PARITY_AUDIT.md` (2026-04-22).

These rows classify **selected tooling slices that have tests**. They
deliberately **do not imply blanket Godot editor parity** — each row is scoped to
the behaviour its cited test actually drives, and the gap in every case is
breadth, not absence.

A family is `Measured` only when its evidence test is **green at HEAD**. One is
not, and is classified by what the repo can currently prove rather than by what
the audit claimed in April.

| Tooling Family | Classification | Evidence (paths current as of 2026-10-30) | Scope and caveats |
|----------------|----------------|-------------------------------------------|-------------------|
| Script editor | Implemented, not yet measured | `engine-rs/tests/script_suite/script_editor_core_parity_test.rs` | core editing, find/replace and gutter slice; ⚠️ `gutter_bookmark_navigation` is red at HEAD, so the audit's April "Measured for tested slice" no longer holds |
| Inspector | Measured for the typed-editor and resource sub-editor slice | `engine-rs/tests/property_suite/property_inspector_typed_editors_test.rs`, `engine-rs/tests/property_suite/property_inspector_resource_sub_editor_test.rs` | green at HEAD; typed editors and resource sub-editors only, not the full inspector surface |
| Animation editor | Measured for the tested tooling slice | `engine-rs/tests/animation_suite/animation_editor_parity_test.rs` | green at HEAD; track/key model parity, not the full animation UI |
| Theme editor | Measured for the live-preview slice | `engine-rs/tests/theme_suite/theme_editor_live_preview_test.rs` | green at HEAD; live preview path only |
| Tilemap tooling | Measured for the painting slice | `engine-rs/tests/misc_suite_f/tilemap_editor_painting_test.rs` | green at HEAD; painting operations only, not terrain or autotile breadth |
| Editor systems | Measured for the local API/model slice | `engine-rs/tests/editor_suite/editor_systems_parity_test.rs` | green at HEAD; settings, VCS and export-dialog models. The audit flags this row as `docs-overclaim`: it is bounded systems parity, **not** full editor parity |
| EditorInterface compatibility layer | Measured for the explicit API slice | `engine-rs/tests/editor_suite/editor_interface_compat_test.rs` | green at HEAD; the explicitly implemented `EditorInterface` methods only |
| Import pipeline / import settings | Implemented, not yet measured | `engine-rs/tests/` — the audit cites `audio_import_pipeline_test.rs` and `import_settings_panel_test.rs` | carried over from the audit, which already classified this family below Measured |

⚠️ The audit's evidence column cites bare `<name>_test.rs` filenames that no
longer resolve: `engine-rs/tests` was reorganised into suite directories, so each
former standalone file is now a module inside `tests/<suite>/main.rs`. The paths
in this table were each located on disk. The same staleness is why
`phase8_editor_compat_layer_validation_test::all_audit_evidence_test_files_exist`
reports 13 "missing" files that all exist.

Red-row state is from `docs/test-debt/2026-10-30-failing-tests-at-HEAD.txt`,
which drifts within the day — a caveat above means **re-measure**, not
"permanently red".

## Update Protocol

This matrix is updated when:

1. A subsystem transitions between status levels.
2. New fixture coverage is added for a subsystem.
3. Parity test results change materially.
4. A new subsystem row is added.

Each update should include the date and the bead or PR that prompted the change.
