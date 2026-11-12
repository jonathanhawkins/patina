// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test node_suite -E 'test(/^<mod>::/)'
mod node_creation_dialog_test;
mod node_id_461_oracle_verification_test;
mod node_instance_id_parity_test;
