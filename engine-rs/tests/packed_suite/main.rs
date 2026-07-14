// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test packed_suite -E 'test(/^<mod>::/)'
mod packed_scene_edge_cases_test;
mod packed_scene_ext_sub_advanced_edge_cases_test;
mod packed_scene_ext_sub_edge_cases_test;
mod packed_scene_instancing_ownership_unique_name_test;
mod packed_scene_save_restore_roundtrip_test;
