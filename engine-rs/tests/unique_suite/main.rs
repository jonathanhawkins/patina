// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test unique_suite -E 'test(/^<mod>::/)'
mod unique_name_nodepath_461_revalidation_test;
mod unique_name_oracle_test;
mod unique_name_parity_test;
