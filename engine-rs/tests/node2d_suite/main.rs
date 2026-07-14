// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test node2d_suite -E 'test(/^<mod>::/)'
mod node2d_body_sync_parity_test;
mod node2d_canvas_ordering_parity_test;
mod node2d_physics_sync_fixtures_parity_test;
mod node2d_properties_breadth_test;
