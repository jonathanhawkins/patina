// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test notification_suite -E 'test(/^<mod>::/)'
mod notification_coverage_test;
mod notification_extended_parity_test;
mod notification_lifecycle_breadth_test;
mod notification_nonlifecycle_parity_test;
mod notification_ordering_parity_test;
