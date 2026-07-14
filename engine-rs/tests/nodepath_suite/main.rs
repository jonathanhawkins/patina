// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test nodepath_suite -E 'test(/^<mod>::/)'
mod nodepath_breadth_test;
mod nodepath_generic_nodeid_resolver_parity_test;
mod nodepath_resolution_test;
