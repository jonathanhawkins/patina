// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test fuzz_suite -E 'test(/^<mod>::/)'
mod fuzz_binary_res_format_test;
mod fuzz_collision_property_test;
mod fuzz_geometry2d_property_test;
mod fuzz_input_weakref_classdb_property_test;
mod fuzz_property_audit_test;
mod fuzz_property_coverage_test;
mod fuzz_property_runtime_test;
mod fuzz_resource_loader_roundtrip_test;
mod fuzz_resource_parser_property_test;
mod fuzz_scene_tree_property_test;
mod fuzz_signal_property_test;
mod fuzz_tres_tscn_parser_test;
mod fuzz_variant_coercion_test;
mod fuzz_variant_property_test;
mod fuzz_variant_serialization_roundtrip_test;
