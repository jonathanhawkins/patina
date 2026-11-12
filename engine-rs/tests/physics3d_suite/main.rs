// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test physics3d_suite -E 'test(/^<mod>::/)'
mod physics3d_deterministic_hooks_test;
mod physics3d_scene_hooks_deterministic_test;
mod physics3d_single_body_golden_test;
mod physics3d_trace_comparison_test;
