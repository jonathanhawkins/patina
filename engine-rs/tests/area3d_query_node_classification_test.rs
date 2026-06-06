//! pat-z1i84: Validate that Area3D and query-node claims are correctly
//! classified and not inflated beyond measured evidence.
//!
//! Ensures:
//! 1. Area3D is "Implemented, not yet measured" (not "Measured" or "Full")
//! 2. PhysicsRayQueryParameters3D / PhysicsShapeQueryParameters3D are classified
//!    separately from RayCast3D / ShapeCast3D
//! 3. RayCast3D and ShapeCast3D are not labeled "Measured"
//! 4. The parity report distinguishes query-object from node parity
//! 5. All five entities are separately classified in the matrix

use std::fs;

const AUDIT_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../prd/PHASE6_3D_PARITY_AUDIT.md");
const PARITY_REPORT_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/3D_DEMO_PARITY_REPORT.md");
const MIGRATION_GUIDE_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/migration-guide.md");

fn read_file(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("must read {path}: {e}"))
}

fn extract_matrix_section(doc: &str, header: &str) -> String {
    let start = doc.find(header).unwrap_or_else(|| panic!("section '{header}' must exist"));
    let rest = &doc[start + header.len()..];
    let end = rest
        .find("\n### ")
        .or_else(|| rest.find("\n## "))
        .unwrap_or(rest.len());
    rest[..end].to_string()
}

fn find_matrix_row<'a>(section: &'a str, family: &str) -> Option<Vec<String>> {
    section
        .lines()
        .filter(|l| l.starts_with('|') && l.contains(family))
        .map(|l| {
            l.split('|')
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty())
                .collect()
        })
        .next()
}

// ===========================================================================
// 1. Area3D is NOT labeled "Measured" in the matrix
// ===========================================================================

#[test]
fn area3d_not_measured_in_matrix() {
    let audit = read_file(AUDIT_PATH);
    let section = extract_matrix_section(&audit, "### scene/3d/physics — Physics Families");
    let row = find_matrix_row(&section, "Area3D").expect("Area3D must be in physics matrix");

    assert_ne!(
        row[2], "Measured",
        "Area3D should not be 'Measured' — overlap storage exists but runtime \
         signal parity (body_entered/area_entered) is not yet proven"
    );
    assert!(
        row[2].contains("Implemented") || row[2].contains("not yet measured"),
        "Area3D status should reflect 'Implemented, not yet measured', got: {}",
        row[2]
    );
}

// ===========================================================================
// 2. PhysicsRayQueryParameters3D and PhysicsShapeQueryParameters3D are
//    classified separately from RayCast3D / ShapeCast3D
// ===========================================================================

#[test]
fn query_params_and_cast_nodes_are_separate_rows() {
    let audit = read_file(AUDIT_PATH);
    let section = extract_matrix_section(&audit, "### scene/3d/physics — Physics Families");

    let ray_query = find_matrix_row(&section, "PhysicsRayQueryParameters3D");
    let shape_query = find_matrix_row(&section, "PhysicsShapeQueryParameters3D");
    let raycast = find_matrix_row(&section, "RayCast3D");
    let shapecast = find_matrix_row(&section, "ShapeCast3D");

    assert!(ray_query.is_some(), "PhysicsRayQueryParameters3D must have its own row");
    assert!(shape_query.is_some(), "PhysicsShapeQueryParameters3D must have its own row");
    assert!(raycast.is_some(), "RayCast3D must have its own row");
    assert!(shapecast.is_some(), "ShapeCast3D must have its own row");
}

// ===========================================================================
// 3. RayCast3D and ShapeCast3D are NOT labeled "Measured"
// ===========================================================================

#[test]
fn raycast3d_not_measured() {
    let audit = read_file(AUDIT_PATH);
    let section = extract_matrix_section(&audit, "### scene/3d/physics — Physics Families");
    let row = find_matrix_row(&section, "RayCast3D").expect("RayCast3D must be in matrix");

    assert_ne!(
        row[2], "Measured",
        "RayCast3D should not be 'Measured' — only low-level query objects exist, \
         no node-level integration"
    );
}

#[test]
fn shapecast3d_not_measured() {
    let audit = read_file(AUDIT_PATH);
    let section = extract_matrix_section(&audit, "### scene/3d/physics — Physics Families");
    let row = find_matrix_row(&section, "ShapeCast3D").expect("ShapeCast3D must be in matrix");

    assert_ne!(
        row[2], "Measured",
        "ShapeCast3D should not be 'Measured' — only low-level query objects exist, \
         no node-level integration"
    );
}

// ===========================================================================
// 4. PhysicsRayQueryParameters3D IS measured (query-object layer is real)
// ===========================================================================

#[test]
fn physics_ray_query_is_measured() {
    let audit = read_file(AUDIT_PATH);
    let section = extract_matrix_section(&audit, "### scene/3d/physics — Physics Families");
    let row = find_matrix_row(&section, "PhysicsRayQueryParameters3D")
        .expect("PhysicsRayQueryParameters3D must be in matrix");

    assert_eq!(
        row[2], "Measured",
        "PhysicsRayQueryParameters3D should be 'Measured' — query tests exist"
    );
}

#[test]
fn physics_shape_query_is_measured() {
    let audit = read_file(AUDIT_PATH);
    let section = extract_matrix_section(&audit, "### scene/3d/physics — Physics Families");
    let row = find_matrix_row(&section, "PhysicsShapeQueryParameters3D")
        .expect("PhysicsShapeQueryParameters3D must be in matrix");

    assert_eq!(
        row[2], "Measured",
        "PhysicsShapeQueryParameters3D should be 'Measured' — query tests exist"
    );
}

// ===========================================================================
// 5. Parity report distinguishes query-object from node parity
// ===========================================================================

#[test]
fn parity_report_clarifies_query_not_node() {
    let report = read_file(PARITY_REPORT_PATH);

    assert!(
        report.contains("query-object layer, not `RayCast3D` node parity")
            || report.contains("query-object layer"),
        "parity report must clarify that PhysicsRayQuery3D is query-object level, \
         not RayCast3D node parity"
    );
}

#[test]
fn parity_report_area3d_is_not_measured() {
    let report = read_file(PARITY_REPORT_PATH);

    // Area3D should appear in "Implemented, not yet measured" section, not "Measured"
    let measured_start = report
        .find("### Measured")
        .expect("parity report must have Measured section");
    let implemented_start = report
        .find("### Implemented")
        .expect("parity report must have Implemented section");

    let measured_section = &report[measured_start..implemented_start];
    assert!(
        !measured_section.contains("Area3D"),
        "Area3D should NOT appear in the Measured section of the parity report"
    );

    let implemented_section = &report[implemented_start..];
    assert!(
        implemented_section.contains("Area3D"),
        "Area3D should appear in the Implemented section"
    );
}

// ===========================================================================
// 6. Migration guide does not overstate Area3D
// ===========================================================================

#[test]
fn migration_guide_area3d_not_full() {
    let guide = read_file(MIGRATION_GUIDE_PATH);
    // Check that Area3D is not marked as "Full" anywhere
    for line in guide.lines() {
        if line.contains("Area3D") {
            assert!(
                !line.contains("| Full |") && !line.contains("| full |"),
                "migration guide must not mark Area3D as Full — got: {line}"
            );
        }
    }
}

// ===========================================================================
// 7. All 5 entities exist in the matrix (comprehensive coverage)
// ===========================================================================

#[test]
fn all_five_entities_in_physics_matrix() {
    let audit = read_file(AUDIT_PATH);
    let section = extract_matrix_section(&audit, "### scene/3d/physics — Physics Families");

    let entities = [
        "Area3D",
        "PhysicsRayQueryParameters3D",
        "PhysicsShapeQueryParameters3D",
        "RayCast3D",
        "ShapeCast3D",
    ];

    for entity in &entities {
        assert!(
            section.contains(entity),
            "physics matrix must contain {entity}"
        );
    }
}
