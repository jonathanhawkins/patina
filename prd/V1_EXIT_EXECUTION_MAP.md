# V1 Exit Execution Map

Current reality:

- oracle parity is 81.4% (180/221) — target is ≥98%
- 2D fixtures are near-green (97%+), 3D fixtures drag the average down (58–74%)
- unchecked V1_EXIT_CRITERIA.md items span object model, resources, scenes, scripting, physics, rendering, and platform
- all 2D regressions are bounded; the remaining work is feature completion and 3D parity

## Rule

Do not claim V1 exit while:

- oracle parity is below 98%
- any subsystem gate in V1_EXIT_CRITERIA.md is unchecked
- `cargo test --workspace` has failures on golden comparisons

Workers: these beads require IMPLEMENTATION, not just tests. The acceptance test for each bead already exists in `engine-rs/tests/v1_acceptance_gate_test.rs` and currently FAILS. Your job is to make it pass by implementing the feature in the engine crate. The coordinator will reject completions where the acceptance test still fails.

## Now

1. `v1-obj-classdb` Full ClassDB property and method enumeration against oracle output
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_classdb
2. `v1-obj-notif` Object.notification() dispatch with correct ordering
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_notification_dispatch_ordering
3. `v1-obj-weakref` WeakRef behavior matches oracle
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_weakref
4. `v1-obj-free` Object.free() plus use-after-free guard
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_object_free
5. `v1-res-uid` Resource UID registry for uid:// references
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_uid_registry
6. `v1-res-subres` Sub-resource inline loading in .tres files
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_subresource
7. `v1-res-extref` External resource reference resolution across files
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_external_ref
8. `v1-res-roundtrip` Resource load-inspect-resave roundtrip produces equivalent output
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_resource_roundtrip
9. `v1-res-oracle` Oracle comparison for at least one fixture resource
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_resource_oracle
10. `v1-scene-inherit` Instance inheritance for scenes using ext_resource
    Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_instance_inherit
11. `v1-scene-packed-rt` PackedScene save/restore roundtrip
    Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_packed_scene_roundtrip
12. `v1-scene-signals` Scene-level signal connections wired during instantiation
    Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_scene_signals
13. `v1-scene-oracle` Oracle golden comparison for non-trivial scene tree
    Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_scene_oracle

## Next

### Team A: Scripting Runtime

Goal:

- make GDScript interop pass its V1 exit gate

Claim order:

1. `v1-script-ast` GDScript parser produces stable AST for representative scripts
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_gdscript_ast
2. `v1-script-onready` @onready variable resolution after _ready
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_onready
3. `v1-script-dispatch` func dispatch via object method table
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_func_dispatch
4. `v1-script-signal` signal declaration and emit_signal callable from script
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_script_signal
5. `v1-script-fixture` At least one script-driven fixture executes and matches oracle
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_script_fixture

### Team B: Physics Completion

Goal:

- close remaining gdphysics2d gaps for V1

Claim order:

1. `v1-phys-api` PhysicsServer2D API surface body_create body_set_state body_get_state
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_physics_server_api
2. `v1-phys-layers` Collision layers and masks respected
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_collision_layers
3. `v1-phys-kinematic` KinematicBody2D move_and_collide baseline behavior
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_kinematic_body
4. `v1-phys-multibody` Oracle comparison for one multi-body deterministic trace
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_multibody_oracle

### Team C: Rendering Completion

Goal:

- close remaining gdrender2d gaps for V1

Claim order:

1. `v1-render-atlas` Texture atlas sampling matches upstream pixel output within tolerance
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_texture_atlas
2. `v1-render-zindex` CanvasItem z-index ordering respected
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_zindex
3. `v1-render-visibility` Visibility false suppresses draw calls
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_visibility
4. `v1-render-camera2d` Camera2D transform applied correctly to render output
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_camera2d
5. `v1-render-pixeldiff` Pixel diff against upstream golden at most 0.5 percent error rate
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_pixel_diff

### Team D: Platform Layer

Goal:

- bring gdplatform from not-started to V1 exit

Claim order:

1. `v1-plat-window` Window creation abstraction backed by winit
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_window_creation
2. `v1-plat-input` Input event delivery keyboard mouse gamepad stubs
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_input_events
3. `v1-plat-os` OS singleton get_ticks_msec get_name
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_os_singleton
4. `v1-plat-time` Time singleton get_ticks_usec
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_time_singleton
5. `v1-plat-headless` Headless mode no window for CI supported
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_headless_mode

## Later

1. `v1-3d-camera` Camera3D current property emitted by Patina for 3D parity
2. `v1-3d-transform` Transform3D basis format normalization for oracle match
3. `v1-3d-light-precision` Light3D float precision normalization within tolerance
4. `v1-3d-light-shadow` Light3D shadow_enabled hint value alignment
5. `v1-parity-space-shooter` Close space_shooter script-exported property gap
6. `v1-parity-test-scripts` Close test_scripts Mover position drift gap
7. `v1-parity-gate` Oracle parity reaches 98 percent across all fixtures
   Acceptance: cargo test --test v1_acceptance_gate_test -- --ignored test_v1_overall_parity_gate

## Do Not Do Yet

- new editor feature expansion
- new subsystem imports beyond V1 scope
- 3D runtime expansion beyond parity normalization fixes
