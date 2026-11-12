// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test release_suite -E 'test(/^<mod>::/)'
mod release_delta_461_audit_test;
mod release_train_audit_test;
mod release_train_workflow_test;
