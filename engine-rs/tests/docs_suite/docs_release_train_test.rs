//! pat-6zomc: Validate `prd/RELEASE_TRAIN.md` keeps the required release-train
//! workflow sections in place.
//!
//! Acceptance: the release-train workflow doc must document entry criteria,
//! exit criteria, cadence, and at least one worked example milestone, with
//! at least one bullet under each section so future planners can see real
//! content rather than empty headers.

use std::path::PathBuf;

fn doc_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("prd")
        .join("RELEASE_TRAIN.md")
}

fn doc_body() -> String {
    let p = doc_path();
    std::fs::read_to_string(&p).unwrap_or_else(|e| {
        panic!("failed to read {}: {e}", p.display());
    })
}

const REQUIRED_SECTIONS: &[&str] = &[
    "# Entry Criteria",
    "# Exit Criteria",
    "# Cadence",
    "# Example Milestone",
];

#[test]
fn doc_file_exists_and_is_nonempty() {
    let body = doc_body();
    assert!(
        body.len() > 200,
        "RELEASE_TRAIN.md should be substantive, got {} bytes",
        body.len()
    );
}

#[test]
fn doc_has_required_section_headers() {
    let body = doc_body();
    for section in REQUIRED_SECTIONS {
        // Match either H1 (# Entry Criteria) or H2 (## Entry Criteria) form
        // so the doc can be reorganized without churn-only updates.
        let h1 = format!("\n{section}\n");
        let h2 = format!("\n#{section}\n");
        let prefixed_h1 = body.starts_with(&format!("{section}\n")) || body.contains(&h1);
        let prefixed_h2 = body.contains(&h2);
        assert!(
            prefixed_h1 || prefixed_h2,
            "RELEASE_TRAIN.md is missing required section header: {section}"
        );
    }
}

#[test]
fn doc_has_at_least_one_bullet_under_each_section() {
    let body = doc_body();
    for section in REQUIRED_SECTIONS {
        let section_body = section_body_for(&body, section).unwrap_or_else(|| {
            panic!("RELEASE_TRAIN.md is missing section body for {section}");
        });
        let has_bullet = section_body
            .lines()
            .any(|l| l.trim_start().starts_with("- ") || l.trim_start().starts_with("* "));
        assert!(
            has_bullet,
            "section '{section}' must contain at least one bullet, got:\n{section_body}"
        );
    }
}

/// Extract the body of a section starting at `header` and ending at the next
/// H1/H2 header (or EOF). Returns `None` if `header` is absent. Tolerates
/// either H1 or H2 form for both the matched header and the terminator.
fn section_body_for(body: &str, header: &str) -> Option<String> {
    let mut iter = body.lines().peekable();
    // Find the header line, accepting either # Foo or ## Foo.
    let h1_line = header.to_string();
    let h2_line = format!("#{header}");
    let mut found = false;
    let mut collected: Vec<&str> = Vec::new();
    while let Some(line) = iter.next() {
        if !found {
            if line == h1_line || line == h2_line {
                found = true;
            }
            continue;
        }
        // Stop at the next H1 or H2 header.
        if line.starts_with("# ") || line.starts_with("## ") {
            break;
        }
        collected.push(line);
    }
    if !found {
        return None;
    }
    Some(collected.join("\n"))
}
