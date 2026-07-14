// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test script_suite -E 'test(/^<mod>::/)'
mod script_completion_integration_test;
mod script_editor_breakpoint_bookmark_gutter_test;
mod script_editor_core_parity_test;
mod script_editor_find_replace_regex_test;
mod script_editor_syntax_highlight_test;
mod script_fixture_oracle_match_test;
mod script_frame_evolution_breadth_test;
mod script_var_export_regression_test;
