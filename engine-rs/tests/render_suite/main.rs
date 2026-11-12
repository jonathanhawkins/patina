// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test render_suite -E 'test(/^<mod>::/)'
mod render_2d_parity_test;
mod render_3d_gpu_pipeline_test;
mod render_3d_parity_hooks_test;
mod render_3d_parity_test;
mod render_3d_pipeline_shader_material_test;
mod render_3d_texture_sampling_test;
mod render_benchmark_fixtures_test;
mod render_camera_viewport_test;
mod render_draw_ordering_test;
mod render_golden_test;
mod render_physics_comparison_tooling_test;
mod render_sprite_property_test;
mod render_vertical_slice_test;
