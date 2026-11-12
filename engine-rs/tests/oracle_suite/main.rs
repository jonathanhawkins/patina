// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test oracle_suite -E 'test(/^<mod>::/)'
mod oracle_parity_461_metrics_test;
mod oracle_parity_test;
mod oracle_regression_test;
