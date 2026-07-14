// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test docs_suite -E 'test(/^<mod>::/)'
mod docs_3d_demo_parity_report_test;
mod docs_contributor_onboarding_test;
mod docs_landed_networking_animation_audio_test;
mod docs_phase5_broader_integration_fixtures_test;
mod docs_phase5_first_audio_test_harness_test;
mod docs_phase5_improved_compat_matrix_test;
mod docs_phase5_initial_3d_architecture_spec_test;
mod docs_phase6_3d_fixture_corpus_test;
mod docs_phase6_first_3d_crate_set_test;
mod docs_phase6_render_physics_comparison_tooling_test;
mod docs_phase7_ci_matrix_supported_targets_test;
mod docs_phase7_desktop_platform_targets_test;
mod docs_phase7_gdplatform_first_stable_layer_test;
mod docs_phase7_startup_runtime_packaging_flow_test;
mod docs_phase8_editor_architecture_plan_test;
mod docs_phase8_minimal_editor_compat_layer_test;
mod docs_phase9_benchmark_dashboards_test;
mod docs_phase9_crash_triage_process_test;
mod docs_phase9_fuzz_property_high_risk_test;
mod docs_release_train_test;
mod docs_tooling_milestones_test;
