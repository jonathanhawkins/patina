// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test misc_suite_f -E 'test(/^<mod>::/)'
mod res_uid_unified_loader_parity_test;
mod rigidbody3d_forces_torques_contacts_test;
mod robustness_tests;
mod runtime_2d_canvas_render_test;
mod rustdoc_generation_test;
mod shader_material_3d_scene_integration_test;
mod shader_tokenizer_integration_test;
mod sky_resource_panoramic_procedural_test;
mod sky_resource_parity_test;
mod space_shooter_live_test;
mod space_shooter_test;
mod startup_packaging_ci_gate_test;
mod startup_runtime_packaging_flow_test;
mod subresource_inline_loading_test;
mod subviewport_viewport_texture_test;
mod test_scripts_frame_evolution_461_test;
mod texture_atlas_sampling_parity_test;
mod texture_draw_sprite_parity_test;
mod tilemap_editor_painting_test;
mod tilemap_parity_test;
