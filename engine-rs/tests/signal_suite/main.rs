// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test signal_suite -E 'test(/^<mod>::/)'
mod signal_argument_forwarding_parity_test;
mod signal_declaration_emit_test;
mod signal_instantiation_parity_test;
mod signal_parity_breadth_test;
mod signal_trace_oracle_comparison_test;
