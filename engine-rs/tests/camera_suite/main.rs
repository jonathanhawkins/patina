// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test camera_suite -E 'test(/^<mod>::/)'
mod camera_viewport_composition_parity_test;
mod camera_viewport_extended_parity_test;
mod camera_viewport_render_parity_extended_test;
