//! pat-nzgdu.177: ConfigFile encrypted save/load roundtrip, plus
//! `parse`/`encode_to_text` and single-key erase.
//!
//! The encrypted family (`save_encrypted_pass`/`load_encrypted_pass`) serializes
//! the config to INI text, then AES-256-CBC encrypts it under a key derived from
//! the password (SHA-256). A correct password round-trips the values exactly; a
//! wrong password fails to recover them.

use gdresource::ConfigFile;
use gdvariant::Variant;

fn temp_path(name: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(name);
    p
}

#[test]
fn config_file_encrypted_roundtrip() {
    let mut cfg = ConfigFile::new();
    cfg.set_value("player", "name", Variant::String("Hero".to_string()));
    cfg.set_value("player", "hp", Variant::Int(100));
    cfg.set_value("game", "title", Variant::String("My Game".to_string()));

    let path = temp_path("pat_nzgdu177_roundtrip.cfg");
    let path_str = path.to_str().unwrap();

    cfg.save_encrypted_pass(path_str, "s3cret-pass")
        .expect("save_encrypted_pass should succeed");

    // The same password reads back the identical values.
    let loaded = ConfigFile::load_encrypted_pass(path_str, "s3cret-pass")
        .expect("load_encrypted_pass with the correct password should succeed");
    assert_eq!(
        loaded.get_value("player", "name"),
        cfg.get_value("player", "name")
    );
    assert_eq!(loaded.get_value("player", "hp"), cfg.get_value("player", "hp"));
    assert_eq!(loaded.get_value("game", "title"), cfg.get_value("game", "title"));

    // A wrong password must NOT recover the original value (decrypt / UTF-8 /
    // parse fails, or yields different data).
    let recovered_with_wrong_pass = ConfigFile::load_encrypted_pass(path_str, "not-the-pass")
        .ok()
        .and_then(|c| c.get_value("player", "name").cloned());
    assert_ne!(
        recovered_with_wrong_pass,
        Some(Variant::String("Hero".to_string())),
        "a wrong password must not read back the original value"
    );

    // erase_section_key removes a single key, leaving its siblings intact.
    let mut c2 = loaded;
    c2.erase_section_key("player", "hp");
    assert!(
        !c2.has_section_key("player", "hp"),
        "erased key should be gone"
    );
    assert!(
        c2.has_section_key("player", "name"),
        "sibling key should remain"
    );

    let _ = std::fs::remove_file(path_str);
}

#[test]
fn config_file_parse_and_encode_to_text_roundtrip() {
    // `encode_to_text` serializes and `parse` reads back into an existing
    // ConfigFile — the two are inverses over the values that were set.
    let mut cfg = ConfigFile::new();
    cfg.set_value("net", "port", Variant::Int(8080));
    cfg.set_value("net", "host", Variant::String("localhost".to_string()));

    let text = cfg.encode_to_text();

    let mut reparsed = ConfigFile::new();
    reparsed
        .parse(&text)
        .expect("parse of encoded text should succeed");
    assert_eq!(
        reparsed.get_value("net", "port"),
        cfg.get_value("net", "port")
    );
    assert_eq!(
        reparsed.get_value("net", "host"),
        cfg.get_value("net", "host")
    );
}
