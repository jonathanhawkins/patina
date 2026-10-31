// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test misc_suite_c -E 'test(/^<mod>::/)'
mod ext_resource_cross_file_test;
mod file_access_parity_test;
mod fileaccess_typed_io_script_roundtrip;
mod filesystem_dock_directory_tree_test;
mod fixed_step_body_sync_parity_test;
mod fixture_corpus_coverage_test;
mod fog_volume_volumetric_regions_test;
mod frame_evolution_coverage_test;
mod frame_processing_semantics_test;
mod func_dispatch_method_table_test;
mod gdext_lab_461_verification_test;
mod gdphysics2d_scene_connection_parity_test;
mod gdphysics2d_scene_fixed_step_test;
mod geometry2d_singleton_parity_test;
mod global_lifecycle_signal_ordering_trace_parity_test;
mod golden_refresh_repin_test;
mod golden_staleness_test;
mod hierarchy_3d_fixture_parity_test;
mod import_parity_test;
mod import_settings_panel_test;
mod instance_id_oracle_property_shape_test;
