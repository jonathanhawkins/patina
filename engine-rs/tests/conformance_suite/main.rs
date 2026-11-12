// Consolidated integration-test binary: each module here is one former
// standalone tests/*.rs, so a single engine link serves them all. Run one
// module's tests with
//   cargo nextest run --test conformance_suite -E 'test(/^<mod>::/)'
mod conformance_gate_test;
