// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test ci_suite -E 'test(/^<mod>::/)'
mod ci_3d_crate_boundary_audit_test;
mod ci_build_matrix_platform_test;
mod ci_domain_coverage_validation_test;
mod ci_golden_bless_gate_test;
mod ci_render_golden_gate_test;
mod ci_repin_lane_validation_test;
mod ci_sharded_integration_tests_test;
