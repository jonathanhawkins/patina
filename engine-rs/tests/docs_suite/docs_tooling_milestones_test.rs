//! pat-6llrl: Validate `prd/TOOLING_PARITY_MILESTONES.md` keeps every
//! enumerated milestone populated with required fields.

use std::path::PathBuf;

fn doc_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("prd")
        .join("TOOLING_PARITY_MILESTONES.md")
}

fn doc_body() -> String {
    let p = doc_path();
    std::fs::read_to_string(&p).unwrap_or_else(|e| {
        panic!("failed to read {}: {e}", p.display());
    })
}

const MIN_MILESTONES: usize = 5;
const REQUIRED_FIELDS: &[&str] = &["Owner:", "Exit evidence:", "Status:"];
const ALLOWED_STATUSES: &[&str] = &["not-started", "in-progress", "done"];

#[test]
fn doc_file_exists_and_is_nonempty() {
    let body = doc_body();
    assert!(
        body.len() > 200,
        "TOOLING_PARITY_MILESTONES.md should be substantive, got {} bytes",
        body.len()
    );
}

#[test]
fn milestone_count_meets_floor() {
    let body = doc_body();
    let count = body.matches("### Milestone:").count();
    assert!(
        count >= MIN_MILESTONES,
        "expected at least {} milestones, found {}",
        MIN_MILESTONES,
        count
    );
}

#[test]
fn each_milestone_has_required_fields() {
    let body = doc_body();
    let blocks: Vec<&str> = body.split("### Milestone:").skip(1).collect();
    assert!(!blocks.is_empty(), "no milestone blocks parsed from doc");

    for block in &blocks {
        // Stop at the next milestone header (already split) or at horizontal rule.
        let block_text = block.split("\n---").next().unwrap_or(block);
        for field in REQUIRED_FIELDS {
            assert!(
                block_text.contains(field),
                "milestone block is missing required field '{field}': {block_text}"
            );
        }
    }
}

#[test]
fn each_milestone_status_is_known() {
    let body = doc_body();
    let blocks: Vec<&str> = body.split("### Milestone:").skip(1).collect();
    for block in &blocks {
        let block_text = block.split("\n---").next().unwrap_or(block);
        let status_line = block_text
            .lines()
            .find(|l| l.contains("Status:"))
            .unwrap_or_else(|| panic!("milestone block has no Status: line: {block_text}"));
        let status = status_line
            .split("Status:")
            .nth(1)
            .map(str::trim)
            .unwrap_or("");
        assert!(
            ALLOWED_STATUSES.contains(&status),
            "unknown status '{status}'; allowed: {ALLOWED_STATUSES:?}"
        );
    }
}
