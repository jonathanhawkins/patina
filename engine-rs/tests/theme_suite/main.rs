// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test theme_suite -E 'test(/^<mod>::/)'
mod theme_editor_live_preview_test;
mod theme_editor_overrides_test;
mod theme_parity_test;
