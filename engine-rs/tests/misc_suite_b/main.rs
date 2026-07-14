// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test misc_suite_b -E 'test(/^<mod>::/)'
mod compat_matrix_measured_scenes_test;
mod compatibility_docs_version_test;
mod core_runtime_gaps_closing_test;
mod core_runtime_subset_gaps_test;
mod cpu_particles2d_emitter_test;
mod csg_boolean_operations_test;
mod cursor_shape_custom_test;
mod debugger_panel_breakpoint_test;
mod debugger_remote_tcp_test;
mod default_property_stripping_parity_test;
mod deferred_3d_systems_classification_test;
mod dependency_audit_test;
mod dependency_license_audit_test;
mod deterministic_physics_trace_golden_test;
mod directional_light_shadow_mapping_test;
mod draw_ordering_visibility_layer_test;
mod environment_resource_parity_test;
mod example_gallery_validation_test;
mod export_integration_test;
mod export_template_debug_release_test;
