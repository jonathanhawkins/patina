// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test scene_suite -E 'test(/^<mod>::/)'
mod scene_golden_regenerate_test;
mod scene_instancing_edge_cases_test;
mod scene_lifecycle_notification_461_revalidation_test;
mod scene_lifecycle_packed_transition_parity_test;
mod scene_oracle_breadth_test;
mod scene_oracle_golden_comparison_test;
mod scene_tree_broad_execution_test;
mod scene_tree_root_window_gap_test;
