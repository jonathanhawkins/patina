//! pat-zq6st: Runtime resource loading from PCK archives.
//!
//! Validates:
//! 1. PckPacker: create archive, add files, set version/alignment
//! 2. PckArchive: parse header, directory, extract files, sorted paths
//! 3. PckEntry: offset, size, md5 metadata
//! 4. PckResourceLoader: load .tres, .tscn, non-text as PackedFile
//! 5. PckResourceLoader: extract_raw, has_file, file_count, header
//! 6. PckMountManager: mount/unmount, priority ordering
//! 7. PckMountManager: highest-priority wins, which_mount, all_files
//! 8. PckMountManager: remount replaces same label, extract_raw
//! 9. PckMountManager as ResourceLoader: load across mounts
//! 10. Error cases: invalid magic, truncated data, missing files
//! 11. Edge cases: empty files, large files, empty archive
//! 12. ClassDB PCKPacker registration

use gdresource::{
    PckArchive, PckMountManager, PckPacker, PckResourceLoader, PCK_MAGIC,
};
use gdresource::loader::ResourceLoader;
use gdvariant::Variant;

// ── Helpers ─────────────────────────────────────────────────────────

fn make_tres(name: &str) -> String {
    format!(
        r#"[gd_resource type="Resource" format=3]

[resource]
name = "{name}"
"#
    )
}

fn make_pck(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut packer = PckPacker::new();
    for &(path, data) in files {
        packer.add_file(path, data.to_vec());
    }
    packer.pack().unwrap()
}

// ── PckPacker ───────────────────────────────────────────────────────

#[test]
fn packer_empty_archive() {
    let packer = PckPacker::new();
    assert_eq!(packer.file_count(), 0);
    let data = packer.pack().unwrap();
    assert!(&data[..4] == PCK_MAGIC);
}

#[test]
fn packer_add_files_increments_count() {
    let mut packer = PckPacker::new();
    packer.add_file("res://a.txt", b"alpha".to_vec());
    packer.add_file("res://b.txt", b"beta".to_vec());
    assert_eq!(packer.file_count(), 2);
}

#[test]
fn packer_with_version() {
    let packer = PckPacker::new().with_version(5, 1, 0);
    let data = packer.pack().unwrap();
    let archive = PckArchive::from_bytes(&data).unwrap();
    assert_eq!(archive.header.ver_major, 5);
    assert_eq!(archive.header.ver_minor, 1);
    assert_eq!(archive.header.ver_patch, 0);
}

#[test]
fn packer_with_alignment() {
    let mut packer = PckPacker::new().with_alignment(128);
    packer.add_file("res://a.txt", b"data".to_vec());
    packer.add_file("res://b.txt", b"more".to_vec());
    let data = packer.pack().unwrap();
    let archive = PckArchive::from_bytes(&data).unwrap();
    for entry in archive.entries.values() {
        assert_eq!(entry.offset % 128, 0, "entry not aligned to 128");
    }
}

#[test]
fn packer_default_trait() {
    let packer = PckPacker::default();
    assert_eq!(packer.file_count(), 0);
}

// ── PckArchive ──────────────────────────────────────────────────────

#[test]
fn archive_roundtrip_single_file() {
    let data = make_pck(&[("res://hello.txt", b"Hello, world!")]);
    let archive = PckArchive::from_bytes(&data).unwrap();
    assert_eq!(archive.file_count(), 1);
    assert!(archive.has_file("res://hello.txt"));
    let content = archive.extract_file(&data, "res://hello.txt").unwrap();
    assert_eq!(content, b"Hello, world!");
}

#[test]
fn archive_roundtrip_multiple_files() {
    let data = make_pck(&[
        ("res://a.txt", b"alpha"),
        ("res://b.txt", b"beta"),
        ("res://sub/c.txt", b"gamma"),
    ]);
    let archive = PckArchive::from_bytes(&data).unwrap();
    assert_eq!(archive.file_count(), 3);
    assert_eq!(archive.extract_file(&data, "res://a.txt").unwrap(), b"alpha");
    assert_eq!(archive.extract_file(&data, "res://sub/c.txt").unwrap(), b"gamma");
}

#[test]
fn archive_file_paths_sorted() {
    let data = make_pck(&[
        ("res://z.txt", b""),
        ("res://a.txt", b""),
        ("res://m.txt", b""),
    ]);
    let archive = PckArchive::from_bytes(&data).unwrap();
    assert_eq!(archive.file_paths(), vec!["res://a.txt", "res://m.txt", "res://z.txt"]);
}

#[test]
fn archive_header_default_version() {
    let data = make_pck(&[]);
    let archive = PckArchive::from_bytes(&data).unwrap();
    assert_eq!(archive.header.format_version, 2);
    assert_eq!(archive.header.ver_major, 4);
}

#[test]
fn archive_total_data_size() {
    let data = make_pck(&[
        ("res://a.bin", &[0u8; 100]),
        ("res://b.bin", &[0u8; 200]),
    ]);
    let archive = PckArchive::from_bytes(&data).unwrap();
    assert_eq!(archive.total_data_size(), 300);
}

#[test]
fn archive_get_entry_metadata() {
    let data = make_pck(&[("res://test.gd", b"extends Node")]);
    let archive = PckArchive::from_bytes(&data).unwrap();
    let entry = archive.get_entry("res://test.gd").unwrap();
    assert_eq!(entry.path, "res://test.gd");
    assert_eq!(entry.size, 12);
    assert_eq!(entry.md5, [0u8; 16]); // not computed by default
}

#[test]
fn archive_extract_nonexistent_returns_none() {
    let data = make_pck(&[]);
    let archive = PckArchive::from_bytes(&data).unwrap();
    assert!(archive.extract_file(&data, "res://nope.txt").is_none());
}

#[test]
fn archive_entry_offsets_aligned() {
    let data = make_pck(&[
        ("res://small.txt", b"hi"),
        ("res://other.txt", b"there"),
    ]);
    let archive = PckArchive::from_bytes(&data).unwrap();
    for entry in archive.entries.values() {
        assert_eq!(entry.offset % 64, 0);
    }
}

#[test]
fn archive_empty_file_content() {
    let data = make_pck(&[("res://empty.txt", b"")]);
    let archive = PckArchive::from_bytes(&data).unwrap();
    let content = archive.extract_file(&data, "res://empty.txt").unwrap();
    assert!(content.is_empty());
}

#[test]
fn archive_large_file_roundtrip() {
    let big = vec![0xAB_u8; 10_000];
    let data = make_pck(&[("res://big.bin", &big)]);
    let archive = PckArchive::from_bytes(&data).unwrap();
    let content = archive.extract_file(&data, "res://big.bin").unwrap();
    assert_eq!(content.len(), 10_000);
    assert_eq!(content[0], 0xAB);
    assert_eq!(content[9_999], 0xAB);
}

// ── Error cases ─────────────────────────────────────────────────────

#[test]
fn invalid_magic_rejected() {
    let mut data = PckPacker::new().pack().unwrap();
    data[0] = b'X';
    let result = PckArchive::from_bytes(&data);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("invalid PCK magic"));
}

#[test]
fn truncated_data_rejected() {
    let result = PckArchive::from_bytes(&[0u8; 10]);
    assert!(result.is_err());
}

// ── PckResourceLoader ───────────────────────────────────────────────

#[test]
fn loader_loads_tres_resource() {
    let tres = make_tres("FromPCK");
    let data = make_pck(&[("res://item.tres", tres.as_bytes())]);
    let loader = PckResourceLoader::from_bytes(data).unwrap();
    let res = loader.load("res://item.tres").unwrap();
    assert_eq!(res.class_name, "Resource");
    assert_eq!(res.path, "res://item.tres");
}

#[test]
fn loader_loads_tscn() {
    let tscn = r#"[gd_scene format=3 uid="uid://test123"]

[node name="Root" type="Node2D"]
"#;
    let data = make_pck(&[("res://level.tscn", tscn.as_bytes())]);
    let loader = PckResourceLoader::from_bytes(data).unwrap();
    let res = loader.load("res://level.tscn").unwrap();
    assert_eq!(res.path, "res://level.tscn");
}

#[test]
fn loader_non_tres_as_packed_file() {
    let data = make_pck(&[("res://icon.png", &[0x89, 0x50, 0x4E, 0x47])]);
    let loader = PckResourceLoader::from_bytes(data).unwrap();
    let res = loader.load("res://icon.png").unwrap();
    assert_eq!(res.class_name, "PackedFile");
    assert_eq!(res.get_property("byte_size"), Some(&Variant::Int(4)));
}

#[test]
fn loader_missing_file_returns_error() {
    let data = make_pck(&[]);
    let loader = PckResourceLoader::from_bytes(data).unwrap();
    assert!(loader.load("res://missing.tres").is_err());
}

#[test]
fn loader_extract_raw() {
    let content = b"raw binary data";
    let data = make_pck(&[("res://data.bin", content.as_slice())]);
    let loader = PckResourceLoader::from_bytes(data).unwrap();
    assert_eq!(loader.extract_raw("res://data.bin").unwrap(), content);
    assert!(loader.extract_raw("res://nope.bin").is_none());
}

#[test]
fn loader_has_file_and_count() {
    let data = make_pck(&[("res://a.txt", b""), ("res://b.txt", b"")]);
    let loader = PckResourceLoader::from_bytes(data).unwrap();
    assert!(loader.has_file("res://a.txt"));
    assert!(!loader.has_file("res://c.txt"));
    assert_eq!(loader.file_count(), 2);
}

#[test]
fn loader_file_paths_sorted() {
    let data = make_pck(&[("res://z.gd", b""), ("res://a.gd", b"")]);
    let loader = PckResourceLoader::from_bytes(data).unwrap();
    assert_eq!(loader.file_paths(), vec!["res://a.gd", "res://z.gd"]);
}

#[test]
fn loader_header_metadata() {
    let packer = PckPacker::new().with_version(4, 6, 1);
    let data = packer.pack().unwrap();
    let loader = PckResourceLoader::from_bytes(data).unwrap();
    assert_eq!(loader.header().ver_major, 4);
    assert_eq!(loader.header().ver_minor, 6);
    assert_eq!(loader.header().ver_patch, 1);
}

#[test]
fn loader_archive_accessor() {
    let data = make_pck(&[("res://x.txt", b"x")]);
    let loader = PckResourceLoader::from_bytes(data).unwrap();
    let archive = loader.archive();
    assert_eq!(archive.file_count(), 1);
    assert!(archive.has_file("res://x.txt"));
}

#[test]
fn loader_invalid_utf8_tres_returns_error() {
    let data = make_pck(&[("res://bad.tres", &[0xFF, 0xFE, 0x80, 0x81])]);
    let loader = PckResourceLoader::from_bytes(data).unwrap();
    assert!(loader.load("res://bad.tres").is_err());
}

#[test]
fn loader_invalid_archive_bytes() {
    let result = PckResourceLoader::from_bytes(vec![0, 1, 2, 3]);
    assert!(result.is_err());
}

#[test]
fn loader_multiple_tres_resources() {
    let tres_a = make_tres("Alpha");
    let tres_b = make_tres("Beta");
    let data = make_pck(&[
        ("res://a.tres", tres_a.as_bytes()),
        ("res://b.tres", tres_b.as_bytes()),
    ]);
    let loader = PckResourceLoader::from_bytes(data).unwrap();
    let a = loader.load("res://a.tres").unwrap();
    let b = loader.load("res://b.tres").unwrap();
    assert_ne!(a.path, b.path);
}

// ── PckMountManager ─────────────────────────────────────────────────

#[test]
fn mount_manager_empty() {
    let mgr = PckMountManager::new();
    assert_eq!(mgr.mount_count(), 0);
    assert!(mgr.mounted_labels().is_empty());
    assert!(!mgr.has_file("res://anything"));
}

#[test]
fn mount_manager_default_trait() {
    let mgr = PckMountManager::default();
    assert_eq!(mgr.mount_count(), 0);
}

#[test]
fn mount_and_load_resource() {
    let tres = make_tres("Mounted");
    let data = make_pck(&[("res://item.tres", tres.as_bytes())]);
    let mut mgr = PckMountManager::new();
    mgr.mount("base", data, 0).unwrap();
    assert_eq!(mgr.mount_count(), 1);
    assert!(mgr.has_file("res://item.tres"));
    let res = mgr.load("res://item.tres").unwrap();
    assert_eq!(res.class_name, "Resource");
}

#[test]
fn unmount_removes_archive() {
    let data = make_pck(&[("res://a.txt", b"hello")]);
    let mut mgr = PckMountManager::new();
    mgr.mount("pack1", data, 0).unwrap();
    assert!(mgr.unmount("pack1"));
    assert!(!mgr.has_file("res://a.txt"));
    assert_eq!(mgr.mount_count(), 0);
}

#[test]
fn unmount_nonexistent_returns_false() {
    let mut mgr = PckMountManager::new();
    assert!(!mgr.unmount("nope"));
}

#[test]
fn higher_priority_wins() {
    let tres_low = make_tres("Low");
    let tres_high = make_tres("High");
    let data_low = make_pck(&[("res://item.tres", tres_low.as_bytes())]);
    let data_high = make_pck(&[("res://item.tres", tres_high.as_bytes())]);

    let mut mgr = PckMountManager::new();
    mgr.mount("low", data_low, 0).unwrap();
    mgr.mount("high", data_high, 10).unwrap();

    let res = mgr.load("res://item.tres").unwrap();
    assert_eq!(
        res.get_property("name"),
        Some(&Variant::String("High".into()))
    );
}

#[test]
fn priority_order_labels() {
    let data1 = make_pck(&[("res://a.txt", b"a")]);
    let data2 = make_pck(&[("res://b.txt", b"b")]);
    let data3 = make_pck(&[("res://c.txt", b"c")]);

    let mut mgr = PckMountManager::new();
    mgr.mount("low", data1, 0).unwrap();
    mgr.mount("high", data2, 100).unwrap();
    mgr.mount("mid", data3, 50).unwrap();
    assert_eq!(mgr.mounted_labels(), vec!["high", "mid", "low"]);
}

#[test]
fn remount_replaces_same_label() {
    let data1 = make_pck(&[("res://old.txt", b"old")]);
    let data2 = make_pck(&[("res://new.txt", b"new")]);

    let mut mgr = PckMountManager::new();
    mgr.mount("pack", data1, 0).unwrap();
    assert!(mgr.has_file("res://old.txt"));

    mgr.mount("pack", data2, 0).unwrap();
    assert!(!mgr.has_file("res://old.txt"));
    assert!(mgr.has_file("res://new.txt"));
    assert_eq!(mgr.mount_count(), 1);
}

#[test]
fn which_mount_returns_highest_priority() {
    let data1 = make_pck(&[("res://shared.txt", b"from-low")]);
    let data2 = make_pck(&[("res://shared.txt", b"from-high")]);

    let mut mgr = PckMountManager::new();
    mgr.mount("low", data1, 0).unwrap();
    mgr.mount("high", data2, 10).unwrap();

    assert_eq!(mgr.which_mount("res://shared.txt"), Some("high"));
    assert_eq!(mgr.which_mount("res://missing"), None);
}

#[test]
fn all_files_across_mounts_deduplicated() {
    let data1 = make_pck(&[("res://a.txt", b"a"), ("res://shared.txt", b"1")]);
    let data2 = make_pck(&[("res://b.txt", b"b"), ("res://shared.txt", b"2")]);

    let mut mgr = PckMountManager::new();
    mgr.mount("first", data1, 0).unwrap();
    mgr.mount("second", data2, 0).unwrap();

    let files = mgr.all_files();
    assert!(files.contains(&"res://a.txt".to_string()));
    assert!(files.contains(&"res://b.txt".to_string()));
    assert!(files.contains(&"res://shared.txt".to_string()));
    assert_eq!(files.len(), 3);
}

#[test]
fn extract_raw_from_priority_mount() {
    let data1 = make_pck(&[("res://data.bin", b"low-content")]);
    let data2 = make_pck(&[("res://data.bin", b"high-content")]);

    let mut mgr = PckMountManager::new();
    mgr.mount("low", data1, 0).unwrap();
    mgr.mount("high", data2, 10).unwrap();

    let raw = mgr.extract_raw("res://data.bin").unwrap();
    assert_eq!(raw, b"high-content");
}

#[test]
fn load_not_found_in_any_mount() {
    let data = make_pck(&[("res://exists.txt", b"yes")]);
    let mut mgr = PckMountManager::new();
    mgr.mount("base", data, 0).unwrap();
    let err = mgr.load("res://missing.tres").unwrap_err();
    assert!(err.to_string().contains("not found"));
}

#[test]
fn mount_invalid_data_fails() {
    let mut mgr = PckMountManager::new();
    let result = mgr.mount("bad", vec![0, 1, 2, 3], 0);
    assert!(result.is_err());
    assert_eq!(mgr.mount_count(), 0);
}

// ── ClassDB PCKPacker registration ──────────────────────────────────

#[test]
fn classdb_pckpacker_registered() {
    use gdobject::class_db;
    class_db::register_3d_classes();
    assert!(
        class_db::class_exists("PCKPacker"),
        "PCKPacker should be registered in ClassDB"
    );
    let methods = class_db::get_method_list("PCKPacker", false);
    let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"pck_start"));
    assert!(names.contains(&"add_file"));
    assert!(names.contains(&"flush"));
}

#[test]
fn classdb_project_settings_load_resource_pack() {
    use gdobject::class_db;
    class_db::register_3d_classes();
    let methods = class_db::get_method_list("ProjectSettings", false);
    let names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"load_resource_pack"));
}
