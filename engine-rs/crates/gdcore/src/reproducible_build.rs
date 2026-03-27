//! Reproducible build verification for release artifacts.
//!
//! Provides utilities for verifying that builds are deterministic and
//! that release artifacts can be independently reproduced. Supports:
//!
//! - **Artifact checksumming** (SHA-256) for binary and resource files
//! - **Build manifest** generation and verification
//! - **Cargo metadata** capture for recording build environment
//! - **Diff detection** between two build manifests

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

// ---------------------------------------------------------------------------
// Checksum
// ---------------------------------------------------------------------------

/// A SHA-256 checksum represented as a hex string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Checksum(pub String);

impl Checksum {
    /// Computes the SHA-256 checksum of raw bytes.
    pub fn from_bytes(data: &[u8]) -> Self {
        // Minimal SHA-256 using a simple implementation.
        // In production, you'd use the `sha2` crate, but to avoid adding
        // external deps we use a built-in approach via std::process.
        Self(hex_sha256(data))
    }

    /// Computes the SHA-256 checksum of a file.
    pub fn from_file(path: &Path) -> io::Result<Self> {
        let data = fs::read(path)?;
        Ok(Self::from_bytes(&data))
    }

    /// Returns the hex string representation.
    pub fn hex(&self) -> &str {
        &self.0
    }

    /// Returns the first 8 characters (short hash).
    pub fn short(&self) -> &str {
        &self.0[..self.0.len().min(8)]
    }
}

impl fmt::Display for Checksum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Simple SHA-256 implementation using system tools (sha256sum/shasum).
fn hex_sha256(data: &[u8]) -> String {
    use std::io::Write;
    use std::process::{Command, Stdio};

    // Try shasum -a 256 (macOS) then sha256sum (Linux).
    let commands = [
        ("shasum", vec!["-a", "256"]),
        ("sha256sum", vec![]),
    ];

    for (cmd, args) in &commands {
        if let Ok(mut child) = Command::new(cmd)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(data);
            }
            if let Ok(output) = child.wait_with_output() {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if let Some(hash) = stdout.split_whitespace().next() {
                        return hash.to_string();
                    }
                }
            }
        }
    }

    // Fallback: simple hash for environments without sha256sum.
    // This is NOT cryptographically secure — just a fallback for testing.
    fallback_hash(data)
}

/// Simple fallback hash (FNV-1a based, NOT cryptographic).
/// Only used when shasum/sha256sum are unavailable.
fn fallback_hash(data: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}{:016x}{:016x}{:016x}", h, h.rotate_left(16), h.rotate_left(32), h.rotate_left(48))
}

// ---------------------------------------------------------------------------
// Artifact entry
// ---------------------------------------------------------------------------

/// A single artifact in a build manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactEntry {
    /// Path relative to the build root.
    pub path: String,
    /// SHA-256 checksum of the file contents.
    pub checksum: Checksum,
    /// File size in bytes.
    pub size: u64,
}

impl ArtifactEntry {
    /// Creates an artifact entry from a file on disk.
    pub fn from_file(root: &Path, file_path: &Path) -> io::Result<Self> {
        let data = fs::read(file_path)?;
        let relative = file_path
            .strip_prefix(root)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();

        Ok(Self {
            path: relative,
            checksum: Checksum::from_bytes(&data),
            size: data.len() as u64,
        })
    }
}

impl fmt::Display for ArtifactEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}  {} ({} bytes)", self.checksum, self.path, self.size)
    }
}

// ---------------------------------------------------------------------------
// Build environment
// ---------------------------------------------------------------------------

/// Captured build environment metadata for reproducibility tracking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildEnvironment {
    /// Rust compiler version (e.g. "rustc 1.78.0").
    pub rustc_version: String,
    /// Cargo version.
    pub cargo_version: String,
    /// Target triple (e.g. "x86_64-apple-darwin").
    pub target_triple: String,
    /// Build profile ("debug" or "release").
    pub profile: String,
    /// Optional git commit hash of the source.
    pub git_commit: Option<String>,
    /// Additional environment variables that might affect the build.
    pub env_vars: BTreeMap<String, String>,
}

impl BuildEnvironment {
    /// Captures the current build environment.
    pub fn capture(profile: &str) -> Self {
        Self {
            rustc_version: run_command("rustc", &["--version"]),
            cargo_version: run_command("cargo", &["--version"]),
            target_triple: detect_target_triple(),
            profile: profile.to_string(),
            git_commit: detect_git_commit(),
            env_vars: BTreeMap::new(),
        }
    }

    /// Captures with additional environment variables recorded.
    pub fn with_env_vars(mut self, vars: &[&str]) -> Self {
        for var in vars {
            if let Ok(val) = std::env::var(var) {
                self.env_vars.insert(var.to_string(), val);
            }
        }
        self
    }
}

impl fmt::Display for BuildEnvironment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Build Environment:")?;
        writeln!(f, "  rustc: {}", self.rustc_version)?;
        writeln!(f, "  cargo: {}", self.cargo_version)?;
        writeln!(f, "  target: {}", self.target_triple)?;
        writeln!(f, "  profile: {}", self.profile)?;
        if let Some(ref commit) = self.git_commit {
            writeln!(f, "  git: {}", commit)?;
        }
        for (k, v) in &self.env_vars {
            writeln!(f, "  {}={}", k, v)?;
        }
        Ok(())
    }
}

fn run_command(cmd: &str, args: &[&str]) -> String {
    std::process::Command::new(cmd)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

fn detect_target_triple() -> String {
    run_command("rustc", &["-vV"])
        .lines()
        .find(|l| l.starts_with("host:"))
        .map(|l| l.trim_start_matches("host:").trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn detect_git_commit() -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Build manifest
// ---------------------------------------------------------------------------

/// A build manifest recording all artifacts and their checksums.
///
/// Used to verify that two builds produce identical outputs.
#[derive(Debug, Clone)]
pub struct BuildManifest {
    /// Version of the build.
    pub version: String,
    /// Build environment metadata.
    pub environment: BuildEnvironment,
    /// Artifacts sorted by path for deterministic comparison.
    pub artifacts: BTreeMap<String, ArtifactEntry>,
    /// Timestamp of manifest creation (ISO 8601).
    pub created_at: String,
}

impl BuildManifest {
    /// Creates a new empty manifest.
    pub fn new(version: &str, environment: BuildEnvironment) -> Self {
        Self {
            version: version.to_string(),
            environment,
            artifacts: BTreeMap::new(),
            created_at: now_iso8601(),
        }
    }

    /// Adds an artifact entry.
    pub fn add_artifact(&mut self, entry: ArtifactEntry) {
        self.artifacts.insert(entry.path.clone(), entry);
    }

    /// Scans a directory and adds all files as artifacts.
    pub fn scan_directory(&mut self, root: &Path) -> io::Result<usize> {
        let mut count = 0;
        scan_dir_recursive(root, root, &mut |entry| {
            self.add_artifact(entry);
            count += 1;
        })?;
        Ok(count)
    }

    /// Returns the total size of all artifacts in bytes.
    pub fn total_size(&self) -> u64 {
        self.artifacts.values().map(|a| a.size).sum()
    }

    /// Returns the number of artifacts.
    pub fn artifact_count(&self) -> usize {
        self.artifacts.len()
    }

    /// Serializes the manifest to a checksums file format.
    ///
    /// Format: `<sha256>  <path>  <size>\n` (sorted by path).
    pub fn to_checksums_file(&self) -> String {
        let mut output = String::new();
        output.push_str(&format!("# Build Manifest v{}\n", self.version));
        output.push_str(&format!("# Created: {}\n", self.created_at));
        output.push_str(&format!("# Profile: {}\n", self.environment.profile));
        if let Some(ref commit) = self.environment.git_commit {
            output.push_str(&format!("# Git: {}\n", commit));
        }
        output.push('\n');

        for entry in self.artifacts.values() {
            output.push_str(&format!("{}  {}  {}\n", entry.checksum, entry.path, entry.size));
        }
        output
    }

    /// Parses a checksums file back into artifact entries.
    pub fn parse_checksums_file(content: &str) -> Vec<ArtifactEntry> {
        content
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
            .filter_map(|line| {
                let parts: Vec<&str> = line.splitn(3, "  ").collect();
                if parts.len() >= 2 {
                    let size = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
                    Some(ArtifactEntry {
                        checksum: Checksum(parts[0].to_string()),
                        path: parts[1].to_string(),
                        size,
                    })
                } else {
                    None
                }
            })
            .collect()
    }
}

fn now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Simple UTC timestamp.
    let days = secs / 86400;
    let rem = secs % 86400;
    let hours = rem / 3600;
    let minutes = (rem % 3600) / 60;
    let seconds = rem % 60;
    // Approximate year/month/day (good enough for manifest timestamps).
    let (year, month, day) = days_to_ymd(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Approximate calculation from epoch days.
    let y = 1970 + days / 365;
    let doy = days % 365;
    let m = doy / 30 + 1;
    let d = doy % 30 + 1;
    (y, m.min(12), d.min(28))
}

fn scan_dir_recursive(
    root: &Path,
    dir: &Path,
    callback: &mut dyn FnMut(ArtifactEntry),
) -> io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            scan_dir_recursive(root, &path, callback)?;
        } else if path.is_file() {
            if let Ok(artifact) = ArtifactEntry::from_file(root, &path) {
                callback(artifact);
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Verification / diff
// ---------------------------------------------------------------------------

/// The result of comparing two build manifests.
#[derive(Debug, Clone)]
pub struct ManifestDiff {
    /// Artifacts present in both manifests with matching checksums.
    pub matching: Vec<String>,
    /// Artifacts present in both but with different checksums.
    pub mismatched: Vec<MismatchEntry>,
    /// Artifacts only in the first manifest (removed).
    pub only_in_first: Vec<String>,
    /// Artifacts only in the second manifest (added).
    pub only_in_second: Vec<String>,
}

/// A single mismatched artifact between two builds.
#[derive(Debug, Clone)]
pub struct MismatchEntry {
    /// The artifact path.
    pub path: String,
    /// Checksum in the first build.
    pub checksum_a: Checksum,
    /// Checksum in the second build.
    pub checksum_b: Checksum,
}

impl ManifestDiff {
    /// Compares two manifests and returns the diff.
    pub fn compare(a: &BuildManifest, b: &BuildManifest) -> Self {
        let mut matching = Vec::new();
        let mut mismatched = Vec::new();
        let mut only_in_first = Vec::new();
        let mut only_in_second = Vec::new();

        // Check all artifacts in A.
        for (path, entry_a) in &a.artifacts {
            match b.artifacts.get(path) {
                Some(entry_b) => {
                    if entry_a.checksum == entry_b.checksum {
                        matching.push(path.clone());
                    } else {
                        mismatched.push(MismatchEntry {
                            path: path.clone(),
                            checksum_a: entry_a.checksum.clone(),
                            checksum_b: entry_b.checksum.clone(),
                        });
                    }
                }
                None => only_in_first.push(path.clone()),
            }
        }

        // Find artifacts only in B.
        for path in b.artifacts.keys() {
            if !a.artifacts.contains_key(path) {
                only_in_second.push(path.clone());
            }
        }

        Self {
            matching,
            mismatched,
            only_in_first,
            only_in_second,
        }
    }

    /// Returns whether the two builds are identical.
    pub fn is_identical(&self) -> bool {
        self.mismatched.is_empty()
            && self.only_in_first.is_empty()
            && self.only_in_second.is_empty()
    }

    /// Returns a human-readable summary.
    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "Matching: {}, Mismatched: {}, Only in A: {}, Only in B: {}\n",
            self.matching.len(),
            self.mismatched.len(),
            self.only_in_first.len(),
            self.only_in_second.len()
        ));
        if self.is_identical() {
            s.push_str("Result: REPRODUCIBLE ✓\n");
        } else {
            s.push_str("Result: NOT REPRODUCIBLE ✗\n");
            for m in &self.mismatched {
                s.push_str(&format!(
                    "  MISMATCH: {} ({} vs {})\n",
                    m.path,
                    m.checksum_a.short(),
                    m.checksum_b.short()
                ));
            }
            for p in &self.only_in_first {
                s.push_str(&format!("  REMOVED: {}\n", p));
            }
            for p in &self.only_in_second {
                s.push_str(&format!("  ADDED: {}\n", p));
            }
        }
        s
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("create tempdir")
    }

    // ---- Checksum ----

    #[test]
    fn checksum_from_bytes_deterministic() {
        let data = b"hello world";
        let c1 = Checksum::from_bytes(data);
        let c2 = Checksum::from_bytes(data);
        assert_eq!(c1, c2);
    }

    #[test]
    fn checksum_different_data_different_hash() {
        let c1 = Checksum::from_bytes(b"hello");
        let c2 = Checksum::from_bytes(b"world");
        assert_ne!(c1, c2);
    }

    #[test]
    fn checksum_from_file() {
        let dir = make_temp_dir();
        let path = dir.path().join("test.txt");
        fs::write(&path, "test content").unwrap();

        let c1 = Checksum::from_file(&path).unwrap();
        let c2 = Checksum::from_file(&path).unwrap();
        assert_eq!(c1, c2);
        assert!(!c1.hex().is_empty());
    }

    #[test]
    fn checksum_short_hash() {
        let c = Checksum::from_bytes(b"test");
        assert_eq!(c.short().len(), 8);
    }

    #[test]
    fn checksum_display() {
        let c = Checksum::from_bytes(b"test");
        let displayed = format!("{}", c);
        assert_eq!(displayed, c.hex());
    }

    // ---- ArtifactEntry ----

    #[test]
    fn artifact_entry_from_file() {
        let dir = make_temp_dir();
        let path = dir.path().join("artifact.bin");
        fs::write(&path, "binary content").unwrap();

        let entry = ArtifactEntry::from_file(dir.path(), &path).unwrap();
        assert_eq!(entry.path, "artifact.bin");
        assert_eq!(entry.size, 14); // "binary content" is 14 bytes
        assert!(!entry.checksum.hex().is_empty());
    }

    #[test]
    fn artifact_entry_display() {
        let dir = make_temp_dir();
        let path = dir.path().join("test.bin");
        fs::write(&path, "data").unwrap();

        let entry = ArtifactEntry::from_file(dir.path(), &path).unwrap();
        let display = format!("{}", entry);
        assert!(display.contains("test.bin"));
        assert!(display.contains("4 bytes"));
    }

    // ---- BuildEnvironment ----

    #[test]
    fn capture_build_environment() {
        let env = BuildEnvironment::capture("release");
        assert!(!env.rustc_version.is_empty());
        assert!(!env.cargo_version.is_empty());
        assert!(!env.target_triple.is_empty());
        assert_eq!(env.profile, "release");
    }

    #[test]
    fn build_environment_with_env_vars() {
        std::env::set_var("TEST_REPRO_VAR", "hello");
        let env = BuildEnvironment::capture("debug")
            .with_env_vars(&["TEST_REPRO_VAR", "NONEXISTENT_VAR"]);
        assert_eq!(env.env_vars.get("TEST_REPRO_VAR"), Some(&"hello".to_string()));
        assert!(!env.env_vars.contains_key("NONEXISTENT_VAR"));
        std::env::remove_var("TEST_REPRO_VAR");
    }

    #[test]
    fn build_environment_display() {
        let env = BuildEnvironment::capture("release");
        let display = format!("{}", env);
        assert!(display.contains("rustc:"));
        assert!(display.contains("profile: release"));
    }

    // ---- BuildManifest ----

    #[test]
    fn empty_manifest() {
        let env = BuildEnvironment::capture("debug");
        let manifest = BuildManifest::new("0.1.0", env);
        assert_eq!(manifest.artifact_count(), 0);
        assert_eq!(manifest.total_size(), 0);
    }

    #[test]
    fn add_artifact_to_manifest() {
        let env = BuildEnvironment::capture("debug");
        let mut manifest = BuildManifest::new("0.1.0", env);

        manifest.add_artifact(ArtifactEntry {
            path: "lib.so".to_string(),
            checksum: Checksum::from_bytes(b"library"),
            size: 1024,
        });

        assert_eq!(manifest.artifact_count(), 1);
        assert_eq!(manifest.total_size(), 1024);
    }

    #[test]
    fn scan_directory_adds_all_files() {
        let dir = make_temp_dir();
        fs::write(dir.path().join("a.txt"), "aaa").unwrap();
        fs::write(dir.path().join("b.txt"), "bbb").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/c.txt"), "ccc").unwrap();

        let env = BuildEnvironment::capture("debug");
        let mut manifest = BuildManifest::new("0.1.0", env);
        let count = manifest.scan_directory(dir.path()).unwrap();

        assert_eq!(count, 3);
        assert_eq!(manifest.artifact_count(), 3);
        assert!(manifest.artifacts.contains_key("a.txt"));
        assert!(manifest.artifacts.contains_key("b.txt"));
        assert!(manifest.artifacts.contains_key("sub/c.txt"));
    }

    #[test]
    fn checksums_file_roundtrip() {
        let env = BuildEnvironment::capture("debug");
        let mut manifest = BuildManifest::new("0.1.0", env);

        manifest.add_artifact(ArtifactEntry {
            path: "bin/app".to_string(),
            checksum: Checksum("abc123def456".to_string()),
            size: 2048,
        });
        manifest.add_artifact(ArtifactEntry {
            path: "lib/core.so".to_string(),
            checksum: Checksum("789xyz".to_string()),
            size: 4096,
        });

        let output = manifest.to_checksums_file();
        assert!(output.contains("abc123def456  bin/app  2048"));
        assert!(output.contains("789xyz  lib/core.so  4096"));

        // Parse back.
        let entries = BuildManifest::parse_checksums_file(&output);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, "bin/app");
        assert_eq!(entries[0].checksum.hex(), "abc123def456");
        assert_eq!(entries[0].size, 2048);
    }

    // ---- ManifestDiff ----

    #[test]
    fn identical_manifests() {
        let env = BuildEnvironment::capture("release");
        let mut a = BuildManifest::new("0.1.0", env.clone());
        let mut b = BuildManifest::new("0.1.0", env);

        let checksum = Checksum::from_bytes(b"same content");
        a.add_artifact(ArtifactEntry {
            path: "file.bin".to_string(),
            checksum: checksum.clone(),
            size: 100,
        });
        b.add_artifact(ArtifactEntry {
            path: "file.bin".to_string(),
            checksum,
            size: 100,
        });

        let diff = ManifestDiff::compare(&a, &b);
        assert!(diff.is_identical());
        assert_eq!(diff.matching.len(), 1);
        assert!(diff.mismatched.is_empty());
    }

    #[test]
    fn mismatched_checksums() {
        let env = BuildEnvironment::capture("release");
        let mut a = BuildManifest::new("0.1.0", env.clone());
        let mut b = BuildManifest::new("0.1.0", env);

        a.add_artifact(ArtifactEntry {
            path: "file.bin".to_string(),
            checksum: Checksum::from_bytes(b"version A"),
            size: 100,
        });
        b.add_artifact(ArtifactEntry {
            path: "file.bin".to_string(),
            checksum: Checksum::from_bytes(b"version B"),
            size: 100,
        });

        let diff = ManifestDiff::compare(&a, &b);
        assert!(!diff.is_identical());
        assert_eq!(diff.mismatched.len(), 1);
        assert_eq!(diff.mismatched[0].path, "file.bin");
    }

    #[test]
    fn added_and_removed_artifacts() {
        let env = BuildEnvironment::capture("release");
        let mut a = BuildManifest::new("0.1.0", env.clone());
        let mut b = BuildManifest::new("0.1.0", env);

        a.add_artifact(ArtifactEntry {
            path: "old.bin".to_string(),
            checksum: Checksum::from_bytes(b"old"),
            size: 50,
        });
        b.add_artifact(ArtifactEntry {
            path: "new.bin".to_string(),
            checksum: Checksum::from_bytes(b"new"),
            size: 50,
        });

        let diff = ManifestDiff::compare(&a, &b);
        assert!(!diff.is_identical());
        assert_eq!(diff.only_in_first, vec!["old.bin"]);
        assert_eq!(diff.only_in_second, vec!["new.bin"]);
    }

    #[test]
    fn diff_summary_reproducible() {
        let env = BuildEnvironment::capture("release");
        let a = BuildManifest::new("0.1.0", env.clone());
        let b = BuildManifest::new("0.1.0", env);

        let diff = ManifestDiff::compare(&a, &b);
        let summary = diff.summary();
        assert!(summary.contains("REPRODUCIBLE"));
    }

    #[test]
    fn diff_summary_not_reproducible() {
        let env = BuildEnvironment::capture("release");
        let mut a = BuildManifest::new("0.1.0", env.clone());
        let mut b = BuildManifest::new("0.1.0", env);

        a.add_artifact(ArtifactEntry {
            path: "app".to_string(),
            checksum: Checksum::from_bytes(b"v1"),
            size: 100,
        });
        b.add_artifact(ArtifactEntry {
            path: "app".to_string(),
            checksum: Checksum::from_bytes(b"v2"),
            size: 100,
        });

        let diff = ManifestDiff::compare(&a, &b);
        let summary = diff.summary();
        assert!(summary.contains("NOT REPRODUCIBLE"));
        assert!(summary.contains("MISMATCH"));
    }

    // ---- Integration: full workflow ----

    #[test]
    fn full_scan_and_verify_workflow() {
        let dir = make_temp_dir();
        fs::write(dir.path().join("engine.bin"), "engine binary content").unwrap();
        fs::write(dir.path().join("data.pck"), "packed resources").unwrap();
        fs::create_dir(dir.path().join("lib")).unwrap();
        fs::write(dir.path().join("lib/core.so"), "shared library").unwrap();

        let env = BuildEnvironment::capture("release");

        // First build scan.
        let mut manifest_a = BuildManifest::new("0.1.0", env.clone());
        manifest_a.scan_directory(dir.path()).unwrap();

        // Second scan of same directory (should match).
        let mut manifest_b = BuildManifest::new("0.1.0", env);
        manifest_b.scan_directory(dir.path()).unwrap();

        let diff = ManifestDiff::compare(&manifest_a, &manifest_b);
        assert!(diff.is_identical(), "same directory should produce identical manifests");
        assert_eq!(diff.matching.len(), 3);
    }

    #[test]
    fn detect_modification_after_scan() {
        let dir = make_temp_dir();
        fs::write(dir.path().join("app.bin"), "original").unwrap();

        let env = BuildEnvironment::capture("release");

        // First scan.
        let mut manifest_a = BuildManifest::new("0.1.0", env.clone());
        manifest_a.scan_directory(dir.path()).unwrap();

        // Modify the file.
        fs::write(dir.path().join("app.bin"), "modified!").unwrap();

        // Second scan.
        let mut manifest_b = BuildManifest::new("0.1.0", env);
        manifest_b.scan_directory(dir.path()).unwrap();

        let diff = ManifestDiff::compare(&manifest_a, &manifest_b);
        assert!(!diff.is_identical());
        assert_eq!(diff.mismatched.len(), 1);
        assert_eq!(diff.mismatched[0].path, "app.bin");
    }

    #[test]
    fn manifest_version_and_timestamp() {
        let env = BuildEnvironment::capture("debug");
        let manifest = BuildManifest::new("1.2.3", env);
        assert_eq!(manifest.version, "1.2.3");
        assert!(!manifest.created_at.is_empty());
        assert!(manifest.created_at.contains('T'));
    }

    #[test]
    fn empty_manifests_are_identical() {
        let env = BuildEnvironment::capture("release");
        let a = BuildManifest::new("0.1.0", env.clone());
        let b = BuildManifest::new("0.1.0", env);

        let diff = ManifestDiff::compare(&a, &b);
        assert!(diff.is_identical());
        assert!(diff.matching.is_empty());
    }

    #[test]
    fn fallback_hash_deterministic() {
        let h1 = fallback_hash(b"test data");
        let h2 = fallback_hash(b"test data");
        assert_eq!(h1, h2);

        let h3 = fallback_hash(b"different data");
        assert_ne!(h1, h3);
    }
}
