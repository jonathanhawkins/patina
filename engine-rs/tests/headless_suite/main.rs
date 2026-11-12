// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test headless_suite -E 'test(/^<mod>::/)'
mod headless_demo_smoke_test;
mod headless_milestone_report_test;
mod headless_mode_ci_test;
