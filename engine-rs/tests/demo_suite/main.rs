// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test demo_suite -E 'test(/^<mod>::/)'
mod demo_2d_test;
mod demo_3d_parity_report_test;
mod demo_3d_real_parity_report_test;
mod demo_3d_report_doc_test;
mod demo_scenes_test;
