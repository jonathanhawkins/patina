// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test physics_suite -E 'test(/^<mod>::/)'
mod physics_integration_test;
mod physics_playground_extended_golden_test;
mod physics_playground_golden_trace_test;
mod physics_ray_shape_query3d_test;
mod physics_server_2d_api_test;
mod physics_stepping_contract_parity_test;
mod physics_trace_golden_parity_test;
