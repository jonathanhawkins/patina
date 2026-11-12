//! Saving an edited scene as an EDIT of the file it was loaded from.
//!
//! [`crate::scene_saver::TscnSaver`] rebuilds a `.tscn` from the tree. That
//! reconstruction cannot reproduce every file: it does not know an editable
//! child's `parent_id_path`/`index`/`unique_id`, an instance's foreign nodes are
//! in the tree but not in the file, and nested sub-resources it cannot model
//! come back empty. Writing it verbatim turned an open-then-save of the
//! platformer's `player.tscn` into a different scene (pat-hivyf).
//!
//! [`merge_edits`] is a three-way merge instead, the way a VCS merges a patch:
//!
//! - `original` — the file as it was on disk when the scene was opened;
//! - `baseline` — what the saver produced for the tree right after opening it;
//! - `current`  — what the saver produces for the tree now.
//!
//! Whatever the saver writes the SAME in `baseline` and `current` was not
//! edited, so the original text is kept for it (or nothing, when the original
//! never had it — an instance's foreign node, a derived property). Only what
//! differs is taken from `current`, down to the single property line. An
//! unedited scene therefore saves byte-for-byte as it was opened.

use std::collections::{HashMap, HashSet};

/// One `[section]` of a `.tscn`: its header line, its property entries and the
/// blank lines after it.
#[derive(Clone, Debug)]
struct Section {
    key: String,
    kind: Kind,
    header: String,
    props: Vec<Prop>,
    /// Text between the end of this section and the next header (blank lines).
    trailing: String,
}

#[derive(Clone, Debug)]
struct Prop {
    key: String,
    /// The full entry, every line of it, with its final newline.
    text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Head,
    Ext,
    Sub,
    Node,
    Connection,
    Editable,
    Other,
}

impl Section {
    fn text(&self) -> String {
        let mut s = String::with_capacity(self.header.len() + 64);
        s.push_str(&self.header);
        for p in &self.props {
            s.push_str(&p.text);
        }
        s
    }

    fn prop(&self, key: &str) -> Option<&Prop> {
        self.props.iter().find(|p| p.key == key)
    }
}

const SECTION_TAGS: &[&str] = &[
    "gd_scene",
    "gd_resource",
    "ext_resource",
    "sub_resource",
    "node",
    "connection",
    "editable",
    "resource",
];

fn is_header(line: &str) -> bool {
    let Some(rest) = line.strip_prefix('[') else {
        return false;
    };
    SECTION_TAGS.iter().any(|tag| {
        rest.strip_prefix(tag)
            .is_some_and(|after| after.starts_with(' ') || after.starts_with(']'))
    })
}

/// Parses `key=value` header attributes. Values are quoted strings, or bare
/// tokens that may hold parenthesised lists (`PackedInt32Array(1, 2)`).
fn header_attrs(header: &str) -> Vec<(String, String)> {
    let inner = header.trim_end().trim_start_matches('[');
    let inner = inner.strip_suffix(']').unwrap_or(inner);
    let mut out = Vec::new();
    let chars: Vec<char> = inner.chars().collect();
    let mut i = 0;
    // Skip the tag.
    while i < chars.len() && chars[i] != ' ' {
        i += 1;
    }
    while i < chars.len() {
        while i < chars.len() && chars[i] == ' ' {
            i += 1;
        }
        let start = i;
        while i < chars.len() && chars[i] != '=' && chars[i] != ' ' {
            i += 1;
        }
        let key: String = chars[start..i].iter().collect();
        if i >= chars.len() || chars[i] != '=' {
            continue;
        }
        i += 1;
        let vstart = i;
        if i < chars.len() && chars[i] == '"' {
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
        } else {
            let mut depth = 0i32;
            while i < chars.len() {
                match chars[i] {
                    '(' | '[' | '{' => depth += 1,
                    ')' | ']' | '}' => depth -= 1,
                    '"' => {
                        i += 1;
                        while i < chars.len() && chars[i] != '"' {
                            if chars[i] == '\\' {
                                i += 1;
                            }
                            i += 1;
                        }
                    }
                    ' ' if depth <= 0 => break,
                    _ => {}
                }
                i += 1;
            }
        }
        let value: String = chars[vstart..i.min(chars.len())].iter().collect();
        out.push((key, value));
    }
    out
}

fn attr<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

fn unquote(v: &str) -> String {
    v.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(v)
        .to_string()
}

fn section_key(header: &str) -> (Kind, String) {
    let tag: String = header
        .trim_start_matches('[')
        .chars()
        .take_while(|c| *c != ' ' && *c != ']')
        .collect();
    let attrs = header_attrs(header);
    let get = |k: &str| attr(&attrs, k).map(unquote).unwrap_or_default();
    match tag.as_str() {
        "gd_scene" | "gd_resource" => (Kind::Head, "head".into()),
        "ext_resource" => (Kind::Ext, format!("ext:{}", get("id"))),
        "sub_resource" => (Kind::Sub, format!("sub:{}", get("id"))),
        "node" => {
            let parent = attr(&attrs, "parent").map(unquote);
            let key = match parent {
                None => format!("node:|{}", get("name")),
                Some(p) => format!("node:{p}|{}", get("name")),
            };
            (Kind::Node, key)
        }
        "connection" => (
            Kind::Connection,
            format!(
                "conn:{}|{}|{}|{}",
                get("signal"),
                get("from"),
                get("to"),
                get("method")
            ),
        ),
        "editable" => (Kind::Editable, format!("editable:{}", get("path"))),
        _ => (Kind::Other, format!("other:{}", header.trim_end())),
    }
}

/// Where a property value ends: scans from `start` (just after `=`) across
/// lines, tracking strings and bracket depth. Returns the byte index just past
/// the value's final newline (or the end of the text).
fn value_end(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let mut i = start;
    let mut depth = 0i32;
    let mut in_str = false;
    while i < bytes.len() {
        let c = bytes[i];
        if in_str {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
        } else {
            match c {
                b'"' => in_str = true,
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                b'\n' if depth <= 0 => return i + 1,
                _ => {}
            }
        }
        i += 1;
    }
    bytes.len()
}

/// The key of a property entry starting at `line`: a quoted key decoded of its
/// quotes, or the text before `=`.
fn prop_key(line: &str) -> Option<(String, usize)> {
    if let Some(rest) = line.strip_prefix('"') {
        let mut key = String::new();
        let mut chars = rest.char_indices();
        while let Some((i, c)) = chars.next() {
            match c {
                '\\' => {
                    if let Some((_, e)) = chars.next() {
                        key.push(e);
                    }
                }
                '"' => {
                    let after = &rest[i + 1..];
                    let eq = after.find('=')?;
                    return Some((key, 1 + i + 1 + eq + 1));
                }
                _ => key.push(c),
            }
        }
        return None;
    }
    let eq = line.find('=')?;
    Some((line[..eq].trim().to_string(), eq + 1))
}

fn parse(text: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let line_end = text[i..]
            .find('\n')
            .map(|n| i + n + 1)
            .unwrap_or(text.len());
        let line = &text[i..line_end];
        if is_header(line) {
            let (kind, key) = section_key(line);
            sections.push(Section {
                key,
                kind,
                header: line.to_string(),
                props: Vec::new(),
                trailing: String::new(),
            });
            i = line_end;
            continue;
        }
        if line.trim().is_empty() {
            if let Some(last) = sections.last_mut() {
                last.trailing.push_str(line);
            }
            i = line_end;
            continue;
        }
        match (sections.last_mut(), prop_key(line)) {
            (Some(last), Some((key, eq_at))) => {
                let end = value_end(text, i + eq_at);
                // Blank lines that preceded this entry belong to the section.
                let pending = std::mem::take(&mut last.trailing);
                last.props.push(Prop {
                    key,
                    text: format!("{pending}{}", &text[i..end]),
                });
                i = end;
            }
            (Some(last), None) => {
                let pending = std::mem::take(&mut last.trailing);
                last.props.push(Prop {
                    key: format!("?{}", line.trim()),
                    text: format!("{pending}{line}"),
                });
                i = line_end;
            }
            (None, _) => {
                // Text before the first header: keep it as its own section.
                sections.push(Section {
                    key: "preamble".into(),
                    kind: Kind::Other,
                    header: line.to_string(),
                    props: Vec::new(),
                    trailing: String::new(),
                });
                i = line_end;
            }
        }
    }
    sections
}

/// The header to write for an edited section: `current`'s, keeping the
/// original's identity attributes the saver does not reproduce.
fn merged_header(orig: &Section, base: &Section, now: &Section) -> String {
    if base.header == now.header {
        return orig.header.clone();
    }
    let orig_attrs = header_attrs(&orig.header);
    let now_attrs = header_attrs(&now.header);
    let mut header = now.header.trim_end().to_string();
    if header.ends_with(']') {
        header.pop();
        for keep in ["parent_id_path", "index", "unique_id"] {
            if attr(&now_attrs, keep).is_none() {
                if let Some(v) = attr(&orig_attrs, keep) {
                    header.push_str(&format!(" {keep}={v}"));
                }
            }
        }
        header.push(']');
    }
    header.push('\n');
    header
}

/// Property-level three-way merge of one edited section.
fn merged_props(orig: &Section, base: &Section, now: &Section) -> Vec<Prop> {
    let mut out = Vec::new();
    let now_keys: HashSet<&str> = now.props.iter().map(|p| p.key.as_str()).collect();
    for p in &orig.props {
        match (base.prop(&p.key), now.prop(&p.key)) {
            // Edited: the new value replaces the line.
            (b, Some(n)) if b.map(|b| b.text.trim()) != Some(n.text.trim()) => {
                out.push(Prop {
                    key: p.key.clone(),
                    text: n.text.trim_start_matches('\n').to_string(),
                });
            }
            // Removed since the baseline.
            (Some(_), None) => {}
            // Unedited, or a value the saver never writes: keep the original.
            _ => out.push(p.clone()),
        }
    }
    let orig_keys: HashSet<&str> = orig.props.iter().map(|p| p.key.as_str()).collect();
    for n in &now.props {
        if orig_keys.contains(n.key.as_str()) || !now_keys.contains(n.key.as_str()) {
            continue;
        }
        let edited = base.prop(&n.key).map(|b| b.text.trim()) != Some(n.text.trim());
        if edited {
            out.push(Prop {
                key: n.key.clone(),
                text: n.text.trim_start_matches('\n').to_string(),
            });
        }
    }
    out
}

/// Only the properties of `now` that differ from `base` — an edited section
/// the original file never had (an instance's foreign node given an override).
fn changed_props(base: &Section, now: &Section) -> Vec<Prop> {
    now.props
        .iter()
        .filter(|n| base.prop(&n.key).map(|b| b.text.trim()) != Some(n.text.trim()))
        .map(|n| Prop {
            key: n.key.clone(),
            text: n.text.trim_start_matches('\n').to_string(),
        })
        .collect()
}

/// Resource ids a section's text references, as section keys.
fn references(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (needle, prefix) in [("SubResource(\"", "sub:"), ("ExtResource(\"", "ext:")] {
        let mut rest = text;
        while let Some(at) = rest.find(needle) {
            rest = &rest[at + needle.len()..];
            if let Some(end) = rest.find('"') {
                out.push(format!("{prefix}{}", &rest[..end]));
                rest = &rest[end..];
            }
        }
    }
    out
}

/// Merges the edits `baseline → current` into `original`. See the module docs.
pub fn merge_edits(original: &str, baseline: &str, current: &str) -> String {
    if baseline == current {
        return original.to_string();
    }
    let orig = parse(original);
    let base = parse(baseline);
    let now = parse(current);
    let base_by: HashMap<&str, &Section> = base.iter().map(|s| (s.key.as_str(), s)).collect();
    let now_by: HashMap<&str, &Section> = now.iter().map(|s| (s.key.as_str(), s)).collect();

    // 1. The original, section by section: kept, edited in place, or dropped.
    let mut out: Vec<Section> = Vec::new();
    for o in &orig {
        if o.kind == Kind::Head || o.key == "preamble" {
            out.push(o.clone());
            continue;
        }
        match (base_by.get(o.key.as_str()), now_by.get(o.key.as_str())) {
            (Some(b), Some(n)) if b.text() == n.text() => out.push(o.clone()),
            (Some(b), Some(n)) => out.push(Section {
                key: o.key.clone(),
                kind: o.kind,
                header: merged_header(o, b, n),
                props: merged_props(o, b, n),
                trailing: o.trailing.clone(),
            }),
            // Deleted since the baseline.
            (Some(_), None) => {}
            // Something the saver does not write at all: keep it.
            (None, _) => out.push(o.clone()),
        }
    }

    // 2. What `current` has that the original does not, when it is an edit.
    let mut present: HashSet<String> = out.iter().map(|s| s.key.clone()).collect();
    for (idx, n) in now.iter().enumerate() {
        if present.contains(&n.key) || n.kind == Kind::Head {
            continue;
        }
        let section = match base_by.get(n.key.as_str()) {
            // Unchanged and never in the file: an invented section.
            Some(b) if b.text() == n.text() => continue,
            // A section the file lacks whose values were edited — an override.
            Some(b) => {
                let props = changed_props(b, n);
                if props.is_empty() && b.header == n.header {
                    continue;
                }
                Section {
                    props,
                    trailing: String::new(),
                    ..(*n).clone()
                }
            }
            None => Section {
                trailing: String::new(),
                ..(*n).clone()
            },
        };
        insert(&mut out, section, &now[..idx]);
        present.insert(n.key.clone());
    }

    // 3. Resources that an edited or new section now references.
    let mut queue: Vec<String> = out
        .iter()
        .filter(|s| {
            orig.iter()
                .find(|o| o.key == s.key)
                .is_none_or(|o| o.text() != s.text())
        })
        .flat_map(|s| references(&s.text()))
        .collect();
    while let Some(key) = queue.pop() {
        if present.contains(&key) {
            continue;
        }
        let Some(n) = now_by.get(key.as_str()) else {
            continue;
        };
        let idx = now.iter().position(|s| s.key == key).unwrap_or(0);
        queue.extend(references(&n.text()));
        insert(
            &mut out,
            Section {
                trailing: String::new(),
                ..(*n).clone()
            },
            &now[..idx],
        );
        present.insert(key);
    }

    let mut text = String::new();
    for s in &out {
        text.push_str(&s.text());
        text.push_str(&s.trailing);
    }
    text
}

/// Inserts `section` where it belongs: resources after the last of their kind,
/// a node after the node that precedes it in `current`, a connection after the
/// last connection or node, an `[editable]` at the end.
fn insert(out: &mut Vec<Section>, mut section: Section, before_in_now: &[Section]) {
    let last_of =
        |out: &Vec<Section>, kinds: &[Kind]| out.iter().rposition(|s| kinds.contains(&s.kind));
    let at = match section.kind {
        Kind::Ext => last_of(out, &[Kind::Ext, Kind::Head]),
        Kind::Sub => last_of(out, &[Kind::Sub, Kind::Ext, Kind::Head]),
        Kind::Node => before_in_now
            .iter()
            .rev()
            .filter(|p| p.kind == Kind::Node)
            .find_map(|p| out.iter().position(|s| s.key == p.key))
            .map(|i| {
                // After that node's whole run of descendants in the output.
                let prefix = node_path_of(&out[i]);
                let mut j = i;
                while j + 1 < out.len()
                    && out[j + 1].kind == Kind::Node
                    && is_below(&out[j + 1], &prefix)
                {
                    j += 1;
                }
                j
            })
            .or_else(|| last_of(out, &[Kind::Node, Kind::Sub, Kind::Ext, Kind::Head])),
        Kind::Connection => last_of(out, &[Kind::Connection, Kind::Node]),
        _ => None,
    };
    match at {
        Some(i) => {
            let prev = &mut out[i];
            let same_run = prev.kind == Kind::Ext && section.kind == Kind::Ext
                || prev.kind == Kind::Connection && section.kind == Kind::Connection;
            section.trailing = std::mem::take(&mut prev.trailing);
            prev.trailing = if same_run { String::new() } else { "\n".into() };
            if section.trailing.is_empty() && i + 1 < out.len() {
                section.trailing = "\n".into();
            }
            out.insert(i + 1, section);
        }
        None => {
            if let Some(last) = out.last_mut() {
                if last.trailing.is_empty() {
                    last.trailing = "\n".into();
                }
            }
            out.push(section);
        }
    }
}

/// A node section's scene-relative path (`.` for the root).
fn node_path_of(s: &Section) -> String {
    let rest = s.key.strip_prefix("node:").unwrap_or(&s.key);
    let (parent, name) = rest.split_once('|').unwrap_or(("", rest));
    match parent {
        "" => ".".into(),
        "." => name.into(),
        p => format!("{p}/{name}"),
    }
}

fn is_below(s: &Section, prefix: &str) -> bool {
    let rest = s.key.strip_prefix("node:").unwrap_or(&s.key);
    let parent = rest.split_once('|').map(|(p, _)| p).unwrap_or("");
    if prefix == "." {
        return !parent.is_empty();
    }
    parent == prefix || parent.starts_with(&format!("{prefix}/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIG: &str = "[gd_scene format=3 uid=\"uid://x\"]\n\n[ext_resource type=\"PackedScene\" uid=\"uid://g\" path=\"res://p.glb\" id=\"1_g\"]\n\n[node name=\"P\" type=\"Node3D\" unique_id=1]\n\n[node name=\"G\" parent=\".\" unique_id=2 instance=ExtResource(\"1_g\")]\n\n[node name=\"Robot\" parent=\"G/Skeleton\" parent_id_path=PackedInt32Array(2, 7) index=\"0\" unique_id=3]\nlayers = 2\n\n[editable path=\"G\"]\n";

    // What a saver that knows nothing of ids, and writes foreign nodes, emits.
    const BASE: &str = "[gd_scene load_steps=2 format=3]\n\n[ext_resource type=\"PackedScene\" path=\"res://p.glb\" id=\"1_g\"]\n\n[node name=\"P\" type=\"Node3D\"]\n\n[node name=\"G\" parent=\".\" instance=ExtResource(\"1_g\")]\n\n[node name=\"Skeleton\" parent=\"G\"]\nfoo = 1\n\n[node name=\"Robot\" parent=\"G/Skeleton\"]\nlayers = 2\nmesh = SubResource(\"m\")\n\n[editable path=\"G\"]\n";

    #[test]
    fn an_unedited_scene_is_the_original_byte_for_byte() {
        assert_eq!(merge_edits(ORIG, BASE, BASE), ORIG);
    }

    #[test]
    fn an_edited_override_keeps_its_identity_attributes_and_other_lines() {
        let now = BASE.replace("layers = 2", "layers = 5");
        let out = merge_edits(ORIG, BASE, &now);
        assert_eq!(out, ORIG.replace("layers = 2", "layers = 5"));
    }

    #[test]
    fn a_foreign_node_edited_for_the_first_time_becomes_an_override_of_just_that_value() {
        let now = BASE.replace("foo = 1", "foo = 9");
        let out = merge_edits(ORIG, BASE, &now);
        assert!(
            out.contains("[node name=\"Skeleton\" parent=\"G\"]\nfoo = 9\n"),
            "{out}"
        );
        // Inserted after its parent, so the file still declares parents first.
        assert!(
            out.find("name=\"Skeleton\"").unwrap() > out.find("name=\"G\"").unwrap(),
            "{out}"
        );
        assert!(
            out.contains("parent_id_path=PackedInt32Array(2, 7)"),
            "{out}"
        );
    }

    #[test]
    fn a_deleted_node_is_removed_and_an_added_one_is_written() {
        let now = BASE.replace(
            "[node name=\"Robot\" parent=\"G/Skeleton\"]\nlayers = 2\nmesh = SubResource(\"m\")\n\n",
            "[node name=\"New\" type=\"Node3D\" parent=\".\"]\nvisible = false\n\n",
        );
        let out = merge_edits(ORIG, BASE, &now);
        assert!(!out.contains("Robot"), "{out}");
        assert!(
            out.contains("[node name=\"New\" type=\"Node3D\" parent=\".\"]\nvisible = false\n"),
            "{out}"
        );
        assert!(
            out.find("name=\"New\"").unwrap() < out.find("[editable").unwrap(),
            "{out}"
        );
    }

    #[test]
    fn multi_line_values_and_quoted_keys_are_one_entry() {
        let text = "[node name=\"A\" type=\"Node\"]\n\"a b/c\" = 1\nd = {\n\"x\": 2\n}\ns = \"line\n[node name=\\\"fake\\\"]\"\n";
        let s = parse(text);
        assert_eq!(s.len(), 1);
        let keys: Vec<&str> = s[0].props.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(keys, ["a b/c", "d", "s"]);
    }

    #[test]
    fn header_attributes_parse_lists_and_quotes() {
        let a = header_attrs("[node name=\"a b\" parent=\"P/Q\" parent_id_path=PackedInt32Array(1, 2) index=\"0\" unique_id=5]\n");
        assert_eq!(attr(&a, "name"), Some("\"a b\""));
        assert_eq!(attr(&a, "parent_id_path"), Some("PackedInt32Array(1, 2)"));
        assert_eq!(attr(&a, "unique_id"), Some("5"));
    }
}
