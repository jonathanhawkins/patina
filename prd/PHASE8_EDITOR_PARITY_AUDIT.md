# Phase 8 Editor Parity Audit

Date: 2026-03-29
Target upstream: Godot `4.6.1-stable`
Patina phase: `Phase 8 - Editor-Facing Work`

## Purpose

This document turns Phase 8 from a broad editor roadmap item into a parity
audit for Patina's current editor-facing slice.

It answers four questions:

1. What editor-facing behavior does Godot expose in the Phase 8 scope?
2. What does Patina currently implement and measure?
3. Where do Patina docs overclaim relative to measured evidence?
4. Which remaining gaps should become beads without duplicating existing work?

## Audit Rules

Use this workflow for all future Phase 8 parity work.

1. Scope only the current editor-facing compatibility layer and tooling slice.
2. Keep browser/editor-shell coverage distinct from full Godot editor parity.
3. Classify each family as one of:
   - `Measured`
   - `Implemented, not yet measured`
   - `Deferred`
   - `Missing`
4. Do not create a new bead if an active or closed bead already covers the
   same measurable outcome.
5. Prefer one bead per feature cluster, not one bead per widget or panel.
6. Treat maintenance/revalidation coverage separately from broader parity claims.

## Sources To Compare

### Upstream Godot

Primary behavior families for this phase:

- editor-facing APIs and compatibility surfaces
- scene tree, inspector, menu, and editor shell behavior
- import/export-related editor tooling surfaces
- browser/editor workflow support where Patina intentionally diverges

### Patina

Primary local crates:

- `engine-rs/crates/gdeditor/`
- `engine-rs/crates/gdscene/`
- `engine-rs/crates/gdresource/`
- `engine-rs/crates/gdplatform/`

Primary local evidence:

- `docs/EDITOR_ARCHITECTURE.md`
- `docs/migration-guide.md`
- `engine-rs/tests/editor_smoke_test.rs`
- `engine-rs/tests/editor_461_revalidation_test.rs`
- `engine-rs/tests/editor_interface_compat_test.rs`
- `engine-rs/tests/editor_menu_parity_test.rs`
- `engine-rs/tests/editor_systems_parity_test.rs`
- `engine-rs/tests/editor_dom_parity_test.rs`
- `engine-rs/tests/script_editor_core_parity_test.rs`
- `engine-rs/tests/property_inspector_typed_editors_test.rs`
- `engine-rs/tests/property_inspector_resource_sub_editor_test.rs`
- `engine-rs/tests/animation_editor_parity_test.rs`
- `engine-rs/tests/tilemap_editor_painting_test.rs`
- `engine-rs/tests/theme_editor_live_preview_test.rs`
- `engine-rs/tests/tooling_parity_milestone_test.rs`

## Current Patina Phase 8 Read

Phase 8 is also more real than the coarse milestone beads suggest.

Patina already has:

- a browser-served editor shell
- a large `gdeditor` crate with scene/editor/server modules
- a measured REST/smoke/revalidation surface
- compatibility tests for `EditorInterface`, menu structure, editor systems,
  script editor, inspector editors, animation editor, theme editor, tilemap
  tooling, and milestone structure

The main audit problem is not whether editor code exists.

The real question is how much of this is:

- measured browser/editor-shell behavior
- editor API/model compatibility
- maintenance/revalidation coverage
- broader Godot editor parity

## Claim Mismatch: Docs vs Measured Evidence

The current docs and compatibility summaries tend to compress all of `gdeditor`
into a single “Measured / Parity” claim.

That is too broad.

The evidence strongly supports:

- a real measured browser/editor shell
- measured compatibility for specific editor-facing subsystems
- maintenance/revalidation coverage against the repinned runtime

The evidence is weaker for:

- broad parity against the full native Godot editor feature surface
- import/export/editor-workflow parity across the entire Godot editor
- a claim that Phase 8 is “the primary focus” without further scoping

## Initial Phase 8 Classification

This is the first audit pass, not the final matrix.

### First Matrix Rows

| Upstream Family | Patina Area | Current Status | Evidence | Gap Type | Existing Bead | Action |
|-----------------|-------------|----------------|----------|----------|---------------|--------|
| browser-served editor shell and REST workflow | `gdeditor::editor_server`, browser shell | Measured for bounded slice | `editor_smoke_test.rs`, `editor_461_revalidation_test.rs` | missing breadth | `pat-6m9ky` overlaps compatibility layer scope | reuse evidence |
| `EditorInterface` compatibility layer | `gdeditor::editor_interface` | Measured for explicit API slice | `editor_interface_compat_test.rs` | missing breadth | `pat-6m9ky` | reuse evidence |
| editor menu surface | `gdeditor::editor_menu` | Measured for tested menu/parity slice | `editor_menu_parity_test.rs` | missing breadth | none active specific | keep scoped to tested menu actions |
| editor systems: settings, VCS, export dialog model, variant coverage | `gdeditor::settings`, `vcs`, `export_dialog` | Measured for local API/model slice | `editor_systems_parity_test.rs` | docs-overclaim | `pat-4vy88` tooling overlaps | treat as bounded systems parity, not full editor parity |
| script editor core | `gdeditor::script_editor`, `find_replace`, `script_gutter` | Measured for tested slice | `script_editor_core_parity_test.rs`, related script editor tests | missing breadth | none active specific | reuse evidence |
| inspector typed editors and resource sub-editors | `gdeditor::inspector` | Measured for tested slice | `property_inspector_typed_editors_test.rs`, `property_inspector_resource_sub_editor_test.rs` | missing breadth | none active specific | reuse evidence |
| animation/theme/tilemap tooling | `animation_editor`, `theme_editor`, `tilemap_editor` | Measured for selected tooling slices | `animation_editor_parity_test.rs`, `theme_editor_live_preview_test.rs`, `tilemap_editor_painting_test.rs` | missing breadth | `pat-4vy88` | reuse milestone evidence |
| import pipeline/editor import settings | `gdeditor::import`, `import_settings` | Implemented, partly measured | `audio_import_pipeline_test.rs`, `import_settings_panel_test.rs` | missing-test / docs-overclaim | none active specific | classify as bounded import tooling support |
| editor architecture / roadmap doc | `docs/EDITOR_ARCHITECTURE.md` | Implemented plan, not parity proof | `editor_architecture_plan_test.rs`, doc itself | docs-overclaim | `editor architecture plan` live bead | narrow claims and keep as source-of-truth plan |

### Browser / Compatibility Layer Notes

#### Browser-served editor shell

- Patina evidence:
  - `engine-rs/tests/editor_smoke_test.rs`
  - `engine-rs/tests/editor_461_revalidation_test.rs`
- Current classification: `Measured for bounded slice`
- Reason:
  - The server starts, serves HTML/JSON, performs scene round trips, and
    exercises runtime play/stop/pause/step integration against the repinned
    runtime.
  - This is strong evidence for a browser-served editor shell.
  - It is not the same as full Godot editor parity.

#### `EditorInterface` compatibility layer

- Patina evidence:
  - `engine-rs/tests/editor_interface_compat_test.rs`
- Current classification: `Measured for explicit API slice`
- Reason:
  - Selection, command execution, undo/redo, settings mutation, project root,
    and class registration are all covered directly.
  - This supports a real minimal compatibility-layer claim.
  - It should not be inflated into broad editor-plugin parity.

### Tooling / Systems Notes

#### Editor systems parity

- Patina evidence:
  - `engine-rs/tests/editor_systems_parity_test.rs`
- Current classification: `Measured for local systems/model slice`
- Reason:
  - Project settings defaults, editor settings persistence, VCS status models,
    and export dialog model coverage are tested.
  - The strongest safe claim is that Patina has measured local editor systems
    behavior for selected slices.
  - This is not a blanket claim that all Godot editor systems are parity-complete.

#### Selected tooling milestones

- Patina evidence:
  - `engine-rs/tests/tooling_parity_milestone_test.rs`
  - script/inspector/animation/theme/tilemap-specific tests
- Current classification: `Measured for milestone slices`
- Reason:
  - The repo does have measurable tooling slices.
  - The current live bead should stay focused on which tooling slices are
    explicitly claimed, not on generic “tooling parity” language.

### Plan / Architecture Notes

#### Editor architecture plan

- Patina evidence:
  - `docs/EDITOR_ARCHITECTURE.md`
  - `engine-rs/tests/editor_architecture_plan_test.rs`
- Current classification: `Implemented plan, not parity proof`
- Reason:
  - The document is useful as a source-of-truth plan and inventory.
  - Some language currently reads closer to “broad editor parity/work is the
    primary focus” than the measured evidence supports.
  - This should remain a planning artifact, not evidence of full editor parity.

## Existing Beads To Reuse

Do not create duplicates for these active Phase 8 beads:

- `pat-6m9ky` minimal editor-facing compatibility layer
- `pat-4vy88` Define selected tooling parity milestones
- `Write the editor architecture plan for post-V1 work` (current live title)

Any new Phase 8 bead must answer:

1. Why do the current compatibility/tooling/architecture beads not already
   cover it?
2. Is the gap about browser/editor-shell evidence, tooling measurement, or docs alignment?
3. What exact test, fixture, or artifact proves it done?

## Bead Candidates From This Audit

These are the first non-duplicative candidate beads.

### Candidate 1

Title:
`Phase 8 audit: reconcile editor support claims with measured browser/editor-shell evidence`

Acceptance:

- `docs/migration-guide.md`, `COMPAT_MATRIX.md`, and `COMPAT_DASHBOARD.md`
  distinguish measured editor-shell coverage from broader editor parity
- docs no longer imply blanket editor parity from the existence of many modules

### Candidate 2

Title:
`Phase 8 parity: classify selected tooling milestones against concrete tested editor slices`

Acceptance:

- script editor, inspector, animation editor, theme editor, tilemap tooling,
  and editor systems are each labeled as measured, implemented-not-measured,
  deferred, or missing
- the milestone list cites concrete tests for each claimed slice

### Candidate 3

Title:
`Phase 8 docs: keep editor architecture plan separate from parity evidence`

Acceptance:

- `docs/EDITOR_ARCHITECTURE.md` is framed as architecture and roadmap, not as
  standalone parity proof
- any broad “primary focus” or broad parity wording is narrowed to match the
  actual measured editor slice

## Instructions For Continuing This Audit

Follow this order:

1. Build the matrix from editor behavior clusters, not module inventory alone.
2. Map test evidence before keeping any “Measured” editor claim.
3. Reconcile docs before opening new implementation beads.
4. Open new beads only where there is a real missing measured slice not already
   covered by the current compatibility/tooling/architecture beads.

## Immediate Next Step

The next useful step is to narrow the public editor claims:

- compatibility docs should describe a measured browser/editor-shell slice
- architecture docs should remain roadmap/source-of-truth documents
- phase-8 beads should stay focused on compatibility layer, tooling milestones,
  and architecture boundaries rather than generic “editor parity”
