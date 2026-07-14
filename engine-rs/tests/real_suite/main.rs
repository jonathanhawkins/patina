// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test real_suite -E 'test(/^<mod>::/)'
mod real_3d_demo_aggregate_parity_test;
mod real_3d_demo_parity_report_artifact_test;
mod real_3d_demo_unified_parity_test;
