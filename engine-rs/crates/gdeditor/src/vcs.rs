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
// Stash operations
// ---------------------------------------------------------------------------

/// A single stash entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StashEntry {
    /// Stash index (0, 1, 2, ...).
    pub index: u32,
    /// Stash message/description.
    pub message: String,
}

/// Lists all stash entries.
pub fn list_stashes(project_dir: &Path) -> Vec<StashEntry> {
    let output = Command::new("git")
        .args(["stash", "list", "--format=%gd\t%gs"])
        .current_dir(project_dir)
        .output();

    let stdout = match output {
        Ok(ref o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return Vec::new(),
    };

    stdout
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(2, '\t').collect();
            if parts.len() < 2 {
                return None;
            }
            // Parse "stash@{N}" → N
            let idx_str = parts[0]
                .strip_prefix("stash@{")
                .and_then(|s| s.strip_suffix('}'))?;
            let index = idx_str.parse().ok()?;
            Some(StashEntry {
                index,
                message: parts[1].to_string(),
            })
        })
        .collect()
}

/// Creates a new stash with an optional message.
pub fn stash_push(project_dir: &Path, message: Option<&str>) -> Result<(), String> {
    let mut args = vec!["stash", "push"];
    if let Some(msg) = message {
        args.push("-m");
        args.push(msg);
    }
    let output = Command::new("git")
        .args(&args)
        .current_dir(project_dir)
        .output()
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

/// Pops the top stash entry.
pub fn stash_pop(project_dir: &Path) -> Result<(), String> {
    let output = Command::new("git")
        .args(["stash", "pop"])
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
// Branch operations
// ---------------------------------------------------------------------------

/// Lists local branches. Returns (name, is_current) pairs.
pub fn list_branches(project_dir: &Path) -> Vec<(String, bool)> {
    let output = Command::new("git")
        .args(["branch", "--format=%(HEAD)\t%(refname:short)"])
        .current_dir(project_dir)
        .output();

    let stdout = match output {
        Ok(ref o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return Vec::new(),
    };

    stdout
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(2, '\t').collect();
            if parts.len() < 2 {
                return None;
            }
            let is_current = parts[0].trim() == "*";
            Some((parts[1].to_string(), is_current))
        })
        .collect()
}

/// Switches to the given branch.
pub fn switch_branch(project_dir: &Path, branch_name: &str) -> Result<(), String> {
    let output = Command::new("git")
        .args(["switch", branch_name])
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
// Commit creation
// ---------------------------------------------------------------------------

/// Creates a commit with the given message from currently staged files.
pub fn create_commit(project_dir: &Path, message: &str) -> Result<String, String> {
    let output = Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(project_dir)
        .output()
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        // Return the new commit hash.
        let hash_output = Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .current_dir(project_dir)
            .output()
            .map_err(|e| e.to_string())?;
        Ok(String::from_utf8_lossy(&hash_output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

// ---------------------------------------------------------------------------
// VcsPanel — stateful editor panel
// ---------------------------------------------------------------------------

/// Filter mode for the VCS panel file list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VcsPanelFilter {
    /// Show all changed files.
    All,
    /// Show only staged files.
    Staged,
    /// Show only unstaged files.
    Unstaged,
    /// Show only files with a specific status.
    Status(FileChangeStatus),
}

/// The editor VCS panel state — caches git status and provides UI interaction.
#[derive(Debug)]
pub struct VcsPanel {
    /// Cached VCS status.
    status: VcsStatus,
    /// Currently selected file index in the filtered view.
    selected_index: Option<usize>,
    /// Current filter mode.
    filter: VcsPanelFilter,
    /// Commit message being composed.
    commit_message: String,
    /// Whether the panel is expanded/visible.
    visible: bool,
}

impl Default for VcsPanel {
    fn default() -> Self {
        Self {
            status: VcsStatus::default(),
            selected_index: None,
            filter: VcsPanelFilter::All,
            commit_message: String::new(),
            visible: true,
        }
    }
}

impl VcsPanel {
    /// Creates a new VCS panel.
    pub fn new() -> Self {
        Self::default()
    }

    /// Refreshes the cached status by querying git.
    pub fn refresh(&mut self, project_dir: &Path) {
        self.status = query_git_status(project_dir);
        // Clamp selection if files changed.
        if let Some(idx) = self.selected_index {
            if idx >= self.filtered_files().len() {
                self.selected_index = None;
            }
        }
    }

    /// Returns the cached VCS status.
    pub fn status(&self) -> &VcsStatus {
        &self.status
    }

    /// Sets the filter mode.
    pub fn set_filter(&mut self, filter: VcsPanelFilter) {
        self.filter = filter;
        self.selected_index = None;
    }

    /// Returns the current filter.
    pub fn filter(&self) -> VcsPanelFilter {
        self.filter
    }

    /// Returns files matching the current filter.
    pub fn filtered_files(&self) -> Vec<&VcsFileStatus> {
        self.status
            .files
            .iter()
            .filter(|f| match self.filter {
                VcsPanelFilter::All => true,
                VcsPanelFilter::Staged => f.area == ChangeArea::Staged,
                VcsPanelFilter::Unstaged => f.area == ChangeArea::Unstaged,
                VcsPanelFilter::Status(s) => f.status == s,
            })
            .collect()
    }

    /// Returns the count of filtered files.
    pub fn filtered_count(&self) -> usize {
        self.filtered_files().len()
    }

    /// Selects a file by index in the filtered view.
    pub fn select(&mut self, index: usize) {
        let count = self.filtered_files().len();
        if index < count {
            self.selected_index = Some(index);
        }
    }

    /// Moves selection up.
    pub fn select_prev(&mut self) {
        if let Some(idx) = self.selected_index {
            if idx > 0 {
                self.selected_index = Some(idx - 1);
            }
        } else if !self.filtered_files().is_empty() {
            self.selected_index = Some(0);
        }
    }

    /// Moves selection down.
    pub fn select_next(&mut self) {
        let count = self.filtered_files().len();
        if count == 0 {
            return;
        }
        if let Some(idx) = self.selected_index {
            if idx + 1 < count {
                self.selected_index = Some(idx + 1);
            }
        } else {
            self.selected_index = Some(0);
        }
    }

    /// Returns the currently selected file, if any.
    pub fn selected_file(&self) -> Option<&VcsFileStatus> {
        let files = self.filtered_files();
        self.selected_index.and_then(|i| files.get(i).copied())
    }

    /// Returns the selected index.
    pub fn selected_index(&self) -> Option<usize> {
        self.selected_index
    }

    /// Sets the commit message.
    pub fn set_commit_message(&mut self, msg: impl Into<String>) {
        self.commit_message = msg.into();
    }

    /// Returns the current commit message.
    pub fn commit_message(&self) -> &str {
        &self.commit_message
    }

    /// Returns the number of staged files.
    pub fn staged_count(&self) -> usize {
        self.status
            .files
            .iter()
            .filter(|f| f.area == ChangeArea::Staged)
            .count()
    }

    /// Returns the number of unstaged files.
    pub fn unstaged_count(&self) -> usize {
        self.status
            .files
            .iter()
            .filter(|f| f.area == ChangeArea::Unstaged)
            .count()
    }

    /// Returns the total change count (for badge display).
    pub fn change_count(&self) -> usize {
        self.status.files.len()
    }

    /// Returns a short status string for the editor status bar.
    pub fn status_bar_text(&self) -> String {
        if !self.status.is_git_repo {
            return "No VCS".to_string();
        }
        let branch = self
            .status
            .branch
            .as_ref()
            .map(|b| b.name.as_str())
            .unwrap_or("?");
        let changes = self.status.files.len();
        if changes > 0 {
            format!("{} ({} changes)", branch, changes)
        } else {
            branch.to_string()
        }
    }

    /// Toggles panel visibility.
    pub fn toggle_visible(&mut self) {
        self.visible = !self.visible;
    }

    /// Returns whether the panel is visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Loads status from a pre-built VcsStatus (for testing without git).
    pub fn load_status(&mut self, status: VcsStatus) {
        self.status = status;
        self.selected_index = None;
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

    // ── Stash operations ──────────────────────────────────────────

    #[test]
    fn stash_push_and_pop() {
        let repo = make_temp_repo();
        fs::write(repo.path().join("README.md"), "# Changed\n").expect("modify");

        stash_push(repo.path(), Some("test stash")).expect("stash push");

        // File should be restored after stash.
        let content = fs::read_to_string(repo.path().join("README.md")).expect("read");
        assert_eq!(content, "# Test Project\n");

        let stashes = list_stashes(repo.path());
        assert_eq!(stashes.len(), 1);
        assert!(stashes[0].message.contains("test stash"));

        stash_pop(repo.path()).expect("stash pop");
        let content = fs::read_to_string(repo.path().join("README.md")).expect("read");
        assert_eq!(content, "# Changed\n");
    }

    #[test]
    fn list_stashes_empty() {
        let repo = make_temp_repo();
        let stashes = list_stashes(repo.path());
        assert!(stashes.is_empty());
    }

    // ── Branch operations ────────────────────────────────────────

    #[test]
    fn list_branches_includes_current() {
        let repo = make_temp_repo();
        let branches = list_branches(repo.path());
        assert!(!branches.is_empty());
        assert!(branches.iter().any(|(_, is_current)| *is_current));
    }

    #[test]
    fn switch_branch_works() {
        let repo = make_temp_repo();
        // Create a new branch.
        Command::new("git")
            .args(["branch", "feature-test"])
            .current_dir(repo.path())
            .output()
            .expect("create branch");

        switch_branch(repo.path(), "feature-test").expect("switch");

        let branches = list_branches(repo.path());
        let current = branches.iter().find(|(_, is_current)| *is_current);
        assert_eq!(current.unwrap().0, "feature-test");
    }

    // ── Commit creation ──────────────────────────────────────────

    #[test]
    fn create_commit_returns_hash() {
        let repo = make_temp_repo();
        fs::write(repo.path().join("new.txt"), "content").expect("write");
        Command::new("git")
            .args(["add", "new.txt"])
            .current_dir(repo.path())
            .output()
            .expect("git add");

        let hash = create_commit(repo.path(), "Test commit").expect("commit");
        assert!(!hash.is_empty());

        let commits = get_commit_log(repo.path(), 1);
        assert_eq!(commits[0].subject, "Test commit");
    }

    #[test]
    fn create_commit_fails_with_nothing_staged() {
        let repo = make_temp_repo();
        let result = create_commit(repo.path(), "Empty commit");
        assert!(result.is_err());
    }

    // ── VcsPanel ─────────────────────────────────────────────────

    fn make_test_status() -> VcsStatus {
        VcsStatus {
            is_git_repo: true,
            branch: Some(BranchInfo {
                name: "main".to_string(),
                ahead: 1,
                behind: 0,
                detached: false,
            }),
            files: vec![
                VcsFileStatus {
                    path: "src/main.rs".to_string(),
                    status: FileChangeStatus::Modified,
                    area: ChangeArea::Staged,
                },
                VcsFileStatus {
                    path: "src/lib.rs".to_string(),
                    status: FileChangeStatus::Modified,
                    area: ChangeArea::Unstaged,
                },
                VcsFileStatus {
                    path: "new.txt".to_string(),
                    status: FileChangeStatus::Untracked,
                    area: ChangeArea::Unstaged,
                },
                VcsFileStatus {
                    path: "deleted.rs".to_string(),
                    status: FileChangeStatus::Deleted,
                    area: ChangeArea::Staged,
                },
            ],
            summary: HashMap::new(),
        }
    }

    #[test]
    fn panel_default() {
        let panel = VcsPanel::new();
        assert!(panel.is_visible());
        assert!(panel.selected_index().is_none());
        assert_eq!(panel.filter(), VcsPanelFilter::All);
        assert!(panel.commit_message().is_empty());
    }

    #[test]
    fn panel_load_and_filter_all() {
        let mut panel = VcsPanel::new();
        panel.load_status(make_test_status());

        assert_eq!(panel.filtered_count(), 4);
        assert_eq!(panel.change_count(), 4);
    }

    #[test]
    fn panel_filter_staged() {
        let mut panel = VcsPanel::new();
        panel.load_status(make_test_status());
        panel.set_filter(VcsPanelFilter::Staged);

        assert_eq!(panel.filtered_count(), 2);
        assert!(panel
            .filtered_files()
            .iter()
            .all(|f| f.area == ChangeArea::Staged));
    }

    #[test]
    fn panel_filter_unstaged() {
        let mut panel = VcsPanel::new();
        panel.load_status(make_test_status());
        panel.set_filter(VcsPanelFilter::Unstaged);

        assert_eq!(panel.filtered_count(), 2);
    }

    #[test]
    fn panel_filter_by_status() {
        let mut panel = VcsPanel::new();
        panel.load_status(make_test_status());
        panel.set_filter(VcsPanelFilter::Status(FileChangeStatus::Modified));

        assert_eq!(panel.filtered_count(), 2);
    }

    #[test]
    fn panel_selection_navigation() {
        let mut panel = VcsPanel::new();
        panel.load_status(make_test_status());

        assert!(panel.selected_index().is_none());
        panel.select_next();
        assert_eq!(panel.selected_index(), Some(0));

        panel.select_next();
        assert_eq!(panel.selected_index(), Some(1));

        panel.select_prev();
        assert_eq!(panel.selected_index(), Some(0));

        panel.select_prev(); // Can't go below 0
        assert_eq!(panel.selected_index(), Some(0));
    }

    #[test]
    fn panel_select_by_index() {
        let mut panel = VcsPanel::new();
        panel.load_status(make_test_status());

        panel.select(2);
        assert_eq!(panel.selected_index(), Some(2));
        let file = panel.selected_file().unwrap();
        assert_eq!(file.path, "new.txt");
    }

    #[test]
    fn panel_select_out_of_bounds() {
        let mut panel = VcsPanel::new();
        panel.load_status(make_test_status());

        panel.select(100);
        assert!(panel.selected_index().is_none());
    }

    #[test]
    fn panel_staged_unstaged_counts() {
        let mut panel = VcsPanel::new();
        panel.load_status(make_test_status());

        assert_eq!(panel.staged_count(), 2);
        assert_eq!(panel.unstaged_count(), 2);
    }

    #[test]
    fn panel_commit_message() {
        let mut panel = VcsPanel::new();
        panel.set_commit_message("Fix bug #123");
        assert_eq!(panel.commit_message(), "Fix bug #123");
    }

    #[test]
    fn panel_status_bar_text() {
        let mut panel = VcsPanel::new();
        panel.load_status(make_test_status());

        let text = panel.status_bar_text();
        assert!(text.contains("main"));
        assert!(text.contains("4 changes"));
    }

    #[test]
    fn panel_status_bar_no_vcs() {
        let panel = VcsPanel::new();
        assert_eq!(panel.status_bar_text(), "No VCS");
    }

    #[test]
    fn panel_status_bar_clean() {
        let mut panel = VcsPanel::new();
        panel.load_status(VcsStatus {
            is_git_repo: true,
            branch: Some(BranchInfo {
                name: "develop".to_string(),
                ahead: 0,
                behind: 0,
                detached: false,
            }),
            files: vec![],
            summary: HashMap::new(),
        });
        assert_eq!(panel.status_bar_text(), "develop");
    }

    #[test]
    fn panel_toggle_visible() {
        let mut panel = VcsPanel::new();
        assert!(panel.is_visible());
        panel.toggle_visible();
        assert!(!panel.is_visible());
        panel.toggle_visible();
        assert!(panel.is_visible());
    }

    #[test]
    fn panel_filter_resets_selection() {
        let mut panel = VcsPanel::new();
        panel.load_status(make_test_status());
        panel.select(2);
        assert_eq!(panel.selected_index(), Some(2));

        panel.set_filter(VcsPanelFilter::Staged);
        assert!(panel.selected_index().is_none());
    }

    #[test]
    fn panel_select_next_on_empty() {
        let mut panel = VcsPanel::new();
        panel.select_next();
        assert!(panel.selected_index().is_none());
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
