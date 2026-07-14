// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test window_suite -E 'test(/^<mod>::/)'
mod window_creation_abstraction_test;
mod window_lifecycle_parity_test;
mod window_lifecycle_test;
