# Tooling Milestones Specification

Date: 2026-04-07
Bead: `pat-4vy88`
Source audit: `prd/PHASE8_EDITOR_PARITY_AUDIT.md`
Dependency: `pat-6m9ky` (editor-facing compatibility layer)

## Purpose

This document is the formal enumeration of Patina's **selected tooling parity
milestones** for Phase 8 editor work. It maps each milestone to a concrete
tested editor slice, lists the smallest exit evidence that defends the claim,
and classifies each as measured, implemented-not-measured, deferred, or missing.

This is the operational companion to the Phase 8 audit's milestone inventory
table. If a milestone is added, removed, or retitled, update this document,
the audit table, and the validation test
(`engine-rs/tests/tooling_parity_milestone_test.rs`) together.

## Scope

These milestones exercise Patina's **editor tooling slices**, not full Godot
editor parity. Each milestone maps to a specific audit family and
classification. Broader editor behavior (plugin ecosystem, native editor shell)
is outside this scope.

The milestones are bounded to their concrete tested slices. For example,
Milestone 2 (Inspector) is measured for "property editor registry and Variant
coercion", not for the full Godot inspector surface.

## Relationship to the Compatibility Layer

The compatibility layer (`pat-6m9ky`, specified in
`prd/EDITOR_COMPAT_LAYER_SPEC.md`) covers `editor_compat` and
`editor_interface` — the Godot-compatible API names and type aliases.

Tooling milestones cover **everything else** in `gdeditor` that has measured
test evidence: specialized editors, dock panels, import pipeline, profiler,
command palette, and structural gates. Tooling modules may use the compatibility
layer, but they are not part of it.

## Classification Key

| Classification | Meaning |
|----------------|---------|
| Measured | Automated tests verify behavior; the claim is machine-checkable |
| Measured (structural) | Tests verify module/file existence and inventory, not runtime behavior |
| Measured for local model slice | Tests exercise a local model (settings, presets, status) but not a live upstream comparison |
| Implemented, not yet measured | Code exists but lacks parity-quality tests |
| Deferred | Explicitly out of scope for this milestone |
| Missing | Not implemented |

## Milestone Inventory

### Milestone 1: Editor Crate Structure

- **Tooling family:** Editor crate structure
- **Concrete tested slice:** Module inventory and boundary shape
- **Classification:** Measured (structural)
- **Exit evidence:**
  - `engine-rs/tests/tooling_parity_milestone_test.rs` — `milestone1_editor_crate_has_core_modules`, `milestone1_editor_has_viewport_modules`, `milestone1_editor_has_advanced_panels`
  - `engine-rs/crates/gdeditor/src/` module inventory (≥30 `.rs` files)
- **What is measured:** The `gdeditor` crate contains all required module files: inspector, dock, undo_redo, editor_plugin, script_editor, export_dialog, editor_server, settings, filesystem, import, viewport_2d, viewport_3d, profiler_panel, output_panel, animation_editor, shader_editor, theme_editor, tilemap_editor, command_palette.
- **What is NOT measured:** Runtime behavior of these modules (covered by milestones 2–14).

### Milestone 2: Inspector Typed Editors

- **Tooling family:** Inspector typed editors
- **Concrete tested slice:** Property editor registry and Variant coercion
- **Classification:** Measured for tested slice
- **Exit evidence:**
  - `engine-rs/tests/property_inspector_typed_editors_test.rs`
  - `engine-rs/tests/property_inspector_resource_sub_editor_test.rs`
  - `engine-rs/tests/tooling_parity_milestone_test.rs` — `milestone2_*` tests
- **What is measured:** InspectorPanel construction, SectionedInspector grouping, InspectorPluginRegistry, PropertyHint variants (None, Range, Enum), Variant coercion (int→float), Variant validation.
- **What is NOT measured:** Full Godot inspector parity (toolbar, history, sub-resource navigation, property favorites, grouped exports).

### Milestone 3: Undo/Redo Command Pattern

- **Tooling family:** Undo/redo command pattern
- **Concrete tested slice:** Empty-stack behavior and command execution
- **Classification:** Measured for tested slice
- **Exit evidence:**
  - `engine-rs/tests/tooling_parity_milestone_test.rs` — `milestone3_editor_create`, `milestone3_undo_redo_empty_stack`
- **What is measured:** Editor construction from SceneTree, undo on empty stack returns error, redo on empty stack returns error.
- **What is NOT measured:** Multi-action undo chains, merge groups, action naming, transaction nesting.

### Milestone 4: Dock Panels

- **Tooling family:** Dock panels
- **Concrete tested slice:** Scene tree dock and property dock shape
- **Classification:** Measured for tested slice
- **Exit evidence:**
  - `engine-rs/tests/tooling_parity_milestone_test.rs` — `milestone4_scene_tree_dock`, `milestone4_property_dock`, `milestone4_dock_panel_trait`
- **What is measured:** SceneTreeDock construction (empty entries), PropertyDock wrapping InspectorPanel, DockPanel trait (title method).
- **What is NOT measured:** Full dock panel interactions (drag-drop, context menus, hierarchy operations, indicators).

### Milestone 5: Script Editor Core

- **Tooling family:** Script editor core
- **Concrete tested slice:** Find/replace and syntax-aware editor plumbing
- **Classification:** Measured for tested slice
- **Exit evidence:**
  - `engine-rs/tests/script_editor_core_parity_test.rs`
  - `engine-rs/tests/tooling_parity_milestone_test.rs` — `milestone5_*` tests
- **What is measured:** ScriptEditor construction (zero tabs), FindReplace basic search (multi-match), FindReplace regex mode (function declarations).
- **What is NOT measured:** Syntax highlighting, code completion, breakpoints, minimap, diagnostics, method outline.

### Milestone 6: Export Dialog

- **Tooling family:** Export dialog
- **Concrete tested slice:** Export presets and platform coverage
- **Classification:** Measured for local model slice
- **Exit evidence:**
  - `engine-rs/tests/editor_systems_parity_test.rs`
  - `engine-rs/tests/tooling_parity_milestone_test.rs` — `milestone6_*` tests
- **What is measured:** ExportDialog construction (zero presets), ExportPreset creation with platform enum (Linux) and build profile (Release).
- **What is NOT measured:** Actual export execution, platform-specific export options, signing, notarization.

### Milestone 7: Editor/Project Settings

- **Tooling family:** Editor/project settings
- **Concrete tested slice:** Settings defaults and round-trip persistence
- **Classification:** Measured for local model slice
- **Exit evidence:**
  - `engine-rs/tests/editor_systems_parity_test.rs`
  - `engine-rs/tests/tooling_parity_milestone_test.rs` — `milestone7_*` tests
- **What is measured:** EditorSettings default (Dark theme), ProjectSettingsDialog Application category has properties.
- **What is NOT measured:** Full settings persistence, all Godot project settings categories, settings import/export.

### Milestone 8: VCS Integration

- **Tooling family:** VCS integration
- **Concrete tested slice:** Branch/status model coverage
- **Classification:** Measured for local model slice
- **Exit evidence:**
  - `engine-rs/tests/editor_systems_parity_test.rs`
  - `engine-rs/tests/vcs_git_status_integration_test.rs`
  - `engine-rs/tests/tooling_parity_milestone_test.rs` — `milestone8_*` tests
- **What is measured:** FileChangeStatus variants (Modified, Added, Deleted, Untracked), BranchInfo struct (name, ahead, behind, detached).
- **What is NOT measured:** Live git operations, diff display, commit/push from editor, merge conflict resolution.

### Milestone 9: Shader Editor

- **Tooling family:** Shader editor
- **Concrete tested slice:** Shader editor construction and highlighting
- **Classification:** Measured for tested slice
- **Exit evidence:**
  - `engine-rs/tests/tooling_parity_milestone_test.rs` — `milestone9_*` tests
  - `engine-rs/crates/gdeditor/src/shader_editor.rs`
- **What is measured:** ShaderEditor construction (zero tabs), ShaderHighlighter parsing GLSL-style fragment shader, highlight span generation.
- **What is NOT measured:** Visual shader graph, shader preview, shader parameter editing, full GLSL/Godot shader language coverage.

### Milestone 10: Theme Editor

- **Tooling family:** Theme editor
- **Concrete tested slice:** Theme editing and live preview
- **Classification:** Measured for tested slice
- **Exit evidence:**
  - `engine-rs/tests/theme_editor_live_preview_test.rs`
  - `engine-rs/tests/theme_editor_overrides_test.rs`
  - `engine-rs/tests/tooling_parity_milestone_test.rs` — `milestone10_*` tests
- **What is measured:** ThemeEditor construction (zero overrides), ThemeResource default (no font, zero overrides).
- **What is NOT measured:** Full theme property editing, style box editing, font configuration, live widget preview across all control types.

### Milestone 11: Command Palette

- **Tooling family:** Command palette
- **Concrete tested slice:** Command registration and fuzzy search behavior
- **Classification:** Measured for tested slice
- **Exit evidence:**
  - `engine-rs/tests/command_palette_fuzzy_search_test.rs`
  - `engine-rs/tests/tooling_parity_milestone_test.rs` — `milestone11_*` tests
- **What is measured:** CommandPalette construction, command count API.
- **What is NOT measured:** Full fuzzy matching algorithm quality, keybinding integration, command categories, recent command history.

### Milestone 12: Import Pipeline

- **Tooling family:** Import pipeline
- **Concrete tested slice:** Import pipeline and importer registry
- **Classification:** Implemented, not yet measured
- **Exit evidence:**
  - `engine-rs/tests/audio_import_pipeline_test.rs`
  - `engine-rs/tests/import_settings_panel_test.rs`
  - `engine-rs/tests/tooling_parity_milestone_test.rs` — `milestone12_*` tests
- **What is measured:** ImportPipeline construction (zero importers), SceneFormatImporterRegistry supported_extensions API.
- **What is NOT measured:** Actual import execution, reimport workflows, import presets, import dock integration. The "not yet measured" classification reflects that existing tests exercise the API shape but not parity-quality import behavior.

### Milestone 13: Editor Server

- **Tooling family:** Editor server
- **Concrete tested slice:** HTTP surface and browser/editor-shell bootstrap
- **Classification:** Measured for bounded slice
- **Exit evidence:**
  - `engine-rs/tests/editor_smoke_test.rs`
  - `engine-rs/tests/editor_461_revalidation_test.rs`
  - `engine-rs/tests/tooling_parity_milestone_test.rs` — `milestone13_*` tests
- **What is measured:** editor_server.rs module existence (structural gate). The broader evidence is in the smoke/revalidation tests which exercise HTTP endpoints, scene round-trips, and runtime integration.
- **What is NOT measured:** WebSocket real-time sync, multi-client sessions, full REST API coverage for all 120+ documented endpoints.

### Milestone 14: Profiler Panel

- **Tooling family:** Profiler panel
- **Concrete tested slice:** Frame profile aggregation and panel state
- **Classification:** Measured for tested slice
- **Exit evidence:**
  - `engine-rs/tests/tooling_parity_milestone_test.rs` — `milestone14_*` tests
  - `engine-rs/crates/gdeditor/src/profiler_panel.rs`
- **What is measured:** ProfilerPanel construction (capacity, zero frames), FrameProfile struct (frame_number, cpu/gpu/physics times, entries), cpu_time_ms conversion.
- **What is NOT measured:** Live frame capture, GPU profiling integration, visual flamegraph, per-function breakdown.

### Milestone 15: Module Count Gate

- **Tooling family:** Module count gate
- **Concrete tested slice:** Editor module breadth check
- **Classification:** Measured (structural)
- **Exit evidence:**
  - `engine-rs/tests/tooling_parity_milestone_test.rs` — `milestone15_editor_has_at_least_30_modules`
- **What is measured:** `gdeditor/src/` contains ≥30 `.rs` files (excluding lib.rs).
- **What is NOT measured:** Quality or completeness of individual modules.

### Milestone 16: Editor Test Coverage Gate

- **Tooling family:** Editor test coverage gate
- **Concrete tested slice:** Editor integration test breadth check
- **Classification:** Measured (structural)
- **Exit evidence:**
  - `engine-rs/tests/tooling_parity_milestone_test.rs` — `milestone16_editor_integration_tests_exist`
- **What is measured:** `engine-rs/tests/` contains ≥5 files matching `*editor*_test.rs`.
- **What is NOT measured:** Test quality, coverage percentage, or pass rates.

## Summary Table

| # | Family | Classification | Primary Evidence |
|---|--------|---------------|------------------|
| 1 | Editor crate structure | Measured (structural) | `tooling_parity_milestone_test.rs` M1 |
| 2 | Inspector typed editors | Measured | `property_inspector_typed_editors_test.rs`, M2 |
| 3 | Undo/redo command pattern | Measured | `tooling_parity_milestone_test.rs` M3 |
| 4 | Dock panels | Measured | `tooling_parity_milestone_test.rs` M4 |
| 5 | Script editor core | Measured | `script_editor_core_parity_test.rs`, M5 |
| 6 | Export dialog | Measured (local model) | `editor_systems_parity_test.rs`, M6 |
| 7 | Editor/project settings | Measured (local model) | `editor_systems_parity_test.rs`, M7 |
| 8 | VCS integration | Measured (local model) | `vcs_git_status_integration_test.rs`, M8 |
| 9 | Shader editor | Measured | `tooling_parity_milestone_test.rs` M9 |
| 10 | Theme editor | Measured | `theme_editor_live_preview_test.rs`, M10 |
| 11 | Command palette | Measured | `command_palette_fuzzy_search_test.rs`, M11 |
| 12 | Import pipeline | Implemented, not yet measured | `audio_import_pipeline_test.rs`, M12 |
| 13 | Editor server | Measured (bounded) | `editor_smoke_test.rs`, M13 |
| 14 | Profiler panel | Measured | `tooling_parity_milestone_test.rs` M14 |
| 15 | Module count gate | Measured (structural) | `tooling_parity_milestone_test.rs` M15 |
| 16 | Editor test coverage gate | Measured (structural) | `tooling_parity_milestone_test.rs` M16 |

## Gaps and Next Steps

### Currently Not Measured (Milestone 12)

Milestone 12 (Import pipeline) is classified as "Implemented, not yet measured."
The existing tests exercise API shape (ImportPipeline, SceneFormatImporterRegistry)
but do not verify import behavior against Godot's import pipeline. Upgrading this
to "Measured" requires:

- Tests that import a known asset and verify the output matches Godot's import
- Coverage of reimport behavior and import preset round-trips

### Bounded Claims

All "Measured" milestones are bounded to their concrete tested slices. The gap
between "measured slice" and "full Godot parity" is documented per-milestone
in the "What is NOT measured" section above. Expanding any milestone's scope
requires new test evidence and an update to both this document and the audit.

## Validation

This specification is validated by:

- `engine-rs/tests/tooling_parity_milestone_test.rs` — 16 milestone groups with
  individual test functions per capability
- Audit sync tests in the same file:
  - `audit_sync_phase8_doc_exists_and_cites_tooling_bead` — Phase 8 audit references pat-4vy88
  - `audit_sync_documents_all_tooling_families` — audit mentions all 14 tooling families
  - `audit_sync_measured_families_have_test_evidence` — each family has at least one evidence path
  - `audit_sync_milestone_count_matches_audit_claim` — exactly 16 milestone numbers exist

## Boundary Rule

A tooling milestone claim is valid if and only if:

1. It appears in this document with a classification, AND
2. It maps to a concrete tested slice with cited test files, AND
3. The "What is measured" section accurately reflects what the tests verify, AND
4. The "What is NOT measured" section documents known gaps

If a new milestone is added or an existing one's scope changes, update this
document, the Phase 8 audit table, and the validation test together.
