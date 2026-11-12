// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test property_suite -E 'test(/^<mod>::/)'
mod property_inspector_resource_sub_editor_test;
mod property_inspector_typed_editors_test;
mod property_tests;
