//! pat-e3ryt: Validate that deferred 3D systems are explicitly classified and
//! no Phase 6 doc implies parity for them without evidence.
//!
//! Ensures:
//! 1. The Phase 6 audit explicitly lists all deferred systems
//! 2. The parity report classifies deferred systems separately
//! 3. No deferred system is labeled "Measured" in the upstream class surface matrix
//! 4. The migration guide accurately reflects joint data-model status

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

/// Systems that are deferred or explicitly limited in Phase 6.
const DEFERRED_SYSTEMS: &[&str] = &[
    "SoftBody3D",
    "VehicleBody3D",
    "SpringArm3D",
    "NavigationAgent3D",
    "ConeTwistJoint3D",
    "Generic6DOFJoint3D",
    "LightmapGI",
];

/// Systems with data models only (no runtime integration).
const DATA_MODEL_ONLY: &[&str] = &[
    "PinJoint3D",
    "HingeJoint3D",
    "SliderJoint3D",
];

// ===========================================================================
// 1. Audit lists all deferred systems
// ===========================================================================

#[test]
fn audit_lists_all_deferred_systems() {
    let audit = read_file(AUDIT_PATH);
    let deferred_section_start = audit
        .find("### Deferred or explicitly limited")
        .expect("audit must have 'Deferred or explicitly limited' section");
    let deferred_section = &audit[deferred_section_start..];

    for system in DEFERRED_SYSTEMS {
        assert!(
            deferred_section.contains(system),
            "audit deferred section must mention {system}"
        );
    }
}

#[test]
fn audit_lists_data_model_only_joints() {
    let audit = read_file(AUDIT_PATH);
    let deferred_section_start = audit
        .find("### Deferred or explicitly limited")
        .expect("audit must have deferred section");
    let deferred_section = &audit[deferred_section_start..];

    for joint in DATA_MODEL_ONLY {
        assert!(
            deferred_section.contains(joint),
            "audit deferred section must mention data-model-only joint {joint}"
        );
    }
}

// ===========================================================================
// 2. Parity report classifies deferred systems separately
// ===========================================================================

#[test]
fn parity_report_has_deferred_section() {
    let report = read_file(PARITY_REPORT_PATH);
    assert!(
        report.contains("### Deferred or explicitly limited"),
        "parity report must have a 'Deferred or explicitly limited' section"
    );
}

#[test]
fn parity_report_lists_key_deferred_systems() {
    let report = read_file(PARITY_REPORT_PATH);
    let deferred_start = report
        .find("### Deferred or explicitly limited")
        .expect("parity report must have deferred section");
    let deferred_section = &report[deferred_start..];

    let key_systems = [
        "SoftBody3D",
        "VehicleBody3D",
        "NavigationAgent3D",
        "RayCast3D",
        "ShapeCast3D",
    ];

    for system in &key_systems {
        assert!(
            deferred_section.contains(system),
            "parity report deferred section must mention {system}"
        );
    }
}

// ===========================================================================
// 3. No deferred system is labeled "Measured" in the matrix
// ===========================================================================

fn extract_matrix_section(doc: &str, header: &str) -> String {
    let start = doc.find(header).unwrap_or_else(|| panic!("section '{header}' must exist"));
    let rest = &doc[start + header.len()..];
    let end = rest
        .find("\n### ")
        .or_else(|| rest.find("\n## "))
        .unwrap_or(rest.len());
    rest[..end].to_string()
}

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

#[test]
fn no_deferred_system_labeled_measured_in_matrix() {
    let audit = read_file(AUDIT_PATH);
    let sections = [
        "### scene/3d — Node and Rendering Families",
        "### scene/3d/physics — Physics Families",
        "### Render Server / Material Families",
    ];

    let all_deferred: Vec<&str> = DEFERRED_SYSTEMS
        .iter()
        .chain(DATA_MODEL_ONLY.iter())
        .copied()
        .collect();

    for header in &sections {
        let section = extract_matrix_section(&audit, header);
        let rows = parse_table_rows(&section);

        for row in &rows {
            let family = &row[0];
            let status = &row[2];

            for deferred in &all_deferred {
                if family.contains(deferred) && status == "Measured" {
                    panic!(
                        "deferred system {deferred} must not be labeled 'Measured' \
                         in matrix (found in row: {family})"
                    );
                }
            }
        }
    }
}

// ===========================================================================
// 4. Migration guide accurately reflects joint status
// ===========================================================================

#[test]
fn migration_guide_does_not_say_no_physics_joints() {
    let guide = read_file(MIGRATION_GUIDE_PATH);
    assert!(
        !guide.contains("No physics joints"),
        "migration guide must not say 'No physics joints' — \
         PinJoint3D/HingeJoint3D/SliderJoint3D exist as data models"
    );
}

#[test]
fn migration_guide_mentions_joint_data_model() {
    let guide = read_file(MIGRATION_GUIDE_PATH);
    assert!(
        guide.contains("data-model only") || guide.contains("data model"),
        "migration guide should clarify that joints are data-model only"
    );
}

// ===========================================================================
// 5. Audit has explicit "no implied parity" warning
// ===========================================================================

#[test]
fn audit_has_no_implied_parity_warning() {
    let audit = read_file(AUDIT_PATH);
    assert!(
        audit.contains("No Phase 6 doc should imply parity")
            || audit.contains("should not be represented as parity gaps"),
        "audit must have a warning against implying parity for deferred systems"
    );
}
