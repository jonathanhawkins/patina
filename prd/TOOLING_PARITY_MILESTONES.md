# Tooling Parity Milestones — Phase 8

This document enumerates the editor- and CLI-tooling milestones the Patina engine targets for Godot parity, with measurable exit evidence per milestone. The list is checked by `engine-rs/tests/docs_tooling_milestones_test.rs`, which fails if any milestone loses required fields.

Each milestone is an H3 with prefix "Milestone:" followed by the name, then bulleted Owner / Exit evidence / Status fields. Status is one of: not-started, in-progress, done.

---

### Milestone: Editor Server REST Surface

- Owner: editor-team
- Exit evidence: `engine-rs/tests/editor_architecture_plan_test.rs` enumerates documented modules and REST endpoints, asserting every endpoint in `docs/EDITOR_ARCHITECTURE.md` has a matching route in `gdeditor::editor_server`.
- Status: done

### Milestone: Editor Compatibility Layer

- Owner: editor-team
- Exit evidence: `engine-rs/tests/editor_compat_layer_test.rs` and `engine-rs/tests/editor_compat_layer_surface_test.rs` exercise `gdeditor::editor_compat` against `prd/EDITOR_COMPAT_LAYER_SPEC.md`; the surface test fails the build if any documented type is added or removed without spec sync.
- Status: done

### Milestone: Project Scaffolding CLI

- Owner: cli-team
- Exit evidence: `engine-rs/tests/project_scaffolding_cli_test.rs` invokes the scaffolding command in a tempdir and asserts the produced directory tree matches the documented Godot-style project layout.
- Status: in-progress

### Milestone: Headless Export Pipeline

- Owner: platform-team
- Exit evidence: `engine-rs/tests/startup_runtime_packaging_flow_test.rs` runs bootstrap → frame loop → packaging → verify end-to-end and asserts produced artifacts match `prd/PHASE7_PLATFORM_PARITY_AUDIT.md` shape.
- Status: done

### Milestone: Inspector Property Editors

- Owner: editor-team
- Exit evidence: `engine-rs/tests/property_inspector_typed_editors_test.rs` and `property_inspector_resource_sub_editor_test.rs` confirm every Godot inspector editor type listed in the editor architecture doc has a corresponding adapter under `gdeditor::property_inspector`.
- Status: in-progress

### Milestone: Benchmark Dashboard CI Gate

- Owner: perf-team
- Exit evidence: `engine-rs/tests/benchmark_dashboard_audit_test.rs` and `engine-rs/tests/perf_benchmark_ci_gate_test.rs` enforce the `prd/PHASE9_HARDENING_AUDIT.md` baselines and fail CI on regression beyond the published threshold.
- Status: done

### Milestone: Migration Guide Validation

- Owner: docs-team
- Exit evidence: `engine-rs/tests/migration_guide_validation_test.rs` parses `docs/migration-guide.md` and asserts the section index, crate references, target triples, and Godot concept mapping table remain complete.
- Status: done
