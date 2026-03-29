//! Dependency license audit — verifies all third-party crate dependencies use
//! licenses compatible with Patina's MIT/Apache-2.0 dual-license.
//!
//! Runs `cargo metadata` and checks every external dependency's license field
//! against an allowlist of known-compatible SPDX expressions.

use std::collections::HashMap;
use std::process::Command;

/// SPDX license expressions (or fragments) that are compatible with
/// MIT/Apache-2.0 dual-licensed projects. We normalize to lowercase for
/// comparison. Each entry is checked as a substring of the normalized license.
const ALLOWED_LICENSES: &[&str] = &[
    "mit",
    "apache-2.0",
    "bsd-2-clause",
    "bsd-3-clause",
    "isc",
    "unlicense",
    "0bsd",
    "zlib",
    "unicode-3.0",
    "unicode-dfs-2016",
    "cc0-1.0",
    "bsl-1.0",           // Boost
    "lgpl-2.1-or-later", // LGPL with "or later" is acceptable for linking
];

/// Licenses that are NOT compatible (copyleft without linking exception).
const BLOCKED_LICENSES: &[&str] = &["gpl-2.0-only", "gpl-3.0-only", "agpl-", "sspl", "eupl"];

fn is_license_compatible(license: &str) -> bool {
    let lower = license.to_lowercase();

    // Check for blocked licenses first
    for blocked in BLOCKED_LICENSES {
        if lower.contains(blocked) {
            return false;
        }
    }

    // Check if at least one allowed license is present
    // (handles OR expressions like "MIT OR Apache-2.0")
    ALLOWED_LICENSES
        .iter()
        .any(|allowed| lower.contains(allowed))
}

#[test]
fn all_dependencies_have_compatible_licenses() {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("Failed to run cargo metadata");

    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("Failed to parse cargo metadata JSON");

    let packages = metadata["packages"]
        .as_array()
        .expect("packages should be an array");

    let mut violations = Vec::new();
    let mut license_summary: HashMap<String, Vec<String>> = HashMap::new();
    let mut total_deps = 0;

    for pkg in packages {
        // Skip workspace-local packages (no source = local)
        if pkg["source"].is_null() {
            continue;
        }

        total_deps += 1;
        let name = pkg["name"].as_str().unwrap_or("unknown");
        let version = pkg["version"].as_str().unwrap_or("?");
        let license = pkg["license"].as_str().unwrap_or("UNKNOWN");

        license_summary
            .entry(license.to_string())
            .or_default()
            .push(format!("{name}@{version}"));

        if license == "UNKNOWN" || !is_license_compatible(license) {
            violations.push(format!("  {name}@{version}: {license}"));
        }
    }

    // Print summary
    eprintln!("\n=== Dependency License Audit ===");
    eprintln!("Total external dependencies: {total_deps}");
    eprintln!("Unique license expressions: {}", license_summary.len());
    for (lic, crates) in license_summary.iter() {
        eprintln!("  {lic}: {} crate(s)", crates.len());
    }

    if !violations.is_empty() {
        panic!(
            "\n\nDependency license audit FAILED — {} crate(s) have incompatible or unknown licenses:\n{}\n\n\
             Allowed licenses: MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Unlicense,\n\
             0BSD, Zlib, Unicode-3.0, Unicode-DFS-2016, CC0-1.0, BSL-1.0\n\n\
             To fix: either replace the dependency, or add its license to ALLOWED_LICENSES\n\
             after legal review confirms compatibility.\n",
            violations.len(),
            violations.join("\n")
        );
    }

    eprintln!("License audit PASSED: all {total_deps} dependencies have compatible licenses.");
}

#[test]
fn no_gpl_dependencies() {
    // Stricter check: ensure zero GPL/AGPL crates sneak in.
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("Failed to run cargo metadata");

    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("Failed to parse cargo metadata JSON");

    let packages = metadata["packages"]
        .as_array()
        .expect("packages should be an array");

    let mut gpl_crates = Vec::new();

    for pkg in packages {
        if pkg["source"].is_null() {
            continue;
        }
        let name = pkg["name"].as_str().unwrap_or("unknown");
        let license = pkg["license"].as_str().unwrap_or("").to_lowercase();

        // Match GPL-only licenses (not "OR MIT" alternatives)
        let has_gpl = license.contains("gpl");
        let has_permissive_alt =
            license.contains(" or ") && ALLOWED_LICENSES.iter().any(|a| license.contains(a));

        if has_gpl && !has_permissive_alt {
            gpl_crates.push(format!("  {name}: {license}"));
        }
    }

    if !gpl_crates.is_empty() {
        panic!(
            "\n\nGPL-only dependency detected! These crates have no permissive alternative:\n{}\n\n\
             GPL-only dependencies are incompatible with Patina's MIT/Apache-2.0 license.\n\
             Replace these dependencies or negotiate a licensing exception.\n",
            gpl_crates.join("\n")
        );
    }
}

#[test]
fn all_dependencies_have_license_field() {
    // Ensure no dependency is missing a license field entirely.
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("Failed to run cargo metadata");

    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("Failed to parse cargo metadata JSON");

    let packages = metadata["packages"]
        .as_array()
        .expect("packages should be an array");

    let mut missing = Vec::new();

    for pkg in packages {
        if pkg["source"].is_null() {
            continue;
        }
        let name = pkg["name"].as_str().unwrap_or("unknown");
        let version = pkg["version"].as_str().unwrap_or("?");
        let license = pkg["license"].as_str();

        if license.is_none() || license == Some("") {
            missing.push(format!("  {name}@{version}"));
        }
    }

    if !missing.is_empty() {
        panic!(
            "\n\n{} crate(s) are missing a license field in their Cargo.toml:\n{}\n\n\
             All dependencies must declare their license. Check the crate's repository\n\
             for a LICENSE file and contact the maintainer if needed.\n",
            missing.len(),
            missing.join("\n")
        );
    }
}
