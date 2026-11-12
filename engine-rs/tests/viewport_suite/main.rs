// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test viewport_suite -E 'test(/^<mod>::/)'
mod viewport_2d_pan_zoom_grid_test;
mod viewport_2d_parity_test;
mod viewport_2d_snap_grid_smart_test;
mod viewport_3d_environment_preview_test;
mod viewport_3d_node_selection_gizmo_test;
mod viewport_golden_image_regression_test;
mod viewport_golden_image_test;
