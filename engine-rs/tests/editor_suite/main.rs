// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test editor_suite -E 'test(/^<mod>::/)'
mod editor_461_revalidation_test;
mod editor_animation_persistence_test;
mod editor_architecture_plan_test;
mod editor_atomic_save_test;
mod editor_audit_log_test;
mod editor_auth_required_test;
mod editor_capabilities_schema_test;
mod editor_compat_layer_surface_test;
mod editor_compat_layer_test;
mod editor_concurrent_edit_conflict_test;
mod editor_cors_allowlist_test;
mod editor_default_backend_wgpu_test;
mod editor_dom_parity_test;
mod editor_error_envelope_test;
mod editor_events_ws_test;
mod editor_filesystem_sandbox_test;
mod editor_headless_roundtrip_test;
mod editor_hot_reload_stability_test;
mod editor_idempotency_test;
mod editor_inspector_dom_parity_test;
mod editor_integration_test;
mod editor_interface_compat_test;
mod editor_menu_parity_test;
mod editor_openapi_validity_test;
mod editor_parity_bootstrap_map_test;
mod editor_plugin_api_tool_script_test;
mod editor_rate_limit_test;
mod editor_scene_tree_dom_parity_test;
mod editor_smoke_test;
mod editor_systems_parity_test;
mod editor_test;
mod editor_viewport_latency_budget_test;
