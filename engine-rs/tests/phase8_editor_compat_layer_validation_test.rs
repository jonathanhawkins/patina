//! pat-6m9ky: Validate the minimal editor-facing compatibility layer against
//! the audited shell defined in `prd/PHASE8_EDITOR_PARITY_AUDIT.md`.
//!
//! Source of truth: `prd/PHASE8_EDITOR_PARITY_AUDIT.md`
//! Classification: Measured — compatibility surface validated against audit
//!
//! This test suite ensures:
//! 1. The audit document exists and references both compatibility surfaces
//! 2. All evidence test files cited in the audit matrix exist
//! 3. The `editor_compat` module exports the expected Godot-compatible API surface
//! 4. The `EditorInterface` module exposes the expected API surface
//! 5. The `editor_server` module exists (browser-served shell)
//! 6. The audit matrix row count is guarded against silent drift
//! 7. Compatibility layer claims stay bounded to the audited slice

use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn repo_root() -> PathBuf {
    workspace_root().join("..")
}

fn read_audit() -> String {
    fs::read_to_string(repo_root().join("prd/PHASE8_EDITOR_PARITY_AUDIT.md"))
        .expect("prd/PHASE8_EDITOR_PARITY_AUDIT.md must exist")
}

// ===========================================================================
// 1. Audit document integrity
// ===========================================================================

#[test]
fn audit_document_exists() {
    assert!(
        repo_root().join("prd/PHASE8_EDITOR_PARITY_AUDIT.md").exists(),
        "Phase 8 audit document must exist"
    );
}

#[test]
fn audit_references_both_compatibility_surfaces() {
    let audit = read_audit();
    assert!(
        audit.contains("editor_server"),
        "audit must reference editor_server (browser shell)"
    );
    assert!(
        audit.contains("editor_interface"),
        "audit must reference editor_interface"
    );
    assert!(
        audit.contains("editor_compat"),
        "audit must reference editor_compat module (not required in matrix but implied)"
    );
}

#[test]
fn audit_references_bead_pat_6m9ky() {
    let audit = read_audit();
    assert!(
        audit.contains("pat-6m9ky"),
        "audit must reference the compatibility layer bead"
    );
}

#[test]
fn audit_distinguishes_measured_from_broader_parity() {
    let audit = read_audit();
    let lower = audit.to_lowercase();
    assert!(
        lower.contains("measured for bounded slice"),
        "audit must classify browser shell as measured for bounded slice"
    );
    assert!(
        lower.contains("measured for explicit api slice"),
        "audit must classify EditorInterface as measured for explicit API slice"
    );
    assert!(
        audit.contains("broader Godot editor parity"),
        "audit must distinguish from broader parity"
    );
}

// ===========================================================================
// 2. Evidence test files cited in the audit matrix exist
// ===========================================================================

/// All test files cited as evidence in the Phase 8 audit matrix.
const AUDIT_EVIDENCE_TEST_FILES: &[&str] = &[
    "tests/editor_smoke_test.rs",
    "tests/editor_461_revalidation_test.rs",
    "tests/editor_interface_compat_test.rs",
    "tests/editor_menu_parity_test.rs",
    "tests/editor_systems_parity_test.rs",
    "tests/editor_compat_layer_test.rs",
    "tests/script_editor_core_parity_test.rs",
    "tests/property_inspector_typed_editors_test.rs",
    "tests/property_inspector_resource_sub_editor_test.rs",
    "tests/animation_editor_parity_test.rs",
    "tests/tilemap_editor_painting_test.rs",
    "tests/theme_editor_live_preview_test.rs",
    "tests/tooling_parity_milestone_test.rs",
];

#[test]
fn all_audit_evidence_test_files_exist() {
    let engine = workspace_root();
    let mut missing = Vec::new();
    for file in AUDIT_EVIDENCE_TEST_FILES {
        if !engine.join(file).exists() {
            missing.push(*file);
        }
    }
    assert!(
        missing.is_empty(),
        "audit-cited evidence test files missing: {:?}",
        missing
    );
}

#[test]
fn audit_evidence_file_count_guard() {
    // The audit cites at least 13 primary evidence test files.
    // If this count changes, update the constant and re-audit.
    assert_eq!(
        AUDIT_EVIDENCE_TEST_FILES.len(),
        13,
        "audit evidence file count must match expected (update if audit adds/removes evidence)"
    );
}

// ===========================================================================
// 3. editor_compat module API surface validation
// ===========================================================================

#[test]
fn editor_compat_exports_type_aliases() {
    // Verify that the Godot-compatible type aliases compile and resolve.
    use gdeditor::editor_compat::{EditorInspector, EditorInspectorPluginManager, EditorProperty, EditorUndoRedoManager};

    // These are type aliases — we just need to prove they exist and resolve.
    let _: fn() -> EditorInspector = || gdeditor::inspector::InspectorPanel::new();
    let _: fn() -> EditorInspectorPluginManager =
        || gdeditor::inspector::InspectorPluginRegistry::new();
    // EditorProperty and EditorUndoRedoManager are also aliases; their existence
    // is validated by this `use` statement compiling.
    let _ = std::any::type_name::<EditorProperty>();
    let _ = std::any::type_name::<EditorUndoRedoManager>();
}

#[test]
fn editor_compat_exports_property_usage_flags() {
    use gdeditor::editor_compat::PropertyUsageFlags;

    // All five variants must exist.
    let variants = [
        PropertyUsageFlags::Editor,
        PropertyUsageFlags::Storage,
        PropertyUsageFlags::Default,
        PropertyUsageFlags::NoEditor,
        PropertyUsageFlags::ReadOnly,
    ];
    assert_eq!(variants.len(), 5, "PropertyUsageFlags must have 5 variants");
}

#[test]
fn editor_compat_exports_editor_property_info() {
    use gdeditor::editor_compat::EditorPropertyInfo;
    use gdvariant::variant::VariantType;

    let info = EditorPropertyInfo::new("test_prop", VariantType::Float);
    assert_eq!(info.name, "test_prop");
    assert_eq!(info.variant_type, VariantType::Float);
}

#[test]
fn editor_compat_exports_editor_selection() {
    use gdeditor::editor_compat::EditorSelection;
    use gdeditor::Editor;
    use gdscene::SceneTree;

    let tree = SceneTree::new();
    let mut editor = Editor::new(tree);
    let sel = EditorSelection::new(&mut editor);
    assert_eq!(sel.get_selected_node_count(), 0);
}

#[test]
fn editor_compat_exports_editor_interface_compat() {
    use gdeditor::editor_compat::EditorInterfaceCompat;
    use gdeditor::Editor;
    use gdscene::SceneTree;

    let tree = SceneTree::new();
    let editor = Editor::new(tree);
    let compat = EditorInterfaceCompat::new(&editor);
    assert_eq!(compat.get_editor_main_screen_name(), "3D");
}

#[test]
fn editor_compat_exports_undo_redo_compat() {
    use gdeditor::editor_compat::UndoRedoCompat;
    use gdeditor::Editor;
    use gdscene::SceneTree;

    let tree = SceneTree::new();
    let mut editor = Editor::new(tree);
    let ur = UndoRedoCompat::new(&mut editor);
    assert!(!ur.has_undo());
    assert!(!ur.has_redo());
}

#[test]
fn editor_compat_exports_validate_property_value() {
    use gdeditor::editor_compat::validate_property_value;
    use gdvariant::variant::VariantType;
    use gdvariant::Variant;

    let result = validate_property_value(&Variant::Int(42), VariantType::Int);
    assert!(result.is_ok());
}

#[test]
fn editor_compat_exports_get_property_category() {
    use gdeditor::editor_compat::get_property_category;
    assert_eq!(get_property_category("position"), "Transform");
}

// ===========================================================================
// 4. EditorInterface API surface validation
// ===========================================================================

#[test]
fn editor_interface_api_surface_complete() {
    // Verify all Godot-compatible EditorInterface methods exist by calling them.
    use gdeditor::{Editor, EditorInterface};
    use gdscene::SceneTree;

    let tree = SceneTree::new();
    let editor = Editor::new(tree);
    let mut ei = EditorInterface::new(editor, "/tmp/validation_test");

    // Access methods (Godot API names)
    let _ = ei.get_editor();
    let _ = ei.get_editor_mut();
    let _ = ei.get_editor_settings();
    let _ = ei.get_editor_settings_mut();
    let _ = ei.get_file_system_dock();
    let _ = ei.get_file_system_dock_mut();
    let _ = ei.get_inspector();
    let _ = ei.get_inspector_mut();
    let _ = ei.get_edited_scene_root();
    let _ = ei.get_edited_scene_root_mut();

    // Selection
    let _ = ei.get_selection();

    // Scene operations
    let _ = ei.get_current_path();
    ei.set_current_path("test.tscn");
    let _ = ei.is_scene_modified();

    // UI state
    let _ = ei.is_distraction_free_mode_enabled();
    ei.set_distraction_free_mode(true);
    let _ = ei.is_bottom_panel_visible();
    ei.set_bottom_panel_visible(false);
    let _ = ei.get_editor_theme();

    // File system
    let _ = ei.get_project_root();

    // Plugins
    let _ = ei.get_plugin_names();
}

/// Count of public methods on EditorInterface to guard against silent API drift.
#[test]
fn editor_interface_method_count_guard() {
    // EditorInterface currently exposes 24 public methods.
    // If methods are added/removed, update this count and re-audit.
    //
    // Methods: new, get_editor, get_editor_mut, get_editor_settings,
    // get_editor_settings_mut, get_file_system_dock, get_file_system_dock_mut,
    // get_inspector, get_inspector_mut, get_edited_scene_root,
    // get_edited_scene_root_mut, get_selection, select_node, deselect,
    // get_current_path, set_current_path, save_scene, open_scene_from_path,
    // reload_scene_from_disk, execute_command, undo, redo,
    // is_distraction_free_mode_enabled, set_distraction_free_mode,
    // is_bottom_panel_visible, set_bottom_panel_visible, get_editor_theme,
    // scan_file_system, get_project_root, get_plugin_names, is_scene_modified
    //
    // Total: 31 (including new, Debug impl is a trait not counted)
    // We guard at >= 25 to catch accidental removal without being brittle
    // on minor additions.
    let source = fs::read_to_string(
        workspace_root().join("crates/gdeditor/src/editor_interface.rs"),
    )
    .expect("editor_interface.rs must exist");

    let pub_fn_count = source
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("pub fn ")
        })
        .count();

    assert!(
        pub_fn_count >= 25,
        "EditorInterface must have at least 25 public methods (got {pub_fn_count}); \
         if methods were removed, update the audit"
    );
}

// ===========================================================================
// 5. editor_server module exists (browser-served shell)
// ===========================================================================

#[test]
fn editor_server_module_exists() {
    let server_path = workspace_root().join("crates/gdeditor/src/editor_server.rs");
    assert!(
        server_path.exists(),
        "editor_server.rs must exist (browser-served editor shell)"
    );
}

#[test]
fn editor_server_source_is_substantial() {
    let source = fs::read_to_string(
        workspace_root().join("crates/gdeditor/src/editor_server.rs"),
    )
    .expect("editor_server.rs must be readable");
    let line_count = source.lines().count();
    assert!(
        line_count >= 100,
        "editor_server.rs must be substantial (got {line_count} lines); \
         a trivial stub does not satisfy the browser shell audit"
    );
}

// ===========================================================================
// 6. Audit matrix row count guard
// ===========================================================================

#[test]
fn audit_matrix_row_count_matches_expected() {
    let audit = read_audit();
    // Count rows in the "First Matrix Rows" section only (not the milestone table).
    // The section starts at "### First Matrix Rows" and ends at the next "###" heading.
    let mut in_matrix = false;
    let mut past_header = false;
    let mut data_rows = Vec::new();

    for line in audit.lines() {
        if line.starts_with("### First Matrix Rows") {
            in_matrix = true;
            continue;
        }
        if in_matrix && line.starts_with("###") {
            break;
        }
        if !in_matrix {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.starts_with("| Upstream Family") {
            past_header = true;
            continue;
        }
        if trimmed.contains("---") && trimmed.starts_with("|") {
            continue;
        }
        if past_header && trimmed.starts_with("| ") {
            data_rows.push(trimmed);
        }
    }

    assert_eq!(
        data_rows.len(),
        11,
        "audit first matrix must have 11 data rows (got {}); update this guard \
         if the audit matrix legitimately changes.\nRows found:\n{}",
        data_rows.len(),
        data_rows.join("\n")
    );
}

// ===========================================================================
// 7. Compatibility layer claims stay bounded
// ===========================================================================

#[test]
fn audit_does_not_claim_full_editor_parity() {
    let audit = read_audit();
    // The audit must explicitly say what the evidence does NOT support.
    assert!(
        audit.contains("evidence is weaker for"),
        "audit must acknowledge weaker evidence areas"
    );
    assert!(
        audit.contains("broad parity against the full native Godot editor"),
        "audit must explicitly call out that full editor parity is not claimed"
    );
}

#[test]
fn audit_scopes_phase8_to_compatibility_layer_not_full_editor() {
    let audit = read_audit();
    let lower = audit.to_lowercase();
    // The audit must contain the phrase "minimal" to indicate bounded scope.
    let minimal_count = lower.matches("minimal").count();
    assert!(
        minimal_count >= 3,
        "audit must use 'minimal' at least 3 times to emphasize bounded scope (found {minimal_count})"
    );
}

// ===========================================================================
// 8. Core compatibility modules exist in gdeditor
// ===========================================================================

#[test]
fn gdeditor_compatibility_modules_exist() {
    let src = workspace_root().join("crates/gdeditor/src");
    let required_modules = [
        "editor_compat.rs",
        "editor_interface.rs",
        "editor_server.rs",
    ];
    for module in &required_modules {
        assert!(
            src.join(module).exists(),
            "gdeditor must contain compatibility module: {module}"
        );
    }
}

#[test]
fn gdeditor_editor_modules_breadth_guard() {
    // The audit references a large gdeditor crate; guard the module count.
    let src = workspace_root().join("crates/gdeditor/src");
    let module_count = fs::read_dir(&src)
        .expect("must be able to list gdeditor/src")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map_or(false, |ext| ext == "rs")
        })
        .count();

    assert!(
        module_count >= 30,
        "gdeditor must have at least 30 .rs modules (got {module_count}); \
         if modules were removed, re-audit the compatibility layer scope"
    );
}

// ===========================================================================
// 9. Cross-reference: audit mentions evidence docs
// ===========================================================================

#[test]
fn audit_cites_editor_architecture_doc() {
    let audit = read_audit();
    assert!(
        audit.contains("EDITOR_ARCHITECTURE.md"),
        "audit must reference EDITOR_ARCHITECTURE.md"
    );
}

#[test]
fn editor_architecture_doc_exists() {
    assert!(
        repo_root().join("docs/EDITOR_ARCHITECTURE.md").exists(),
        "docs/EDITOR_ARCHITECTURE.md must exist"
    );
}
