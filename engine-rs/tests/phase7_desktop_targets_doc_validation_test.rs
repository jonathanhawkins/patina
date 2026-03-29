//! pat-2uc5z: Doc-validation tests for Phase 7 supported desktop targets.
//!
//! Guards that:
//! 1. `prd/PHASE7_PLATFORM_PARITY_AUDIT.md` contains the required target matrix
//! 2. Every `DESKTOP_TARGETS` entry appears in the audit doc
//! 3. CI-tested status in code matches what the doc claims
//! 4. The doc distinguishes measured vs claimed coverage per target

use gdplatform::os::Platform;
use gdplatform::platform_targets::{ci_tested_targets, DESKTOP_TARGETS};

/// Read the Phase 7 audit doc.
fn read_audit_doc() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../prd/PHASE7_PLATFORM_PARITY_AUDIT.md"
    );
    std::fs::read_to_string(path).expect("should read prd/PHASE7_PLATFORM_PARITY_AUDIT.md")
}

// ===========================================================================
// 1. Audit doc contains the required sections
// ===========================================================================

#[test]
fn audit_doc_has_supported_desktop_targets_section() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("## Supported Desktop Targets"),
        "audit doc must contain a '## Supported Desktop Targets' section"
    );
}

#[test]
fn audit_doc_has_target_matrix() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("### Target Matrix"),
        "audit doc must contain a '### Target Matrix' section"
    );
}

#[test]
fn audit_doc_has_coverage_classification() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("### Coverage Classification Per Target"),
        "audit doc must contain a '### Coverage Classification Per Target' section"
    );
}

#[test]
fn audit_doc_has_what_supported_means() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("### What \"Supported\" Means"),
        "audit doc must explain what 'supported' means"
    );
}

// ===========================================================================
// 2. Every DESKTOP_TARGETS entry appears in the audit doc
// ===========================================================================

#[test]
fn audit_doc_lists_all_desktop_target_triples() {
    let doc = read_audit_doc();
    for target in DESKTOP_TARGETS {
        assert!(
            doc.contains(target.rust_triple),
            "audit doc must mention target triple '{}' ({})",
            target.rust_triple,
            target.name
        );
    }
}

#[test]
fn audit_doc_lists_all_desktop_target_names() {
    let doc = read_audit_doc();
    for target in DESKTOP_TARGETS {
        assert!(
            doc.contains(target.name),
            "audit doc must mention target name '{}'",
            target.name
        );
    }
}

// ===========================================================================
// 3. CI-tested status consistency
// ===========================================================================

#[test]
fn audit_doc_marks_ci_tested_targets_as_yes() {
    let doc = read_audit_doc();
    for target in DESKTOP_TARGETS {
        if !target.ci_tested {
            continue;
        }
        // The target row should contain "Yes" for CI tested.
        // Find the row containing the triple and check it has "Yes".
        let row = doc
            .lines()
            .find(|line| line.contains(target.rust_triple) && line.contains('|'))
            .unwrap_or_else(|| {
                panic!(
                    "audit doc must have a table row for CI-tested target '{}'",
                    target.rust_triple
                )
            });
        assert!(
            row.contains("Yes"),
            "target '{}' is ci_tested=true in code but audit doc row does not say 'Yes': {}",
            target.name,
            row
        );
    }
}

#[test]
fn audit_doc_marks_non_ci_tested_targets_as_no() {
    let doc = read_audit_doc();
    for target in DESKTOP_TARGETS {
        if target.ci_tested {
            continue;
        }
        let row = doc
            .lines()
            .find(|line| line.contains(target.rust_triple) && line.contains('|'))
            .unwrap_or_else(|| {
                panic!(
                    "audit doc must have a table row for target '{}'",
                    target.rust_triple
                )
            });
        assert!(
            row.contains("No"),
            "target '{}' is ci_tested=false in code but audit doc row does not say 'No': {}",
            target.name,
            row
        );
    }
}

// ===========================================================================
// 4. Measured vs claimed distinction
// ===========================================================================

#[test]
fn audit_doc_distinguishes_measured_and_claimed_columns() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("Measured Coverage"),
        "target matrix must have a 'Measured Coverage' column"
    );
    assert!(
        doc.contains("Claimed Coverage"),
        "target matrix must have a 'Claimed Coverage' column"
    );
}

#[test]
fn audit_doc_lists_validation_evidence_column() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("Validation Evidence"),
        "target matrix must have a 'Validation Evidence' column"
    );
}

#[test]
fn audit_doc_mentions_tier_classification() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("Tier 1"),
        "audit doc must classify targets into tiers"
    );
    assert!(
        doc.contains("Tier 2"),
        "audit doc must have at least two tiers"
    );
}

// ===========================================================================
// 5. Platform coverage: each OS family appears in the matrix
// ===========================================================================

#[test]
fn audit_doc_covers_linux_platform() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("Linux x86_64") && doc.contains("Linux aarch64"),
        "audit doc target matrix must list both Linux targets"
    );
}

#[test]
fn audit_doc_covers_macos_platform() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("macOS x86_64") && doc.contains("macOS aarch64"),
        "audit doc target matrix must list both macOS targets"
    );
}

#[test]
fn audit_doc_covers_windows_platform() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("Windows x86_64") && doc.contains("Windows aarch64"),
        "audit doc target matrix must list both Windows targets"
    );
}

#[test]
fn audit_doc_covers_web_target() {
    let doc = read_audit_doc();
    assert!(
        doc.contains("Web (WASM)"),
        "audit doc target matrix must list the Web target"
    );
}

// ===========================================================================
// 6. Guard that the doc does not overclaim native parity
// ===========================================================================

#[test]
fn audit_doc_does_not_claim_full_native_parity() {
    let doc = read_audit_doc();
    // The doc should explicitly disclaim full native parity.
    // The doc may use markdown bold (**not**) in the disclaimer.
    assert!(
        doc.contains("not** mean full native Godot DisplayServer parity")
            || doc.contains("not mean full native Godot DisplayServer parity")
            || doc.contains("not yet proven"),
        "audit doc must explicitly disclaim full native parity"
    );
}

// ===========================================================================
// 7. Cross-check: CI-tested target count matches code
// ===========================================================================

#[test]
fn ci_tested_count_matches_doc_tier1() {
    let doc = read_audit_doc();
    let ci_targets = ci_tested_targets();

    // Every CI-tested target name should appear in the Tier 1 section.
    // Find the Tier 1 block.
    let tier1_start = doc.find("Tier 1").expect("doc must have a Tier 1 section");
    let tier2_start = doc.find("Tier 2").unwrap_or(doc.len());
    let tier1_block = &doc[tier1_start..tier2_start];

    for target in &ci_targets {
        // Desktop targets only (skip Web).
        if target.platform == Platform::Web {
            continue;
        }
        assert!(
            tier1_block.contains(target.name),
            "CI-tested target '{}' must appear in Tier 1 section",
            target.name
        );
    }
}
