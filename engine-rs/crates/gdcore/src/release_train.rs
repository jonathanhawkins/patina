//! Release train with semantic versioning and changelog generation.
//!
//! Provides a [`ReleaseTrain`] that manages a sequence of releases following
//! semantic versioning (semver). Supports:
//!
//! - **Version parsing and bumping** (major, minor, patch, pre-release)
//! - **Changelog generation** from structured commit/change entries
//! - **Release lifecycle** (draft → staged → published)
//! - **Release notes** with categorized changes

use std::collections::BTreeMap;
use std::fmt;

// ---------------------------------------------------------------------------
// Semantic Version
// ---------------------------------------------------------------------------

/// A semantic version following the SemVer 2.0.0 specification.
///
/// Format: `MAJOR.MINOR.PATCH[-PRERELEASE][+BUILD]`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SemVer {
    /// Major version — incremented for incompatible API changes.
    pub major: u32,
    /// Minor version — incremented for backwards-compatible features.
    pub minor: u32,
    /// Patch version — incremented for backwards-compatible bug fixes.
    pub patch: u32,
    /// Pre-release label (e.g. "alpha.1", "rc.2").
    pub pre: Option<String>,
    /// Build metadata (e.g. "build.123"). Does not affect version precedence.
    pub build: Option<String>,
}

impl SemVer {
    /// Creates a new version with the given major, minor, patch components.
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
            pre: None,
            build: None,
        }
    }

    /// Creates a version with a pre-release label.
    pub fn with_pre(mut self, pre: &str) -> Self {
        self.pre = Some(pre.to_string());
        self
    }

    /// Creates a version with build metadata.
    pub fn with_build(mut self, build: &str) -> Self {
        self.build = Some(build.to_string());
        self
    }

    /// Parses a version string like "1.2.3", "1.0.0-alpha.1", or "2.0.0+build.42".
    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim().trim_start_matches('v');

        // Split off build metadata first.
        let (version_pre, build) = match s.split_once('+') {
            Some((vp, b)) => (vp, Some(b.to_string())),
            None => (s, None),
        };

        // Split off pre-release.
        let (version, pre) = match version_pre.split_once('-') {
            Some((v, p)) => (v, Some(p.to_string())),
            None => (version_pre, None),
        };

        let parts: Vec<&str> = version.split('.').collect();
        if parts.len() != 3 {
            return Err(format!("expected MAJOR.MINOR.PATCH, got '{}'", version));
        }

        let major = parts[0]
            .parse::<u32>()
            .map_err(|_| format!("invalid major version: '{}'", parts[0]))?;
        let minor = parts[1]
            .parse::<u32>()
            .map_err(|_| format!("invalid minor version: '{}'", parts[1]))?;
        let patch = parts[2]
            .parse::<u32>()
            .map_err(|_| format!("invalid patch version: '{}'", parts[2]))?;

        Ok(Self {
            major,
            minor,
            patch,
            pre,
            build,
        })
    }

    /// Returns a new version with the major version bumped (minor and patch reset).
    pub fn bump_major(&self) -> Self {
        Self::new(self.major + 1, 0, 0)
    }

    /// Returns a new version with the minor version bumped (patch reset).
    pub fn bump_minor(&self) -> Self {
        Self::new(self.major, self.minor + 1, 0)
    }

    /// Returns a new version with the patch version bumped.
    pub fn bump_patch(&self) -> Self {
        Self::new(self.major, self.minor, self.patch + 1)
    }

    /// Returns whether this is a pre-release version.
    pub fn is_prerelease(&self) -> bool {
        self.pre.is_some()
    }

    /// Returns whether this is a stable release (1.0.0+, no pre-release).
    pub fn is_stable(&self) -> bool {
        self.major >= 1 && self.pre.is_none()
    }

    /// Compares two versions for precedence (ignoring build metadata per semver spec).
    pub fn precedence_cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;

        match self.major.cmp(&other.major) {
            Ordering::Equal => {}
            ord => return ord,
        }
        match self.minor.cmp(&other.minor) {
            Ordering::Equal => {}
            ord => return ord,
        }
        match self.patch.cmp(&other.patch) {
            Ordering::Equal => {}
            ord => return ord,
        }

        // Pre-release versions have lower precedence than release.
        match (&self.pre, &other.pre) {
            (None, None) => Ordering::Equal,
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (Some(a), Some(b)) => a.cmp(b),
        }
    }
}

impl fmt::Display for SemVer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(ref pre) = self.pre {
            write!(f, "-{}", pre)?;
        }
        if let Some(ref build) = self.build {
            write!(f, "+{}", build)?;
        }
        Ok(())
    }
}

impl PartialOrd for SemVer {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.precedence_cmp(other))
    }
}

impl Ord for SemVer {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.precedence_cmp(other)
    }
}

// ---------------------------------------------------------------------------
// Change entries and categories
// ---------------------------------------------------------------------------

/// The category of a change entry for changelog generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChangeCategory {
    /// Breaking changes (API incompatibilities).
    Breaking,
    /// New features added.
    Added,
    /// Changes to existing functionality.
    Changed,
    /// Deprecated features.
    Deprecated,
    /// Removed features.
    Removed,
    /// Bug fixes.
    Fixed,
    /// Security-related changes.
    Security,
    /// Performance improvements.
    Performance,
    /// Internal/maintenance changes (refactoring, CI, tests).
    Internal,
}

impl ChangeCategory {
    /// Returns the Keep a Changelog section header.
    pub fn heading(&self) -> &'static str {
        match self {
            ChangeCategory::Breaking => "Breaking Changes",
            ChangeCategory::Added => "Added",
            ChangeCategory::Changed => "Changed",
            ChangeCategory::Deprecated => "Deprecated",
            ChangeCategory::Removed => "Removed",
            ChangeCategory::Fixed => "Fixed",
            ChangeCategory::Security => "Security",
            ChangeCategory::Performance => "Performance",
            ChangeCategory::Internal => "Internal",
        }
    }
}

impl fmt::Display for ChangeCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.heading())
    }
}

/// A single change entry for the changelog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeEntry {
    /// The category of change.
    pub category: ChangeCategory,
    /// Short description of the change.
    pub description: String,
    /// Optional reference (issue ID, PR number, commit hash).
    pub reference: Option<String>,
    /// The author of the change.
    pub author: Option<String>,
}

impl ChangeEntry {
    /// Creates a new change entry.
    pub fn new(category: ChangeCategory, description: &str) -> Self {
        Self {
            category,
            description: description.to_string(),
            reference: None,
            author: None,
        }
    }

    /// Adds a reference to the entry.
    pub fn with_reference(mut self, reference: &str) -> Self {
        self.reference = Some(reference.to_string());
        self
    }

    /// Adds an author to the entry.
    pub fn with_author(mut self, author: &str) -> Self {
        self.author = Some(author.to_string());
        self
    }

    /// Formats this entry as a markdown bullet point.
    pub fn to_markdown(&self) -> String {
        let mut line = format!("- {}", self.description);
        if let Some(ref r) = self.reference {
            line.push_str(&format!(" ({})", r));
        }
        if let Some(ref a) = self.author {
            line.push_str(&format!(" — @{}", a));
        }
        line
    }
}

// ---------------------------------------------------------------------------
// Release
// ---------------------------------------------------------------------------

/// The lifecycle state of a release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseState {
    /// Release is being prepared; changes can still be added.
    Draft,
    /// Release is staged for publishing (frozen, awaiting approval).
    Staged,
    /// Release has been published.
    Published,
    /// Release was abandoned/cancelled.
    Cancelled,
}

impl fmt::Display for ReleaseState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReleaseState::Draft => write!(f, "Draft"),
            ReleaseState::Staged => write!(f, "Staged"),
            ReleaseState::Published => write!(f, "Published"),
            ReleaseState::Cancelled => write!(f, "Cancelled"),
        }
    }
}

/// A single release in the release train.
#[derive(Debug, Clone)]
pub struct Release {
    /// The version of this release.
    pub version: SemVer,
    /// The current lifecycle state.
    pub state: ReleaseState,
    /// Release date (ISO 8601 format, e.g. "2026-03-25").
    pub date: Option<String>,
    /// Categorized change entries.
    pub changes: Vec<ChangeEntry>,
    /// Optional release summary/overview.
    pub summary: Option<String>,
}

impl Release {
    /// Creates a new draft release with the given version.
    pub fn new(version: SemVer) -> Self {
        Self {
            version,
            state: ReleaseState::Draft,
            date: None,
            changes: Vec::new(),
            summary: None,
        }
    }

    /// Adds a change entry to this release. Only allowed in Draft state.
    pub fn add_change(&mut self, entry: ChangeEntry) -> Result<(), String> {
        if self.state != ReleaseState::Draft {
            return Err(format!(
                "cannot add changes to a {} release",
                self.state
            ));
        }
        self.changes.push(entry);
        Ok(())
    }

    /// Sets the release summary.
    pub fn set_summary(&mut self, summary: &str) {
        self.summary = Some(summary.to_string());
    }

    /// Transitions the release to Staged state.
    pub fn stage(&mut self, date: &str) -> Result<(), String> {
        if self.state != ReleaseState::Draft {
            return Err(format!("can only stage a Draft release, current: {}", self.state));
        }
        self.state = ReleaseState::Staged;
        self.date = Some(date.to_string());
        Ok(())
    }

    /// Transitions the release to Published state.
    pub fn publish(&mut self) -> Result<(), String> {
        if self.state != ReleaseState::Staged {
            return Err(format!(
                "can only publish a Staged release, current: {}",
                self.state
            ));
        }
        self.state = ReleaseState::Published;
        Ok(())
    }

    /// Cancels the release.
    pub fn cancel(&mut self) -> Result<(), String> {
        if self.state == ReleaseState::Published {
            return Err("cannot cancel a published release".to_string());
        }
        self.state = ReleaseState::Cancelled;
        Ok(())
    }

    /// Returns changes grouped by category.
    pub fn changes_by_category(&self) -> BTreeMap<ChangeCategory, Vec<&ChangeEntry>> {
        let mut grouped: BTreeMap<ChangeCategory, Vec<&ChangeEntry>> = BTreeMap::new();
        for entry in &self.changes {
            grouped.entry(entry.category).or_default().push(entry);
        }
        grouped
    }

    /// Returns whether any changes are breaking.
    pub fn has_breaking_changes(&self) -> bool {
        self.changes
            .iter()
            .any(|c| c.category == ChangeCategory::Breaking)
    }

    /// Generates a Keep a Changelog formatted markdown section.
    pub fn to_changelog_markdown(&self) -> String {
        let mut md = String::new();

        // Header.
        let date_str = self.date.as_deref().unwrap_or("Unreleased");
        md.push_str(&format!("## [{}] - {}\n\n", self.version, date_str));

        // Summary.
        if let Some(ref summary) = self.summary {
            md.push_str(summary);
            md.push_str("\n\n");
        }

        // Categorized changes.
        let grouped = self.changes_by_category();
        for (category, entries) in &grouped {
            md.push_str(&format!("### {}\n\n", category.heading()));
            for entry in entries {
                md.push_str(&entry.to_markdown());
                md.push('\n');
            }
            md.push('\n');
        }

        md
    }
}

// ---------------------------------------------------------------------------
// Release Train
// ---------------------------------------------------------------------------

/// A release train managing a sequence of releases.
///
/// Tracks all releases (past and in-progress) and provides helpers
/// for determining the next version based on change categories.
#[derive(Debug)]
pub struct ReleaseTrain {
    /// All releases in chronological order (oldest first).
    releases: Vec<Release>,
}

impl ReleaseTrain {
    /// Creates an empty release train.
    pub fn new() -> Self {
        Self {
            releases: Vec::new(),
        }
    }

    /// Returns the current (latest) version, or `0.0.0` if no releases exist.
    pub fn current_version(&self) -> SemVer {
        self.releases
            .last()
            .map(|r| r.version.clone())
            .unwrap_or_else(|| SemVer::new(0, 0, 0))
    }

    /// Returns the latest published release.
    pub fn latest_published(&self) -> Option<&Release> {
        self.releases
            .iter()
            .rev()
            .find(|r| r.state == ReleaseState::Published)
    }

    /// Returns the current draft release, if any.
    pub fn current_draft(&self) -> Option<&Release> {
        self.releases
            .iter()
            .rev()
            .find(|r| r.state == ReleaseState::Draft)
    }

    /// Returns a mutable reference to the current draft release.
    pub fn current_draft_mut(&mut self) -> Option<&mut Release> {
        self.releases
            .iter_mut()
            .rev()
            .find(|r| r.state == ReleaseState::Draft)
    }

    /// Suggests the next version based on a list of pending changes.
    ///
    /// - Breaking changes → bump major (or minor if pre-1.0)
    /// - New features → bump minor
    /// - Bug fixes only → bump patch
    pub fn suggest_next_version(&self, changes: &[ChangeEntry]) -> SemVer {
        let current = self.current_version();

        let has_breaking = changes
            .iter()
            .any(|c| c.category == ChangeCategory::Breaking);
        let has_features = changes.iter().any(|c| {
            matches!(
                c.category,
                ChangeCategory::Added | ChangeCategory::Changed | ChangeCategory::Removed
            )
        });

        if has_breaking {
            if current.major == 0 {
                // Pre-1.0: breaking changes bump minor.
                current.bump_minor()
            } else {
                current.bump_major()
            }
        } else if has_features {
            current.bump_minor()
        } else {
            current.bump_patch()
        }
    }

    /// Creates a new draft release with the given version.
    pub fn create_release(&mut self, version: SemVer) -> Result<&mut Release, String> {
        // Check version is newer than current.
        let current = self.current_version();
        if version.precedence_cmp(&current) != std::cmp::Ordering::Greater {
            return Err(format!(
                "new version {} must be greater than current {}",
                version, current
            ));
        }

        // Check no existing draft.
        if self.current_draft().is_some() {
            return Err("a draft release already exists; stage or cancel it first".to_string());
        }

        self.releases.push(Release::new(version));
        Ok(self.releases.last_mut().unwrap())
    }

    /// Returns all releases.
    pub fn releases(&self) -> &[Release] {
        &self.releases
    }

    /// Returns the count of published releases.
    pub fn published_count(&self) -> usize {
        self.releases
            .iter()
            .filter(|r| r.state == ReleaseState::Published)
            .count()
    }

    /// Generates a full changelog in Keep a Changelog format.
    pub fn generate_changelog(&self) -> String {
        let mut md = String::new();
        md.push_str("# Changelog\n\n");
        md.push_str("All notable changes to this project will be documented in this file.\n\n");
        md.push_str(
            "The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),\n",
        );
        md.push_str(
            "and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).\n\n",
        );

        // Releases in reverse chronological order.
        for release in self.releases.iter().rev() {
            if release.state == ReleaseState::Cancelled {
                continue;
            }
            md.push_str(&release.to_changelog_markdown());
        }

        md
    }
}

impl Default for ReleaseTrain {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- SemVer parsing ----

    #[test]
    fn parse_simple_version() {
        let v = SemVer::parse("1.2.3").unwrap();
        assert_eq!(v, SemVer::new(1, 2, 3));
    }

    #[test]
    fn parse_version_with_v_prefix() {
        let v = SemVer::parse("v2.0.0").unwrap();
        assert_eq!(v, SemVer::new(2, 0, 0));
    }

    #[test]
    fn parse_version_with_prerelease() {
        let v = SemVer::parse("1.0.0-alpha.1").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.pre, Some("alpha.1".to_string()));
    }

    #[test]
    fn parse_version_with_build() {
        let v = SemVer::parse("1.0.0+build.42").unwrap();
        assert_eq!(v.build, Some("build.42".to_string()));
    }

    #[test]
    fn parse_version_with_pre_and_build() {
        let v = SemVer::parse("2.1.0-rc.1+sha.abc123").unwrap();
        assert_eq!(v, SemVer::new(2, 1, 0).with_pre("rc.1").with_build("sha.abc123"));
    }

    #[test]
    fn parse_invalid_version_too_few_parts() {
        assert!(SemVer::parse("1.2").is_err());
    }

    #[test]
    fn parse_invalid_version_non_numeric() {
        assert!(SemVer::parse("a.b.c").is_err());
    }

    #[test]
    fn parse_zero_version() {
        let v = SemVer::parse("0.0.0").unwrap();
        assert_eq!(v, SemVer::new(0, 0, 0));
    }

    // ---- SemVer display ----

    #[test]
    fn display_simple_version() {
        assert_eq!(SemVer::new(1, 2, 3).to_string(), "1.2.3");
    }

    #[test]
    fn display_version_with_pre() {
        assert_eq!(
            SemVer::new(1, 0, 0).with_pre("beta.2").to_string(),
            "1.0.0-beta.2"
        );
    }

    #[test]
    fn display_version_with_build() {
        assert_eq!(
            SemVer::new(1, 0, 0).with_build("build.5").to_string(),
            "1.0.0+build.5"
        );
    }

    #[test]
    fn display_roundtrip() {
        let input = "3.2.1-rc.1+sha.def456";
        let v = SemVer::parse(input).unwrap();
        assert_eq!(v.to_string(), input);
    }

    // ---- SemVer bumping ----

    #[test]
    fn bump_major_resets_minor_patch() {
        let v = SemVer::new(1, 5, 3).bump_major();
        assert_eq!(v, SemVer::new(2, 0, 0));
    }

    #[test]
    fn bump_minor_resets_patch() {
        let v = SemVer::new(1, 5, 3).bump_minor();
        assert_eq!(v, SemVer::new(1, 6, 0));
    }

    #[test]
    fn bump_patch() {
        let v = SemVer::new(1, 5, 3).bump_patch();
        assert_eq!(v, SemVer::new(1, 5, 4));
    }

    #[test]
    fn bump_clears_prerelease() {
        let v = SemVer::new(1, 0, 0).with_pre("alpha.1").bump_patch();
        assert!(!v.is_prerelease());
    }

    // ---- SemVer comparison ----

    #[test]
    fn version_ordering() {
        let v1 = SemVer::new(1, 0, 0);
        let v2 = SemVer::new(1, 1, 0);
        let v3 = SemVer::new(2, 0, 0);
        assert!(v1 < v2);
        assert!(v2 < v3);
    }

    #[test]
    fn prerelease_lower_than_release() {
        let pre = SemVer::new(1, 0, 0).with_pre("alpha.1");
        let release = SemVer::new(1, 0, 0);
        assert!(pre < release);
    }

    #[test]
    fn build_metadata_ignored_in_comparison() {
        let a = SemVer::new(1, 0, 0).with_build("a");
        let b = SemVer::new(1, 0, 0).with_build("b");
        assert_eq!(a.precedence_cmp(&b), std::cmp::Ordering::Equal);
    }

    #[test]
    fn is_stable_checks() {
        assert!(!SemVer::new(0, 9, 0).is_stable());
        assert!(SemVer::new(1, 0, 0).is_stable());
        assert!(!SemVer::new(1, 0, 0).with_pre("rc.1").is_stable());
    }

    // ---- ChangeEntry ----

    #[test]
    fn change_entry_markdown_simple() {
        let e = ChangeEntry::new(ChangeCategory::Added, "New tilemap editor");
        assert_eq!(e.to_markdown(), "- New tilemap editor");
    }

    #[test]
    fn change_entry_markdown_with_reference() {
        let e = ChangeEntry::new(ChangeCategory::Fixed, "Fix crash on load")
            .with_reference("pat-abc");
        assert_eq!(e.to_markdown(), "- Fix crash on load (pat-abc)");
    }

    #[test]
    fn change_entry_markdown_with_author() {
        let e = ChangeEntry::new(ChangeCategory::Added, "New feature")
            .with_author("bone");
        assert_eq!(e.to_markdown(), "- New feature — @bone");
    }

    #[test]
    fn change_entry_markdown_with_both() {
        let e = ChangeEntry::new(ChangeCategory::Fixed, "Fix bug")
            .with_reference("#42")
            .with_author("dev");
        assert_eq!(e.to_markdown(), "- Fix bug (#42) — @dev");
    }

    // ---- Release lifecycle ----

    #[test]
    fn release_starts_as_draft() {
        let r = Release::new(SemVer::new(1, 0, 0));
        assert_eq!(r.state, ReleaseState::Draft);
        assert!(r.date.is_none());
    }

    #[test]
    fn add_change_to_draft() {
        let mut r = Release::new(SemVer::new(1, 0, 0));
        r.add_change(ChangeEntry::new(ChangeCategory::Added, "Feature X"))
            .unwrap();
        assert_eq!(r.changes.len(), 1);
    }

    #[test]
    fn cannot_add_change_to_staged() {
        let mut r = Release::new(SemVer::new(1, 0, 0));
        r.stage("2026-03-25").unwrap();
        let result = r.add_change(ChangeEntry::new(ChangeCategory::Added, "Late addition"));
        assert!(result.is_err());
    }

    #[test]
    fn stage_transitions_to_staged() {
        let mut r = Release::new(SemVer::new(1, 0, 0));
        r.stage("2026-03-25").unwrap();
        assert_eq!(r.state, ReleaseState::Staged);
        assert_eq!(r.date, Some("2026-03-25".to_string()));
    }

    #[test]
    fn publish_transitions_to_published() {
        let mut r = Release::new(SemVer::new(1, 0, 0));
        r.stage("2026-03-25").unwrap();
        r.publish().unwrap();
        assert_eq!(r.state, ReleaseState::Published);
    }

    #[test]
    fn cannot_publish_draft() {
        let mut r = Release::new(SemVer::new(1, 0, 0));
        assert!(r.publish().is_err());
    }

    #[test]
    fn cancel_draft() {
        let mut r = Release::new(SemVer::new(1, 0, 0));
        r.cancel().unwrap();
        assert_eq!(r.state, ReleaseState::Cancelled);
    }

    #[test]
    fn cannot_cancel_published() {
        let mut r = Release::new(SemVer::new(1, 0, 0));
        r.stage("2026-03-25").unwrap();
        r.publish().unwrap();
        assert!(r.cancel().is_err());
    }

    // ---- Release changelog ----

    #[test]
    fn release_changelog_markdown() {
        let mut r = Release::new(SemVer::new(1, 0, 0));
        r.set_summary("First stable release.");
        r.add_change(ChangeEntry::new(ChangeCategory::Added, "Scene editor"))
            .unwrap();
        r.add_change(ChangeEntry::new(ChangeCategory::Fixed, "Memory leak"))
            .unwrap();
        r.stage("2026-03-25").unwrap();

        let md = r.to_changelog_markdown();
        assert!(md.contains("## [1.0.0] - 2026-03-25"));
        assert!(md.contains("First stable release."));
        assert!(md.contains("### Added"));
        assert!(md.contains("- Scene editor"));
        assert!(md.contains("### Fixed"));
        assert!(md.contains("- Memory leak"));
    }

    #[test]
    fn changes_by_category_groups_correctly() {
        let mut r = Release::new(SemVer::new(1, 0, 0));
        r.add_change(ChangeEntry::new(ChangeCategory::Added, "A")).unwrap();
        r.add_change(ChangeEntry::new(ChangeCategory::Fixed, "B")).unwrap();
        r.add_change(ChangeEntry::new(ChangeCategory::Added, "C")).unwrap();

        let grouped = r.changes_by_category();
        assert_eq!(grouped[&ChangeCategory::Added].len(), 2);
        assert_eq!(grouped[&ChangeCategory::Fixed].len(), 1);
    }

    #[test]
    fn has_breaking_changes() {
        let mut r = Release::new(SemVer::new(2, 0, 0));
        r.add_change(ChangeEntry::new(ChangeCategory::Breaking, "Remove API")).unwrap();
        assert!(r.has_breaking_changes());

        let r2 = Release::new(SemVer::new(1, 1, 0));
        assert!(!r2.has_breaking_changes());
    }

    // ---- Release Train ----

    #[test]
    fn empty_train() {
        let train = ReleaseTrain::new();
        assert_eq!(train.current_version(), SemVer::new(0, 0, 0));
        assert!(train.latest_published().is_none());
        assert_eq!(train.published_count(), 0);
    }

    #[test]
    fn create_and_publish_release() {
        let mut train = ReleaseTrain::new();
        let release = train.create_release(SemVer::new(0, 1, 0)).unwrap();
        release
            .add_change(ChangeEntry::new(ChangeCategory::Added, "Initial feature"))
            .unwrap();
        release.stage("2026-01-01").unwrap();
        release.publish().unwrap();

        assert_eq!(train.current_version(), SemVer::new(0, 1, 0));
        assert_eq!(train.published_count(), 1);
    }

    #[test]
    fn version_must_increase() {
        let mut train = ReleaseTrain::new();
        let r = train.create_release(SemVer::new(1, 0, 0)).unwrap();
        r.stage("2026-01-01").unwrap();
        r.publish().unwrap();

        // Try to create a release with same version.
        assert!(train.create_release(SemVer::new(1, 0, 0)).is_err());
        // Lower version also fails.
        assert!(train.create_release(SemVer::new(0, 9, 0)).is_err());
    }

    #[test]
    fn cannot_create_two_drafts() {
        let mut train = ReleaseTrain::new();
        train.create_release(SemVer::new(0, 1, 0)).unwrap();
        assert!(train.create_release(SemVer::new(0, 2, 0)).is_err());
    }

    #[test]
    fn suggest_patch_for_fixes() {
        let mut train = ReleaseTrain::new();
        let r = train.create_release(SemVer::new(1, 0, 0)).unwrap();
        r.stage("2026-01-01").unwrap();
        r.publish().unwrap();

        let changes = vec![ChangeEntry::new(ChangeCategory::Fixed, "Bug fix")];
        assert_eq!(train.suggest_next_version(&changes), SemVer::new(1, 0, 1));
    }

    #[test]
    fn suggest_minor_for_features() {
        let mut train = ReleaseTrain::new();
        let r = train.create_release(SemVer::new(1, 0, 0)).unwrap();
        r.stage("2026-01-01").unwrap();
        r.publish().unwrap();

        let changes = vec![ChangeEntry::new(ChangeCategory::Added, "New feature")];
        assert_eq!(train.suggest_next_version(&changes), SemVer::new(1, 1, 0));
    }

    #[test]
    fn suggest_major_for_breaking_post_v1() {
        let mut train = ReleaseTrain::new();
        let r = train.create_release(SemVer::new(1, 0, 0)).unwrap();
        r.stage("2026-01-01").unwrap();
        r.publish().unwrap();

        let changes = vec![ChangeEntry::new(ChangeCategory::Breaking, "API change")];
        assert_eq!(train.suggest_next_version(&changes), SemVer::new(2, 0, 0));
    }

    #[test]
    fn suggest_minor_for_breaking_pre_v1() {
        let mut train = ReleaseTrain::new();
        let r = train.create_release(SemVer::new(0, 5, 0)).unwrap();
        r.stage("2026-01-01").unwrap();
        r.publish().unwrap();

        let changes = vec![ChangeEntry::new(ChangeCategory::Breaking, "API change")];
        assert_eq!(train.suggest_next_version(&changes), SemVer::new(0, 6, 0));
    }

    #[test]
    fn full_changelog_generation() {
        let mut train = ReleaseTrain::new();

        // v0.1.0
        let r = train.create_release(SemVer::new(0, 1, 0)).unwrap();
        r.add_change(ChangeEntry::new(ChangeCategory::Added, "Initial release")).unwrap();
        r.stage("2026-01-01").unwrap();
        r.publish().unwrap();

        // v0.2.0
        let r = train.create_release(SemVer::new(0, 2, 0)).unwrap();
        r.add_change(ChangeEntry::new(ChangeCategory::Added, "Editor")).unwrap();
        r.add_change(ChangeEntry::new(ChangeCategory::Fixed, "Crash")).unwrap();
        r.stage("2026-02-01").unwrap();
        r.publish().unwrap();

        let changelog = train.generate_changelog();
        assert!(changelog.contains("# Changelog"));
        assert!(changelog.contains("[0.2.0]"));
        assert!(changelog.contains("[0.1.0]"));
        // v0.2.0 should appear before v0.1.0 (reverse chronological).
        let pos_02 = changelog.find("[0.2.0]").unwrap();
        let pos_01 = changelog.find("[0.1.0]").unwrap();
        assert!(pos_02 < pos_01);
    }

    #[test]
    fn cancelled_releases_excluded_from_changelog() {
        let mut train = ReleaseTrain::new();

        let r = train.create_release(SemVer::new(0, 1, 0)).unwrap();
        r.add_change(ChangeEntry::new(ChangeCategory::Added, "Feature")).unwrap();
        r.cancel().unwrap();

        let changelog = train.generate_changelog();
        assert!(!changelog.contains("[0.1.0]"));
    }

    #[test]
    fn current_draft_returns_latest_draft() {
        let mut train = ReleaseTrain::new();
        train.create_release(SemVer::new(0, 1, 0)).unwrap();

        let draft = train.current_draft().unwrap();
        assert_eq!(draft.version, SemVer::new(0, 1, 0));
        assert_eq!(draft.state, ReleaseState::Draft);
    }

    #[test]
    fn change_category_ordering() {
        // Breaking should sort before Added.
        assert!(ChangeCategory::Breaking < ChangeCategory::Added);
        assert!(ChangeCategory::Added < ChangeCategory::Fixed);
    }

    #[test]
    fn change_category_display() {
        assert_eq!(ChangeCategory::Breaking.to_string(), "Breaking Changes");
        assert_eq!(ChangeCategory::Added.to_string(), "Added");
        assert_eq!(ChangeCategory::Security.to_string(), "Security");
    }

    #[test]
    fn unreleased_section_shows_unreleased() {
        let r = Release::new(SemVer::new(1, 0, 0));
        let md = r.to_changelog_markdown();
        assert!(md.contains("Unreleased"));
    }

    #[test]
    fn multiple_releases_train() {
        let mut train = ReleaseTrain::new();

        for i in 1..=5 {
            let r = train.create_release(SemVer::new(0, i, 0)).unwrap();
            r.add_change(ChangeEntry::new(ChangeCategory::Added, &format!("Feature {}", i))).unwrap();
            r.stage(&format!("2026-0{}-01", i)).unwrap();
            r.publish().unwrap();
        }

        assert_eq!(train.published_count(), 5);
        assert_eq!(train.current_version(), SemVer::new(0, 5, 0));
        assert_eq!(train.releases().len(), 5);
    }
}
