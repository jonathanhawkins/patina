// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test keyboard_suite -E 'test(/^<mod>::/)'
mod keyboard_action_snapshot_engine_api_parity_test;
mod keyboard_action_snapshot_extended_parity_test;
mod keyboard_action_snapshot_parity_test;
