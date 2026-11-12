// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test bench_suite -E 'test(/^<mod>::/)'
mod bench_render_baselines;
mod bench_render_fixtures_report_test;
mod bench_runtime_baselines;
