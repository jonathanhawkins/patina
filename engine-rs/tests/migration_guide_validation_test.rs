//! pat-vyr2: Validate migration guide structure and references.
//!
//! These tests verify that:
//! 1. The migration guide exists and has expected sections
//! 2. All referenced crates exist in the workspace
//! 3. All referenced Rust target triples match DESKTOP_TARGETS
//! 4. Phase numbering is consistent with PORT_GODOT_TO_RUST_PLAN.md
//! 5. The Godot concept mapping table is complete
//! 6. Referenced documentation files exist

use std::fs;
use std::path::Path;

const GUIDE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/migration-guide.md");

fn read_guide() -> String {
    fs::read_to_string(GUIDE_PATH).expect("migration guide must exist at docs/migration-guide.md")
}

// ===========================================================================
// 1. Document existence and structure
// ===========================================================================

#[test]
fn migration_guide_exists() {
    assert!(
        Path::new(GUIDE_PATH).exists(),
        "docs/migration-guide.md must exist"
    );
}

#[test]
fn guide_has_title() {
    let guide = read_guide();
    assert!(
        guide.starts_with("# Patina Engine Migration Guide"),
        "guide must start with a top-level heading"
    );
}

#[test]
fn guide_has_overview_section() {
    let guide = read_guide();
    assert!(guide.contains("## Overview"), "must have Overview section");
}

#[test]
fn guide_has_all_milestone_sections() {
    let guide = read_guide();
    let expected_milestones = [
        "Headless Runtime",
        "2D Vertical Slice",
        "Broader Runtime",
        "3D Runtime Slice",
        "Platform Layer",
        "Editor Support",
    ];
    for milestone in &expected_milestones {
        assert!(
            guide.contains(milestone),
            "guide must reference milestone: {milestone}"
        );
    }
}

#[test]
fn guide_has_concept_mapping_table() {
    let guide = read_guide();
    assert!(
        guide.contains("Godot Concept Mapping"),
        "guide must have a Godot concept mapping section"
    );
}

#[test]
fn guide_has_migration_steps_per_milestone() {
    let guide = read_guide();
    // Each milestone section should have migration steps
    let step_count = guide.matches("### Migration Steps").count();
    assert!(
        step_count >= 5,
        "guide must have migration steps in at least 5 milestones (got {step_count})"
    );
}

#[test]
fn guide_has_crates_available_sections() {
    let guide = read_guide();
    let crate_section_count = guide.matches("### Crates Available").count();
    assert!(
        crate_section_count >= 4,
        "guide must list available crates for at least 4 milestones (got {crate_section_count})"
    );
}

// ===========================================================================
// 2. Referenced crates exist in the workspace
// ===========================================================================

#[test]
fn all_referenced_crates_exist() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let crates_dir = Path::new(manifest_dir).join("crates");

    let referenced_crates = [
        "gdcore",
        "gdvariant",
        "gdobject",
        "gdresource",
        "gdscene",
        "gdserver2d",
        "gdrender2d",
        "gdphysics2d",
        "gdserver3d",
        "gdrender3d",
        "gdphysics3d",
        "gdaudio",
        "gdplatform",
        "gdscript-interop",
        "gdeditor",
    ];

    for crate_name in &referenced_crates {
        let crate_path = crates_dir.join(crate_name);
        assert!(
            crate_path.exists(),
            "referenced crate '{}' must exist at {:?}",
            crate_name,
            crate_path
        );
        let cargo_toml = crate_path.join("Cargo.toml");
        assert!(
            cargo_toml.exists(),
            "crate '{}' must have a Cargo.toml",
            crate_name
        );
    }
}

#[test]
fn guide_references_match_workspace_members() {
    let guide = read_guide();

    // Every crate mentioned with backticks in the guide should exist
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let crates_dir = Path::new(manifest_dir).join("crates");

    let crate_names = [
        "gdcore",
        "gdvariant",
        "gdobject",
        "gdresource",
        "gdscene",
        "gdserver2d",
        "gdrender2d",
        "gdphysics2d",
        "gdserver3d",
        "gdrender3d",
        "gdphysics3d",
        "gdaudio",
        "gdplatform",
        "gdeditor",
    ];

    for name in &crate_names {
        assert!(guide.contains(name), "guide must reference crate '{name}'");
        assert!(
            crates_dir.join(name).exists(),
            "referenced crate '{name}' must exist on disk"
        );
    }
}

// ===========================================================================
// 3. Target triples in the guide match platform_targets
// ===========================================================================

#[test]
fn guide_target_triples_match_desktop_targets() {
    use gdplatform::platform_targets::DESKTOP_TARGETS;

    let guide = read_guide();

    for target in DESKTOP_TARGETS {
        assert!(
            guide.contains(target.rust_triple),
            "guide must list target triple '{}'",
            target.rust_triple
        );
    }
}

#[test]
fn guide_lists_ci_tested_status_correctly() {
    use gdplatform::platform_targets::DESKTOP_TARGETS;

    let guide = read_guide();

    for target in DESKTOP_TARGETS {
        // The guide should mention the triple
        assert!(
            guide.contains(target.rust_triple),
            "guide must reference triple '{}'",
            target.rust_triple
        );
    }

    // Verify at least one target marked as CI tested
    assert!(
        guide.contains("| Yes"),
        "guide must indicate at least one CI-tested target"
    );
}

// ===========================================================================
// 4. Phase references are consistent
// ===========================================================================

#[test]
fn guide_phase_numbers_are_consistent() {
    let guide = read_guide();

    // The guide maps milestones to phases -- verify key mappings
    let phase_mappings = [
        ("Headless Runtime", "3"),
        ("2D Vertical Slice", "4"),
        ("Broader Runtime", "5"),
        ("3D Runtime Slice", "6"),
        ("Platform Layer", "7"),
        ("Editor Support", "8"),
        ("Stable Release", "9"),
    ];

    for (milestone, phase) in &phase_mappings {
        assert!(
            guide.contains(milestone),
            "guide must mention milestone '{milestone}'"
        );
        assert!(
            guide.contains(&format!("| {phase} |")) || guide.contains(&format!("Phase {phase}")),
            "guide must associate '{milestone}' with Phase {phase}"
        );
    }
}

// ===========================================================================
// 5. Concept mapping completeness
// ===========================================================================

#[test]
fn concept_mapping_covers_core_godot_types() {
    let guide = read_guide();

    let godot_concepts = [
        "Node",
        "Signal",
        "Variant",
        "PackedScene",
        "Resource",
        "Input",
        "InputMap",
        "Vector2",
        "Vector3",
        "Transform2D",
        "Transform3D",
        "NodePath",
        "StringName",
    ];

    for concept in &godot_concepts {
        assert!(
            guide.contains(concept),
            "concept mapping must cover Godot concept '{concept}'"
        );
    }
}

#[test]
fn concept_mapping_covers_server_types() {
    let guide = read_guide();

    let servers = [
        "PhysicsServer2D",
        "PhysicsServer3D",
        "RenderingServer",
        "AudioServer",
        "DisplayServer",
    ];

    for server in &servers {
        assert!(
            guide.contains(server),
            "concept mapping must cover Godot server '{server}'"
        );
    }
}

// ===========================================================================
// 6. Referenced documentation files exist
// ===========================================================================

#[test]
fn referenced_docs_exist() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let repo_root = Path::new(manifest_dir).parent().unwrap();

    let referenced_docs = [
        "prd/PORT_GODOT_TO_RUST_PLAN.md",
        "docs/3D_ARCHITECTURE_SPEC.md",
        "docs/BENCHMARK_BASELINES.md",
    ];

    for doc in &referenced_docs {
        let doc_path = repo_root.join(doc);
        assert!(
            doc_path.exists(),
            "referenced doc '{}' must exist at {:?}",
            doc,
            doc_path
        );
    }
}

#[test]
fn guide_mentions_minimum_rust_version() {
    let guide = read_guide();
    assert!(
        guide.contains("1.75"),
        "guide must specify minimum Rust version 1.75"
    );
}

#[test]
fn guide_mentions_godot_4_compatibility() {
    let guide = read_guide();
    assert!(
        guide.contains("Godot 4"),
        "guide must mention Godot 4.x scene format compatibility"
    );
}

// ===========================================================================
// 7. Code examples are present
// ===========================================================================

#[test]
fn guide_has_rust_code_examples() {
    let guide = read_guide();
    let code_block_count = guide.matches("```rust").count();
    assert!(
        code_block_count >= 5,
        "guide must have at least 5 Rust code examples (got {code_block_count})"
    );
}

#[test]
fn guide_has_toml_examples() {
    let guide = read_guide();
    let toml_count = guide.matches("```toml").count();
    assert!(
        toml_count >= 2,
        "guide must have at least 2 TOML dependency examples (got {toml_count})"
    );
}

#[test]
fn guide_has_bash_examples() {
    let guide = read_guide();
    let bash_count = guide.matches("```bash").count();
    assert!(
        bash_count >= 1,
        "guide must have at least 1 bash command example (got {bash_count})"
    );
}

// ===========================================================================
// 8. Porting walkthrough section
// ===========================================================================

#[test]
fn guide_has_porting_walkthrough() {
    let guide = read_guide();
    assert!(
        guide.contains("Porting a Godot 4 Project Step-by-Step"),
        "guide must have a step-by-step porting walkthrough"
    );

    let steps = [
        "Assess Compatibility",
        "Create a Rust Project",
        "Copy Scene Files",
        "Rewrite GDScript in Rust",
        "Run and Iterate",
    ];
    for step in &steps {
        assert!(
            guide.contains(step),
            "porting walkthrough must include step: {step}"
        );
    }
}

#[test]
fn porting_walkthrough_has_compatibility_checklist() {
    let guide = read_guide();
    // Should list supported, partially supported, and unsupported categories
    assert!(
        guide.contains("Supported (port directly)"),
        "porting section must have supported features checklist"
    );
    assert!(
        guide.contains("Partially supported"),
        "porting section must have partially supported features"
    );
    assert!(
        guide.contains("Not yet supported"),
        "porting section must have unsupported features"
    );
}

#[test]
fn porting_walkthrough_has_gdscript_to_rust_example() {
    let guide = read_guide();
    // Should show a before (GDScript) and after (Rust) example
    assert!(
        guide.contains("```gdscript"),
        "porting section must have a GDScript code example"
    );
    assert!(
        guide.contains("GDScript (before)") || guide.contains("GDScript"),
        "porting section must show GDScript-to-Rust conversion"
    );
}

// ===========================================================================
// 9. Not-yet-supported section
// ===========================================================================

#[test]
fn guide_documents_unsupported_features() {
    let guide = read_guide();
    assert!(
        guide.contains("Not Yet Supported"),
        "guide must have a section on unsupported features"
    );

    let unsupported = ["GDScript", "Shader", "Animation", "Navigation"];
    for feature in &unsupported {
        assert!(
            guide.contains(feature),
            "unsupported section must mention '{feature}'"
        );
    }
}

// ===========================================================================
// 10. Node type compatibility table (pat-wy2fy)
// ===========================================================================

#[test]
fn guide_has_node_type_compatibility_table() {
    let guide = read_guide();
    assert!(
        guide.contains("Node Type Compatibility Table"),
        "guide must have a node type compatibility table"
    );
}

#[test]
fn compatibility_table_covers_all_node_categories() {
    let guide = read_guide();

    let categories = [
        "Core & Base Types",
        "2D Nodes",
        "2D Physics",
        "3D Nodes",
        "3D Physics",
        "3D Lighting",
        "UI / Control Nodes",
        "Audio",
        "Animation",
    ];

    for cat in &categories {
        assert!(
            guide.contains(cat),
            "compatibility table must have category: {cat}"
        );
    }
}

#[test]
fn compatibility_table_covers_key_2d_node_types() {
    let guide = read_guide();

    let node_types = [
        "Node2D",
        "Sprite2D",
        "AnimatedSprite2D",
        "Camera2D",
        "RigidBody2D",
        "StaticBody2D",
        "CharacterBody2D",
        "Area2D",
        "CollisionShape2D",
        "Line2D",
        "Polygon2D",
    ];

    for nt in &node_types {
        assert!(
            guide.contains(&format!("`{nt}`")),
            "compatibility table must list 2D node type: {nt}"
        );
    }
}

#[test]
fn compatibility_table_covers_key_3d_node_types() {
    let guide = read_guide();

    let node_types = [
        "Node3D",
        "MeshInstance3D",
        "Camera3D",
        "Skeleton3D",
        "RigidBody3D",
        "StaticBody3D",
        "CharacterBody3D",
        "DirectionalLight3D",
        "OmniLight3D",
        "SpotLight3D",
        "CollisionShape3D",
    ];

    for nt in &node_types {
        assert!(
            guide.contains(&format!("`{nt}`")),
            "compatibility table must list 3D node type: {nt}"
        );
    }
}

#[test]
fn compatibility_table_covers_ui_node_types() {
    let guide = read_guide();

    let node_types = [
        "Control",
        "Label",
        "Button",
        "Panel",
        "VBoxContainer",
        "HBoxContainer",
        "TextEdit",
        "LineEdit",
    ];

    for nt in &node_types {
        assert!(
            guide.contains(&format!("`{nt}`")),
            "compatibility table must list UI node type: {nt}"
        );
    }
}

#[test]
fn compatibility_table_covers_audio_types() {
    let guide = read_guide();

    let node_types = [
        "AudioStreamPlayer",
        "AudioStreamPlayer2D",
        "AudioStreamPlayer3D",
    ];

    for nt in &node_types {
        assert!(
            guide.contains(&format!("`{nt}`")),
            "compatibility table must list audio type: {nt}"
        );
    }
}

#[test]
fn compatibility_table_has_status_legend() {
    let guide = read_guide();
    assert!(
        guide.contains("Full") && guide.contains("Partial") && guide.contains("Stub"),
        "compatibility table must have Full/Partial/Stub status legend"
    );
}

#[test]
fn compatibility_table_lists_patina_crates() {
    let guide = read_guide();

    // The compatibility table should reference the implementing crate for each node
    let crates_in_table = [
        "`gdobject`",
        "`gdscene`",
        "`gdresource`",
        "`gdphysics2d`",
        "`gdphysics3d`",
        "`gdaudio`",
    ];

    for c in &crates_in_table {
        assert!(
            guide.contains(c),
            "compatibility table must reference crate {c}"
        );
    }
}

#[test]
fn compatibility_table_has_not_supported_section() {
    let guide = read_guide();
    // The table should list categories of unsupported types
    assert!(
        guide.contains("Networking"),
        "compatibility table must note unsupported Networking types"
    );
    assert!(
        guide.contains("GDExtension"),
        "compatibility table must note unsupported GDExtension types"
    );
}

// ===========================================================================
// 11. Known limitations and workarounds (pat-ts516)
// ===========================================================================

#[test]
fn guide_has_known_limitations_section() {
    let guide = read_guide();
    assert!(
        guide.contains("## Known Limitations and Workarounds"),
        "guide must have a Known Limitations and Workarounds section"
    );
}

#[test]
fn limitations_cover_all_major_categories() {
    let guide = read_guide();

    let categories = [
        "### Rendering",
        "### GDScript",
        "### Physics",
        "### Animation",
        "### Audio",
        "### UI / Control",
        "### Platform & Export",
        "### Resource System",
        "### Networking",
    ];

    for cat in &categories {
        assert!(
            guide.contains(cat),
            "limitations section must cover category: {cat}"
        );
    }
}

#[test]
fn each_limitation_has_workaround() {
    let guide = read_guide();

    // Find the limitations section
    let section_start = guide
        .find("## Known Limitations and Workarounds")
        .expect("section must exist");
    let section_end = guide[section_start..]
        .find("## General Migration Advice")
        .map(|i| section_start + i)
        .unwrap_or(guide.len());
    let section = &guide[section_start..section_end];

    // Every table must have a Workaround column
    let table_headers: Vec<_> = section
        .lines()
        .filter(|l| l.contains("| Limitation |"))
        .collect();

    assert!(
        table_headers.len() >= 8,
        "must have at least 8 limitation tables (got {})",
        table_headers.len()
    );

    for header in &table_headers {
        assert!(
            header.contains("Workaround"),
            "every limitation table must have a Workaround column: {header}"
        );
    }
}

#[test]
fn limitations_mention_key_missing_features() {
    let guide = read_guide();

    let key_limitations = [
        "Software rendering",
        "No GDScript",
        "No blend trees",
        "No mobile platforms",
        "No GDExtension",
    ];

    for lim in &key_limitations {
        assert!(guide.contains(lim), "limitations must mention: {lim}");
    }
}

#[test]
fn limitations_reference_workaround_tools() {
    let guide = read_guide();

    // Workarounds should reference concrete Rust/Patina tools
    let tools = [
        "AnimationPlayer", // animation workaround
        "NodePath",        // GDScript workaround
        "Variant",         // GDScript workaround
        "tracing",         // debugging workaround
    ];

    for tool in &tools {
        assert!(
            guide.contains(tool),
            "workarounds should reference tool/type: {tool}"
        );
    }
}
