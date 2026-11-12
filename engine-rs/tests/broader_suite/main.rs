// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test broader_suite -E 'test(/^<mod>::/)'
mod broader_integration_audio_nav_ui_test;
mod broader_integration_fixtures_test;
mod broader_integration_workflow_test;
