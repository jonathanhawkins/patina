// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test object_suite -E 'test(/^<mod>::/)'
mod object_free_use_after_free_test;
mod object_lifetime_parity_test;
mod object_property_reflection_test;
