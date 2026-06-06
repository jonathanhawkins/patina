//! **Version-control integration** model (pat-cqy8t).
//!
//! A headless model of the editor's VCS interface: a working tree, a staging
//! index, and a commit history. The user edits files, stages/unstages changes,
//! commits (advancing HEAD), and inspects status and diffs that reflect the
//! working tree. Branches can be listed, created, and checked out.
//!
//! This is an in-memory model (not a real Git backend), sufficient to drive the
//! editor's VCS panel.

use std::collections::{BTreeMap, BTreeSet};

/// A recorded commit: an id, message, and the snapshot it captured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// Stable commit id.
    pub id: u64,
    /// Commit message.
    pub message: String,
    /// The file snapshot at this commit.
    pub snapshot: BTreeMap<String, String>,
}

/// An in-memory repository: working tree, index, and history.
#[derive(Debug, Clone)]
pub struct Repo {
    head: BTreeMap<String, String>,
    index: BTreeMap<String, String>,
    working: BTreeMap<String, String>,
    commits: Vec<Commit>,
    branches: Vec<String>,
    current_branch: String,
    next_id: u64,
}

impl Default for Repo {
    fn default() -> Self {
        Self::new()
    }
}

impl Repo {
    /// Creates an empty repository on branch `main`.
    pub fn new() -> Self {
        Self {
            head: BTreeMap::new(),
            index: BTreeMap::new(),
            working: BTreeMap::new(),
            commits: Vec::new(),
            branches: vec!["main".to_string()],
            current_branch: "main".to_string(),
            next_id: 1,
        }
    }

    /// Writes (or overwrites) a file in the working tree.
    pub fn write_file(&mut self, path: impl Into<String>, content: impl Into<String>) {
        self.working.insert(path.into(), content.into());
    }

    /// Deletes a file from the working tree.
    pub fn delete_file(&mut self, path: &str) {
        self.working.remove(path);
    }

    /// The content of a file in the working tree.
    pub fn read_file(&self, path: &str) -> Option<&str> {
        self.working.get(path).map(String::as_str)
    }

    /// Paths whose working-tree content differs from the index (unstaged).
    pub fn unstaged_paths(&self) -> Vec<String> {
        changed_paths(&self.working, &self.index)
    }

    /// Paths whose index content differs from HEAD (staged for commit).
    pub fn staged_paths(&self) -> Vec<String> {
        changed_paths(&self.index, &self.head)
    }

    fn known(&self, path: &str) -> bool {
        self.working.contains_key(path)
            || self.index.contains_key(path)
            || self.head.contains_key(path)
    }

    /// Stages the working-tree state of `path` into the index. Returns whether
    /// the path is known to the repo.
    pub fn stage(&mut self, path: &str) -> bool {
        if !self.known(path) {
            return false;
        }
        match self.working.get(path) {
            Some(content) => {
                self.index.insert(path.to_string(), content.clone());
            }
            None => {
                self.index.remove(path); // staging a deletion
            }
        }
        true
    }

    /// Unstages `path`, reverting the index entry back to HEAD. Returns whether
    /// the path is known.
    pub fn unstage(&mut self, path: &str) -> bool {
        if !self.known(path) {
            return false;
        }
        match self.head.get(path) {
            Some(content) => {
                self.index.insert(path.to_string(), content.clone());
            }
            None => {
                self.index.remove(path);
            }
        }
        true
    }

    /// Commits the staged changes with `message`, advancing HEAD. Returns the
    /// new commit id, or `None` if there is nothing staged.
    pub fn commit(&mut self, message: impl Into<String>) -> Option<u64> {
        if self.staged_paths().is_empty() {
            return None;
        }
        self.head = self.index.clone();
        let id = self.next_id;
        self.next_id += 1;
        self.commits.push(Commit {
            id,
            message: message.into(),
            snapshot: self.head.clone(),
        });
        Some(id)
    }

    /// The commit history, oldest first.
    pub fn history(&self) -> &[Commit] {
        &self.commits
    }

    /// A line diff of `path` between HEAD and the working tree (`-` removed,
    /// `+` added). Empty when there's no change.
    pub fn diff(&self, path: &str) -> Vec<String> {
        let head = self.head.get(path);
        let work = self.working.get(path);
        if head.map(String::as_str) == work.map(String::as_str) {
            return Vec::new();
        }
        let head_lines: Vec<&str> = head.map(|s| s.lines().collect()).unwrap_or_default();
        let work_lines: Vec<&str> = work.map(|s| s.lines().collect()).unwrap_or_default();
        let mut out = Vec::new();
        for l in &head_lines {
            if !work_lines.contains(l) {
                out.push(format!("-{}", l));
            }
        }
        for l in &work_lines {
            if !head_lines.contains(l) {
                out.push(format!("+{}", l));
            }
        }
        out
    }

    /// The branch names.
    pub fn branches(&self) -> &[String] {
        &self.branches
    }

    /// The current branch.
    pub fn current_branch(&self) -> &str {
        &self.current_branch
    }

    /// Creates a branch named `name` (no-op if it exists). Returns whether it
    /// was created.
    pub fn create_branch(&mut self, name: impl Into<String>) -> bool {
        let name = name.into();
        if self.branches.contains(&name) {
            return false;
        }
        self.branches.push(name);
        true
    }

    /// Checks out the branch `name`. Returns whether it exists.
    pub fn checkout(&mut self, name: &str) -> bool {
        if self.branches.iter().any(|b| b == name) {
            self.current_branch = name.to_string();
            true
        } else {
            false
        }
    }
}

/// Paths where `a` and `b` differ (present-in-one or different content).
fn changed_paths(a: &BTreeMap<String, String>, b: &BTreeMap<String, String>) -> Vec<String> {
    let keys: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    keys.into_iter()
        .filter(|k| a.get(*k) != b.get(*k))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-cqy8t): staging changes and committing creates a commit
    /// and the status/diff views reflect the working tree.
    #[test]
    fn systems_vcs_stage_commit_and_diff() {
        let mut repo = Repo::new();
        assert_eq!(repo.current_branch(), "main");
        assert!(repo.staged_paths().is_empty());

        // A new file shows up as an unstaged change.
        repo.write_file("src/main.gd", "func _ready():\n\tpass\n");
        assert_eq!(repo.unstaged_paths(), vec!["src/main.gd"]);
        assert!(repo.staged_paths().is_empty());

        // Staging moves it to the index.
        assert!(repo.stage("src/main.gd"));
        assert_eq!(repo.staged_paths(), vec!["src/main.gd"]);
        assert!(repo.unstaged_paths().is_empty());

        // Committing creates a commit and clears the staging area.
        let c1 = repo.commit("initial").expect("commit created");
        assert_eq!(repo.history().len(), 1);
        assert_eq!(repo.history()[0].message, "initial");
        assert!(repo.staged_paths().is_empty());
        assert!(repo.unstaged_paths().is_empty());
        // Committing with nothing staged does nothing.
        assert!(repo.commit("empty").is_none());

        // Editing the file produces an unstaged modification and a diff.
        repo.write_file("src/main.gd", "func _ready():\n\tprint(\"hi\")\n");
        assert_eq!(repo.unstaged_paths(), vec!["src/main.gd"]);
        let diff = repo.diff("src/main.gd");
        assert!(diff.iter().any(|l| l == "-\tpass"));
        assert!(diff.iter().any(|l| l == "+\tprint(\"hi\")"));

        // Stage and commit again — a new distinct commit.
        repo.stage("src/main.gd");
        let c2 = repo.commit("update").unwrap();
        assert_ne!(c1, c2);
        assert_eq!(repo.history().len(), 2);
        // After committing, the working tree matches HEAD → empty diff.
        assert!(repo.diff("src/main.gd").is_empty());

        // Branches can be listed, created, and checked out.
        assert_eq!(repo.branches(), ["main"]);
        assert!(repo.create_branch("feature"));
        assert!(!repo.create_branch("feature")); // already exists
        assert!(repo.checkout("feature"));
        assert_eq!(repo.current_branch(), "feature");
        assert!(!repo.checkout("nope"));

        // Unstage reverts a staged change back to an unstaged one.
        repo.write_file("notes.txt", "todo");
        repo.stage("notes.txt");
        assert_eq!(repo.staged_paths(), vec!["notes.txt"]);
        assert!(repo.unstage("notes.txt"));
        assert!(repo.staged_paths().is_empty());
        assert_eq!(repo.unstaged_paths(), vec!["notes.txt"]);
    }
}
