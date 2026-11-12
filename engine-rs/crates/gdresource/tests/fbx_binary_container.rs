//! pat-s3rrv.136 — the binary FBX container reader.
//!
//! The repo has **no `.fbx` file at all**, so this builds its fixtures byte by
//! byte from the layout in ufbx — the parser Godot itself uses, vendored at
//! `upstream/godot/thirdparty/ufbx/ufbx.c`.
//!
//! ⚠️ A builder and a reader written by the same hand prove they **agree**, not
//! that either matches real FBX. What makes this worth more than a round trip is
//! that the builder is written to the *cited* layout — header sizes and field
//! widths from `ufbx.c:11177-11198` and `:8967` — rather than to whatever the
//! reader happens to expect. Each offset below carries its citation.

use gdresource::fbx_import::{is_binary_fbx, parse, parse_header, FbxProperty, FBX_BINARY_MAGIC};

/// FBX 7.4 — below the 7500 boundary where the record header widens to 64-bit.
const VERSION_7400: u32 = 7400;

/// Builds a file header: 22-byte magic, endianness byte, `u32` version
/// (`ufbx.c:11177-11198`, total 27 bytes).
fn header(version: u32, big_endian: bool) -> Vec<u8> {
    let mut out = FBX_BINARY_MAGIC.to_vec();
    out.push(if big_endian { 1 } else { 0 });
    out.extend_from_slice(&version.to_le_bytes());
    out
}

/// Appends one 32-bit node record and returns the file bytes.
///
/// The record header is `u32 end_offset`, `u32 num_properties`,
/// `u32 property_list_len`, `u8 name_len` (`ufbx.c:8967`, 13 bytes). `end_offset`
/// is an absolute file offset, so it is back-patched once the record's true
/// extent is known — which is exactly why a reader cannot guess it.
fn push_record(out: &mut Vec<u8>, name: &str, props: &[FbxProperty], children: &[(&str, i32)]) {
    let start = out.len();
    out.extend_from_slice(&[0u8; 13]); // placeholder header

    out.extend_from_slice(name.as_bytes());
    let props_start = out.len();
    for p in props {
        match p {
            FbxProperty::I32(v) => {
                out.push(b'I');
                out.extend_from_slice(&v.to_le_bytes());
            }
            FbxProperty::Str(s) => {
                out.push(b'S');
                out.extend_from_slice(&(s.len() as u32).to_le_bytes());
                out.extend_from_slice(s);
            }
            other => panic!("fixture builder does not emit {other:?}"),
        }
    }
    let props_len = out.len() - props_start;

    for (child_name, child_value) in children {
        push_record(out, child_name, &[FbxProperty::I32(*child_value)], &[]);
    }
    if !children.is_empty() {
        // The NULL record that ends a child list.
        out.extend_from_slice(&[0u8; 13]);
    }

    let end = out.len();
    out[start..start + 4].copy_from_slice(&(end as u32).to_le_bytes());
    out[start + 4..start + 8].copy_from_slice(&(props.len() as u32).to_le_bytes());
    out[start + 8..start + 12].copy_from_slice(&(props_len as u32).to_le_bytes());
    out[start + 12] = name.len() as u8;
}

/// A one-record file: `Objects` with an int and a string, and two children.
fn fixture() -> Vec<u8> {
    let mut out = header(VERSION_7400, false);
    push_record(
        &mut out,
        "Objects",
        &[FbxProperty::I32(42), FbxProperty::Str(b"Cube".to_vec())],
        &[("Model", 7), ("Geometry", 9)],
    );
    out.extend_from_slice(&[0u8; 13]); // top-level NULL record
    out
}

#[test]
fn the_magic_identifies_a_binary_fbx() {
    assert!(is_binary_fbx(&fixture()));
    assert!(
        !is_binary_fbx(b"; FBX 7.4.0 project file"),
        "ASCII FBX is not binary"
    );
    assert!(!is_binary_fbx(b"not an fbx at all"));
    assert!(
        !is_binary_fbx(b"Kaydara"),
        "a truncated magic must not match"
    );
}

/// 🔑 The byte after the magic is the **endianness indicator**, not padding
/// (`ufbx.c:11184-11186`).
///
/// ⚠️ The widely-circulated community notes describe it as an unknown/filler
/// byte. A reader that believes that and always parses little-endian misreads
/// every big-endian file's version — and then picks the wrong record-header
/// width, so the whole node tree decodes as garbage.
#[test]
fn the_byte_after_the_magic_is_endianness_not_padding() {
    let le = parse_header(&header(VERSION_7400, false)).expect("little-endian header");
    assert_eq!(le.version, 7400);
    assert!(!le.big_endian);

    let be = parse_header(&header(VERSION_7400, true)).expect("big-endian header");
    assert!(be.big_endian, "a non-zero byte there means big-endian");
    assert_ne!(
        be.version, 7400,
        "and the version must then be read big-endian, so it differs"
    );
}

#[test]
fn a_non_fbx_buffer_is_rejected() {
    assert!(parse(b"definitely not an fbx file").is_err());
    assert!(parse(&[]).is_err(), "an empty buffer must not parse");
}

/// **The core**: the record tree comes back with its names, properties and
/// nesting.
#[test]
fn the_node_tree_round_trips() {
    let (hdr, nodes) = parse(&fixture()).expect("fixture parses");
    assert_eq!(hdr.version, 7400);
    assert_eq!(nodes.len(), 1, "one top-level record: {nodes:?}");

    let objects = &nodes[0];
    assert_eq!(objects.name, "Objects");
    assert_eq!(objects.properties.len(), 2);
    assert_eq!(objects.properties[0], FbxProperty::I32(42));
    assert_eq!(
        objects.properties[1],
        FbxProperty::Str(b"Cube".to_vec()),
        "FBX strings keep their raw bytes — they are not NUL-terminated and may \
         contain interior NULs"
    );

    // 🔑 Children are found by the record's own `end_offset`, not by a count —
    // the format carries no child count. A reader that assumed "always a child
    // list" would eat the next record's header as a NULL terminator.
    assert_eq!(objects.children.len(), 2, "{:?}", objects.children);
    assert_eq!(objects.children[0].name, "Model");
    assert_eq!(objects.children[1].name, "Geometry");
    assert_eq!(objects.children[0].properties[0], FbxProperty::I32(7));
    assert!(objects.child("Geometry").is_some());
    assert!(objects.child("NoSuchNode").is_none());
}

/// ⚠️ An array property is REPORTED, not skipped. Its payload is zlib-deflated,
/// and silently dropping it would produce a node tree that looks complete and
/// has quietly lost its geometry — the failure mode this whole bead exists to
/// remove.
#[test]
fn a_compressed_array_property_is_an_explicit_error() {
    let mut out = header(VERSION_7400, false);
    let start = out.len();
    out.extend_from_slice(&[0u8; 13]);
    out.extend_from_slice(b"Vertices");
    out.push(b'd'); // f64 array tag
    out.extend_from_slice(&[0u8; 12]); // array header, unread
    let end = out.len();
    out[start..start + 4].copy_from_slice(&(end as u32).to_le_bytes());
    out[start + 4..start + 8].copy_from_slice(&1u32.to_le_bytes());
    out[start + 8..start + 12].copy_from_slice(&13u32.to_le_bytes());
    out[start + 12] = 8;
    out.extend_from_slice(&[0u8; 13]);

    let err = parse(&out).expect_err("an array property must not parse silently");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("zlib") || msg.contains("compress"),
        "the error must name why it stopped, got {msg}"
    );
}
