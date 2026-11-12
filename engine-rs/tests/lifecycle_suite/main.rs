// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test lifecycle_suite -E 'test(/^<mod>::/)'
mod lifecycle_root_inside_tree_regression_test;
mod lifecycle_signal_global_order_parity_test;
mod lifecycle_trace_oracle_parity_test;
