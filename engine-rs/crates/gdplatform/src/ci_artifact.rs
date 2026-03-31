//! CI artifact generation with release binary packaging.
//!
//! Models the pipeline for generating distributable artifacts from CI
//! builds. Combines [`ExportConfig`](crate::export::ExportConfig) with
//! platform targets to produce packaged release binaries.
//!
//! # Workflow
//!
//! 1. Create a [`CiArtifactPlan`] from an export config and target list.
//! 2. Call [`generate_artifacts`] to produce [`CiArtifact`] entries.
//! 3. Each artifact describes the output file, checksum, and metadata
//!    needed for CI upload (e.g. GitHub Actions `actions/upload-artifact`).

use crate::export::{BuildProfile, ExportConfig, ExportTemplate};
use crate::platform_targets::{Architecture, DesktopTarget, DESKTOP_TARGETS};

// ---------------------------------------------------------------------------
// CiArtifact
// ---------------------------------------------------------------------------

/// A single CI build artifact ready for upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CiArtifact {
    /// Artifact display name (e.g. "patina-linux-x86_64-release").
    pub name: String,
    /// The output filename (e.g. "patina.linux.release.x86_64").
    pub filename: String,
    /// Rust target triple used for the build.
    pub rust_triple: String,
    /// Build profile.
    pub profile: BuildProfile,
    /// Platform name.
    pub platform: String,
    /// Architecture name.
    pub arch: String,
    /// Whether this artifact should be uploaded to the release.
    pub upload: bool,
    /// SHA-256 checksum placeholder (filled after build).
    pub sha256: Option<String>,
    /// File size in bytes (filled after build).
    pub size_bytes: Option<u64>,
}

impl CiArtifact {
    /// Returns the artifact name in the standard naming convention.
    pub fn standard_name(
        app_name: &str,
        platform: &str,
        arch: &str,
        profile: BuildProfile,
    ) -> String {
        let profile_str = match profile {
            BuildProfile::Debug => "debug",
            BuildProfile::Release => "release",
            BuildProfile::ReleaseDebug => "release-debug",
        };
        format!("{app_name}-{platform}-{arch}-{profile_str}")
    }

    /// Returns the GitHub Actions compatible artifact name (no special chars).
    pub fn actions_safe_name(&self) -> String {
        self.name
            .replace('/', "-")
            .replace(' ', "-")
            .replace('(', "")
            .replace(')', "")
    }

    /// Sets the checksum and size after build completion.
    pub fn set_build_result(&mut self, sha256: String, size_bytes: u64) {
        self.sha256 = Some(sha256);
        self.size_bytes = Some(size_bytes);
    }

    /// Returns `true` if the build result has been recorded.
    pub fn is_built(&self) -> bool {
        self.sha256.is_some() && self.size_bytes.is_some()
    }
}

// ---------------------------------------------------------------------------
// CiArtifactPlan
// ---------------------------------------------------------------------------

/// A plan for generating CI artifacts across multiple platforms.
#[derive(Debug, Clone)]
pub struct CiArtifactPlan {
    /// Application name.
    pub app_name: String,
    /// Build profiles to generate.
    pub profiles: Vec<BuildProfile>,
    /// Target triples to build for.
    pub targets: Vec<String>,
    /// Whether to generate checksums.
    pub generate_checksums: bool,
    /// Whether to create a combined archive.
    pub create_archive: bool,
    /// Archive format (e.g. "tar.gz", "zip").
    pub archive_format: String,
}

impl CiArtifactPlan {
    /// Creates a new plan for the given app name with release profile only.
    pub fn new(app_name: impl Into<String>) -> Self {
        Self {
            app_name: app_name.into(),
            profiles: vec![BuildProfile::Release],
            targets: Vec::new(),
            generate_checksums: true,
            create_archive: true,
            archive_format: "tar.gz".to_string(),
        }
    }

    /// Adds a build profile to the plan.
    pub fn with_profile(mut self, profile: BuildProfile) -> Self {
        if !self.profiles.contains(&profile) {
            self.profiles.push(profile);
        }
        self
    }

    /// Adds both debug and release profiles.
    pub fn with_debug_and_release(mut self) -> Self {
        self.profiles = vec![BuildProfile::Debug, BuildProfile::Release];
        self
    }

    /// Adds a specific target triple.
    pub fn with_target(mut self, triple: impl Into<String>) -> Self {
        self.targets.push(triple.into());
        self
    }

    /// Adds all CI-tested desktop targets.
    pub fn with_ci_targets(mut self) -> Self {
        for target in DESKTOP_TARGETS {
            if target.ci_tested {
                self.targets.push(target.rust_triple.to_string());
            }
        }
        self
    }

    /// Adds all desktop targets (including non-CI).
    pub fn with_all_targets(mut self) -> Self {
        for target in DESKTOP_TARGETS {
            self.targets.push(target.rust_triple.to_string());
        }
        self
    }

    /// Sets the archive format.
    pub fn with_archive_format(mut self, format: impl Into<String>) -> Self {
        self.archive_format = format.into();
        self
    }

    /// Disables checksum generation.
    pub fn without_checksums(mut self) -> Self {
        self.generate_checksums = false;
        self
    }

    /// Generates the list of artifacts from the plan.
    pub fn generate_artifacts(&self) -> Vec<CiArtifact> {
        let mut artifacts = Vec::new();

        for triple in &self.targets {
            let target = DESKTOP_TARGETS.iter().find(|t| t.rust_triple == *triple);
            let (platform, arch) = match target {
                Some(t) => (
                    format!("{:?}", t.platform).to_lowercase(),
                    t.arch.triple_component().to_string(),
                ),
                None => ("unknown".to_string(), triple.clone()),
            };

            for profile in &self.profiles {
                let name = CiArtifact::standard_name(&self.app_name, &platform, &arch, *profile);
                let export_cfg =
                    ExportConfig::new(&platform, &self.app_name).with_build_profile(*profile);
                let template = ExportTemplate::from_config(export_cfg);
                let filename = template.output_filename();

                artifacts.push(CiArtifact {
                    name,
                    filename,
                    rust_triple: triple.clone(),
                    profile: *profile,
                    platform: platform.clone(),
                    arch: arch.clone(),
                    upload: *profile == BuildProfile::Release,
                    sha256: None,
                    size_bytes: None,
                });

                // Break to avoid rebinding `platform` and `arch`.
                // They're consumed by the push above.
                // Actually, we need to re-derive them for the next profile.
            }
        }

        artifacts
    }

    /// Returns the number of artifacts that would be generated.
    pub fn artifact_count(&self) -> usize {
        self.targets.len() * self.profiles.len()
    }

    /// Generates a CI matrix configuration as a list of (triple, profile) pairs.
    pub fn matrix_entries(&self) -> Vec<(String, BuildProfile)> {
        let mut entries = Vec::new();
        for triple in &self.targets {
            for profile in &self.profiles {
                entries.push((triple.clone(), *profile));
            }
        }
        entries
    }

    /// Generates a GitHub Actions matrix JSON string.
    pub fn github_actions_matrix(&self) -> String {
        let mut includes = Vec::new();
        for triple in &self.targets {
            let target = DESKTOP_TARGETS.iter().find(|t| t.rust_triple == *triple);
            let os = match target {
                Some(t) => match t.platform {
                    crate::os::Platform::Linux => "ubuntu-latest",
                    crate::os::Platform::MacOS => "macos-latest",
                    crate::os::Platform::Windows => "windows-latest",
                    _ => "ubuntu-latest",
                },
                None => "ubuntu-latest",
            };

            for profile in &self.profiles {
                let profile_str = match profile {
                    BuildProfile::Debug => "debug",
                    BuildProfile::Release => "release",
                    BuildProfile::ReleaseDebug => "release-debug",
                };
                includes.push(format!(
                    r#"{{"os": "{os}", "target": "{triple}", "profile": "{profile_str}"}}"#,
                ));
            }
        }
        format!("{{\"include\": [{}]}}", includes.join(", "))
    }
}

/// Finds the [`DesktopTarget`] for a given Rust triple.
pub fn find_target(triple: &str) -> Option<&'static DesktopTarget> {
    DESKTOP_TARGETS.iter().find(|t| t.rust_triple == triple)
}

/// Returns the recommended archive extension for a platform.
pub fn archive_extension(platform: &str) -> &'static str {
    match platform {
        "windows" => "zip",
        _ => "tar.gz",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_new_defaults() {
        let plan = CiArtifactPlan::new("patina");
        assert_eq!(plan.app_name, "patina");
        assert_eq!(plan.profiles, vec![BuildProfile::Release]);
        assert!(plan.targets.is_empty());
        assert!(plan.generate_checksums);
        assert!(plan.create_archive);
        assert_eq!(plan.archive_format, "tar.gz");
    }

    #[test]
    fn plan_with_ci_targets() {
        let plan = CiArtifactPlan::new("patina").with_ci_targets();
        assert!(!plan.targets.is_empty());
        for triple in &plan.targets {
            let target = find_target(triple);
            assert!(
                target.is_some(),
                "CI target {triple} must be in DESKTOP_TARGETS"
            );
            assert!(target.unwrap().ci_tested, "{triple} must be CI-tested");
        }
    }

    #[test]
    fn plan_with_all_targets() {
        let plan = CiArtifactPlan::new("patina").with_all_targets();
        assert_eq!(plan.targets.len(), DESKTOP_TARGETS.len());
    }

    #[test]
    fn plan_with_debug_and_release() {
        let plan = CiArtifactPlan::new("app").with_debug_and_release();
        assert_eq!(plan.profiles.len(), 2);
        assert!(plan.profiles.contains(&BuildProfile::Debug));
        assert!(plan.profiles.contains(&BuildProfile::Release));
    }

    #[test]
    fn plan_artifact_count() {
        let plan = CiArtifactPlan::new("app")
            .with_debug_and_release()
            .with_target("x86_64-unknown-linux-gnu")
            .with_target("x86_64-apple-darwin");
        assert_eq!(plan.artifact_count(), 4); // 2 targets × 2 profiles
    }

    #[test]
    fn generate_artifacts_empty_plan() {
        let plan = CiArtifactPlan::new("app");
        let artifacts = plan.generate_artifacts();
        assert!(artifacts.is_empty());
    }

    #[test]
    fn generate_artifacts_single_target() {
        let plan = CiArtifactPlan::new("patina").with_target("x86_64-unknown-linux-gnu");
        let artifacts = plan.generate_artifacts();
        assert_eq!(artifacts.len(), 1);
        let a = &artifacts[0];
        assert_eq!(a.name, "patina-linux-x86_64-release");
        assert_eq!(a.rust_triple, "x86_64-unknown-linux-gnu");
        assert_eq!(a.profile, BuildProfile::Release);
        assert!(a.upload);
        assert!(!a.is_built());
    }

    #[test]
    fn generate_artifacts_debug_not_uploaded() {
        let plan = CiArtifactPlan::new("app")
            .with_debug_and_release()
            .with_target("x86_64-unknown-linux-gnu");
        let artifacts = plan.generate_artifacts();
        let debug = artifacts
            .iter()
            .find(|a| a.profile == BuildProfile::Debug)
            .unwrap();
        let release = artifacts
            .iter()
            .find(|a| a.profile == BuildProfile::Release)
            .unwrap();
        assert!(!debug.upload);
        assert!(release.upload);
    }

    #[test]
    fn artifact_standard_name() {
        let name = CiArtifact::standard_name("patina", "linux", "x86_64", BuildProfile::Release);
        assert_eq!(name, "patina-linux-x86_64-release");
    }

    #[test]
    fn artifact_standard_name_debug() {
        let name = CiArtifact::standard_name("app", "macos", "aarch64", BuildProfile::Debug);
        assert_eq!(name, "app-macos-aarch64-debug");
    }

    #[test]
    fn artifact_actions_safe_name() {
        let a = CiArtifact {
            name: "app/test (special)".to_string(),
            filename: String::new(),
            rust_triple: String::new(),
            profile: BuildProfile::Release,
            platform: String::new(),
            arch: String::new(),
            upload: true,
            sha256: None,
            size_bytes: None,
        };
        assert_eq!(a.actions_safe_name(), "app-test-special");
    }

    #[test]
    fn artifact_set_build_result() {
        let mut a = CiArtifact {
            name: "test".into(),
            filename: "test.bin".into(),
            rust_triple: "x86_64-unknown-linux-gnu".into(),
            profile: BuildProfile::Release,
            platform: "linux".into(),
            arch: "x86_64".into(),
            upload: true,
            sha256: None,
            size_bytes: None,
        };
        assert!(!a.is_built());
        a.set_build_result("abc123".into(), 1024);
        assert!(a.is_built());
        assert_eq!(a.sha256.as_deref(), Some("abc123"));
        assert_eq!(a.size_bytes, Some(1024));
    }

    #[test]
    fn matrix_entries_count() {
        let plan = CiArtifactPlan::new("app")
            .with_debug_and_release()
            .with_target("x86_64-unknown-linux-gnu")
            .with_target("aarch64-apple-darwin");
        let entries = plan.matrix_entries();
        assert_eq!(entries.len(), 4);
    }

    #[test]
    fn github_actions_matrix_format() {
        let plan = CiArtifactPlan::new("app").with_target("x86_64-unknown-linux-gnu");
        let matrix = plan.github_actions_matrix();
        assert!(matrix.contains("\"include\""));
        assert!(matrix.contains("ubuntu-latest"));
        assert!(matrix.contains("x86_64-unknown-linux-gnu"));
        assert!(matrix.contains("release"));
    }

    #[test]
    fn github_actions_matrix_windows() {
        let plan = CiArtifactPlan::new("app").with_target("x86_64-pc-windows-msvc");
        let matrix = plan.github_actions_matrix();
        assert!(matrix.contains("windows-latest"));
    }

    #[test]
    fn github_actions_matrix_macos() {
        let plan = CiArtifactPlan::new("app").with_target("aarch64-apple-darwin");
        let matrix = plan.github_actions_matrix();
        assert!(matrix.contains("macos-latest"));
    }

    #[test]
    fn find_target_known() {
        let t = find_target("x86_64-unknown-linux-gnu");
        assert!(t.is_some());
        assert_eq!(t.unwrap().arch, Architecture::X86_64);
    }

    #[test]
    fn find_target_unknown() {
        assert!(find_target("riscv64-unknown-linux-gnu").is_none());
    }

    #[test]
    fn archive_extension_by_platform() {
        assert_eq!(archive_extension("windows"), "zip");
        assert_eq!(archive_extension("linux"), "tar.gz");
        assert_eq!(archive_extension("macos"), "tar.gz");
    }

    #[test]
    fn plan_without_checksums() {
        let plan = CiArtifactPlan::new("app").without_checksums();
        assert!(!plan.generate_checksums);
    }

    #[test]
    fn plan_with_archive_format() {
        let plan = CiArtifactPlan::new("app").with_archive_format("zip");
        assert_eq!(plan.archive_format, "zip");
    }

    #[test]
    fn plan_duplicate_profile_not_added() {
        let plan = CiArtifactPlan::new("app")
            .with_profile(BuildProfile::Release)
            .with_profile(BuildProfile::Release);
        assert_eq!(plan.profiles.len(), 1);
    }

    #[test]
    fn full_ci_pipeline_workflow() {
        let plan = CiArtifactPlan::new("patina")
            .with_debug_and_release()
            .with_ci_targets();

        let artifacts = plan.generate_artifacts();
        assert!(!artifacts.is_empty());

        // All CI targets × 2 profiles
        let ci_count = DESKTOP_TARGETS.iter().filter(|t| t.ci_tested).count();
        assert_eq!(artifacts.len(), ci_count * 2);

        // Release artifacts should be uploaded, debug should not
        for a in &artifacts {
            if a.profile == BuildProfile::Release {
                assert!(a.upload);
            } else {
                assert!(!a.upload);
            }
        }

        // Matrix should cover all entries
        let matrix = plan.github_actions_matrix();
        assert!(matrix.contains("include"));
    }
}
