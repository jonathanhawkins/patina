// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test crate_suite -E 'test(/^<mod>::/)'
mod crate_boundary_3d_audit_test;
mod crate_boundary_3d_review_test;
mod crate_set_3d_bootstrap_test;
