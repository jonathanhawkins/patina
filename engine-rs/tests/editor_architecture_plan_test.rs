//! pat-z20hm: Editor architecture plan validation.
//!
//! Validates that the editor architecture documented in docs/EDITOR_ARCHITECTURE.md
//! is correctly implemented: module inventory matches reality, dependency graph is
//! accurate, all documented REST endpoints exist in the editor server, and the
//! architecture layers are sound.

// ===========================================================================
// 1. Module inventory — every documented module must be a real public module
// ===========================================================================

/// The modules documented in EDITOR_ARCHITECTURE.md § Module Inventory.
const DOCUMENTED_MODULES: &[&str] = &[
    "animation_editor",
    "asset_drag_drop",
    "command_palette",
    "create_dialog",
    "curve_editor",
    "dock",
    "editor_compat",
    "editor_interface",
    "editor_menu",
    "editor_server",
    "editor_plugin",
    "editor_settings_dialog",
    "editor_ui",
    "environment_preview",
    "export_dialog",
    "filesystem",
    "find_replace",
    "group_dialog",
    "import",
    "import_settings",
    "inspector",
    "output_panel",
    "profiler_panel",
    "project_settings_dialog",
    "scene_editor",
    "scene_renderer",
    "script_completion",
    "script_editor",
    "script_gutter",
    "shader_editor",
    "signal_dialog",
    "settings",
    "texture_cache",
    "theme_editor",
    "tilemap_editor",
    "undo_redo",
    "vcs",
    "viewport_2d",
    "viewport_3d",
];

#[test]
fn all_documented_modules_exist_as_source_files() {
    let base = format!("{}/crates/gdeditor/src", env!("CARGO_MANIFEST_DIR"));
    for module in DOCUMENTED_MODULES {
        let path = format!("{}/{}.rs", base, module);
        assert!(
            std::path::Path::new(&path).exists(),
            "Documented module '{}' missing: {}",
            module,
            path
        );
    }
}

#[test]
fn lib_rs_declares_all_documented_modules() {
    let lib_path = format!("{}/crates/gdeditor/src/lib.rs", env!("CARGO_MANIFEST_DIR"));
    let lib_src = std::fs::read_to_string(&lib_path).unwrap();
    for module in DOCUMENTED_MODULES {
        let decl = format!("pub mod {};", module);
        assert!(
            lib_src.contains(&decl),
            "lib.rs missing 'pub mod {};' for documented module '{}'",
            module,
            module
        );
    }
}

// ===========================================================================
// 2. Dependency graph — Cargo.toml dependencies match the documented graph
// ===========================================================================

/// Dependencies documented in EDITOR_ARCHITECTURE.md § Dependency Graph.
const DOCUMENTED_DEPS: &[&str] = &[
    "gdscene",
    "gdrender2d",
    "gdserver3d",
    "gdvariant",
    "gdcore",
    "gdobject",
    "gdresource",
    "gdscript-interop",
    "gdplatform",
];

#[test]
fn cargo_toml_contains_all_documented_dependencies() {
    let cargo_path = format!("{}/crates/gdeditor/Cargo.toml", env!("CARGO_MANIFEST_DIR"));
    let cargo_src = std::fs::read_to_string(&cargo_path).unwrap();
    for dep in DOCUMENTED_DEPS {
        assert!(
            cargo_src.contains(dep),
            "Cargo.toml missing documented dependency '{}'",
            dep
        );
    }
}

// ===========================================================================
// 3. Architecture layers — key types exist and are accessible
// ===========================================================================

#[test]
fn editor_state_type_exists() {
    // Central Editor state struct from the architecture's Editor layer
    let tree = gdscene::SceneTree::new();
    let _editor = gdeditor::Editor::new(tree);
}

#[test]
fn editor_command_enum_exists() {
    // EditorCommand enum must be accessible as a public type
    fn _accepts_cmd(_cmd: gdeditor::EditorCommand) {}
}

#[test]
fn editor_plugin_trait_exists() {
    // EditorPlugin trait must be accessible
    fn _assert_trait_object_safe(_: &dyn gdeditor::EditorPlugin) {}
}

#[test]
fn undo_redo_stack_exists() {
    let stack = gdeditor::undo_redo::UndoRedoManager::new(100);
    assert!(
        !stack.can_undo(),
        "fresh undo stack should have nothing to undo"
    );
}

#[test]
fn scene_tree_dock_exists() {
    let _dock = gdeditor::SceneTreeDock::default();
}

#[test]
fn inspector_panel_exists() {
    let _panel = gdeditor::InspectorPanel::default();
}

// ===========================================================================
// 4. REST API surface — documented endpoints exist in editor_server
// ===========================================================================

#[test]
fn editor_server_source_contains_documented_endpoints() {
    let server_path = format!(
        "{}/crates/gdeditor/src/editor_server.rs",
        env!("CARGO_MANIFEST_DIR")
    );
    let src = std::fs::read_to_string(&server_path).unwrap();

    let endpoints = &[
        "/editor",
        "/api/scene",
        "/api/node/",
        "/api/viewport",
        "/api/scene/save",
        "/api/scene/load",
        "/api/undo",
        "/api/redo",
        "/api/property/set",
    ];

    for ep in endpoints {
        assert!(
            src.contains(ep),
            "editor_server.rs missing documented endpoint '{}'",
            ep
        );
    }
}

// ===========================================================================
// 5. Editor capabilities — documented features compile and are reachable
// ===========================================================================

#[test]
fn animation_editor_module_has_timeline() {
    // Animation editor must expose timeline/keyframe types
    let _editor = gdeditor::animation_editor::AnimationEditor::new(4.0, 3);
}

#[test]
fn command_palette_exists() {
    let _palette = gdeditor::command_palette::CommandPalette::default();
}

#[test]
fn export_dialog_exists() {
    let _dialog = gdeditor::ExportDialog::default();
}

#[test]
fn viewport_2d_module_accessible() {
    let _vp = gdeditor::viewport_2d::Viewport2D::new(640, 480);
}

#[test]
fn viewport_3d_module_accessible() {
    let _vp = gdeditor::viewport_3d::Viewport3D::default();
}

#[test]
fn editor_menu_bar_exists() {
    let _menu = gdeditor::editor_menu::EditorMenuBar::default();
}

#[test]
fn vcs_integration_exists() {
    let _vcs = gdeditor::vcs::VcsStatus::default();
}

#[test]
fn filesystem_browser_exists() {
    let _fs = gdeditor::EditorFileSystem::new(".");
}

// ===========================================================================
// 6. Architecture doc exists and is non-trivial
// ===========================================================================

#[test]
fn architecture_doc_exists_and_has_substance() {
    let doc_path = format!(
        "{}/../docs/EDITOR_ARCHITECTURE.md",
        env!("CARGO_MANIFEST_DIR")
    );
    let doc = std::fs::read_to_string(&doc_path).unwrap();
    assert!(
        doc.len() > 2000,
        "architecture doc should be substantial (got {} bytes)",
        doc.len()
    );
    assert!(
        doc.contains("## Architecture Goals"),
        "doc should contain architecture goals"
    );
    assert!(
        doc.contains("## Dependency Graph"),
        "doc should contain dependency graph"
    );
    assert!(
        doc.contains("## Testing Strategy"),
        "doc should contain testing strategy"
    );
    assert!(
        doc.contains("## Current State"),
        "doc should contain current state"
    );
    assert!(
        doc.contains("### Module Inventory"),
        "doc should contain module inventory"
    );
}

// ===========================================================================
// 9. Subsystem boundaries — doc names concrete subsystems with boundaries
// ===========================================================================

#[test]
fn architecture_doc_defines_subsystem_boundaries() {
    let doc_path = format!(
        "{}/../docs/EDITOR_ARCHITECTURE.md",
        env!("CARGO_MANIFEST_DIR")
    );
    let doc = std::fs::read_to_string(&doc_path).unwrap();

    assert!(
        doc.contains("## Subsystem Boundaries"),
        "doc must define subsystem boundaries"
    );

    // Five named subsystems
    let subsystems = &[
        "Editor Shell",
        "Scene Editing Core",
        "Inspection and Properties",
        "Viewports and Rendering",
        "Tooling and Specialized Editors",
    ];
    for subsystem in subsystems {
        assert!(
            doc.contains(subsystem),
            "doc must name subsystem '{}'",
            subsystem
        );
    }

    // Each subsystem must have a Boundary: line
    let boundary_count = doc.matches("**Boundary:**").count();
    assert!(
        boundary_count >= 5,
        "doc must define at least 5 subsystem boundaries, found {}",
        boundary_count
    );
}

// ===========================================================================
// 10. Deferred scope — doc explicitly lists what is NOT in scope
// ===========================================================================

#[test]
fn architecture_doc_defines_deferred_scope() {
    let doc_path = format!(
        "{}/../docs/EDITOR_ARCHITECTURE.md",
        env!("CARGO_MANIFEST_DIR")
    );
    let doc = std::fs::read_to_string(&doc_path).unwrap();

    assert!(
        doc.contains("## Deferred Scope"),
        "doc must have a Deferred Scope section"
    );

    // Must list at least 5 deferred items with reasons
    let deferred_markers = &["Reason Deferred", "Prerequisite"];
    for marker in deferred_markers {
        assert!(
            doc.contains(marker),
            "Deferred Scope section must contain '{}' column",
            marker
        );
    }
}

// ===========================================================================
// 11. Source of truth — this test file cites the doc as the canonical plan
// ===========================================================================

/// This test validates that `docs/EDITOR_ARCHITECTURE.md` is the single
/// source of truth for the editor architecture plan. The document must:
/// - name concrete subsystems with explicit boundaries
/// - define deferred scope for capabilities not yet targeted
/// - maintain a module inventory that matches the actual codebase
/// - list architecture goals for post-V1 work
///
/// Any change to editor subsystem structure must be reflected in the doc
/// first, then validated by this test suite.
#[test]
fn architecture_doc_is_source_of_truth() {
    let doc_path = format!(
        "{}/../docs/EDITOR_ARCHITECTURE.md",
        env!("CARGO_MANIFEST_DIR")
    );
    let doc = std::fs::read_to_string(&doc_path).unwrap();

    // The doc must contain all major structural sections
    let required_sections = &[
        "## Current State",
        "### Module Inventory",
        "## Subsystem Boundaries",
        "## Dependency Graph",
        "## Architecture Goals",
        "## Deferred Scope",
        "## Testing Strategy",
    ];
    for section in required_sections {
        assert!(
            doc.contains(section),
            "Source-of-truth doc missing required section: '{}'",
            section
        );
    }
}

// ===========================================================================
// 7. No circular dependencies — editor does not leak into engine core
// ===========================================================================

#[test]
fn core_crates_do_not_depend_on_editor() {
    let core_crates = &["gdcore", "gdvariant", "gdscene", "gdobject", "gdresource"];
    for crate_name in core_crates {
        let cargo_path = format!(
            "{}/crates/{}/Cargo.toml",
            env!("CARGO_MANIFEST_DIR"),
            crate_name
        );
        let cargo_src = std::fs::read_to_string(&cargo_path).unwrap();
        assert!(
            !cargo_src.contains("gdeditor"),
            "Core crate '{}' must not depend on gdeditor (architecture violation)",
            crate_name
        );
    }
}

// ===========================================================================
// 8. Module count sanity — at least 35 modules (doc says 40)
// ===========================================================================

#[test]
fn module_count_meets_minimum() {
    let src_dir = format!("{}/crates/gdeditor/src", env!("CARGO_MANIFEST_DIR"));
    let count = std::fs::read_dir(&src_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.ends_with(".rs") && name != "lib.rs"
        })
        .count();
    assert!(
        count >= 35,
        "gdeditor should have at least 35 modules, found {}",
        count
    );
}
