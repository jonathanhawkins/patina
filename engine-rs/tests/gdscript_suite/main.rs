// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test gdscript_suite -E 'test(/^<mod>::/)'
mod gdscript_classdb_dispatch_sweep_test;
mod gdscript_compatibility_reference_test;
mod gdscript_func_dispatch_method_table_test;
mod gdscript_instance_id_parity_test;
mod gdscript_node_core_methods_dispatch_test;
mod gdscript_interop_breadth_test;
mod gdscript_parity_test;
mod gdscript_parser_stable_ast_test;
mod gdscript_singleton_parity_test;
mod gdscript_v1_features_test;
mod gdscript_typed_var_defaults_test;
mod stringname_methods_operators_test;
mod variant_string_like_equality_test;
