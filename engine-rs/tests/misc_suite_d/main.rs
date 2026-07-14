// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test misc_suite_d -E 'test(/^<mod>::/)'
mod instance_inheritance_ext_resource_test;
mod instanced_resource_sharing_test;
mod instancing_ownership_test;
mod light3d_shadow_hint_alignment_test;
mod linux_x11_wayland_platform_test;
mod macos_platform_layer_test;
mod mainloop_physics_advance_test;
mod mainloop_physics_stepping_parity_test;
mod measured_2d_vertical_slice_test;
mod memory_profiler_ci_gate_test;
mod memory_profiler_leak_detection_test;
mod migration_guide_validation_test;
mod mouse_input_snapshot_parity_test;
mod multi_body_oracle_trace_test;
mod multimesh_instance3d_rendering_test;
mod navigation_region3d_mesh_baking_test;
mod nested_instance_unique_name_parity_test;
mod networking_parity_test;
mod nightly_ci_infrastructure_test;
mod node3d_runtime_slice_test;
