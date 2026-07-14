// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test misc_suite_a -E 'test(/^<mod>::/)'
mod area2d_overlap_signal_parity_test;
mod area3d_query_node_classification_test;
mod asset_drag_drop_viewport_inspector_test;
mod audited_3d_comparison_tooling_test;
mod benchmark_dashboard_audit_test;
mod benchmark_dashboard_test;
mod cache_regression_test;
mod camera2d_transform_render_test;
mod camera3d_light_registration_parity_test;
mod change_scene_to_packed_parity_test;
mod character_static_body_behavior_test;
mod character_static_body_extended_parity_test;
mod characterbody3d_move_and_slide_3d_test;
mod characterbody3d_move_and_slide_test;
mod classdb_probe_signature_parity_test;
mod clipboard_read_write_test;
mod command_palette_fuzzy_search_test;
mod community_issue_template_triage_test;
mod comparison_tooling_3d_test;
mod compat_docs_version_distinction_test;
