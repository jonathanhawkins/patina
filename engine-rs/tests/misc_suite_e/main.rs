// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test misc_suite_e -E 'test(/^<mod>::/)'
mod node3d_transform_propagation_parity_test;
mod onready_variable_resolution_test;
mod os_singleton_parity_test;
mod pck_runtime_resource_loading_test;
mod perf_benchmark_ci_gate_test;
mod perf_comparison_report_test;
mod phase9_hardening_classification_test;
mod pixel_diff_threshold_test;
mod platformer_test;
mod probe_output_schema_test;
mod process_priority_and_pause_test;
mod project_loading_test;
mod publish_3d_parity_report_test;
mod raycast3d_shapecast3d_parity_test;
mod regex_singleton_parity_test;
mod reparent_notification_order_parity_test;
mod repeated_load_cache_dedup_test;
mod repin_diff_report_test;
mod representative_3d_fixtures_test;
mod representative_scene_workflow_integration_test;
