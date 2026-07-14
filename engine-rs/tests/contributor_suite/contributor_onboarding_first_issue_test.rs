//! pat-0vp2h: Validate first-issue guide and dev environment setup sections
//! in contributor-onboarding.md.
//!
//! Coverage:
//!  1. Dev Environment Setup section exists with all subsections
//!  2. Clone instructions present
//!  3. Rust install instructions present
//!  4. Node/pnpm install instructions present
//!  5. Python install instructions present
//!  6. Godot install instructions present
//!  7. Setup verification commands present
//!  8. Platform notes for macOS, Linux, Windows present
//!  9. Your First Issue section exists
//! 10. First issue walkthrough covers find, claim, implement, submit steps
//! 11. br CLI usage documented in first issue guide
//! 12. Example first issue workflow present

use std::fs;

const ONBOARDING_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../docs/contributor-onboarding.md"
);

fn read_onboarding() -> String {
    fs::read_to_string(ONBOARDING_PATH).expect("contributor-onboarding.md must exist")
}

// ===========================================================================
// Dev Environment Setup
// ===========================================================================

#[test]
fn dev_environment_setup_section_exists() {
    let doc = read_onboarding();
    assert!(
        doc.contains("## Dev Environment Setup"),
        "doc must have Dev Environment Setup section"
    );
}

#[test]
fn dev_setup_has_clone_instructions() {
    let doc = read_onboarding();
    assert!(
        doc.contains("git clone") && doc.contains("--recurse-submodules"),
        "dev setup must include clone instructions with submodules"
    );
}

#[test]
fn dev_setup_has_rust_install() {
    let doc = read_onboarding();
    assert!(
        doc.contains("rustup") && doc.contains("Install Rust"),
        "dev setup must include Rust installation instructions"
    );
}

#[test]
fn dev_setup_has_node_pnpm_install() {
    let doc = read_onboarding();
    assert!(
        doc.contains("pnpm") && doc.contains("Node"),
        "dev setup must include Node.js and pnpm instructions"
    );
}

#[test]
fn dev_setup_has_python_install() {
    let doc = read_onboarding();
    assert!(
        doc.contains("Python 3") && doc.contains("python3"),
        "dev setup must include Python install instructions"
    );
}

#[test]
fn dev_setup_has_godot_install() {
    let doc = read_onboarding();
    assert!(
        doc.contains("PATINA_GODOT") && doc.contains("Godot 4.6"),
        "dev setup must include Godot install instructions with env var"
    );
}

#[test]
fn dev_setup_has_verification_commands() {
    let doc = read_onboarding();
    assert!(
        doc.contains("Verify the Setup") || doc.contains("verify the setup"),
        "dev setup must include verification steps"
    );
    assert!(
        doc.contains("cargo build") && doc.contains("cargo test"),
        "verification must include build and test commands"
    );
}

#[test]
fn dev_setup_has_platform_notes() {
    let doc = read_onboarding();
    assert!(
        doc.contains("Platform Notes"),
        "dev setup must include platform-specific notes"
    );
    assert!(doc.contains("macOS"), "platform notes must cover macOS");
    assert!(doc.contains("Linux"), "platform notes must cover Linux");
    assert!(doc.contains("Windows"), "platform notes must cover Windows");
}

// ===========================================================================
// Your First Issue
// ===========================================================================

#[test]
fn first_issue_section_exists() {
    let doc = read_onboarding();
    assert!(
        doc.contains("## Your First Issue"),
        "doc must have Your First Issue section"
    );
}

#[test]
fn first_issue_covers_finding_work() {
    let doc = read_onboarding();
    assert!(
        doc.contains("br ready") && doc.contains("Find"),
        "first issue guide must explain how to find available beads"
    );
}

#[test]
fn first_issue_covers_claiming() {
    let doc = read_onboarding();
    assert!(
        doc.contains("br update") && doc.contains("Claim"),
        "first issue guide must explain how to claim a bead"
    );
}

#[test]
fn first_issue_covers_reading_description() {
    let doc = read_onboarding();
    assert!(
        doc.contains("br show") && doc.contains("Acceptance"),
        "first issue guide must explain reading bead description and acceptance criteria"
    );
}

#[test]
fn first_issue_covers_implementation() {
    let doc = read_onboarding();
    assert!(
        doc.contains("Implement") && doc.contains("Write tests"),
        "first issue guide must cover implementation and testing"
    );
}

#[test]
fn first_issue_covers_submission() {
    let doc = read_onboarding();
    assert!(
        doc.contains("Submit") && doc.contains("PR"),
        "first issue guide must cover PR submission"
    );
}

#[test]
fn first_issue_has_example_workflow() {
    let doc = read_onboarding();
    assert!(
        doc.contains("Example") && doc.contains("Parity Test"),
        "first issue guide must include an example workflow"
    );
}

#[test]
fn first_issue_example_shows_oracle_usage() {
    let doc = read_onboarding();
    assert!(
        doc.contains("oracle_outputs") && doc.contains("cargo test --test"),
        "example must show oracle data usage and running a specific test"
    );
}
