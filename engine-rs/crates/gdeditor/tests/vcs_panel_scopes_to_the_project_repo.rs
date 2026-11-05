//! pat-24o1e: the Version Control panel shows the repository of the OPEN
//! PROJECT, not the repository the editor process was launched from.
//!
//! Godot's VCS plugin discovers the repository upward from the project path
//! (godot-git-plugin `GitPlugin::_initialize`:
//! `git_repository_discover(project_path, across_fs=1)`), and lists the
//! repository's status with repo-relative paths. Patina ran every git command
//! in the process cwd, so with this test's cwd (the Patina checkout) the panel
//! showed Patina's branch and files whatever project was open.
//!
//! The fixture puts the project in a SUBFOLDER of its own repository, which is
//! also the case Godot handles by discovery: the diff/stage routes must accept
//! the repo-relative paths that status lists.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn http(port: u16, req: &str) -> serde_json::Value {
    let mut stream = None;
    for _ in 0..40 {
        if let Ok(s) = TcpStream::connect(("127.0.0.1", port)) {
            stream = Some(s);
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let mut s = stream.expect("editor server never accepted");
    s.set_read_timeout(Some(Duration::from_secs(10))).ok();
    s.write_all(req.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = s.read_to_end(&mut resp);
    let resp = String::from_utf8_lossy(&resp).to_string();
    let body = resp.split("\r\n\r\n").nth(1).unwrap_or("");
    serde_json::from_str(body).unwrap_or_else(|e| panic!("not JSON ({e}): {resp}"))
}

fn get(port: u16, path: &str) -> serde_json::Value {
    http(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"),
    )
}

fn post(port: u16, path: &str, body: &str) -> serde_json::Value {
    http(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

#[test]
fn vcs_routes_use_the_repository_that_contains_the_project() {
    let repo = std::env::temp_dir().join(format!("patina_vcs_scope_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&repo);
    let project = repo.join("my_game");
    std::fs::create_dir_all(&project).unwrap();
    git(&repo, &["init", "-q", "-b", "vcs-scope-branch"]);
    git(&repo, &["config", "user.email", "t@example.com"]);
    git(&repo, &["config", "user.name", "T"]);
    std::fs::write(project.join("project.godot"), "config_version=5\n").unwrap();
    std::fs::write(project.join("player.gd"), "extends Node\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init", "--no-gpg-sign"]);
    std::fs::write(project.join("player.gd"), "extends Node2D\n").unwrap();

    // The process cwd is NOT in that repository (it is the Patina checkout,
    // or no repository at all), so only project-rooted discovery finds it.
    let cwd = std::env::current_dir().unwrap();
    assert!(!cwd.starts_with(&repo));

    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Root", "Node2D")).unwrap();
    let mut state = EditorState::new(tree);
    state.project_root = Some(project.to_string_lossy().to_string());
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let _h = EditorServerHandle::start(port, state);

    let status = get(port, "/api/vcs/status");
    assert_eq!(status["is_git_repo"], true, "{status}");
    assert_eq!(
        status["branch"]["name"], "vcs-scope-branch",
        "branch of the project's repo: {status}"
    );
    let files: Vec<(String, String)> = status["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["path"].as_str().unwrap().to_string(),
                f["area"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    // Repo-relative, like godot-git-plugin's `git_status_entry` paths.
    assert_eq!(
        files,
        vec![("my_game/player.gd".to_string(), "unstaged".to_string())]
    );

    // The path status listed is the path diff and stage accept.
    let diff = get(port, "/api/vcs/diff?file=my_game/player.gd");
    assert!(
        diff["diff"].as_str().unwrap().contains("+extends Node2D"),
        "{diff}"
    );
    let staged = post(port, "/api/vcs/stage", r#"{"file":"my_game/player.gd"}"#);
    assert_eq!(staged["ok"], true, "{staged}");
    let after = get(port, "/api/vcs/status");
    assert_eq!(after["files"][0]["area"], "staged", "{after}");

    let _ = std::fs::remove_dir_all(&repo);
}
