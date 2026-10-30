// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run -p gdeditor --test editor_suite_d -E 'test(/^<mod>::/)'
mod editor_property_set_http_test;
mod editor_runtime_3d_render_test;
mod editor_save_scene_e2e_test;
mod editor_save_scene_wiring_test;
mod editor_scene_tree_context_ops_wired_test;
mod editor_scene_tree_dock_chrome_test;
mod editor_scene_tree_row_warning_test;
mod editor_script_history_help_test;
mod editor_script_list_members_test;
mod editor_script_menu_actions_test;
mod editor_script_toolbar_test;
mod editor_script_view_test;
mod editor_selection_newscene_e2e_test;
mod editor_selection_prune_test;
mod editor_shader_editor_test;
mod editor_signals_edit_disconnect_test;
mod editor_signals_goto_autocreate_test;
mod editor_signals_groups_chrome_test;
mod editor_sky_sun_fog_parity_test;
mod editor_smoke_test;
mod editor_tab_label_test;
