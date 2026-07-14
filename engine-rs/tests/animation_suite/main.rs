// Consolidated integration-test binary: each former standalone
// tests/*.rs is a module here, so one engine link serves them all
// (build-perf Tier 3). Run one module's tests with
//   cargo nextest run --test animation_suite -E 'test(/^<mod>::/)'
mod animation_blending_preview_test;
mod animation_editor_parity_test;
mod animation_parity_test;
mod animation_timeline_keyframe_editing_test;
mod animation_track_creation_test;
mod animation_track_creation_typed_test;
