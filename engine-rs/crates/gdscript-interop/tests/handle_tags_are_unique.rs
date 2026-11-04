//! Every opaque-object handle family owns a distinct top byte.
//!
//! Handles (`Variant::ObjectId`s that name an RNG, a Regex, an audio effect
//! slot, an editor dock, ...) are dispatched by testing their top byte against
//! each family's `*_HANDLE_TAG`. Two families on one byte means the first
//! matcher wins and the other family's methods become unreachable — exactly how
//! `AudioServer.get_bus_effect_instance(..)` handles came back as
//! `FileSystemDock` (both were `0x12`) and an AudioEffectCapture could not be
//! drained. The source comments already called the tag space workspace-wide;
//! this makes that a checked rule instead of a convention.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("readable dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn every_handle_tag_owns_a_distinct_top_byte() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    rust_files(&crates, &mut files);

    // top byte -> every `NAME` (file) declaring a tag on it
    let mut owners: BTreeMap<u8, Vec<String>> = BTreeMap::new();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for line in text.lines() {
            let line = line.trim_start();
            if !line.starts_with("const ") && !line.starts_with("pub const ") && !line.starts_with("pub(crate) const ") {
                continue;
            }
            let Some((lhs, rhs)) = line.split_once("_TAG: u64 = 0x") else {
                continue;
            };
            let hex: String = rhs
                .chars()
                .take_while(|c| c.is_ascii_hexdigit() || *c == '_')
                .filter(|c| *c != '_')
                .collect();
            if hex.len() != 16 {
                continue; // not a 64-bit top-byte tag
            }
            let byte = u8::from_str_radix(&hex[..2], 16).expect("hex");
            let name = lhs.rsplit(' ').next().unwrap_or(lhs);
            owners
                .entry(byte)
                .or_default()
                .push(format!("{name}_TAG ({})", file.display()));
        }
    }

    assert!(owners.len() > 20, "the scan must find the tag families at all");
    let clashes: Vec<String> = owners
        .iter()
        .filter(|(_, names)| names.len() > 1)
        .map(|(byte, names)| format!("0x{byte:02X}: {}", names.join(", ")))
        .collect();
    assert!(
        clashes.is_empty(),
        "handle families sharing a top byte (the first matcher steals the other's \
         handles):\n{}",
        clashes.join("\n")
    );
}
