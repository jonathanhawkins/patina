// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run -p gdeditor --test editor_suite_a -E 'test(/^<mod>::/)'
mod editor_3d_box_select_test;
mod editor_3d_nav_test;
mod editor_3d_toolbar_actions_test;
mod editor_3d_toolbar_chrome_test;
mod editor_3d_viewport_test;
mod editor_animation_panel_test;
mod editor_audio_panel_test;
mod editor_beads_test;
mod editor_bottom_panel_chrome_test;
mod editor_bottom_panel_dock_test;
mod editor_bottom_panel_tabs_test;
mod editor_bottom_panel_toolbars_test;
mod editor_capabilities_routes_test;
mod editor_close_scene_e2e_test;
mod editor_close_scene_wiring_test;
mod editor_confirm_dialog_test;
mod editor_create_dialog_godot_parity_test;
mod editor_debugger_subtabs_test;
mod editor_debugger_tabs_test;
mod editor_default_backend_wgpu_test;
