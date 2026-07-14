// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test input_suite -E 'test(/^<mod>::/)'
mod input_action_binding_parity_test;
mod input_action_coverage_test;
mod input_edge_transition_fixed_idle_test;
mod input_map_loading_test;
mod input_snapshot_routing_parity_test;
