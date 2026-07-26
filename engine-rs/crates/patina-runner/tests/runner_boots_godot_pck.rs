//! pat-0q2kx.134 acceptance: the runner boots an exported game from a Godot
//! `.pck`, with no loose project directory on disk.
//!
//! Patina's PCK reader/writer existed and was tested, and had **zero runtime
//! callers**: the runner booted only loose directories via `std::fs`, so a
//! shipped `.pck` could not start a game at all. `ProjectSettings
//! .load_resource_pack` was a doc comment. Removal records were parsed and then
//! discarded, so a patch pack could not delete a file.
//!
//! These pack a project with `gdresource::PckPacker`, delete the loose files,
//! and boot from the archive.

use std::path::PathBuf;

use gdcore::file_access::FileAccess;
use gdresource::pck::{load_resource_pack, unmount_resource_pack, PckPacker};
use patina_runner::project_boot::boot_project_from_pck;

/// The `res://` roots and pack mounts are process-global, so these tests must
/// not interleave.
static BOOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn temp_dir(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("patina_pck_{tag}_{}_{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

const PROJECT_GODOT: &str = r#"config_version=5

[application]

config/name="Packed Game"
run/main_scene="res://main.tscn"
"#;

const MAIN_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Texture2D" path="res://icon.png" id="1_icon"]

[node name="Main" type="Node2D"]

[node name="Art" type="Sprite2D" parent="."]
texture = ExtResource("1_icon")
"#;

/// A one-file project packed as a Godot `.pck`.
fn pack_project() -> Vec<u8> {
    let mut packer = PckPacker::new();
    packer.add_file("res://project.godot", PROJECT_GODOT.as_bytes().to_vec());
    packer.add_file("res://main.tscn", MAIN_TSCN.as_bytes().to_vec());
    packer.add_file("res://icon.png", b"\x89PNG\r\n\x1a\nFAKE".to_vec());
    packer.pack().expect("pack")
}

fn reset() {
    gdcore::file_access::clear_mounted_packs();
}

// ---------------------------------------------------------------------------
// The bead's acceptance test
// ---------------------------------------------------------------------------

/// **The acceptance test.** Pack a project, keep *only* the `.pck` on disk, and
/// boot: the main scene instances, its texture property resolves, and
/// `FileAccess.file_exists("res://icon.png")` is true.
#[test]
fn runner_boots_godot_pck() {
    let _lock = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset();

    let dir = temp_dir("boot");
    let pck_path = dir.join("game.pck");
    std::fs::write(&pck_path, pack_project()).expect("write pck");

    // Nothing but the pack exists — no project.godot, no main.tscn, no icon.
    assert!(!dir.join("project.godot").exists());
    assert!(!dir.join("main.tscn").exists());

    let booted = boot_project_from_pck(&pck_path, None).expect("boot from .pck");

    // (a) project.godot came out of the pack.
    assert_eq!(
        booted.settings.project_name(),
        "Packed Game",
        "project.godot must be parsed out of the pack",
    );

    // (b) The main scene instanced, and the Sprite2D's texture property survived
    // — the scene was read through res:// with only the pack backing it.
    let main = booted
        .main_scene_root
        .expect("run/main_scene must instance from the pack");
    let art = booted
        .tree
        .find_children(main, "*", Some("Sprite2D"), true)
        .first()
        .copied()
        .expect("the scene's Sprite2D must exist");
    // `packed_scene` keeps the raw `ExtResource("id")` on the declared property
    // and caches the *resolved* path under `__extres__<prop>`; the resolved one
    // is what "the texture property is resolvable" means here.
    let node = booted.tree.get_node(art).unwrap();
    let resolved = node.get_property("__extres__texture");
    assert!(
        matches!(&resolved, gdvariant::Variant::String(s) if s.contains("icon.png")),
        "the sprite's texture must resolve to the packed icon, got {resolved:?}",
    );
    assert!(
        matches!(node.get_property("texture"), gdvariant::Variant::String(ref s) if s.contains("1_icon")),
        "the declared property keeps its ExtResource reference for round-tripping",
    );

    // (c) A script's `FileAccess.file_exists("res://icon.png")` is true with the
    // file only inside the pack. This is the check that used to be impossible.
    assert!(
        FileAccess::file_exists("res://icon.png"),
        "a packed file must satisfy FileAccess.file_exists",
    );
    assert_eq!(
        FileAccess::read_file_bytes("res://icon.png").expect("read packed icon")[..4],
        *b"\x89PNG",
        "the packed bytes must come back intact",
    );

    reset();
}

/// `load_resource_pack` of a patch pack overrides a file from the base pack —
/// the DLC path a game uses at runtime.
#[test]
fn patch_pack_overrides_a_file_from_the_base_pack() {
    let _lock = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset();

    let dir = temp_dir("patch");
    let base_path = dir.join("game.pck");
    std::fs::write(&base_path, pack_project()).expect("write base");
    boot_project_from_pck(&base_path, None).expect("boot");

    assert_eq!(
        FileAccess::read_file_bytes("res://icon.png").unwrap()[..4],
        *b"\x89PNG",
    );

    // A patch pack replacing the icon, mounted with replace = true so it wins.
    let mut patch = PckPacker::new();
    patch.add_file("res://icon.png", b"PATCHED!".to_vec());
    let patch_path = dir.join("patch.pck");
    std::fs::write(&patch_path, patch.pack().unwrap()).expect("write patch");

    load_resource_pack("patch", patch_path.to_str().unwrap(), true, 0)
        .expect("load_resource_pack must mount a patch pack");

    assert_eq!(
        FileAccess::read_file_bytes("res://icon.png").unwrap(),
        b"PATCHED!".to_vec(),
        "a replace=true patch pack must override the base pack's file",
    );

    assert!(unmount_resource_pack("patch"));
    assert_eq!(
        FileAccess::read_file_bytes("res://icon.png").unwrap()[..4],
        *b"\x89PNG",
        "unmounting the patch must restore the base pack's file",
    );

    reset();
}

/// `replace = false` mounts *below* what is already mounted, so the existing
/// content keeps precedence — Godot's `replace_files = false`.
#[test]
fn non_replacing_pack_does_not_override_existing_content() {
    let _lock = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset();

    let dir = temp_dir("norepl");
    let base_path = dir.join("game.pck");
    std::fs::write(&base_path, pack_project()).expect("write base");
    boot_project_from_pck(&base_path, None).expect("boot");

    let mut lower = PckPacker::new();
    lower.add_file("res://icon.png", b"LOWER".to_vec());
    let lower_path = dir.join("lower.pck");
    std::fs::write(&lower_path, lower.pack().unwrap()).expect("write");

    load_resource_pack("lower", lower_path.to_str().unwrap(), false, 0).expect("mount below");

    assert_eq!(
        FileAccess::read_file_bytes("res://icon.png").unwrap()[..4],
        *b"\x89PNG",
        "replace=false must NOT override already-mounted content",
    );
    // But it does supply files nothing else has.
    let mut extra = PckPacker::new();
    extra.add_file("res://extra.txt", b"new".to_vec());
    let extra_path = dir.join("extra.pck");
    std::fs::write(&extra_path, extra.pack().unwrap()).expect("write");
    load_resource_pack("extra", extra_path.to_str().unwrap(), false, 0).expect("mount below");
    assert!(
        FileAccess::file_exists("res://extra.txt"),
        "a non-replacing pack still contributes paths nobody else has",
    );

    reset();
}

/// Booting a file that is not a pack fails with a clear message rather than
/// panicking or half-mounting.
#[test]
fn booting_a_non_pck_reports_an_error() {
    let _lock = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset();

    let dir = temp_dir("bad");
    let bad = dir.join("not-a-pack.pck");
    std::fs::write(&bad, b"just some bytes").expect("write");

    let err = boot_project_from_pck(&bad, None).expect_err("must reject a non-pck");
    assert!(
        err.contains("not a valid .pck"),
        "error should name the problem, got: {err}"
    );
    reset();
}
