// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run -p gdeditor --test editor_suite_c -E 'test(/^<mod>::/)'
mod editor_monitors_panel_test;
mod editor_network_profiler_test;
mod editor_new_scene_e2e_test;
mod editor_new_scene_wiring_test;
mod editor_node_dock_groups_test;
mod editor_node_dock_signals_test;
mod editor_output_collapse_categories_test;
mod editor_output_panel_test;
mod editor_parity_test;
mod editor_play_custom_scene_runtime_test;
mod editor_play_e2e_test;
mod editor_play_edit_lock_test;
mod editor_play_game_view_wiring_test;
mod editor_play_pause_resume_e2e_test;
mod editor_play_pause_resume_test;
mod editor_play_stop_restore_test;
mod editor_polling_bounded_test;
mod editor_profiler_modes_test;
mod editor_profiler_panel_test;
mod editor_project_settings_input_map_test;
