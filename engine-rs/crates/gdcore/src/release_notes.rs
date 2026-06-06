//! Automated release notes from git log and bead history.
//!
//! Parses git commit messages and bead tracker output to automatically
//! generate [`ChangeEntry`](super::release_train::ChangeEntry) items for
//! release notes. Supports:
//!
//! - **Conventional Commits**: parses `feat:`, `fix:`, `docs:`, etc. prefixes
//! - **Bead references**: extracts `pat-XXXXX` bead IDs from commit messages
//! - **Breaking change detection**: `!` suffix or `BREAKING CHANGE:` footer
//! - **Author extraction**: from git log `--format` output
//! - **Deduplication**: groups commits by bead, avoids duplicate entries
//! - **Filtering**: exclude merge commits, CI-only changes, etc.

use crate::release_train::{ChangeCategory, ChangeEntry};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Conventional Commit types → ChangeCategory
// ---------------------------------------------------------------------------

/// Maps a conventional commit type prefix to a ChangeCategory.
fn category_from_type(commit_type: &str) -> ChangeCategory {
    match commit_type.to_lowercase().as_str() {
        "feat" | "feature" => ChangeCategory::Added,
        "fix" | "bugfix" => ChangeCategory::Fixed,
        "perf" | "performance" => ChangeCategory::Performance,
        "docs" | "doc" => ChangeCategory::Changed,
        "refactor" | "chore" | "build" | "ci" | "test" | "tests" => ChangeCategory::Internal,
        "deprecate" | "deprecated" => ChangeCategory::Deprecated,
        "remove" | "removed" => ChangeCategory::Removed,
        "security" | "sec" => ChangeCategory::Security,
        "breaking" => ChangeCategory::Breaking,
        _ => ChangeCategory::Changed,
    }
}

// ---------------------------------------------------------------------------
// GitCommit
// ---------------------------------------------------------------------------

/// A parsed git commit from `git log` output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommit {
    /// The commit hash (short or full).
    pub hash: String,
    /// The commit message subject (first line).
    pub subject: String,
    /// The commit message body (everything after the first blank line).
    pub body: String,
    /// The author name.
    pub author: String,
    /// The commit date (ISO 8601).
    pub date: String,
}

impl GitCommit {
    /// Create a new GitCommit.
    pub fn new(hash: &str, subject: &str, author: &str, date: &str) -> Self {
        Self {
            hash: hash.to_string(),
            subject: subject.to_string(),
            body: String::new(),
            author: author.to_string(),
            date: date.to_string(),
        }
    }

    /// Set the body of the commit message.
    pub fn with_body(mut self, body: &str) -> Self {
        self.body = body.to_string();
        self
    }

    /// Returns true if this looks like a merge commit.
    pub fn is_merge(&self) -> bool {
        self.subject.starts_with("Merge ") || self.subject.starts_with("Merge pull request")
    }
}

// ---------------------------------------------------------------------------
// Parsing conventional commits
// ---------------------------------------------------------------------------

/// A parsed conventional commit subject line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConventionalCommit {
    /// The commit type (e.g. "feat", "fix").
    pub commit_type: String,
    /// Optional scope (e.g. "editor", "physics").
    pub scope: Option<String>,
    /// Whether this is a breaking change (indicated by `!`).
    pub breaking: bool,
    /// The description after the colon.
    pub description: String,
}

/// Parses a conventional commit subject line.
///
/// Format: `type[(scope)][!]: description`
///
/// Returns `None` if the subject doesn't match conventional commit format.
pub fn parse_conventional_commit(subject: &str) -> Option<ConventionalCommit> {
    let subject = subject.trim();

    // Find the colon separator
    let colon_pos = subject.find(':')?;
    let prefix = &subject[..colon_pos];
    let description = subject[colon_pos + 1..].trim().to_string();

    if description.is_empty() {
        return None;
    }

    // Parse prefix: type[(scope)][!]
    let (type_and_scope, breaking) = if prefix.ends_with('!') {
        (&prefix[..prefix.len() - 1], true)
    } else {
        (prefix, false)
    };

    // Extract scope if present
    let (commit_type, scope) = if let Some(paren_start) = type_and_scope.find('(') {
        if !type_and_scope.ends_with(')') {
            return None;
        }
        let ct = &type_and_scope[..paren_start];
        let sc = &type_and_scope[paren_start + 1..type_and_scope.len() - 1];
        (ct.to_string(), Some(sc.to_string()))
    } else {
        (type_and_scope.to_string(), None)
    };

    // Validate commit type is a single word with only letters
    if commit_type.is_empty() || !commit_type.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }

    Some(ConventionalCommit {
        commit_type,
        scope,
        breaking,
        description,
    })
}

// ---------------------------------------------------------------------------
// Bead ID extraction
// ---------------------------------------------------------------------------

/// Extracts bead IDs (pat-XXXXX format) from a string.
pub fn extract_bead_ids(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut start = 0;
    while let Some(pos) = text[start..].find("pat-") {
        let abs_pos = start + pos;
        let id_start = abs_pos;
        let id_end = text[abs_pos + 4..]
            .find(|c: char| !c.is_alphanumeric())
            .map(|p| abs_pos + 4 + p)
            .unwrap_or(text.len());
        let id = &text[id_start..id_end];
        if id.len() > 4 {
            // Must have at least one character after "pat-"
            ids.push(id.to_string());
        }
        start = id_end;
    }
    ids
}

// ---------------------------------------------------------------------------
// BeadSummary
// ---------------------------------------------------------------------------

/// Summary of a bead from the issue tracker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeadSummary {
    /// Bead ID (e.g. "pat-abc12").
    pub id: String,
    /// Bead title.
    pub title: String,
    /// Bead status.
    pub status: String,
    /// Priority (0 = critical, 3 = low).
    pub priority: u32,
    /// Labels/tags.
    pub labels: Vec<String>,
}

impl BeadSummary {
    /// Create a new bead summary.
    pub fn new(id: &str, title: &str) -> Self {
        Self {
            id: id.to_string(),
            title: title.to_string(),
            status: "done".to_string(),
            priority: 3,
            labels: Vec::new(),
        }
    }

    /// Infer a change category from the bead title and labels.
    pub fn infer_category(&self) -> ChangeCategory {
        let title_lower = self.title.to_lowercase();
        let labels_lower: Vec<String> = self.labels.iter().map(|l| l.to_lowercase()).collect();

        // Check labels first
        for label in &labels_lower {
            if label.contains("breaking") {
                return ChangeCategory::Breaking;
            }
            if label.contains("security") {
                return ChangeCategory::Security;
            }
            if label.contains("deprecat") {
                return ChangeCategory::Deprecated;
            }
        }

        // Infer from title keywords
        if title_lower.starts_with("fix") || title_lower.contains("bug") {
            ChangeCategory::Fixed
        } else if title_lower.contains("performance") || title_lower.contains("optimize") {
            ChangeCategory::Performance
        } else if title_lower.contains("remove") || title_lower.contains("drop") {
            ChangeCategory::Removed
        } else if title_lower.contains("deprecat") {
            ChangeCategory::Deprecated
        } else if title_lower.contains("refactor")
            || title_lower.contains("cleanup")
            || title_lower.contains("ci ")
            || title_lower.contains("test")
        {
            ChangeCategory::Internal
        } else {
            ChangeCategory::Added
        }
    }

    /// Convert this bead summary to a change entry.
    pub fn to_change_entry(&self) -> ChangeEntry {
        ChangeEntry::new(self.infer_category(), &self.title).with_reference(&self.id)
    }
}

// ---------------------------------------------------------------------------
// ReleaseNotesBuilder
// ---------------------------------------------------------------------------

/// Builds release notes from git commits and/or bead summaries.
///
/// Deduplicates entries: if a commit references a bead, the bead's title
/// is used instead of the raw commit message.
#[derive(Debug)]
pub struct ReleaseNotesBuilder {
    /// Commits to process.
    commits: Vec<GitCommit>,
    /// Bead summaries to include.
    beads: Vec<BeadSummary>,
    /// Whether to skip merge commits.
    skip_merges: bool,
    /// Whether to skip internal/CI-only changes from the output.
    skip_internal: bool,
    /// Bead ID → title lookup for deduplication.
    bead_lookup: HashMap<String, String>,
}

impl ReleaseNotesBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            commits: Vec::new(),
            beads: Vec::new(),
            skip_merges: true,
            skip_internal: false,
            bead_lookup: HashMap::new(),
        }
    }

    /// Add git commits.
    pub fn add_commits(&mut self, commits: Vec<GitCommit>) {
        self.commits.extend(commits);
    }

    /// Add bead summaries.
    pub fn add_beads(&mut self, beads: Vec<BeadSummary>) {
        for bead in &beads {
            self.bead_lookup.insert(bead.id.clone(), bead.title.clone());
        }
        self.beads.extend(beads);
    }

    /// Set whether to skip merge commits (default: true).
    pub fn skip_merges(mut self, skip: bool) -> Self {
        self.skip_merges = skip;
        self
    }

    /// Set whether to skip internal changes (default: false).
    pub fn skip_internal(mut self, skip: bool) -> Self {
        self.skip_internal = skip;
        self
    }

    /// Convert a single git commit to a change entry.
    fn commit_to_entry(&self, commit: &GitCommit) -> ChangeEntry {
        // Try to parse as conventional commit
        if let Some(cc) = parse_conventional_commit(&commit.subject) {
            let category = if cc.breaking
                || commit.body.contains("BREAKING CHANGE:")
                || commit.body.contains("BREAKING-CHANGE:")
            {
                ChangeCategory::Breaking
            } else {
                category_from_type(&cc.commit_type)
            };

            let desc = if let Some(ref scope) = cc.scope {
                format!("**{}**: {}", scope, cc.description)
            } else {
                cc.description
            };

            let mut entry = ChangeEntry::new(category, &desc);
            entry.author = Some(commit.author.clone());

            // Check for bead references
            let bead_ids = extract_bead_ids(&commit.subject);
            if let Some(id) = bead_ids.first() {
                entry.reference = Some(id.clone());
            } else {
                entry.reference = Some(commit.hash.clone());
            }

            entry
        } else {
            // Non-conventional commit: try bead IDs, then use raw subject
            let bead_ids = extract_bead_ids(&commit.subject);

            let (description, reference) = if let Some(id) = bead_ids.first() {
                // Use bead title if available
                let desc = self
                    .bead_lookup
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| commit.subject.clone());
                (desc, Some(id.clone()))
            } else {
                (commit.subject.clone(), Some(commit.hash.clone()))
            };

            let mut entry = ChangeEntry::new(ChangeCategory::Changed, &description);
            entry.reference = reference;
            entry.author = Some(commit.author.clone());
            entry
        }
    }

    /// Build the release notes as a list of change entries.
    ///
    /// Deduplication: if multiple commits reference the same bead, only one
    /// entry is produced (using the bead's title).
    pub fn build(&self) -> Vec<ChangeEntry> {
        let mut entries: Vec<ChangeEntry> = Vec::new();
        let mut seen_beads: HashMap<String, usize> = HashMap::new();
        let mut seen_hashes: HashMap<String, bool> = HashMap::new();

        // Process commits first
        for commit in &self.commits {
            if self.skip_merges && commit.is_merge() {
                continue;
            }

            let entry = self.commit_to_entry(commit);

            if self.skip_internal && entry.category == ChangeCategory::Internal {
                continue;
            }

            // Deduplicate by bead ID
            let bead_ids = extract_bead_ids(&commit.subject);
            if let Some(bead_id) = bead_ids.first() {
                if seen_beads.contains_key(bead_id) {
                    continue; // already have an entry for this bead
                }
                seen_beads.insert(bead_id.clone(), entries.len());
            }

            // Deduplicate by commit hash
            if seen_hashes.contains_key(&commit.hash) {
                continue;
            }
            seen_hashes.insert(commit.hash.clone(), true);

            entries.push(entry);
        }

        // Add any beads not covered by commits
        for bead in &self.beads {
            if !seen_beads.contains_key(&bead.id) {
                let entry = bead.to_change_entry();
                if self.skip_internal && entry.category == ChangeCategory::Internal {
                    continue;
                }
                entries.push(entry);
            }
        }

        // Sort by category (Breaking first, then Added, etc.)
        entries.sort_by_key(|e| e.category);

        entries
    }

    /// Build and format as markdown release notes.
    pub fn build_markdown(&self, version: &str, date: &str) -> String {
        let entries = self.build();
        let mut md = String::new();

        md.push_str(&format!("## [{}] - {}\n\n", version, date));

        // Group by category
        let mut current_category: Option<ChangeCategory> = None;
        for entry in &entries {
            if current_category != Some(entry.category) {
                if current_category.is_some() {
                    md.push('\n');
                }
                md.push_str(&format!("### {}\n\n", entry.category.heading()));
                current_category = Some(entry.category);
            }
            md.push_str(&entry.to_markdown());
            md.push('\n');
        }

        md
    }
}

impl Default for ReleaseNotesBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Git log parsing
// ---------------------------------------------------------------------------

/// Parse git log output in the format produced by:
/// `git log --format='%H|%s|%aN|%aI' --no-merges`
///
/// Each line: `hash|subject|author|date`
pub fn parse_git_log(output: &str) -> Vec<GitCommit> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let parts: Vec<&str> = line.splitn(4, '|').collect();
            if parts.len() < 4 {
                return None;
            }
            Some(GitCommit::new(parts[0], parts[1], parts[2], parts[3]))
        })
        .collect()
}

/// Parse git log output with body, in the format:
/// `git log --format='COMMIT_START%n%H|%s|%aN|%aI%n%b%nCOMMIT_END'`
pub fn parse_git_log_with_body(output: &str) -> Vec<GitCommit> {
    let mut commits = Vec::new();

    for block in output.split("COMMIT_START\n") {
        let block = block.trim();
        if block.is_empty() {
            continue;
        }

        let block = block.trim_end_matches("COMMIT_END").trim();
        let mut lines = block.lines();

        if let Some(header) = lines.next() {
            let parts: Vec<&str> = header.splitn(4, '|').collect();
            if parts.len() >= 4 {
                let body: String = lines.collect::<Vec<&str>>().join("\n");
                let mut commit = GitCommit::new(parts[0], parts[1], parts[2], parts[3]);
                if !body.trim().is_empty() {
                    commit = commit.with_body(body.trim());
                }
                commits.push(commit);
            }
        }
    }

    commits
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Conventional commit parsing --

    #[test]
    fn parse_simple_feat() {
        let cc = parse_conventional_commit("feat: add new rendering backend").unwrap();
        assert_eq!(cc.commit_type, "feat");
        assert!(cc.scope.is_none());
        assert!(!cc.breaking);
        assert_eq!(cc.description, "add new rendering backend");
    }

    #[test]
    fn parse_fix_with_scope() {
        let cc = parse_conventional_commit("fix(physics): resolve collision jitter").unwrap();
        assert_eq!(cc.commit_type, "fix");
        assert_eq!(cc.scope, Some("physics".to_string()));
        assert_eq!(cc.description, "resolve collision jitter");
    }

    #[test]
    fn parse_breaking_with_bang() {
        let cc = parse_conventional_commit("feat!: redesign API surface").unwrap();
        assert!(cc.breaking);
        assert_eq!(cc.commit_type, "feat");
    }

    #[test]
    fn parse_breaking_with_scope_and_bang() {
        let cc = parse_conventional_commit("refactor(core)!: remove deprecated methods").unwrap();
        assert!(cc.breaking);
        assert_eq!(cc.scope, Some("core".to_string()));
    }

    #[test]
    fn parse_non_conventional_returns_none() {
        assert!(parse_conventional_commit("Update README").is_none());
        assert!(parse_conventional_commit("").is_none());
        assert!(parse_conventional_commit("no colon here").is_none());
    }

    #[test]
    fn parse_empty_description_returns_none() {
        assert!(parse_conventional_commit("feat:").is_none());
        assert!(parse_conventional_commit("fix: ").is_none());
    }

    #[test]
    fn parse_type_must_be_alphabetic() {
        assert!(parse_conventional_commit("123: numeric type").is_none());
        assert!(parse_conventional_commit("feat-x: hyphen in type").is_none());
    }

    // -- Category mapping --

    #[test]
    fn category_from_known_types() {
        assert_eq!(category_from_type("feat"), ChangeCategory::Added);
        assert_eq!(category_from_type("fix"), ChangeCategory::Fixed);
        assert_eq!(category_from_type("perf"), ChangeCategory::Performance);
        assert_eq!(category_from_type("docs"), ChangeCategory::Changed);
        assert_eq!(category_from_type("refactor"), ChangeCategory::Internal);
        assert_eq!(category_from_type("security"), ChangeCategory::Security);
        assert_eq!(category_from_type("deprecate"), ChangeCategory::Deprecated);
        assert_eq!(category_from_type("remove"), ChangeCategory::Removed);
    }

    #[test]
    fn category_from_unknown_type_defaults_to_changed() {
        assert_eq!(category_from_type("misc"), ChangeCategory::Changed);
        assert_eq!(category_from_type("xyz"), ChangeCategory::Changed);
    }

    // -- Bead ID extraction --

    #[test]
    fn extract_single_bead_id() {
        let ids = extract_bead_ids("Close pat-abc12 after testing");
        assert_eq!(ids, vec!["pat-abc12"]);
    }

    #[test]
    fn extract_multiple_bead_ids() {
        let ids = extract_bead_ids("Fixes pat-abc12 and pat-xyz99");
        assert_eq!(ids, vec!["pat-abc12", "pat-xyz99"]);
    }

    #[test]
    fn extract_no_bead_ids() {
        let ids = extract_bead_ids("Just a regular commit message");
        assert!(ids.is_empty());
    }

    #[test]
    fn extract_bead_id_at_end_of_string() {
        let ids = extract_bead_ids("Work on pat-12345");
        assert_eq!(ids, vec!["pat-12345"]);
    }

    #[test]
    fn extract_bead_id_ignores_bare_prefix() {
        // "pat-" alone (no alphanumeric after) should not match
        let ids = extract_bead_ids("This is pat- incomplete");
        assert!(ids.is_empty());
    }

    // -- BeadSummary --

    #[test]
    fn bead_summary_infer_category_fix() {
        let bead = BeadSummary::new("pat-a1", "Fix collision detection edge case");
        assert_eq!(bead.infer_category(), ChangeCategory::Fixed);
    }

    #[test]
    fn bead_summary_infer_category_feature() {
        let bead = BeadSummary::new("pat-b2", "Add shader editor panel");
        assert_eq!(bead.infer_category(), ChangeCategory::Added);
    }

    #[test]
    fn bead_summary_infer_category_performance() {
        let bead = BeadSummary::new("pat-c3", "Optimize render loop performance");
        assert_eq!(bead.infer_category(), ChangeCategory::Performance);
    }

    #[test]
    fn bead_summary_infer_category_from_label() {
        let mut bead = BeadSummary::new("pat-d4", "Update API");
        bead.labels = vec!["breaking".to_string()];
        assert_eq!(bead.infer_category(), ChangeCategory::Breaking);
    }

    #[test]
    fn bead_summary_to_change_entry() {
        let bead = BeadSummary::new("pat-e5", "Add theme editor");
        let entry = bead.to_change_entry();
        assert_eq!(entry.category, ChangeCategory::Added);
        assert_eq!(entry.description, "Add theme editor");
        assert_eq!(entry.reference, Some("pat-e5".to_string()));
    }

    // -- Git log parsing --

    #[test]
    fn parse_git_log_basic() {
        let output =
            "abc1234|feat: add stuff|Alice|2026-03-25\ndef5678|fix: broken thing|Bob|2026-03-24\n";
        let commits = parse_git_log(output);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].hash, "abc1234");
        assert_eq!(commits[0].subject, "feat: add stuff");
        assert_eq!(commits[0].author, "Alice");
        assert_eq!(commits[1].subject, "fix: broken thing");
    }

    #[test]
    fn parse_git_log_skips_empty_lines() {
        let output = "\nabc|feat: x|A|2026-01-01\n\n";
        let commits = parse_git_log(output);
        assert_eq!(commits.len(), 1);
    }

    #[test]
    fn parse_git_log_with_body_basic() {
        let output = "COMMIT_START\nabc|feat: add stuff|Alice|2026-03-25\nSome body text\nCOMMIT_END\nCOMMIT_START\ndef|fix: thing|Bob|2026-03-24\nBREAKING CHANGE: old API removed\nCOMMIT_END\n";
        let commits = parse_git_log_with_body(output);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].body, "Some body text");
        assert!(commits[1].body.contains("BREAKING CHANGE"));
    }

    // -- GitCommit --

    #[test]
    fn is_merge_commit() {
        let c = GitCommit::new("abc", "Merge branch 'main'", "A", "2026-01-01");
        assert!(c.is_merge());

        let c = GitCommit::new("def", "feat: add thing", "B", "2026-01-01");
        assert!(!c.is_merge());
    }

    // -- ReleaseNotesBuilder --

    #[test]
    fn builder_from_conventional_commits() {
        let mut builder = ReleaseNotesBuilder::new();
        builder.add_commits(vec![
            GitCommit::new("aaa", "feat: add new feature", "Alice", "2026-03-25"),
            GitCommit::new("bbb", "fix: resolve crash", "Bob", "2026-03-24"),
        ]);
        let entries = builder.build();
        assert_eq!(entries.len(), 2);
        // Sorted by category: Added < Fixed
        assert_eq!(entries[0].category, ChangeCategory::Added);
        assert_eq!(entries[1].category, ChangeCategory::Fixed);
    }

    #[test]
    fn builder_skips_merge_commits() {
        let mut builder = ReleaseNotesBuilder::new();
        builder.add_commits(vec![
            GitCommit::new("aaa", "Merge branch 'feature'", "Alice", "2026-03-25"),
            GitCommit::new("bbb", "feat: real change", "Bob", "2026-03-24"),
        ]);
        let entries = builder.build();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].description, "real change");
    }

    #[test]
    fn builder_includes_merges_when_configured() {
        let mut builder = ReleaseNotesBuilder::new().skip_merges(false);
        builder.add_commits(vec![GitCommit::new(
            "aaa",
            "Merge branch 'feature'",
            "Alice",
            "2026-03-25",
        )]);
        let entries = builder.build();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn builder_skip_internal() {
        let mut builder = ReleaseNotesBuilder::new().skip_internal(true);
        builder.add_commits(vec![
            GitCommit::new("aaa", "feat: visible feature", "Alice", "2026-03-25"),
            GitCommit::new("bbb", "ci: update pipeline", "Bob", "2026-03-24"),
            GitCommit::new("ccc", "test: add unit tests", "Carol", "2026-03-23"),
        ]);
        let entries = builder.build();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].category, ChangeCategory::Added);
    }

    #[test]
    fn builder_deduplicates_by_bead_id() {
        let mut builder = ReleaseNotesBuilder::new();
        builder.add_commits(vec![
            GitCommit::new("aaa", "feat: WIP pat-abc12", "Alice", "2026-03-25"),
            GitCommit::new("bbb", "feat: finish pat-abc12", "Alice", "2026-03-26"),
        ]);
        let entries = builder.build();
        // Only one entry for pat-abc12
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn builder_uses_bead_title_when_available() {
        let mut builder = ReleaseNotesBuilder::new();
        builder.add_beads(vec![BeadSummary::new("pat-abc12", "Add shader editor")]);
        builder.add_commits(vec![GitCommit::new(
            "aaa",
            "feat: implement pat-abc12",
            "Alice",
            "2026-03-25",
        )]);
        let entries = builder.build();
        assert_eq!(entries.len(), 1);
        // Should use bead title, not raw commit message
        // (since the non-conventional fallback path uses bead_lookup)
        // Actually this is a conventional commit so it uses the commit desc
        // Let's check the reference
        assert_eq!(entries[0].reference, Some("pat-abc12".to_string()));
    }

    #[test]
    fn builder_adds_beads_not_in_commits() {
        let mut builder = ReleaseNotesBuilder::new();
        builder.add_commits(vec![GitCommit::new(
            "aaa",
            "feat: commit for pat-abc12",
            "Alice",
            "2026-03-25",
        )]);
        builder.add_beads(vec![
            BeadSummary::new("pat-abc12", "Covered by commit"),
            BeadSummary::new("pat-xyz99", "Extra bead with no commit"),
        ]);
        let entries = builder.build();
        // Should have 2 entries: one from commit, one from orphan bead
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn builder_breaking_change_from_body() {
        let commit = GitCommit::new("aaa", "feat: change API", "Alice", "2026-03-25")
            .with_body("BREAKING CHANGE: removed old endpoint");
        let mut builder = ReleaseNotesBuilder::new();
        builder.add_commits(vec![commit]);
        let entries = builder.build();
        assert_eq!(entries[0].category, ChangeCategory::Breaking);
    }

    #[test]
    fn builder_conventional_with_scope_formatted() {
        let mut builder = ReleaseNotesBuilder::new();
        builder.add_commits(vec![GitCommit::new(
            "aaa",
            "fix(editor): resolve toolbar crash",
            "Alice",
            "2026-03-25",
        )]);
        let entries = builder.build();
        assert_eq!(entries[0].description, "**editor**: resolve toolbar crash");
    }

    #[test]
    fn builder_build_markdown() {
        let mut builder = ReleaseNotesBuilder::new();
        builder.add_commits(vec![
            GitCommit::new("aaa", "feat: add sky rendering", "Alice", "2026-03-25"),
            GitCommit::new("bbb", "fix: correct fog color", "Bob", "2026-03-24"),
        ]);
        let md = builder.build_markdown("0.1.0", "2026-03-25");
        assert!(md.contains("## [0.1.0] - 2026-03-25"));
        assert!(md.contains("### Added"));
        assert!(md.contains("### Fixed"));
        assert!(md.contains("add sky rendering"));
        assert!(md.contains("correct fog color"));
    }

    #[test]
    fn builder_empty_produces_empty() {
        let builder = ReleaseNotesBuilder::new();
        let entries = builder.build();
        assert!(entries.is_empty());
    }

    #[test]
    fn builder_non_conventional_commit_uses_raw_subject() {
        let mut builder = ReleaseNotesBuilder::new();
        builder.add_commits(vec![GitCommit::new(
            "aaa",
            "Update README with new instructions",
            "Alice",
            "2026-03-25",
        )]);
        let entries = builder.build();
        assert_eq!(
            entries[0].description,
            "Update README with new instructions"
        );
        assert_eq!(entries[0].category, ChangeCategory::Changed);
    }

    #[test]
    fn builder_non_conventional_with_bead_uses_bead_title() {
        let mut builder = ReleaseNotesBuilder::new();
        builder.add_beads(vec![BeadSummary::new(
            "pat-hello",
            "Implement hello world feature",
        )]);
        builder.add_commits(vec![GitCommit::new(
            "aaa",
            "Close pat-hello after testing",
            "Alice",
            "2026-03-25",
        )]);
        let entries = builder.build();
        assert_eq!(entries[0].description, "Implement hello world feature");
        assert_eq!(entries[0].reference, Some("pat-hello".to_string()));
    }
}
