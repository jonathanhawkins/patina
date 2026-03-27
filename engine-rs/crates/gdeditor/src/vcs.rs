//! Version control integration with git status display.
//!
//! Provides a [`VcsStatus`] model that mirrors Godot's built-in VCS panel,
//! showing file-level change status (modified, added, deleted, renamed,
//! untracked) by shelling out to `git`.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// The change status of a single file in the working tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileChangeStatus {
    /// File has been modified relative to HEAD.
    Modified,
    /// File is newly added (staged).
    Added,
    /// File has been deleted.
    Deleted,
    /// File has been renamed.
    Renamed,
    /// File is present in the working tree but not tracked by git.
    Untracked,
    /// File has merge conflicts.
    Conflicted,
    /// File has been copied.
    Copied,
}

/// Whether the change is staged (in the index) or unstaged (working tree).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeArea {
    /// Change is staged in the git index.
    Staged,
    /// Change is in the working tree (not staged).
    Unstaged,
}

/// A single file's VCS status entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VcsFileStatus {
    /// Path relative to the repository root.
    pub path: String,
    /// The type of change.
    pub status: FileChangeStatus,
    /// Whether the change is staged or unstaged.
    pub area: ChangeArea,
}

/// The current branch information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchInfo {
    /// The current branch name (or "HEAD" if detached).
    pub name: String,
    /// Number of commits ahead of upstream, if tracking.
    pub ahead: u32,
    /// Number of commits behind upstream, if tracking.
    pub behind: u32,
    /// Whether the HEAD is detached.
    pub detached: bool,
}

/// Summary of the repository's VCS status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcsStatus {
    /// Whether this directory is inside a git repository.
    pub is_git_repo: bool,
    /// Branch information (None if not a git repo).
    pub branch: Option<BranchInfo>,
    /// List of file status entries.
    pub files: Vec<VcsFileStatus>,
    /// Counts by status type.
    pub summary: HashMap<String, usize>,
}

impl Default for VcsStatus {
    fn default() -> Self {
        Self {
            is_git_repo: false,
            branch: None,
            files: Vec::new(),
            summary: HashMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Git integration
// ---------------------------------------------------------------------------

/// Queries git for the current VCS status of the given project directory.
///
/// Returns a [`VcsStatus`] with branch info, file statuses, and summary counts.
/// If the directory is not a git repository, returns a default status with
/// `is_git_repo = false`.
pub fn query_git_status(project_dir: &Path) -> VcsStatus {
    // Check if this is a git repo.
    let is_repo = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(project_dir)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !is_repo {
        return VcsStatus::default();
    }

    let branch = query_branch_info(project_dir);
    let files = query_file_statuses(project_dir);

    // Build summary counts.
    let mut summary: HashMap<String, usize> = HashMap::new();
    for f in &files {
        let key = format!("{:?}", f.status).to_lowercase();
        *summary.entry(key).or_insert(0) += 1;
    }

    VcsStatus {
        is_git_repo: true,
        branch: Some(branch),
        files,
        summary,
    }
}

/// Queries the current branch name and ahead/behind counts.
fn query_branch_info(project_dir: &Path) -> BranchInfo {
    // Get current branch name.
    let branch_output = Command::new("git")
        .args(["symbolic-ref", "--short", "HEAD"])
        .current_dir(project_dir)
        .output();

    let (name, detached) = match branch_output {
        Ok(ref o) if o.status.success() => {
            let name = String::from_utf8_lossy(&o.stdout).trim().to_string();
            (name, false)
        }
        _ => {
            // Detached HEAD — get the short hash instead.
            let hash = Command::new("git")
                .args(["rev-parse", "--short", "HEAD"])
                .current_dir(project_dir)
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_else(|_| "HEAD".to_string());
            (hash, true)
        }
    };

    // Get ahead/behind counts relative to upstream.
    let (ahead, behind) = query_ahead_behind(project_dir);

    BranchInfo {
        name,
        ahead,
        behind,
        detached,
    }
}

/// Queries ahead/behind counts relative to the upstream tracking branch.
fn query_ahead_behind(project_dir: &Path) -> (u32, u32) {
    let output = Command::new("git")
        .args(["rev-list", "--left-right", "--count", "HEAD...@{upstream}"])
        .current_dir(project_dir)
        .output();

    match output {
        Ok(ref o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            let parts: Vec<&str> = text.trim().split_whitespace().collect();
            if parts.len() == 2 {
                let ahead = parts[0].parse().unwrap_or(0);
                let behind = parts[1].parse().unwrap_or(0);
                (ahead, behind)
            } else {
                (0, 0)
            }
        }
        _ => (0, 0), // No upstream configured.
    }
}

/// Parses `git status --porcelain=v1` output into file status entries.
fn query_file_statuses(project_dir: &Path) -> Vec<VcsFileStatus> {
    let output = Command::new("git")
        .args(["status", "--porcelain=v1"])
        .current_dir(project_dir)
        .output();

    let stdout = match output {
        Ok(ref o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return Vec::new(),
    };

    let mut files = Vec::new();

    for line in stdout.lines() {
        if line.len() < 4 {
            continue;
        }

        let index_status = line.as_bytes()[0];
        let worktree_status = line.as_bytes()[1];
        let path = line[3..].to_string();

        // Handle renamed files: "R  old -> new"
        let display_path = if path.contains(" -> ") {
            path.split(" -> ").last().unwrap_or(&path).to_string()
        } else {
            path.clone()
        };

        // Parse index (staged) changes.
        if let Some(status) = parse_status_char(index_status) {
            files.push(VcsFileStatus {
                path: display_path.clone(),
                status,
                area: ChangeArea::Staged,
            });
        }

        // Parse working tree (unstaged) changes.
        if let Some(status) = parse_status_char(worktree_status) {
            files.push(VcsFileStatus {
                path: display_path.clone(),
                status,
                area: ChangeArea::Unstaged,
            });
        }

        // Untracked files: "??" prefix.
        if index_status == b'?' && worktree_status == b'?' {
            files.push(VcsFileStatus {
                path: display_path,
                status: FileChangeStatus::Untracked,
                area: ChangeArea::Unstaged,
            });
        }
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

/// Maps a single git porcelain status character to a [`FileChangeStatus`].
fn parse_status_char(ch: u8) -> Option<FileChangeStatus> {
    match ch {
        b'M' => Some(FileChangeStatus::Modified),
        b'A' => Some(FileChangeStatus::Added),
        b'D' => Some(FileChangeStatus::Deleted),
        b'R' => Some(FileChangeStatus::Renamed),
        b'C' => Some(FileChangeStatus::Copied),
        b'U' => Some(FileChangeStatus::Conflicted),
        _ => None,
    }
}

/// Retrieves the diff for a specific file (unstaged changes).
pub fn get_file_diff(project_dir: &Path, file_path: &str) -> String {
    let output = Command::new("git")
        .args(["diff", "--", file_path])
        .current_dir(project_dir)
        .output();

    match output {
        Ok(ref o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => String::new(),
    }
}

/// Retrieves the staged diff for a specific file.
pub fn get_file_diff_staged(project_dir: &Path, file_path: &str) -> String {
    let output = Command::new("git")
        .args(["diff", "--cached", "--", file_path])
        .current_dir(project_dir)
        .output();

    match output {
        Ok(ref o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => String::new(),
    }
}

/// Lists the recent commit log (last N commits).
pub fn get_commit_log(project_dir: &Path, max_count: u32) -> Vec<CommitEntry> {
    let output = Command::new("git")
        .args([
            "log",
            &format!("--max-count={}", max_count),
            "--pretty=format:%H%n%h%n%an%n%ae%n%at%n%s",
        ])
        .current_dir(project_dir)
        .output();

    let stdout = match output {
        Ok(ref o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return Vec::new(),
    };

    let mut commits = Vec::new();
    let lines: Vec<&str> = stdout.lines().collect();

    // Each commit is 6 lines.
    for chunk in lines.chunks(6) {
        if chunk.len() < 6 {
            break;
        }
        commits.push(CommitEntry {
            hash: chunk[0].to_string(),
            short_hash: chunk[1].to_string(),
            author_name: chunk[2].to_string(),
            author_email: chunk[3].to_string(),
            timestamp: chunk[4].parse().unwrap_or(0),
            subject: chunk[5].to_string(),
        });
    }

    commits
}

/// A single commit log entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitEntry {
    /// Full commit hash.
    pub hash: String,
    /// Short commit hash.
    pub short_hash: String,
    /// Author name.
    pub author_name: String,
    /// Author email.
    pub author_email: String,
    /// Unix timestamp of the commit.
    pub timestamp: u64,
    /// Commit subject (first line of message).
    pub subject: String,
}

/// Stages a file for commit.
pub fn stage_file(project_dir: &Path, file_path: &str) -> Result<(), String> {
    let output = Command::new("git")
        .args(["add", "--", file_path])
        .current_dir(project_dir)
        .output()
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

/// Unstages a file (removes from index, keeps working tree changes).
pub fn unstage_file(project_dir: &Path, file_path: &str) -> Result<(), String> {
    let output = Command::new("git")
        .args(["restore", "--staged", "--", file_path])
        .current_dir(project_dir)
        .output()
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

/// Discards working tree changes for a file (restores to HEAD).
pub fn discard_changes(project_dir: &Path, file_path: &str) -> Result<(), String> {
    let output = Command::new("git")
        .args(["checkout", "--", file_path])
        .current_dir(project_dir)
        .output()
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Helper to create a temporary git repo for testing.
    fn make_temp_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path();

        // Initialize a git repo.
        Command::new("git")
            .args(["init"])
            .current_dir(path)
            .output()
            .expect("git init");

        // Configure user for commits.
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(path)
            .output()
            .expect("git config email");
        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(path)
            .output()
            .expect("git config name");

        // Create an initial commit so HEAD exists.
        fs::write(path.join("README.md"), "# Test Project\n").expect("write README");
        Command::new("git")
            .args(["add", "README.md"])
            .current_dir(path)
            .output()
            .expect("git add");
        Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(path)
            .output()
            .expect("git commit");

        dir
    }

    #[test]
    fn non_git_directory_returns_not_repo() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let status = query_git_status(dir.path());
        assert!(!status.is_git_repo);
        assert!(status.branch.is_none());
        assert!(status.files.is_empty());
    }

    #[test]
    fn clean_repo_has_no_changes() {
        let repo = make_temp_repo();
        let status = query_git_status(repo.path());
        assert!(status.is_git_repo);
        assert!(status.files.is_empty());
    }

    #[test]
    fn detects_branch_name() {
        let repo = make_temp_repo();
        let status = query_git_status(repo.path());
        let branch = status.branch.as_ref().expect("branch info");
        // Default branch is typically "main" or "master".
        assert!(!branch.name.is_empty());
        assert!(!branch.detached);
    }

    #[test]
    fn detects_modified_file() {
        let repo = make_temp_repo();
        fs::write(repo.path().join("README.md"), "# Modified\n").expect("modify file");

        let status = query_git_status(repo.path());
        assert!(!status.files.is_empty());

        let readme_entry = status
            .files
            .iter()
            .find(|f| f.path == "README.md")
            .expect("README.md in status");
        assert_eq!(readme_entry.status, FileChangeStatus::Modified);
        assert_eq!(readme_entry.area, ChangeArea::Unstaged);
    }

    #[test]
    fn detects_untracked_file() {
        let repo = make_temp_repo();
        fs::write(repo.path().join("newfile.txt"), "hello").expect("write new file");

        let status = query_git_status(repo.path());
        let entry = status
            .files
            .iter()
            .find(|f| f.path == "newfile.txt")
            .expect("newfile.txt in status");
        assert_eq!(entry.status, FileChangeStatus::Untracked);
        assert_eq!(entry.area, ChangeArea::Unstaged);
    }

    #[test]
    fn detects_staged_added_file() {
        let repo = make_temp_repo();
        fs::write(repo.path().join("staged.txt"), "staged content").expect("write file");
        Command::new("git")
            .args(["add", "staged.txt"])
            .current_dir(repo.path())
            .output()
            .expect("git add");

        let status = query_git_status(repo.path());
        let entry = status
            .files
            .iter()
            .find(|f| f.path == "staged.txt" && f.area == ChangeArea::Staged)
            .expect("staged.txt staged");
        assert_eq!(entry.status, FileChangeStatus::Added);
    }

    #[test]
    fn detects_deleted_file() {
        let repo = make_temp_repo();
        fs::remove_file(repo.path().join("README.md")).expect("delete file");

        let status = query_git_status(repo.path());
        let entry = status
            .files
            .iter()
            .find(|f| f.path == "README.md")
            .expect("README.md in status");
        assert_eq!(entry.status, FileChangeStatus::Deleted);
    }

    #[test]
    fn summary_counts_correct() {
        let repo = make_temp_repo();
        fs::write(repo.path().join("README.md"), "# Modified\n").expect("modify");
        fs::write(repo.path().join("new1.txt"), "new1").expect("new1");
        fs::write(repo.path().join("new2.txt"), "new2").expect("new2");

        let status = query_git_status(repo.path());
        // Should have at least 1 modified + 2 untracked.
        let untracked_count = status.summary.get("untracked").copied().unwrap_or(0);
        let modified_count = status.summary.get("modified").copied().unwrap_or(0);
        assert!(untracked_count >= 2, "expected >= 2 untracked, got {}", untracked_count);
        assert!(modified_count >= 1, "expected >= 1 modified, got {}", modified_count);
    }

    #[test]
    fn get_file_diff_returns_content() {
        let repo = make_temp_repo();
        fs::write(repo.path().join("README.md"), "# Modified Content\n").expect("modify");

        let diff = get_file_diff(repo.path(), "README.md");
        assert!(diff.contains("Modified Content"), "diff should contain the change");
    }

    #[test]
    fn get_commit_log_returns_entries() {
        let repo = make_temp_repo();
        let commits = get_commit_log(repo.path(), 10);
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].subject, "Initial commit");
        assert_eq!(commits[0].author_name, "Test User");
        assert!(!commits[0].hash.is_empty());
        assert!(!commits[0].short_hash.is_empty());
    }

    #[test]
    fn stage_and_unstage_file() {
        let repo = make_temp_repo();
        fs::write(repo.path().join("test.txt"), "content").expect("write");

        // Stage the file.
        stage_file(repo.path(), "test.txt").expect("stage");
        let status = query_git_status(repo.path());
        assert!(
            status
                .files
                .iter()
                .any(|f| f.path == "test.txt" && f.area == ChangeArea::Staged),
            "file should be staged"
        );

        // Unstage the file.
        unstage_file(repo.path(), "test.txt").expect("unstage");
        let status = query_git_status(repo.path());
        assert!(
            !status
                .files
                .iter()
                .any(|f| f.path == "test.txt" && f.area == ChangeArea::Staged),
            "file should not be staged after unstage"
        );
    }

    #[test]
    fn discard_changes_restores_file() {
        let repo = make_temp_repo();
        let original = fs::read_to_string(repo.path().join("README.md")).expect("read original");
        fs::write(repo.path().join("README.md"), "# Totally Changed\n").expect("modify");

        discard_changes(repo.path(), "README.md").expect("discard");
        let restored = fs::read_to_string(repo.path().join("README.md")).expect("read restored");
        assert_eq!(restored, original);
    }

    #[test]
    fn parse_status_char_mapping() {
        assert_eq!(parse_status_char(b'M'), Some(FileChangeStatus::Modified));
        assert_eq!(parse_status_char(b'A'), Some(FileChangeStatus::Added));
        assert_eq!(parse_status_char(b'D'), Some(FileChangeStatus::Deleted));
        assert_eq!(parse_status_char(b'R'), Some(FileChangeStatus::Renamed));
        assert_eq!(parse_status_char(b'C'), Some(FileChangeStatus::Copied));
        assert_eq!(parse_status_char(b'U'), Some(FileChangeStatus::Conflicted));
        assert_eq!(parse_status_char(b' '), None);
        assert_eq!(parse_status_char(b'?'), None);
    }

    #[test]
    fn multiple_commits_in_log() {
        let repo = make_temp_repo();

        // Add a second commit.
        fs::write(repo.path().join("second.txt"), "second").expect("write");
        Command::new("git")
            .args(["add", "second.txt"])
            .current_dir(repo.path())
            .output()
            .expect("git add");
        Command::new("git")
            .args(["commit", "-m", "Second commit"])
            .current_dir(repo.path())
            .output()
            .expect("git commit");

        let commits = get_commit_log(repo.path(), 10);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].subject, "Second commit");
        assert_eq!(commits[1].subject, "Initial commit");
    }

    #[test]
    fn staged_and_unstaged_same_file() {
        let repo = make_temp_repo();

        // Modify README and stage it.
        fs::write(repo.path().join("README.md"), "# Staged version\n").expect("write");
        Command::new("git")
            .args(["add", "README.md"])
            .current_dir(repo.path())
            .output()
            .expect("git add");

        // Modify README again (unstaged change on top of staged).
        fs::write(repo.path().join("README.md"), "# Both staged and unstaged\n").expect("write");

        let status = query_git_status(repo.path());
        let staged = status
            .files
            .iter()
            .any(|f| f.path == "README.md" && f.area == ChangeArea::Staged);
        let unstaged = status
            .files
            .iter()
            .any(|f| f.path == "README.md" && f.area == ChangeArea::Unstaged);
        assert!(staged, "README.md should have staged change");
        assert!(unstaged, "README.md should have unstaged change");
    }
}
