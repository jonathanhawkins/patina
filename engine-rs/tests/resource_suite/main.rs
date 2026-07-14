// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test resource_suite -E 'test(/^<mod>::/)'
mod resource_cache_concrete_resolve_parity_test;
mod resource_cache_concrete_subresource_test;
mod resource_cache_lifecycle_broad_test;
mod resource_cache_mutation_invalidation_test;
mod resource_cache_subresource_parity_test;
mod resource_cache_subresource_resolve_parity_test;
mod resource_cache_subresource_roundtrip_parity_test;
mod resource_edge_cases_breadth_test;
mod resource_ext_ref_fidelity_test;
mod resource_metadata_probe_parity_test;
mod resource_oracle_comparison_test;
mod resource_roundtrip_equivalence_test;
mod resource_roundtrip_parity_test;
mod resource_scene_broad_execution_test;
mod resource_scene_broader_execution_test;
mod resource_scene_execution_path_broad_test;
mod resource_type_breadth_parity_test;
mod resource_uid_cache_loader_integration_test;
mod resource_uid_cache_test;
mod resource_uid_loader_path_parity_test;
mod resource_uid_registry_test;
mod resource_uid_res_precedence_parity_test;
mod resource_validation_probe_parity_test;
