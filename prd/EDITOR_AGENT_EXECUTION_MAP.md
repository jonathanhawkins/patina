# Editor + Agent Execution Map

This is the active bead execution map for the editor + agent-driveable phase.

Each entry is a single bead. Section headers (`## Now / Next / Later`) drive priority. The text after the backtick key becomes the bead title. The `Acceptance:` line becomes the bead's `acceptance_criteria` field that the verifier checks.

## Now

1. `editor-agent-auth-bearer-token` Add bearer token authentication to the editor HTTP server
   Acceptance: editor_auth_required_test proves every /api/* endpoint returns HTTP 401 without a valid bearer token in the Authorization header and HTTP 200 with one; tokens come from a config file or environment variable

2. `editor-agent-fs-sandbox-project-root` Sandbox editor filesystem endpoints to the project root
   Acceptance: editor_filesystem_sandbox_test proves /api/filesystem, /api/filesystem/tree, /api/filesystem/delete, /api/filesystem/mkdir, /api/filesystem/rename all reject paths containing `..` or pointing outside the project root with HTTP 403 and a machine-readable error code

3. `editor-agent-audit-log-mutations` Record every state-mutating REST call in an append-only audit log
   Acceptance: editor_audit_log_test proves a log file at .editor/audit.log captures (timestamp, token_id, method, path, status, body_hash) one line per state-mutating call, and read-only routes (GET) are not recorded

4. `editor-agent-rate-limit-per-token` Add a per-token rate limit to the editor HTTP server
   Acceptance: editor_rate_limit_test proves more than 60 requests per second from one token returns HTTP 429 with a Retry-After header, and a different token is unaffected

5. `editor-agent-cors-allowlist` Make CORS origin allowlist configurable for the editor server
   Acceptance: editor_cors_allowlist_test proves a request with Origin not in the configured allowlist returns HTTP 403 and a request with an allowed Origin returns the matching Access-Control-Allow-Origin header

## Next

6. `editor-agent-capabilities-endpoint` Add GET /api/capabilities returning a machine-readable route schema
   Acceptance: editor_capabilities_schema_test proves GET /api/capabilities returns a deterministic JSON document listing every route with its method, path, params, request body schema, and response shape; output is stable across runs

7. `editor-agent-openapi-spec` Generate and check in an OpenAPI 3 spec for the editor HTTP server
   Acceptance: editor_openapi_validity_test proves prd/editor_openapi.yaml is valid OpenAPI 3, parses with openapiv3 crate, and lists every route that /api/capabilities reports

8. `editor-agent-idempotency-key` Honor Idempotency-Key header on state-mutating endpoints
   Acceptance: editor_idempotency_test proves a POST with an Idempotency-Key returns the cached response and does not re-apply the mutation when sent twice within the cache window

9. `editor-agent-events-websocket` Add /api/events WebSocket broadcasting scene-tree mutations
   Acceptance: editor_events_ws_test proves two clients connected to /api/events both receive the same ordered stream of mutation events when a third client mutates the scene tree

10. `editor-agent-concurrent-edit-conflict` Detect concurrent edits to the same node and reject with HTTP 409
    Acceptance: editor_concurrent_edit_conflict_test proves a node has a monotonic version number; a PATCH that supplies a stale version returns HTTP 409; a fresh version succeeds

11. `editor-agent-headless-roundtrip` Provide a headless curl-driven roundtrip script proving an agent can drive the editor end to end
    Acceptance: editor_headless_roundtrip_test runs a script that uses only curl to create a scene, add three nodes, save the scene, reload from disk, and assert the tree matches; exits zero on success

12. `editor-agent-atomic-scene-save` Make scene save atomic with temp-write plus rename
    Acceptance: editor_atomic_save_test proves a SIGKILL injected mid-save leaves either the previous scene or the new scene on disk and never a partial or corrupt file across 100 iterations

13. `editor-agent-error-envelope` Standardize error response envelope across all REST routes
    Acceptance: editor_error_envelope_test proves every non-2xx response body is JSON of shape {"error": {"code": "<machine_code>", "message": "<human>"}} and codes are documented in prd/editor_error_codes.md

## Later

14. `editor-quality-wgpu-default-renderer` Make wgpu the default editor viewport renderer; software rasterizer becomes opt-in
    Acceptance: editor_default_backend_wgpu_test proves a default `cargo build -p gdeditor` links wgpu; the software path is gated behind --features software-render; the viewport pixel-matches a wgpu golden on GPU CI

15. `editor-quality-viewport-latency-budget` Establish and enforce a viewport latency budget
    Acceptance: editor_viewport_latency_budget_test measures /api/viewport p99 across 1000 sequential calls and asserts p99 < 50 ms on the CI baseline machine

16. `editor-quality-hot-reload-stability` Validate editor stability under 1000 hot-reload cycles
    Acceptance: editor_hot_reload_stability_test runs 1000 scene-reload cycles and asserts no leaked file descriptors, no leaked sockets, no growing scene-ID table

17. `editor-visual-inspector-dom-parity` DOM parity for Inspector panel across five reference scenes
    Acceptance: editor_inspector_dom_parity_test compares the rendered Inspector DOM for five reference scenes against Godot reference snapshots and asserts structural equivalence (same fields in the same order)

18. `editor-visual-scene-tree-dom-parity` DOM parity for Scene Tree dock across five reference scenes
    Acceptance: editor_scene_tree_dom_parity_test compares the rendered Scene Tree DOM for five reference scenes against Godot reference snapshots and asserts structural equivalence

19. `editor-visual-bottom-panel-parity` DOM parity for bottom panel set (output, debugger, audio, shader, animation)
    Acceptance: editor_bottom_panel_parity_test verifies each bottom panel's DOM matches the Godot reference layout for an empty scene and an active scene

20. `editor-visual-viewport-golden-parity` Viewport pixel-diff parity against Godot on five reference scenes
    Acceptance: editor_viewport_golden_parity_test renders five reference scenes through the Patina viewport and asserts mean pixel error against the Godot golden is under 2%
