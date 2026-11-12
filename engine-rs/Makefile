.PHONY: test test-render test-render-ci test-fast test-golden

# Run all tests (Tier 3 — full suite)
test:
	cargo test --workspace

# Run all render pixel tests (CPU-only, headless, CI-safe)
test-render:
	cargo test --test render_suite -- \
		render_draw_ordering_test \
		render_camera_viewport_test \
		render_sprite_property_test \
		render_golden_test \
		render_vertical_slice_test

# CI render gate (`.github/workflows/ci.yml` rust-render-goldens runs this).
# The integration tests live in suite binaries (`tests/<suite>/`), so each
# phase names its suite and filters by module.
test-render-ci:
	# Phase 1 — goldens are not stale against the pinned upstream
	cargo test --test misc_suite_c -- golden_staleness_test
	# Phase 2 — render goldens (the same set as test-render)
	cargo test --test render_suite -- \
		render_draw_ordering_test \
		render_camera_viewport_test \
		render_sprite_property_test \
		render_golden_test \
		render_vertical_slice_test
	# Phase 3 — the gate's own wiring audit
	cargo test --test ci_suite -- ci_render_golden_gate_test

# Tier 1 — fast unit/integration tests (<10s)
test-fast:
	cargo test --workspace -- --skip golden --skip stress --skip render_golden --skip staleness --skip bench_

# Tier 2 — golden comparison tests (~30s)
test-golden:
	cargo test --workspace -- --skip stress --skip render_golden --skip bench_
