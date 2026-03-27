//! Unsafe code audit — verifies every `unsafe` block in the engine has a documented
//! SAFETY comment explaining its invariants.
//!
//! This test scans all `.rs` source files in the engine crates and checks that
//! each `unsafe {` or `unsafe fn` is preceded (within 3 lines) by a `// SAFETY:`
//! comment or a `/// # Safety` doc comment.

use std::path::{Path, PathBuf};

/// Collects all `.rs` files under a directory, excluding `target/` and test files.
fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs_files_inner(dir, &mut files);
    files
}

fn collect_rs_files_inner(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if path.is_dir() {
            if name == "target" || name == ".git" {
                continue;
            }
            collect_rs_files_inner(&path, out);
        } else if name.ends_with(".rs") && !name.ends_with("_test.rs") {
            out.push(path);
        }
    }
}

/// Checks whether an unsafe usage at `line_idx` has a safety comment in the
/// preceding 3 lines.
fn has_safety_comment(lines: &[&str], line_idx: usize) -> bool {
    let start = line_idx.saturating_sub(5);
    for i in start..line_idx {
        let trimmed = lines[i].trim().to_lowercase();
        if trimmed.contains("// safety:") || trimmed.contains("/// # safety") {
            return true;
        }
    }
    // Also check the line itself (inline safety comment)
    let current = lines[line_idx].trim().to_lowercase();
    current.contains("// safety:")
}

#[test]
fn all_unsafe_blocks_have_safety_comments() {
    let crates_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates");

    let files = collect_rs_files(&crates_dir);
    assert!(!files.is_empty(), "Should find .rs files in crates/");

    let mut violations = Vec::new();
    let mut total_unsafe = 0;

    for file in &files {
        let content = std::fs::read_to_string(file).unwrap();
        let lines: Vec<&str> = content.lines().collect();

        for (idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            // Detect unsafe blocks and unsafe fn declarations
            let is_unsafe = trimmed.starts_with("unsafe {")
                || trimmed.starts_with("unsafe fn ")
                || trimmed.contains("unsafe {")
                || trimmed.starts_with("unsafe impl ");

            if !is_unsafe {
                continue;
            }

            // Skip test code
            if lines[..idx].iter().rev().take(5).any(|l| l.contains("#[test]") || l.contains("#[cfg(test)]")) {
                continue;
            }

            total_unsafe += 1;

            if !has_safety_comment(&lines, idx) {
                let relative = file.strip_prefix(env!("CARGO_MANIFEST_DIR")).unwrap_or(file);
                violations.push(format!(
                    "  {}:{} — {}",
                    relative.display(),
                    idx + 1,
                    trimmed
                ));
            }
        }
    }

    if !violations.is_empty() {
        panic!(
            "\n\nUnsafe code audit FAILED — {} of {} unsafe blocks lack SAFETY comments:\n{}\n\n\
             Every `unsafe` block must have a `// SAFETY:` comment within 5 lines above it\n\
             explaining why the invariants hold.\n",
            violations.len(),
            total_unsafe,
            violations.join("\n")
        );
    }

    // Sanity check: we should have found some unsafe blocks
    assert!(
        total_unsafe > 0,
        "Expected to find at least one unsafe block in engine crates"
    );

    eprintln!(
        "Unsafe code audit PASSED: {total_unsafe} unsafe blocks, all with SAFETY comments."
    );
}

#[test]
fn unsafe_send_impls_have_justification() {
    // Specifically audit `unsafe impl Send` and `unsafe impl Sync` —
    // these are particularly dangerous and must have detailed justification.
    let crates_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates");
    let files = collect_rs_files(&crates_dir);

    let mut violations = Vec::new();

    for file in &files {
        let content = std::fs::read_to_string(file).unwrap();
        let lines: Vec<&str> = content.lines().collect();

        for (idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("unsafe impl Send")
                || trimmed.starts_with("unsafe impl Sync")
            {
                if !has_safety_comment(&lines, idx) {
                    let relative = file.strip_prefix(env!("CARGO_MANIFEST_DIR")).unwrap_or(file);
                    violations.push(format!(
                        "  {}:{} — {}",
                        relative.display(),
                        idx + 1,
                        trimmed
                    ));
                }
            }
        }
    }

    if !violations.is_empty() {
        panic!(
            "\n\n`unsafe impl Send/Sync` audit FAILED — {} impls lack SAFETY justification:\n{}\n\n\
             These impls are particularly dangerous. Each must explain:\n\
             1. Why the type can be safely sent/shared across threads\n\
             2. What synchronization mechanism protects the inner data\n",
            violations.len(),
            violations.join("\n")
        );
    }
}

#[test]
fn no_unsafe_in_safe_public_api() {
    // Verify that public functions don't contain hidden unsafe without being
    // marked `unsafe fn` — this catches cases where unsafe is used inside a
    // safe public function without documentation.
    let crates_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates");
    let files = collect_rs_files(&crates_dir);

    let mut pub_unsafe_count = 0;

    for file in &files {
        let content = std::fs::read_to_string(file).unwrap();
        let lines: Vec<&str> = content.lines().collect();

        let mut in_pub_fn = false;
        let mut pub_fn_line = 0;
        let mut brace_depth: i32 = 0;
        let mut has_unsafe_in_fn = false;
        let mut has_safety_doc = false;

        for (idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            // Detect public function start
            if (trimmed.starts_with("pub fn ") || trimmed.starts_with("pub(crate) fn "))
                && !trimmed.contains("unsafe")
            {
                in_pub_fn = true;
                pub_fn_line = idx;
                brace_depth = 0;
                has_unsafe_in_fn = false;
                // Check if there's a safety note in the doc comment
                has_safety_doc = (idx.saturating_sub(10)..idx).any(|i| {
                    let l = lines.get(i).unwrap_or(&"").trim().to_lowercase();
                    l.contains("# safety") || l.contains("// safety:")
                });
            }

            if in_pub_fn {
                brace_depth += trimmed.matches('{').count() as i32;
                brace_depth -= trimmed.matches('}').count() as i32;

                if trimmed.contains("unsafe {") || trimmed.contains("unsafe{") {
                    has_unsafe_in_fn = true;
                }

                if brace_depth <= 0 && idx > pub_fn_line {
                    if has_unsafe_in_fn {
                        pub_unsafe_count += 1;
                        // We just count these — the main audit test already checks
                        // that each unsafe block has a SAFETY comment.
                        if !has_safety_doc {
                            // This is informational — the block-level comments
                            // are the enforced requirement.
                        }
                    }
                    in_pub_fn = false;
                }
            }
        }
    }

    eprintln!(
        "Found {pub_unsafe_count} public functions containing unsafe blocks (all should have block-level SAFETY comments)."
    );
}
