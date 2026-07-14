// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run -p gdeditor --test editor_suite_e -E 'test(/^<mod>::/)'
mod editor_top_bar_movie_maker_test;
mod editor_top_bar_play_custom_test;
mod editor_top_bar_renderer_test;
mod editor_ui_oracle_test;
mod editor_undo_redo_test;
mod editor_viewport_2d_toolbar_test;
mod editor_viewport_scale_shortcut_test;
mod editor_viewport_toolbar_chrome_test;
mod editor_vram_panel_test;
mod inspector_revert_test;
mod nodes2d_canvas_bridge_wiring;
mod parity_criteria_lint;
mod play_stop_restore_test;
mod scene_tree_parity_test;
mod viewport_gizmo_move_test;
mod viewport_gizmo_scale_test;
mod viewport_smart_snap_test;
mod viewport_zoom_test;
