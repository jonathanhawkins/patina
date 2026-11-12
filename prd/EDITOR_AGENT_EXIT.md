# Editor + Agent-Driveable Runtime Exit Criteria

This is the active exit-criteria document for the post-V1 phase.

Goal: make the Rust editor production-quality and fully driveable by browser-based AI agents over the existing HTTP API.

V1 runtime parity is already complete (see `V1_EXIT_CRITERIA.md`, historical). This document supersedes it as the planner's source of truth.

Each criterion below names the test file that proves it. The verifier lane runs those tests on every claimed bead.

## Security

- [ ] Editor HTTP server requires bearer token authentication on every `/api/*` endpoint (test: `editor_auth_required_test`)
- [ ] Filesystem endpoints reject paths outside the project root with HTTP 403 (test: `editor_filesystem_sandbox_test`)
- [ ] Every state-mutating REST call is recorded in an append-only audit log (test: `editor_audit_log_test`)
- [ ] Per-token rate limit returns HTTP 429 above a configurable request budget (test: `editor_rate_limit_test`)
- [ ] CORS allowlist is configurable and rejects disallowed origins with HTTP 403 (test: `editor_cors_allowlist_test`)

## Agent Driveability

- [ ] `GET /api/capabilities` returns a stable machine-readable schema of every route, method, params, and response shape (test: `editor_capabilities_schema_test`)
- [ ] OpenAPI 3 spec is generated from the live route table and checked into the repo (test: `editor_openapi_validity_test`)
- [ ] State-mutating endpoints honor an `Idempotency-Key` header — duplicate requests return the cached response without re-applying state (test: `editor_idempotency_test`)
- [ ] `/api/events` WebSocket stream broadcasts every scene-tree mutation to all connected clients in order (test: `editor_events_ws_test`)
- [ ] Concurrent edits to the same node by two clients return HTTP 409 to the loser (test: `editor_concurrent_edit_conflict_test`)
- [ ] Editor server can be driven headlessly by a single curl script that creates a scene, adds nodes, saves, reloads, and verifies state (test: `editor_headless_roundtrip_test`)

## Quality & Reliability

- [ ] Scene save is atomic — a SIGKILL mid-save leaves either the prior scene or the new scene, never a corrupt or partial file (test: `editor_atomic_save_test`)
- [ ] Editor process survives 1000 hot-reload cycles without leaking file handles, sockets, or scene IDs (test: `editor_hot_reload_stability_test`)
- [ ] wgpu is the default renderer for the editor viewport — the software rasterizer is opt-in behind a feature flag (test: `editor_default_backend_wgpu_test`)
- [ ] `/api/viewport` p99 latency is under 50 ms over 1000 sequential calls on the CI baseline (test: `editor_viewport_latency_budget_test`)
- [ ] Error responses follow a consistent JSON envelope: `{"error": {"code": "<machine_code>", "message": "<human>"}}` (test: `editor_error_envelope_test`)

## Visual Parity

- [ ] Inspector panel DOM matches Godot inspector on five reference scenes (test: `editor_inspector_dom_parity_test`)
- [ ] Scene Tree dock DOM matches Godot scene tree on five reference scenes (test: `editor_scene_tree_dom_parity_test`)
- [ ] Bottom panel set — output, debugger, audio, shader, animation — matches Godot bottom panels (test: `editor_bottom_panel_parity_test`)
- [ ] Viewport pixel-diff against Godot viewport is within 2% mean error on five reference scenes (test: `editor_viewport_golden_parity_test`)
