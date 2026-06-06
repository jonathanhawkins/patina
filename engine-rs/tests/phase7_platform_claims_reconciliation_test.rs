//! pat-c79eb + pat-rcii8: Validate that Phase 7 platform-layer docs use
//! approved status labels and do not imply full native platform parity where
//! only headless or model/API coverage exists.
//!
//! Ensures:
//! 1. COMPAT_MATRIX platform rows use approved status labels
//! 2. Phase 7 audit uses only the four approved statuses
//! 3. PLATFORM_STABLE_LAYER doc uses approved labels
//! 4. No doc claims native platform parity without evidence
//! 5. Headless vs native distinction is maintained
//! 6. Per-OS classification tables exist in COMPAT_MATRIX and PLATFORM_STABLE_LAYER
//! 7. Per-OS tables cite concrete test files for each claim
//! 8. No per-OS row conflates headless/model coverage with native-runtime parity

use std::fs;

const COMPAT_MATRIX_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../COMPAT_MATRIX.md");
const AUDIT_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../prd/PHASE7_PLATFORM_PARITY_AUDIT.md");
const STABLE_LAYER_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/PLATFORM_STABLE_LAYER.md");
const MIGRATION_GUIDE_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/migration-guide.md");

fn read_file(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("must read {path}: {e}"))
}

const APPROVED_STATUSES: &[&str] = &[
    "Measured",
    "Implemented, not yet measured",
    "Deferred",
    "Missing",
];

// ===========================================================================
// 1. COMPAT_MATRIX platform rows use approved labels
// ===========================================================================

#[test]
fn compat_matrix_platform_rows_use_approved_statuses() {
    let matrix = read_file(COMPAT_MATRIX_PATH);
    let platform_section_start = matrix
        .find("## Platform Support Matrix")
        .expect("COMPAT_MATRIX must have Platform Support Matrix section");
    let section = &matrix[platform_section_start..];

    // Bound to just this section: stop at next ## heading, thematic break (---), or end
    let end = section[1..]
        .find("\n## ")
        .map(|i| i + 1)
        .unwrap_or(section.len());
    let bounded = &section[..end];

    // Also stop at thematic break (--- on its own line) if it comes earlier
    let break_end = bounded
        .find("\n---\n")
        .unwrap_or(bounded.len());
    let bounded = &bounded[..break_end];

    // Find table rows: must start with '|', skip header (contains "Platform")
    // and separator (contains "---")
    let rows: Vec<&str> = bounded
        .lines()
        .filter(|l| l.starts_with('|') && !l.contains("Platform") && !l.contains("---"))
        .collect();

    for row in &rows {
        let cols: Vec<&str> = row.split('|').map(|c| c.trim()).filter(|c| !c.is_empty()).collect();
        if cols.len() >= 2 {
            let status = cols[1].replace("**", ""); // strip bold markers
            // Check it's one of the approved statuses
            assert!(
                APPROVED_STATUSES.iter().any(|s| status.contains(s)),
                "platform row '{}' uses non-approved status '{}'. \
                 Approved: {:?}",
                cols[0],
                status,
                APPROVED_STATUSES
            );
        }
    }
}

#[test]
fn compat_matrix_does_not_use_claimed_status() {
    let matrix = read_file(COMPAT_MATRIX_PATH);
    let platform_section_start = matrix
        .find("## Platform Support Matrix")
        .expect("COMPAT_MATRIX must have Platform Support Matrix section");
    let section = &matrix[platform_section_start..];

    // Find the end of this section
    let end = section[1..]
        .find("\n## ")
        .map(|i| i + 1)
        .unwrap_or(section.len());
    let bounded = &section[..end];

    assert!(
        !bounded.contains("| Claimed |") && !bounded.contains("| Claimed"),
        "platform section must not use 'Claimed' as a status label — \
         use 'Implemented, not yet measured' instead"
    );
}

// ===========================================================================
// 2. Phase 7 audit does not use "partly measured"
// ===========================================================================

#[test]
fn audit_does_not_use_partly_measured() {
    let audit = read_file(AUDIT_PATH);
    assert!(
        !audit.contains("partly measured"),
        "Phase 7 audit must not use 'partly measured' — \
         use 'Implemented, not yet measured' instead"
    );
}

// ===========================================================================
// 3. PLATFORM_STABLE_LAYER uses approved labels
// ===========================================================================

#[test]
fn stable_layer_does_not_use_partly_measured() {
    let doc = read_file(STABLE_LAYER_PATH);
    assert!(
        !doc.contains("partly measured"),
        "PLATFORM_STABLE_LAYER must not use 'partly measured' — \
         use 'Implemented, not yet measured' instead"
    );
}

// ===========================================================================
// 4. Headless vs native distinction is maintained
// ===========================================================================

#[test]
fn stable_layer_notes_headless_scope() {
    let doc = read_file(STABLE_LAYER_PATH);
    assert!(
        doc.contains("headless mode") || doc.contains("headless"),
        "PLATFORM_STABLE_LAYER must mention headless scope"
    );
    assert!(
        doc.contains("not") && doc.contains("full parity")
            || doc.contains("not yet measured")
            || doc.contains("not claimed as full parity"),
        "PLATFORM_STABLE_LAYER must note that native parity is not claimed"
    );
}

#[test]
fn compat_matrix_notes_headless_scope() {
    let matrix = read_file(COMPAT_MATRIX_PATH);
    let platform_section_start = matrix
        .find("## Platform Support Matrix")
        .expect("COMPAT_MATRIX must have Platform Support Matrix");
    let section = &matrix[platform_section_start..];
    let end = section[1..]
        .find("\n## ")
        .map(|i| i + 1)
        .unwrap_or(section.len());
    let bounded = &section[..end];

    assert!(
        bounded.contains("headless") || bounded.contains("stable-layer"),
        "platform section must reference headless or stable-layer scope"
    );
}

// ===========================================================================
// 5. Native platform layers are not labeled "Measured"
// ===========================================================================

#[test]
fn native_platform_layers_not_measured_in_audit() {
    let audit = read_file(AUDIT_PATH);

    let native_families = [
        "Linux platform layer",
        "macOS platform layer",
        "Windows platform layer",
    ];

    for family in &native_families {
        let start = audit.find(family).unwrap_or_else(|| {
            panic!("audit must contain '{family}'");
        });
        let rest = &audit[start..];
        let classification_line = rest
            .lines()
            .find(|l| l.contains("Current classification:"))
            .unwrap_or_else(|| panic!("'{family}' must have a classification line"));

        assert!(
            !classification_line.contains("`Measured`"),
            "native {family} should not be classified as Measured — \
             only headless coverage is measured. Got: {classification_line}"
        );
    }
}

// ===========================================================================
// 6. Per-OS classification tables exist in COMPAT_MATRIX
// ===========================================================================

#[test]
fn compat_matrix_has_per_os_classification_section() {
    let matrix = read_file(COMPAT_MATRIX_PATH);
    assert!(
        matrix.contains("### Per-OS Native Platform-Layer Classification"),
        "COMPAT_MATRIX must have a Per-OS Native Platform-Layer Classification section"
    );
}

#[test]
fn compat_matrix_has_per_os_tables_for_all_three_platforms() {
    let matrix = read_file(COMPAT_MATRIX_PATH);
    for os in &["#### Linux", "#### macOS", "#### Windows"] {
        assert!(
            matrix.contains(os),
            "COMPAT_MATRIX per-OS classification must include {os}"
        );
    }
}

#[test]
fn compat_matrix_per_os_tables_use_approved_classifications() {
    let matrix = read_file(COMPAT_MATRIX_PATH);
    let section_start = matrix
        .find("### Per-OS Native Platform-Layer Classification")
        .expect("must have per-OS section");
    let section = &matrix[section_start..];

    // Bound to this section (stop at next ## or ---)
    let end = section[1..]
        .find("\n## ")
        .map(|i| i + 1)
        .unwrap_or(section.len());
    let bounded = &section[..end];

    let approved = ["Headless/model", "Native-runtime", "Deferred"];

    // Check table rows (start with '|', skip headers and separators)
    let rows: Vec<&str> = bounded
        .lines()
        .filter(|l| {
            l.starts_with('|')
                && !l.contains("Surface")
                && !l.contains("Classification")
                && !l.contains("---")
        })
        .collect();

    assert!(
        rows.len() >= 9,
        "per-OS classification must have at least 9 surface rows (3 per OS), found {}",
        rows.len()
    );

    for row in &rows {
        let cols: Vec<&str> = row.split('|').map(|c| c.trim()).filter(|c| !c.is_empty()).collect();
        if cols.len() >= 2 {
            let classification = cols[1];
            assert!(
                approved.iter().any(|a| classification.contains(a)),
                "per-OS row '{}' uses non-approved classification '{}'. \
                 Approved: {:?}",
                cols[0],
                classification,
                approved
            );
        }
    }
}

#[test]
fn compat_matrix_per_os_tables_cite_test_files() {
    let matrix = read_file(COMPAT_MATRIX_PATH);
    let section_start = matrix
        .find("### Per-OS Native Platform-Layer Classification")
        .expect("must have per-OS section");
    let section = &matrix[section_start..];

    let end = section[1..]
        .find("\n## ")
        .map(|i| i + 1)
        .unwrap_or(section.len());
    let bounded = &section[..end];

    // Every Headless/model row must cite at least one _test.rs file
    let rows: Vec<&str> = bounded
        .lines()
        .filter(|l| l.starts_with('|') && l.contains("Headless/model"))
        .collect();

    assert!(!rows.is_empty(), "must have Headless/model rows");

    for row in &rows {
        assert!(
            row.contains("_test.rs") || row.contains("_test`"),
            "Headless/model row must cite a concrete test file: {}",
            row
        );
    }
}

#[test]
fn compat_matrix_per_os_no_native_runtime_without_evidence() {
    let matrix = read_file(COMPAT_MATRIX_PATH);
    let section_start = matrix
        .find("### Per-OS Native Platform-Layer Classification")
        .expect("must have per-OS section");
    let section = &matrix[section_start..];

    let end = section[1..]
        .find("\n## ")
        .map(|i| i + 1)
        .unwrap_or(section.len());
    let bounded = &section[..end];

    // Any row classified as Native-runtime must cite a test file
    let native_rows: Vec<&str> = bounded
        .lines()
        .filter(|l| l.starts_with('|') && l.contains("Native-runtime"))
        .collect();

    for row in &native_rows {
        assert!(
            row.contains("_test.rs") || row.contains("_test`"),
            "Native-runtime row must cite evidence: {}",
            row
        );
    }
}

// ===========================================================================
// 7. Per-OS classification in PLATFORM_STABLE_LAYER
// ===========================================================================

#[test]
fn stable_layer_has_per_os_classification() {
    let doc = read_file(STABLE_LAYER_PATH);
    assert!(
        doc.contains("## Per-OS Native Platform Classification"),
        "PLATFORM_STABLE_LAYER must have Per-OS Native Platform Classification section"
    );
}

#[test]
fn stable_layer_per_os_covers_all_three_platforms() {
    let doc = read_file(STABLE_LAYER_PATH);
    let section_start = doc
        .find("## Per-OS Native Platform Classification")
        .expect("must have per-OS section");
    let section = &doc[section_start..];

    for os in &["Linux", "macOS", "Windows"] {
        assert!(
            section.contains(os),
            "PLATFORM_STABLE_LAYER per-OS classification must cover {os}"
        );
    }
}

#[test]
fn stable_layer_per_os_cites_test_evidence() {
    let doc = read_file(STABLE_LAYER_PATH);
    let section_start = doc
        .find("### Test Evidence Per OS")
        .expect("must have Test Evidence Per OS section");
    let section = &doc[section_start..];

    // Must cite at least one test file per OS
    for os in &["Linux", "macOS", "Windows"] {
        let os_line = section
            .lines()
            .find(|l| l.contains(os) && l.contains("_test.rs"))
            .unwrap_or_else(|| panic!("Test Evidence section must cite test files for {os}"));
        assert!(
            os_line.contains("platform"),
            "Test evidence for {os} should reference platform test files"
        );
    }
}

#[test]
fn stable_layer_per_os_does_not_conflate_headless_with_native() {
    let doc = read_file(STABLE_LAYER_PATH);
    let section_start = doc
        .find("## Per-OS Native Platform Classification")
        .expect("must have per-OS section");
    let section = &doc[section_start..];

    // The section must explicitly state that all tests are headless/model
    assert!(
        section.contains("headless/model behavior only")
            || section.contains("headless/model"),
        "per-OS section must explicitly state tests are headless/model"
    );

    // Must not claim "full parity" or "native parity" within this section
    let end = section[1..]
        .find("\n## ")
        .map(|i| i + 1)
        .unwrap_or(section.len());
    let bounded = &section[..end];

    assert!(
        !bounded.contains("full parity") && !bounded.contains("full native parity"),
        "per-OS section must not claim full parity"
    );
}

// ===========================================================================
// 8. Migration guide Phase 7 section uses approved status labels
// ===========================================================================

#[test]
fn migration_guide_phase7_uses_approved_status_labels() {
    let guide = read_file(MIGRATION_GUIDE_PATH);
    let phase7_start = guide
        .find("## Milestone 5: Platform Layer (Phase 7)")
        .expect("migration guide must have Phase 7 milestone section");
    let section = &guide[phase7_start..];

    // Bound to just this section
    let end = section[1..]
        .find("\n## ")
        .map(|i| i + 1)
        .unwrap_or(section.len());
    let bounded = &section[..end];

    // Must contain all four approved status labels
    for label in APPROVED_STATUSES {
        assert!(
            bounded.contains(label),
            "migration guide Phase 7 section must use approved label '{}'\n\
             Approved: {:?}",
            label,
            APPROVED_STATUSES
        );
    }
}

#[test]
fn migration_guide_phase7_does_not_imply_full_native_parity() {
    let guide = read_file(MIGRATION_GUIDE_PATH);
    let phase7_start = guide
        .find("## Milestone 5: Platform Layer (Phase 7)")
        .expect("migration guide must have Phase 7 milestone section");
    let section = &guide[phase7_start..];

    let end = section[1..]
        .find("\n## ")
        .map(|i| i + 1)
        .unwrap_or(section.len());
    let bounded = &section[..end];

    // Must mention headless or stable-layer scope
    assert!(
        bounded.contains("headless") || bounded.contains("stable-layer"),
        "migration guide Phase 7 must reference headless or stable-layer scope"
    );

    // Must not claim full native platform parity
    assert!(
        !bounded.contains("full native platform parity is measured")
            && !bounded.contains("complete platform parity"),
        "migration guide Phase 7 must not claim full native platform parity"
    );
}

#[test]
fn migration_guide_phase7_distinguishes_measured_from_implemented() {
    let guide = read_file(MIGRATION_GUIDE_PATH);
    let phase7_start = guide
        .find("## Milestone 5: Platform Layer (Phase 7)")
        .expect("migration guide must have Phase 7 milestone section");
    let section = &guide[phase7_start..];

    let end = section[1..]
        .find("\n## ")
        .map(|i| i + 1)
        .unwrap_or(section.len());
    let bounded = &section[..end];

    // Must have separate sections for Measured and Implemented
    assert!(
        bounded.contains("**Measured:**") || bounded.contains("**Measured**"),
        "migration guide Phase 7 must have a Measured section"
    );
    assert!(
        bounded.contains("**Implemented, not yet measured:**")
            || bounded.contains("**Implemented, not yet measured**"),
        "migration guide Phase 7 must have an Implemented-not-yet-measured section"
    );
    assert!(
        bounded.contains("**Deferred:**") || bounded.contains("**Deferred**"),
        "migration guide Phase 7 must have a Deferred section"
    );
}
