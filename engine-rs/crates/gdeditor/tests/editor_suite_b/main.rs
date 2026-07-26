// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run -p gdeditor --test editor_suite_b -E 'test(/^<mod>::/)'
mod editor_filesystem_bare_get_test;
mod editor_filesystem_chrome_test;
mod editor_fs_dock_context_menu_test;
mod editor_fs_dock_favorites_test;
mod editor_fs_dock_split_mode_test;
mod editor_game_assetlib_views_test;
mod editor_game_toolbar_runtime_test;
mod editor_game_toolbar_test;
mod editor_groups_dock_membership_test;
mod editor_history_dock_test;
mod editor_idle_render_test;
mod editor_import_dock_test;
mod editor_import_hooks_test;
mod editor_input_map_e2e_test;
mod editor_inspector_dock_chrome_test;
mod editor_interaction_sequence_oracle_test;
mod editor_misc_panel_test;
mod editor_mode_e2e_test;
mod editor_modified_flag_undo_test;
mod editor_monitors_live_test;
mod mouse_mode_pointer_lock_pipeline_test;
