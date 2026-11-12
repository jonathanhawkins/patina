// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test phase_suite -E 'test(/^<mod>::/)'
mod phase6_3d_crate_boundary_audit_test;
mod phase6_3d_fixture_corpus_audit_test;
mod phase6_upstream_3d_class_surface_test;
mod phase7_ci_matrix_target_guard_test;
mod phase7_desktop_targets_doc_validation_test;
mod phase7_gdplatform_stable_layer_audit_test;
mod phase7_platform_claims_reconciliation_test;
mod phase7_startup_packaging_doc_validation_test;
mod phase8_editor_compat_layer_validation_test;
mod phase8_editor_compatibility_layer_scope_test;
mod phase8_editor_tooling_classification_test;
