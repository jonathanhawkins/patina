// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test misc_suite_g -E 'test(/^<mod>::/)'
mod time_singleton_parity_test;
mod tooling_milestones_spec_validation_test;
mod tooling_parity_milestone_test;
mod transform3d_camera_light_contract_test;
mod transform3d_camera_light_subset_contract_test;
mod true_defaults_probe_source_test;
mod ui_parity_test;
mod unified_loader_test;
mod unsafe_code_audit_test;
mod upstream_frame_trace_golden_test;
mod vcs_git_status_integration_test;
mod version_consistency_test;
mod vertical_slice_2d_parity_test;
mod vertical_slice_test;
mod visibility_suppression_test;
mod visual_script_stub_compatibility_test;
mod weakref_auto_invalidation_test;
mod web_wasm_export_interop_test;
mod windows_platform_layer_test;
mod windows_platform_win32_backend_test;
