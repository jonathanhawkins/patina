//! pat-0sdqd: Validate the upstream 3D class surface matrix in the Phase 6 audit.
//!
//! Ensures:
//! 1. The matrix exists with expected section headers
//! 2. Every row uses one of the four approved status labels
//! 3. Every "Measured" row cites at least one test or report file
//! 4. Both upstream path and Patina evidence columns are present

use std::fs;

const AUDIT_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../prd/PHASE6_3D_PARITY_AUDIT.md");

fn read_audit() -> String {
    fs::read_to_string(AUDIT_PATH).expect("PHASE6_3D_PARITY_AUDIT.md must exist")
}

const APPROVED_STATUSES: &[&str] = &[
    "Measured",
    "Implemented, not yet measured",
    "Deferred",
    "Missing",
];

/// Parse table rows from a markdown section, skipping header and separator lines.
fn parse_table_rows(section: &str) -> Vec<Vec<String>> {
    section
        .lines()
        .filter(|l| l.starts_with('|') && !l.contains("Upstream Family") && !l.contains("---"))
        .map(|l| {
            l.split('|')
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty())
                .collect()
        })
        .filter(|cols: &Vec<String>| cols.len() >= 3)
        .collect()
}

fn extract_section(doc: &str, header: &str) -> String {
    let start = doc.find(header).unwrap_or_else(|| panic!("section '{header}' must exist"));
    let rest = &doc[start + header.len()..];
    let end = rest
        .find("\n### ")
        .or_else(|| rest.find("\n## "))
        .unwrap_or(rest.len());
    rest[..end].to_string()
}

// ===========================================================================
// 1. Matrix sections exist
// ===========================================================================

#[test]
fn audit_has_upstream_class_surface_matrix() {
    let audit = read_audit();
    assert!(
        audit.contains("## Upstream 3D Class Surface Matrix"),
        "audit must have the upstream 3D class surface matrix section"
    );
}

#[test]
fn matrix_has_all_subsections() {
    let audit = read_audit();
    let subsections = [
        "### scene/3d — Node and Rendering Families",
        "### scene/3d/physics — Physics Families",
        "### Render Server / Material Families",
    ];
    for s in &subsections {
        assert!(audit.contains(s), "matrix must have subsection: {s}");
    }
}

// ===========================================================================
// 2. Every row uses an approved status
// ===========================================================================

#[test]
fn all_node_rendering_rows_use_approved_status() {
    let audit = read_audit();
    let section = extract_section(&audit, "### scene/3d — Node and Rendering Families");
    let rows = parse_table_rows(&section);
    assert!(!rows.is_empty(), "node/rendering section must have rows");

    for row in &rows {
        let status = &row[2]; // Status is the 3rd column
        assert!(
            APPROVED_STATUSES.contains(&status.as_str()),
            "unapproved status '{status}' in node/rendering row: {}",
            row[0]
        );
    }
}

#[test]
fn all_physics_rows_use_approved_status() {
    let audit = read_audit();
    let section = extract_section(&audit, "### scene/3d/physics — Physics Families");
    let rows = parse_table_rows(&section);
    assert!(!rows.is_empty(), "physics section must have rows");

    for row in &rows {
        let status = &row[2];
        assert!(
            APPROVED_STATUSES.contains(&status.as_str()),
            "unapproved status '{status}' in physics row: {}",
            row[0]
        );
    }
}

#[test]
fn all_render_server_rows_use_approved_status() {
    let audit = read_audit();
    let section = extract_section(&audit, "### Render Server / Material Families");
    let rows = parse_table_rows(&section);
    assert!(!rows.is_empty(), "render server section must have rows");

    for row in &rows {
        let status = &row[2];
        assert!(
            APPROVED_STATUSES.contains(&status.as_str()),
            "unapproved status '{status}' in render server row: {}",
            row[0]
        );
    }
}

// ===========================================================================
// 3. Every Measured row cites evidence
// ===========================================================================

#[test]
fn measured_rows_cite_test_or_report() {
    let audit = read_audit();
    let sections = [
        "### scene/3d — Node and Rendering Families",
        "### scene/3d/physics — Physics Families",
        "### Render Server / Material Families",
    ];

    for header in &sections {
        let section = extract_section(&audit, header);
        let rows = parse_table_rows(&section);

        for row in &rows {
            if row[2] == "Measured" {
                let evidence = &row[3]; // Patina Evidence is 4th column
                assert!(
                    evidence.contains("_test") || evidence.contains("_report") || evidence.contains("REPORT"),
                    "Measured row '{}' in {header} must cite a test or report: got '{evidence}'",
                    row[0]
                );
            }
        }
    }
}

// ===========================================================================
// 4. Matrix has both upstream path and Patina evidence columns
// ===========================================================================

#[test]
fn matrix_tables_have_required_columns() {
    let audit = read_audit();
    let headers = [
        "| Upstream Family | Upstream Path | Status | Patina Evidence |",
    ];

    let count = headers
        .iter()
        .map(|h| audit.matches(h).count())
        .sum::<usize>();

    assert!(
        count >= 3,
        "matrix must have at least 3 tables with Upstream Family/Path/Status/Evidence columns (got {count})"
    );
}

// ===========================================================================
// 5. Coverage: key upstream families are present
// ===========================================================================

#[test]
fn matrix_covers_key_upstream_families() {
    let audit = read_audit();

    let required_families = [
        "Node3D",
        "Camera3D",
        "MeshInstance3D",
        "DirectionalLight3D",
        "OmniLight3D",
        "SpotLight3D",
        "RigidBody3D",
        "StaticBody3D",
        "CharacterBody3D",
        "Area3D",
        "CollisionShape3D",
        "VehicleBody3D",
        "SoftBody3D",
        "NavigationRegion3D",
        "WorldEnvironment",
        "Sky",
        "ReflectionProbe",
        "FogVolume",
        "Decal",
        "Skeleton3D",
        "RayCast3D",
        "ShapeCast3D",
    ];

    for family in &required_families {
        assert!(
            audit.contains(&format!("`{family}`")),
            "matrix must classify upstream family: {family}"
        );
    }
}

// ===========================================================================
// 6. Status distribution sanity check
// ===========================================================================

#[test]
fn matrix_has_at_least_10_measured_rows() {
    let audit = read_audit();
    let sections = [
        "### scene/3d — Node and Rendering Families",
        "### scene/3d/physics — Physics Families",
        "### Render Server / Material Families",
    ];

    let mut measured_count = 0;
    for header in &sections {
        let section = extract_section(&audit, header);
        let rows = parse_table_rows(&section);
        measured_count += rows.iter().filter(|r| r[2] == "Measured").count();
    }

    assert!(
        measured_count >= 10,
        "matrix must have at least 10 Measured rows (got {measured_count})"
    );
}

#[test]
fn matrix_has_deferred_and_missing_rows() {
    let audit = read_audit();
    let all_sections = format!(
        "{}{}{}",
        extract_section(&audit, "### scene/3d — Node and Rendering Families"),
        extract_section(&audit, "### scene/3d/physics — Physics Families"),
        extract_section(&audit, "### Render Server / Material Families"),
    );

    let rows: Vec<Vec<String>> = parse_table_rows(&all_sections);
    let deferred = rows.iter().filter(|r| r[2] == "Deferred").count();
    let missing = rows.iter().filter(|r| r[2] == "Missing").count();

    assert!(
        deferred >= 3,
        "matrix must have at least 3 Deferred rows (got {deferred})"
    );
    assert!(
        missing >= 3,
        "matrix must have at least 3 Missing rows (got {missing})"
    );
}
