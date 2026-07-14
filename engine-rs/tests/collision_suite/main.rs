// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test collision_suite -E 'test(/^<mod>::/)'
mod collision_layers_masks_parity_test;
mod collision_overlap_extended_parity_test;
mod collision_shape_3d_all_types_test;
