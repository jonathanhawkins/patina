.PHONY: test test-render test-fast test-golden

# Run all tests (Tier 3 — full suite)
test:
	cargo test --workspace

# Run all render pixel tests (CPU-only, headless, CI-safe)
test-render:
	cargo test --workspace \
		-p gdrender2d \
		--test render_draw_ordering_test \
		--test render_camera_viewport_test \
		--test render_sprite_property_test \
		--test render_golden_test \
		--test render_vertical_slice_test

# Tier 1 — fast unit/integration tests (<10s)
test-fast:
	cargo test --workspace -- --skip golden --skip stress --skip render_golden --skip staleness --skip bench_

# Tier 2 — golden comparison tests (~30s)
test-golden:
	cargo test --workspace -- --skip stress --skip render_golden --skip bench_
