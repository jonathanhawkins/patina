# pat-21oab: what of the 2026-10-30 test-debt snapshot is still true

RosePeak, 2026-10-30 10:57 PDT. Produced by running the prebuilt
`engine-rs/target-verifier/debug/deps/<target>-<hash>` binaries twice each.
No cargo and no build slot — these binaries already existed, so any worker
can repeat this. **This file does not replace the 08:11 snapshot; it dates it.**

## The one thing to take away

**Binary age gates every number below.** Only **36 of 110** measured targets
have a binary built today. For the other 74, the binary predates HEAD by one
to three months, so its answer describes July/August code:

* old binary FAILS a snapshot row -> consistent, but unverified against HEAD.
* old binary PASSES a snapshot row -> **does NOT mean the row is stale.** It is
  weak evidence the test BROKE between that build and today — the opposite
  reading. I drew the wrong one from this data before dating the binaries.

So the trustworthy result is narrow, and the rest is a to-do for whoever can
rebuild.

## Trustworthy — binary built today

35 targets, 224 snapshot rows.

* confirmed STILL FAILING: **206**
* confirmed NOT failing (snapshot row is stale): **17**
* `#[ignore]`d — PARKED DEBT, still work, not stale: **1**
* failing but absent from the snapshot: **6** (this tree, which holds peers' uncommitted work — not attributable to a commit)

### Confirmed stale rows — these are NOT work

| row | binary (today) |
|---|---|
| `audiosample_register_and_playback_handle::registration_is_idempotent` | 09:56:00 |
| `backtrace::a_capture_with_no_nested_call_is_empty` | 09:55:34 |
| `backtrace::captures_are_independent_snapshots` | 09:55:34 |
| `backtrace::format_renders_the_frames` | 09:55:34 |
| `editor_feature_profile_disables_class_and_feature::a_disabled_feature_leaves_the_enabled_set` | 09:55:55 |
| `gdresource::bit_map::tests::create_from_image_alpha_threshold_is_exclusive` | 10:27:53 |
| `gdresource::importers::tests::format_loader_with_defaults` | 10:27:53 |
| `gdresource::noise::tests::ping_pong_folds_into_unit_triangle` | 10:27:53 |
| `midi_ports::close_clears_the_ports_not_just_the_flag` | 09:55:36 |
| `midi_ports::listing_is_not_gated_on_open` | 09:55:36 |
| `midi_ports::open_sets_the_flag_without_inventing_ports` | 09:55:36 |
| `physics_suite::physics_integration_test::za_golden_gravity_fall_trajectory_content_check` | 09:54:52 |
| `physics_suite::physics_playground_extended_golden_test::extended_deterministic_across_runs` | 09:54:52 |
| `physics_suite::physics_playground_extended_golden_test::extended_programmatic_and_tscn_identical` | 09:54:52 |
| `resource_suite::resource_scene_broader_execution_test::animation_player_advances_through_mainloop_step` | 10:37:52 |
| `scene_tree_timer_flags::ignore_time_scale_timers_are_unaffected_by_engine_time_scale` | 10:16:13 |
| `tail_extension_surfaces_registered_and_dispatched::a_declining_script_translation_falls_back_to_the_catalog` | 10:16:06 |

### `#[ignore]`d — parked debt, NOT stale

A failure-count measurement cannot see this, and my first cut of this report got
it wrong: an `#[ignore]`d test reports **0 failed**, so it looks identical to a
fixed one. It is not — it is debt someone deliberately silenced.

| row | ignore reason |
|---|---|
| `physics_suite::physics_integration_test::za_golden_static_blocking_content_check` | `pat-21oab.2`: fixture authors `half_extents`, so the floor has a zero-extent collider and the ball falls through |

**This is a blind spot wider than one row.** The engine test tree carries **38**
`#[ignore]` attributes across 10 files, and only **8** cite a bead — so roughly
30 tests are silenced with no reason recorded and no tracking. None of them
appear in the snapshot, because they do not fail. Any count of "remaining test
debt" that reads failure counts alone omits all of them.

Worth its own bead: enumerate the 38, split deliberate-and-cited from silent,
and either cite or un-ignore the rest.

### Confirmed still failing, by target — safe to decompose off

| target | still failing | binary (today) |
|---|---|---|
| `resource_suite` | 62 | 10:37:52 |
| `misc_suite_e` | 36 | 09:42:27 |
| `ci_suite` | 34 | 09:54:49 |
| `gdscene` | 13 | 10:52:04 |
| `render_suite` | 12 | 09:54:57 |
| `physics_suite` | 8 | 09:54:52 |
| `editor_suite_a` | 6 | 01:19:47 |
| `gdphysics2d` | 5 | 09:42:05 |
| `object_indexed_paths` | 4 | 10:16:04 |
| `classdb_2d_node_property_tables_match_godot` | 2 | 10:16:25 |
| `despawn_when_the_spawner_leaves_the_tree` | 2 | 10:16:18 |
| `editor_suite_b` | 2 | 01:29:52 |
| `editor_suite_d` | 2 | 01:31:53 |
| `line_edit_typing_signals_e2e` | 2 | 10:16:21 |
| `multiplayer_despawn_removes_the_client_copy` | 2 | 10:16:12 |
| `rigidbody2d_continuous_cd` | 2 | 10:16:25 |
| `animation_saves_into_tscn_roundtrip` | 1 | 10:16:09 |
| `audio_effect_capture_and_record_tap_bus` | 1 | 10:16:02 |
| `color_picker_scene_draw_and_pick` | 1 | 10:16:06 |
| `editor_suite_c` | 1 | 01:31:57 |
| `editor_suite_e` | 1 | 01:49:27 |
| `fabrik3d_chain_reaches_target` | 1 | 10:16:02 |
| `format_loader_covers_source_asset_extensions` | 1 | 10:14:55 |
| `gdaudio` | 1 | 01:46:36 |
| `gui_focus_routing_test` | 1 | 10:16:08 |
| `ragdoll3d` | 1 | 10:16:17 |
| `rigidbody3d_set_axis_velocity` | 1 | 10:16:25 |
| `variant_hash_full_type_coverage_from_scene` | 1 | 10:16:26 |

## UNVERIFIED — binary is 1–3 months old

70 targets, 310 snapshot rows (62% of the snapshot). Nothing here should be decomposed or dismissed until
someone with a build slot rebuilds these targets. Sorted by rows at stake.

| target | snapshot rows | binary date | old binary's verdict |
|---|---|---|---|
| `phase_suite` | 41 | 2026-07-26 | 41 of 41 still fail |
| `misc_suite_a` | 29 | 2026-07-26 | 29 of 29 still fail |
| `editor_suite` | 23 | 2026-07-26 | 20 of 23 still fail |
| `misc_suite_g` | 21 | 2026-07-26 | 21 of 21 still fail |
| `packed_suite` | 21 | 2026-07-26 | 21 of 21 still fail |
| `misc_suite_b` | 20 | 2026-07-26 | 19 of 20 still fail |
| `docs_suite` | 14 | 2026-07-26 | 14 of 14 still fail |
| `fuzz_suite` | 13 | 2026-07-26 | 13 of 13 still fail |
| `signal_suite` | 11 | 2026-08-19 | 11 of 11 still fail |
| `scene_suite` | 10 | 2026-07-26 | 10 of 10 still fail |
| `gdscript_suite` | 8 | 2026-07-26 | 8 of 8 still fail |
| `node2d_suite` | 8 | 2026-07-26 | 8 of 8 still fail |
| `v1_acceptance_gate_test` | 5 | 2026-07-26 | 5 of 5 still fail |
| `headless_suite` | 4 | 2026-07-26 | 4 of 4 still fail |
| `lifecycle_suite` | 4 | 2026-07-26 | 4 of 4 still fail |
| `release_suite` | 4 | 2026-07-26 | 4 of 4 still fail |
| `audio_suite` | 3 | 2026-07-26 | all pass (suspect RECENT BREAK) |
| `gdserver2d` | 3 | 2026-08-20 | 3 of 3 still fail |
| `probe_decal` | 3 | 2026-08-20 | all pass (suspect RECENT BREAK) |
| `viewport_suite` | 3 | 2026-07-26 | 3 of 3 still fail |
| `container_and_material_carry_godot_properties` | 2 | 2026-08-19 | all pass (suspect RECENT BREAK) |
| `crate_suite` | 2 | 2026-07-26 | 2 of 2 still fail |
| `custom_shader_uniform_block_is_emitted` | 2 | 2026-08-20 | all pass (suspect RECENT BREAK) |
| `filesystem_dock_script_signals_fire` | 2 | 2026-08-20 | 2 of 2 still fail |
| `gdphysics3d` | 2 | 2026-08-19 | 1 of 2 still fail |
| `gdserver3d` | 2 | 2026-08-20 | 2 of 2 still fail |
| `geometryinstance_cast_shadow_and_visibility_range_consumed` | 2 | 2026-08-19 | all pass (suspect RECENT BREAK) |
| `oracle_suite` | 2 | 2026-07-26 | 2 of 2 still fail |
| `particle_2d_scene_renders_moving_particles` | 2 | 2026-07-26 | all pass (suspect RECENT BREAK) |
| `rng_seedable` | 2 | 2026-07-26 | 1 of 2 still fail |
| `rs_globals` | 2 | 2026-08-20 | 2 of 2 still fail |
| `spatial_highshelf_is_applied_per_voice` | 2 | 2026-08-19 | all pass (suspect RECENT BREAK) |
| `animation_suite` | 1 | 2026-07-26 | all pass (suspect RECENT BREAK) |
| `audio_bone_animnode_metadata` | 1 | 2026-08-19 | all pass (suspect RECENT BREAK) |
| `broader_suite` | 1 | 2026-07-26 | 1 of 1 still fail |
| `button_label_colorrect_carry_godot_properties` | 1 | 2026-08-19 | all pass (suspect RECENT BREAK) |
| `collision_suite` | 1 | 2026-07-26 | 1 of 1 still fail |
| `crash_suite` | 1 | 2026-07-26 | 1 of 1 still fail |
| `ctex_family` | 1 | 2026-07-26 | all pass (suspect RECENT BREAK) |
| `demo_suite` | 1 | 2026-07-26 | all pass (suspect RECENT BREAK) |
| `editor_undo_coverage_e2e` | 1 | 2026-07-26 | 1 of 1 still fail |
| `external_texture_frame_source` | 1 | 2026-07-26 | all pass (suspect RECENT BREAK) |
| `gdplatform` | 1 | 2026-08-20 | 1 of 1 still fail |
| `gdrender2d` | 1 | 2026-08-19 | 1 of 1 still fail |
| `gltf_mesh_renders_in_viewport` | 1 | 2026-08-19 | all pass (suspect RECENT BREAK) |
| `lifecycle_trace_parity_test` | 1 | 2026-07-26 | 1 of 1 still fail |
| `light3d_full_property_set_reaches_editor_render` | 1 | 2026-08-20 | all pass (suspect RECENT BREAK) |
| `material_transparency_modes_render` | 1 | 2026-08-19 | 1 of 1 still fail |
| `midi_e2e` | 1 | 2026-07-27 | 1 of 1 still fail |
| `narrowphase_matrix_3d_test` | 1 | 2026-07-26 | 1 of 1 still fail |
| `navigation_resource_classes_test` | 1 | 2026-07-26 | all pass (suspect RECENT BREAK) |
| `node3d_transform_state_coherence` | 1 | 2026-07-26 | all pass (suspect RECENT BREAK) |
| `os_singleton_surface_wiring` | 1 | 2026-07-26 | all pass (suspect RECENT BREAK) |
| `real_suite` | 1 | 2026-07-26 | 1 of 1 still fail |
| `reflection_probe_capture_render` | 1 | 2026-08-20 | 1 of 1 still fail |
| `remaining_singletons_script_reachable` | 1 | 2026-08-20 | 1 of 1 still fail |
| `replication_config_script` | 1 | 2026-08-19 | 1 of 1 still fail |
| `rs_gvar_enum` | 1 | 2026-07-29 | all pass (suspect RECENT BREAK) |
| `run_scene` | 1 | 2026-08-19 | 1 of 1 still fail |
| `scene_lights_and_shadows_render` | 1 | 2026-07-26 | all pass (suspect RECENT BREAK) |
| `scene_main_classes` | 1 | 2026-07-26 | all pass (suspect RECENT BREAK) |
| `script_dialog` | 1 | 2026-07-26 | all pass (suspect RECENT BREAK) |
| `script_draw_and_gui_input_virtuals` | 1 | 2026-07-26 | all pass (suspect RECENT BREAK) |
| `script_suite` | 1 | 2026-07-26 | 1 of 1 still fail |
| `skeleton_modification_2d_family_registered` | 1 | 2026-08-19 | all pass (suspect RECENT BREAK) |
| `small_class_metadata_batch` | 1 | 2026-08-19 | all pass (suspect RECENT BREAK) |
| `sprite3d_label3d_animatedsprite3d_render_from_scene` | 1 | 2026-08-19 | 1 of 1 still fail |
| `stylebox_family_registers_properties` | 1 | 2026-07-26 | all pass (suspect RECENT BREAK) |
| `websocket_peer_connection_surface_registered` | 1 | 2026-08-19 | all pass (suspect RECENT BREAK) |
| `xml_parser_open_offset_and_line` | 1 | 2026-08-20 | 1 of 1 still fail |

## FLAKY — a per-row bead cannot close these

The **same** binary disagreed with itself across two consecutive runs, so age
is not the explanation and no row-level fix is verifiable. This is the
shared-state problem (pat-21oab.3, currently CLOSED) and it is still live.

| target | run 1 (snapshot, actual) | run 2 |
|---|---|---|
| `editor_cmds` | (3, 3) | (3, 2) |
| `gdeditor` | (10, 11) | (10, 8) |
| `gdscript_interop` | (3, 4) | (3, 3) |
| `globals_boot` | (3, 2) | (3, 3) |
| `misc_suite_d` | (39, 51) | (39, 54) |
| `misc_suite_f` | (19, 21) | TIMEOUT |

## Unmeasured — status unknown, NOT zero

| target | why | snapshot rows |
|---|---|---|
| `gdshader_ast_drives_3d_fragment` | no_binary | 3 |
| `gdshader_parse_diagnostics` | no_binary | 1 |
| `misc_suite_c` | timeout | 38 |
| `misc_suite_f` | timeout | 19 |

## How to repeat or extend this

```bash
ls -t engine-rs/target-verifier/debug/deps/<target>-*          # newest first
stat -f '%Sm %N' -t '%Y-%m-%d %H:%M:%S' <binary>               # DATE IT FIRST
./<binary>                                                     # whole target
./<binary> <substring> --test-threads=1                        # one test
./<binary> --list | grep <name>                                # is it even in there?
```

Traps, all of which cost me something today:

1. **Date the binary before believing it.** Mail timestamps here are UTC and
   the clock is PDT, 7 hours apart; I read a 17:37 UTC binary as "10:37, before
   my edits" when the verifier had built it FROM my uncommitted fix.
2. **Run it twice.** Six targets disagree with themselves.
3. **A timeout or missing binary is not zero failures.**
4. **`--list | grep` first.** A binary from another build answers
   `0 passed; N filtered out`, which reads like success and means "not here".
