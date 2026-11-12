//! pat-u3l8c.113 — mounting a `.pck` over `res://` from bytes already in
//! memory, which is how the web runtime serves a project.
//!
//! On `wasm32-unknown-unknown` there is no `std::fs`: the browser `fetch`es the
//! `.pck` over HTTP and hands back an `ArrayBuffer`. Every mount entry point
//! took a *path* and read it through `FileAccess`, so no amount of fetched bytes
//! could be mounted — the web had no way to serve `res://` at all.
//!
//! [`load_resource_pack_from_bytes`] is that entry point, and
//! [`load_resource_pack`] is now a thin wrapper over it so the path and bytes
//! forms cannot drift apart.
//!
//! These tests deliberately run with **no project root configured**, which is
//! the browser's situation exactly: if anything still reached for the
//! filesystem, `res://` resolution would fail rather than silently pass off a
//! loose file on disk.

use gdcore::file_access::FileAccess;
use gdresource::pck::{
    load_resource_pack, load_resource_pack_from_bytes, unmount_resource_pack, PckPacker,
};

/// `res://` roots and pack mounts are process-global, so these tests must not
/// interleave. (`cargo nextest` runs each test in its own process, but
/// `cargo test` does not, and this file must be correct under both.)
static MOUNT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

const MAIN_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Texture2D" path="res://icon.png" id="1_icon"]

[node name="Main" type="Node2D"]

[node name="Art" type="Sprite2D" parent="."]
texture = ExtResource("1_icon")
"#;

const ICON_BYTES: &[u8] = b"\x89PNG\r\n\x1a\nFAKE-ICON";

/// The bytes a web host would receive from `fetch("/game.pck")`.
fn served_pck() -> Vec<u8> {
    let mut packer = PckPacker::new();
    packer.add_file("res://project.godot", b"config_version=5\n".to_vec());
    packer.add_file("res://main.tscn", MAIN_TSCN.as_bytes().to_vec());
    packer.add_file("res://icon.png", ICON_BYTES.to_vec());
    packer
        .pack()
        .expect("packing a project in memory must succeed")
}

/// Starts every test from the browser's situation: nothing mounted, and — the
/// part that makes these assertions mean something — **no project root**, so
/// there is no filesystem for `res://` to fall back to.
fn reset_to_web_conditions() {
    gdcore::file_access::clear_mounted_packs();
    gdcore::file_access::set_path_roots(None, None);
}

// ---------------------------------------------------------------------------
// The bead's acceptance: a fetched pack serves res:// with no filesystem
// ---------------------------------------------------------------------------

/// **The acceptance test for the `res://` half.** Bytes that arrived over HTTP
/// mount over `res://`, and reads resolve out of them with no project root and
/// nothing on disk.
#[test]
fn fetched_pck_bytes_serve_res_paths_without_a_filesystem() {
    let _guard = MOUNT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset_to_web_conditions();

    // Nothing is mounted yet, and there is no project root: `res://` cannot
    // resolve. This is the assertion that proves the ones below are served by
    // the pack rather than by a stray file on disk.
    assert!(
        !FileAccess::file_exists("res://main.tscn"),
        "with no pack and no project root, res:// must not resolve",
    );

    load_resource_pack_from_bytes(
        "web",
        "https://example.test/game.pck",
        served_pck(),
        true,
        0,
    )
    .expect("fetched .pck bytes must mount over res://");

    assert!(FileAccess::file_exists("res://main.tscn"));
    assert!(FileAccess::file_exists("res://icon.png"));
    assert!(
        !FileAccess::file_exists("res://not-in-the-pack.tres"),
        "a path the pack does not carry must still report missing",
    );

    // Contents, not just presence — a mount that reported `has_file` but
    // returned empty bytes would pass a presence-only test.
    assert_eq!(
        FileAccess::read_file_string("res://main.tscn").expect("scene text reads from the pack"),
        MAIN_TSCN,
    );
    assert_eq!(
        FileAccess::read_file_bytes("res://icon.png").expect("binary reads from the pack"),
        ICON_BYTES,
    );

    // Unmounting takes `res://` away again, which is what makes the mount the
    // actual source of those reads.
    assert!(unmount_resource_pack("web"));
    assert!(!FileAccess::file_exists("res://main.tscn"));
    assert!(FileAccess::read_file_bytes("res://main.tscn").is_err());
}

// ---------------------------------------------------------------------------
// Supporting behaviour
// ---------------------------------------------------------------------------

/// A pack embedded after another payload mounts at an offset — the shape a web
/// export takes when the `.pck` is appended to a wasm/JS shell rather than
/// served as its own file.
#[test]
fn embedded_pck_mounts_at_an_offset() {
    let _guard = MOUNT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset_to_web_conditions();

    let prefix = b"JS-SHELL-PAYLOAD-";
    let mut embedded = prefix.to_vec();
    embedded.extend_from_slice(&served_pck());

    load_resource_pack_from_bytes(
        "embedded",
        "https://example.test/shell.js",
        embedded,
        true,
        prefix.len() as u64,
    )
    .expect("a pack embedded after a payload must mount at its offset");

    assert_eq!(
        FileAccess::read_file_bytes("res://icon.png").expect("read through the embedded pack"),
        ICON_BYTES,
    );

    unmount_resource_pack("embedded");
}

/// An offset past the end of the buffer is rejected rather than panicking on a
/// slice — a truncated download is a real failure mode for a fetched pack.
#[test]
fn offset_past_the_end_is_an_error() {
    let _guard = MOUNT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset_to_web_conditions();

    let bytes = served_pck();
    let past_end = bytes.len() as u64 + 1;
    let err = load_resource_pack_from_bytes(
        "truncated",
        "https://example.test/game.pck",
        bytes,
        true,
        past_end,
    )
    .expect_err("an offset past the end must be an error");

    assert!(
        err.to_string().contains("past the end"),
        "the error must say the offset overran the buffer, got `{err}`",
    );
    assert!(
        !FileAccess::file_exists("res://main.tscn"),
        "a rejected mount must not leave a pack mounted",
    );
}

/// Bytes that are not a `.pck` are rejected, and the error names the **source**
/// the caller passed — for the web that is the URL fetched, which is the only
/// thing that identifies the download. Nothing treats `source` as a path.
#[test]
fn invalid_bytes_report_the_source_they_came_from() {
    let _guard = MOUNT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset_to_web_conditions();

    let err = load_resource_pack_from_bytes(
        "not-a-pack",
        "https://example.test/index.html",
        b"<!doctype html><title>404</title>".to_vec(),
        true,
        0,
    )
    .expect_err("non-pack bytes must be rejected");

    assert!(
        err.to_string().contains("https://example.test/index.html"),
        "the error must name the URL the bytes came from, got `{err}`",
    );
}

/// The path-taking [`load_resource_pack`] still behaves after being rewritten
/// as a wrapper — the refactor that introduced the bytes entry point must not
/// have changed the native boot path.
#[test]
fn path_form_still_mounts_and_delegates() {
    let _guard = MOUNT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset_to_web_conditions();

    let dir = tempfile::tempdir().expect("temp dir");
    let pck_path = dir.path().join("game.pck");
    std::fs::write(&pck_path, served_pck()).expect("write the pack to disk");

    load_resource_pack("native", pck_path.to_str().unwrap(), true, 0)
        .expect("the path form must still mount");

    assert_eq!(
        FileAccess::read_file_bytes("res://icon.png").expect("read through the path-mounted pack"),
        ICON_BYTES,
    );

    // And it still routes its diagnostics through the same code: a bad file
    // reports the path it was given.
    let bad = dir.path().join("bad.pck");
    std::fs::write(&bad, b"not a pack").expect("write a bad pack");
    let err = load_resource_pack("bad", bad.to_str().unwrap(), true, 0)
        .expect_err("a non-pack file must be rejected");
    assert!(
        err.to_string().contains("bad.pck"),
        "the path form must still name the file, got `{err}`",
    );

    unmount_resource_pack("native");
}
