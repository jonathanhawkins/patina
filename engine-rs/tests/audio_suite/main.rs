// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test audio_suite -E 'test(/^<mod>::/)'
mod audio_deterministic_harness_test;
mod audio_import_pipeline_test;
mod audio_nav_ui_fixture_scene_test;
mod audio_runtime_broad_test;
mod audio_smoke_test;
