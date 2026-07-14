// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test platform_suite -E 'test(/^<mod>::/)'
mod platform_backend_test;
mod platform_first_stable_layer_test;
mod platform_targets_validation_test;
