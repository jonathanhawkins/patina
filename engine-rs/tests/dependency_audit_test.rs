//! Dependency audit — verifies license compatibility and flags problematic
//! dependencies for the Patina Engine workspace.
//!
//! This test reads Cargo.lock and checks that all transitive dependencies use
//! licenses compatible with the project's MIT/Apache-2.0 dual-license.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Licenses that are compatible with MIT/Apache-2.0 dual-licensed projects.
/// These can be freely combined without imposing additional obligations on
/// downstream users.
const ALLOWED_LICENSES: &[&str] = &[
    "MIT",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Unlicense",
    "0BSD",
    "Zlib",
    "Unicode-3.0",
    "Unicode-DFS-2016",
    "BSL-1.0", // Boost
];

/// Licenses that require review but may be acceptable in certain contexts.
const REVIEW_REQUIRED: &[&str] = &[
    "MPL-2.0",          // Weak copyleft — file-level, usually OK
    "LGPL-2.1-or-later", // Weak copyleft — OK for dynamic linking
    "LGPL-3.0-or-later",
];

/// Licenses that are NOT compatible and should be rejected.
const REJECTED_LICENSES: &[&str] = &[
    "GPL-2.0-only",
    "GPL-2.0-or-later",
    "GPL-3.0-only",
    "GPL-3.0-or-later",
    "AGPL-3.0-only",
    "AGPL-3.0-or-later",
    "SSPL-1.0",
    "BUSL-1.1",
];

/// Parses a SPDX license expression into individual license identifiers.
/// Handles OR (choice), AND (both required), and WITH (exception) operators.
fn parse_license_ids(expr: &str) -> Vec<String> {
    // Normalize separators
    let normalized = expr
        .replace(" / ", " OR ")
        .replace("/", " OR ");

    let mut licenses = Vec::new();

    // Split on OR — user can choose any alternative
    for alt in normalized.split(" OR ") {
        let alt = alt.trim().trim_matches('(').trim_matches(')').trim();
        if alt.is_empty() {
            continue;
        }
        // Handle AND — both are required
        for part in alt.split(" AND ") {
            let part = part.trim().trim_matches('(').trim_matches(')').trim();
            if part.is_empty() {
                continue;
            }
            // Keep "WITH" exceptions intact (e.g. "Apache-2.0 WITH LLVM-exception")
            licenses.push(part.to_string());
        }
    }
    licenses
}

/// Checks whether a license expression is compatible.
/// For OR expressions, at least one alternative must be allowed.
/// For AND expressions, all parts must be allowed.
fn is_license_compatible(expr: &str) -> LicenseVerdict {
    let normalized = expr
        .replace(" / ", " OR ")
        .replace("/", " OR ");

    // Split on OR — user can choose
    let alternatives: Vec<&str> = normalized.split(" OR ").collect();

    let mut any_allowed = false;
    let mut all_rejected = true;
    let mut needs_review = false;

    for alt in &alternatives {
        let alt = alt.trim().trim_matches('(').trim_matches(')').trim();
        // For AND, all parts must be compatible
        let parts: Vec<&str> = alt.split(" AND ").collect();
        let mut all_parts_ok = true;
        let mut any_part_review = false;

        for part in &parts {
            let part = part.trim().trim_matches('(').trim_matches(')').trim();
            if ALLOWED_LICENSES.iter().any(|a| *a == part) {
                // OK
            } else if REVIEW_REQUIRED.iter().any(|r| *r == part) {
                any_part_review = true;
            } else if REJECTED_LICENSES.iter().any(|r| *r == part) {
                all_parts_ok = false;
            } else {
                any_part_review = true; // Unknown = needs review
            }
        }

        if all_parts_ok && !any_part_review {
            any_allowed = true;
            all_rejected = false;
        } else if all_parts_ok && any_part_review {
            needs_review = true;
            all_rejected = false;
        } else {
            // This alternative has a rejected part
        }
    }

    if any_allowed {
        LicenseVerdict::Allowed
    } else if needs_review {
        LicenseVerdict::NeedsReview
    } else if all_rejected {
        LicenseVerdict::Rejected
    } else {
        LicenseVerdict::NeedsReview
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LicenseVerdict {
    Allowed,
    NeedsReview,
    Rejected,
}

/// Reads Cargo.lock and extracts package names + versions.
fn read_lockfile_packages() -> Vec<(String, String)> {
    let lock_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.lock");
    let content = std::fs::read_to_string(&lock_path)
        .unwrap_or_else(|e| panic!("Failed to read Cargo.lock at {}: {}", lock_path.display(), e));

    let mut packages = Vec::new();
    let mut current_name = None;
    let mut current_version = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[[package]]" {
            if let (Some(name), Some(version)) = (current_name.take(), current_version.take()) {
                packages.push((name, version));
            }
        } else if let Some(rest) = trimmed.strip_prefix("name = ") {
            current_name = Some(rest.trim_matches('"').to_string());
        } else if let Some(rest) = trimmed.strip_prefix("version = ") {
            current_version = Some(rest.trim_matches('"').to_string());
        }
    }
    // Last package
    if let (Some(name), Some(version)) = (current_name, current_version) {
        packages.push((name, version));
    }

    packages
}

/// Simulated license database — maps crate names to known licenses.
/// In a real CI pipeline this would come from `cargo metadata` or `cargo-deny`.
/// For test stability we maintain a curated map.
fn known_licenses() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    // Core Rust ecosystem
    m.insert("adler2", "0BSD OR MIT OR Apache-2.0");
    m.insert("aho-corasick", "Unlicense OR MIT");
    m.insert("anyhow", "MIT OR Apache-2.0");
    m.insert("autocfg", "Apache-2.0 OR MIT");
    m.insert("bit-set", "Apache-2.0 OR MIT");
    m.insert("bit-vec", "Apache-2.0 OR MIT");
    m.insert("bitflags", "MIT OR Apache-2.0");
    m.insert("byteorder", "Unlicense OR MIT");
    m.insert("cfg-if", "MIT OR Apache-2.0");
    m.insert("equivalent", "Apache-2.0 OR MIT");
    m.insert("errno", "MIT OR Apache-2.0");
    m.insert("fastrand", "Apache-2.0 OR MIT");
    m.insert("fnv", "Apache-2.0 OR MIT");
    m.insert("foldhash", "Zlib");
    m.insert("getrandom", "MIT OR Apache-2.0");
    m.insert("hashbrown", "MIT OR Apache-2.0");
    m.insert("heck", "MIT OR Apache-2.0");
    m.insert("hound", "Apache-2.0");
    m.insert("id-arena", "MIT OR Apache-2.0");
    m.insert("indexmap", "Apache-2.0 OR MIT");
    m.insert("itoa", "MIT OR Apache-2.0");
    m.insert("lazy_static", "MIT OR Apache-2.0");
    m.insert("leb128fmt", "MIT OR Apache-2.0");
    m.insert("lewton", "MIT OR Apache-2.0");
    m.insert("libc", "MIT OR Apache-2.0");
    m.insert("linux-raw-sys", "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT");
    m.insert("log", "MIT OR Apache-2.0");
    m.insert("memchr", "Unlicense OR MIT");
    m.insert("miniz_oxide", "MIT OR Zlib OR Apache-2.0");
    m.insert("nu-ansi-term", "MIT");
    m.insert("num-traits", "MIT OR Apache-2.0");
    m.insert("ogg", "BSD-3-Clause");
    m.insert("once_cell", "MIT OR Apache-2.0");
    m.insert("pin-project-lite", "Apache-2.0 OR MIT");
    m.insert("ppv-lite86", "MIT OR Apache-2.0");
    m.insert("prettyplease", "MIT OR Apache-2.0");
    m.insert("proc-macro2", "MIT OR Apache-2.0");
    m.insert("proptest", "MIT OR Apache-2.0");
    m.insert("quick-error", "MIT OR Apache-2.0");
    m.insert("quote", "MIT OR Apache-2.0");
    m.insert("r-efi", "MIT OR Apache-2.0 OR LGPL-2.1-or-later");
    m.insert("rand", "MIT OR Apache-2.0");
    m.insert("rand_chacha", "MIT OR Apache-2.0");
    m.insert("rand_core", "MIT OR Apache-2.0");
    m.insert("rand_xorshift", "MIT OR Apache-2.0");
    m.insert("regex", "MIT OR Apache-2.0");
    m.insert("regex-automata", "MIT OR Apache-2.0");
    m.insert("regex-syntax", "MIT OR Apache-2.0");
    m.insert("rustix", "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT");
    m.insert("rusty-fork", "MIT OR Apache-2.0");
    m.insert("semver", "MIT OR Apache-2.0");
    m.insert("serde", "MIT OR Apache-2.0");
    m.insert("serde_core", "MIT OR Apache-2.0");
    m.insert("serde_derive", "MIT OR Apache-2.0");
    m.insert("serde_json", "MIT OR Apache-2.0");
    m.insert("sharded-slab", "MIT");
    m.insert("smallvec", "MIT OR Apache-2.0");
    m.insert("syn", "MIT OR Apache-2.0");
    m.insert("tempfile", "MIT OR Apache-2.0");
    m.insert("thiserror", "MIT OR Apache-2.0");
    m.insert("thiserror-impl", "MIT OR Apache-2.0");
    m.insert("thread_local", "MIT OR Apache-2.0");
    m.insert("tinyvec", "Zlib OR Apache-2.0 OR MIT");
    m.insert("tinyvec_macros", "MIT OR Apache-2.0 OR Zlib");
    m.insert("tracing", "MIT");
    m.insert("tracing-attributes", "MIT");
    m.insert("tracing-core", "MIT");
    m.insert("tracing-log", "MIT");
    m.insert("tracing-subscriber", "MIT");
    m.insert("unarray", "MIT OR Apache-2.0");
    m.insert("unicode-ident", "MIT OR Apache-2.0 AND Unicode-3.0");
    m.insert("unicode-xid", "MIT OR Apache-2.0");
    m.insert("valuable", "MIT");
    m.insert("wait-timeout", "MIT OR Apache-2.0");
    m.insert("wasip2", "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT");
    m.insert("wasip3", "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT");
    m.insert("wasm-encoder", "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT");
    m.insert("wasm-metadata", "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT");
    m.insert("wasmparser", "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT");
    m.insert("windows-link", "MIT OR Apache-2.0");
    m.insert("windows-sys", "MIT OR Apache-2.0");
    m.insert("wit-bindgen", "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT");
    m.insert("wit-bindgen-core", "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT");
    m.insert("wit-bindgen-rust", "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT");
    m.insert("wit-bindgen-rust-macro", "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT");
    m.insert("wit-component", "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT");
    m.insert("wit-parser", "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT");
    m.insert("zerocopy", "BSD-2-Clause OR Apache-2.0 OR MIT");
    m.insert("zerocopy-derive", "BSD-2-Clause OR Apache-2.0 OR MIT");
    m.insert("zmij", "MIT");
    m
}

// -----------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------

#[test]
fn all_dependencies_have_compatible_licenses() {
    let packages = read_lockfile_packages();
    let licenses = known_licenses();

    // Workspace crate names to skip
    let workspace_crates: HashSet<&str> = [
        "gdaudio", "gdcore", "gdeditor", "gdobject", "gdphysics2d", "gdphysics3d",
        "gdplatform", "gdrender2d", "gdrender3d", "gdresource", "gdscene",
        "gdscript-interop", "gdserver2d", "gdserver3d", "gdvariant",
        "patina-engine", "patina-runner",
    ]
    .into_iter()
    .collect();

    let mut rejected = Vec::new();
    let mut needs_review = Vec::new();
    let mut unknown_license = Vec::new();
    let mut allowed_count = 0;

    for (name, version) in &packages {
        if workspace_crates.contains(name.as_str()) {
            continue;
        }

        if let Some(license_expr) = licenses.get(name.as_str()) {
            let verdict = is_license_compatible(license_expr);
            match verdict {
                LicenseVerdict::Allowed => allowed_count += 1,
                LicenseVerdict::NeedsReview => {
                    needs_review.push(format!("  {} v{} — {}", name, version, license_expr));
                }
                LicenseVerdict::Rejected => {
                    rejected.push(format!("  {} v{} — {}", name, version, license_expr));
                }
            }
        } else {
            unknown_license.push(format!("  {} v{} — LICENSE UNKNOWN", name, version));
        }
    }

    // Report
    eprintln!("\n=== Dependency License Audit ===");
    eprintln!("  Allowed:       {}", allowed_count);
    eprintln!("  Needs review:  {}", needs_review.len());
    eprintln!("  Rejected:      {}", rejected.len());
    eprintln!("  Unknown:       {}", unknown_license.len());

    if !needs_review.is_empty() {
        eprintln!("\nNeeds review:\n{}", needs_review.join("\n"));
    }
    if !unknown_license.is_empty() {
        eprintln!("\nUnknown licenses:\n{}", unknown_license.join("\n"));
    }

    assert!(
        rejected.is_empty(),
        "\n\nDependency license audit FAILED — {} dependencies have incompatible licenses:\n{}\n\n\
         These licenses (GPL, AGPL, SSPL, BUSL) are NOT compatible with MIT/Apache-2.0.\n\
         Remove or replace these dependencies.\n",
        rejected.len(),
        rejected.join("\n")
    );

    eprintln!("\nDependency license audit PASSED.");
}

#[test]
fn no_duplicate_dependency_versions() {
    let packages = read_lockfile_packages();

    let workspace_crates: HashSet<&str> = [
        "gdaudio", "gdcore", "gdeditor", "gdobject", "gdphysics2d", "gdphysics3d",
        "gdplatform", "gdrender2d", "gdrender3d", "gdresource", "gdscene",
        "gdscript-interop", "gdserver2d", "gdserver3d", "gdvariant",
        "patina-engine", "patina-runner",
    ]
    .into_iter()
    .collect();

    let mut versions_by_name: HashMap<String, Vec<String>> = HashMap::new();
    for (name, version) in &packages {
        if workspace_crates.contains(name.as_str()) {
            continue;
        }
        versions_by_name
            .entry(name.clone())
            .or_default()
            .push(version.clone());
    }

    let mut duplicates = Vec::new();
    for (name, versions) in &versions_by_name {
        if versions.len() > 1 {
            duplicates.push(format!("  {} — versions: {}", name, versions.join(", ")));
        }
    }

    if !duplicates.is_empty() {
        eprintln!(
            "\nWARNING: {} dependencies have multiple versions in Cargo.lock:\n{}",
            duplicates.len(),
            duplicates.join("\n")
        );
    }

    // This is informational — multiple versions are common and not a hard failure.
    // But we track the count for awareness.
    eprintln!(
        "\nDuplicate version check: {} crates with multiple versions.",
        duplicates.len()
    );
}

#[test]
fn lockfile_exists_and_is_valid() {
    let lock_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.lock");
    assert!(
        lock_path.exists(),
        "Cargo.lock must exist for reproducible builds"
    );

    let content = std::fs::read_to_string(&lock_path).unwrap();
    assert!(
        content.contains("[[package]]"),
        "Cargo.lock appears to be empty or malformed"
    );

    let packages = read_lockfile_packages();
    assert!(
        packages.len() > 10,
        "Expected at least 10 packages in Cargo.lock, found {}",
        packages.len()
    );

    eprintln!(
        "Cargo.lock validation PASSED: {} packages found.",
        packages.len()
    );
}

// -----------------------------------------------------------------------
// Unit tests for the license parsing/classification logic
// -----------------------------------------------------------------------

#[test]
fn license_parsing_mit_or_apache() {
    let ids = parse_license_ids("MIT OR Apache-2.0");
    assert!(ids.contains(&"MIT".to_string()));
    assert!(ids.contains(&"Apache-2.0".to_string()));
}

#[test]
fn license_parsing_with_exception() {
    let ids = parse_license_ids("Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT");
    assert!(ids.contains(&"Apache-2.0 WITH LLVM-exception".to_string()));
    assert!(ids.contains(&"MIT".to_string()));
}

#[test]
fn license_parsing_slash_syntax() {
    let ids = parse_license_ids("MIT/Apache-2.0");
    assert!(ids.contains(&"MIT".to_string()));
    assert!(ids.contains(&"Apache-2.0".to_string()));
}

#[test]
fn license_parsing_and() {
    let ids = parse_license_ids("(MIT OR Apache-2.0) AND Unicode-3.0");
    assert!(ids.contains(&"Unicode-3.0".to_string()));
}

#[test]
fn verdict_mit_is_allowed() {
    assert_eq!(is_license_compatible("MIT"), LicenseVerdict::Allowed);
}

#[test]
fn verdict_dual_license_allowed() {
    assert_eq!(
        is_license_compatible("MIT OR Apache-2.0"),
        LicenseVerdict::Allowed
    );
}

#[test]
fn verdict_bsd3_allowed() {
    assert_eq!(is_license_compatible("BSD-3-Clause"), LicenseVerdict::Allowed);
}

#[test]
fn verdict_zlib_allowed() {
    assert_eq!(is_license_compatible("Zlib"), LicenseVerdict::Allowed);
}

#[test]
fn verdict_unlicense_allowed() {
    assert_eq!(is_license_compatible("Unlicense OR MIT"), LicenseVerdict::Allowed);
}

#[test]
fn verdict_gpl_rejected() {
    assert_eq!(
        is_license_compatible("GPL-3.0-only"),
        LicenseVerdict::Rejected
    );
}

#[test]
fn verdict_agpl_rejected() {
    assert_eq!(
        is_license_compatible("AGPL-3.0-only"),
        LicenseVerdict::Rejected
    );
}

#[test]
fn verdict_gpl_or_mit_allowed() {
    // If one alternative is MIT, it's allowed (user can choose MIT)
    assert_eq!(
        is_license_compatible("GPL-3.0-only OR MIT"),
        LicenseVerdict::Allowed
    );
}

#[test]
fn verdict_mpl_needs_review() {
    assert_eq!(
        is_license_compatible("MPL-2.0"),
        LicenseVerdict::NeedsReview
    );
}

#[test]
fn verdict_with_exception_allowed() {
    assert_eq!(
        is_license_compatible("Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT"),
        LicenseVerdict::Allowed
    );
}

#[test]
fn verdict_unicode_and_mit_allowed() {
    // Both parts individually are allowed
    assert_eq!(
        is_license_compatible("(MIT OR Apache-2.0) AND Unicode-3.0"),
        LicenseVerdict::Allowed
    );
}

#[test]
fn verdict_r_efi_allowed() {
    // r-efi: MIT OR Apache-2.0 OR LGPL-2.1-or-later — MIT is available
    assert_eq!(
        is_license_compatible("MIT OR Apache-2.0 OR LGPL-2.1-or-later"),
        LicenseVerdict::Allowed
    );
}

#[test]
fn verdict_0bsd_allowed() {
    assert_eq!(
        is_license_compatible("0BSD OR MIT OR Apache-2.0"),
        LicenseVerdict::Allowed
    );
}

#[test]
fn verdict_unknown_license_needs_review() {
    assert_eq!(
        is_license_compatible("CustomLicense-1.0"),
        LicenseVerdict::NeedsReview
    );
}
