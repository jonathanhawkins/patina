//! `project.binary` — the binary ProjectSettings an exported game ships
//! (pat-mgixk.56).
//!
//! # Why this exists
//!
//! A Godot export does **not** put `project.godot` in the pack. It writes
//! `res://project.binary` instead (`editor/export/editor_export_platform.cpp:1676`),
//! and the engine loads that in preference to the text file, falling back to
//! `project.godot` only when the binary is absent
//! (`core/config/project_settings.cpp:993-1002`, `_load_settings_text_or_binary`).
//!
//! `boot_project_from_pck` required `res://project.godot`, so **no real exported
//! `.pck` could start at all** — only one packed by hand from a loose project.
//!
//! # The format
//!
//! `_load_settings_binary` (`project_settings.cpp:903`) is short and total:
//!
//! ```text
//! "ECFG"                     4 bytes, else ERR_FILE_CORRUPT
//! count                      u32
//! count x {
//!     key_len                u32
//!     key                    key_len bytes of UTF-8   (NOT 4-aligned)
//!     value_len              u32
//!     value                  value_len bytes, a marshalled Variant
//! }
//! ```
//!
//! ⚠️ The key is **not** padded to a 4-byte boundary, unlike a `String` *inside*
//! a marshalled Variant (`marshalls.cpp` `_encode_string` pads; this loop does
//! `get_buffer(.., slen)` and nothing more). Reading it as a padded string
//! desynchronises every later entry.
//!
//! # Why this lives in `gdresource` and not next to `ProjectSettings`
//!
//! 🔑 The obvious home is `gdcore::ProjectSettings`, but it cannot go there:
//! decoding a value needs `gdvariant::binary::decode_variant`, and **`gdvariant`
//! depends on `gdcore`**, so using it from `gdcore` is a dependency cycle.
//! `gdcore/src/project_settings.rs:34` says as much about its own value type —
//! `SettingsValue` is "simpler than the full `gdvariant::Variant`". `gdresource`
//! already depends on both, so the decoder belongs here and hands back a
//! finished [`gdcore::ProjectSettings`].

use gdcore::error::{EngineError, EngineResult};
use gdcore::{ProjectSettings, SettingsValue};
use gdvariant::Variant;

/// The result of decoding a `project.binary`.
///
/// `Debug` so a test can `expect_err` on a `Result` carrying this — that helper
/// prints the unexpected `Ok`, so it needs the bound.
#[derive(Debug)]
pub struct BinaryProjectSettings {
    /// Every entry whose value [`SettingsValue`] can represent.
    pub settings: ProjectSettings,
    /// Entries that decoded as a Variant but have no `SettingsValue` shape,
    /// as `(key, variant type name)`.
    ///
    /// 🔑 Reported rather than dropped on purpose. `SettingsValue` covers seven
    /// scalar shapes, and a real `project.binary` also carries the input map
    /// (`input/<action>` is a Dictionary of InputEvent objects) and
    /// `application/config/features` (a PackedStringArray). A decoder that
    /// silently skipped those would look like it had read the whole file, and
    /// the loss would surface much later as a missing input action.
    pub skipped: Vec<(String, &'static str)>,
}

/// Decodes `project.binary` bytes into [`ProjectSettings`].
///
/// Mirrors `ProjectSettings::_load_settings_binary`: a value that fails to
/// decode is skipped with a warning rather than failing the whole file, because
/// Godot does the same (`ERR_CONTINUE_MSG`, `project_settings.cpp:930`) and a
/// game that boots without one setting beats a game that does not boot.
pub fn parse_project_binary(bytes: &[u8]) -> EngineResult<BinaryProjectSettings> {
    let mut r = Cursor { bytes, pos: 0 };

    let magic = r.take(4, "ECFG magic")?;
    if magic != b"ECFG" {
        return Err(EngineError::Parse(format!(
            "not a project.binary: expected the magic ECFG, got {magic:?}"
        )));
    }

    let count = r.u32("entry count")?;
    let mut settings = ProjectSettings::new();
    let mut skipped = Vec::new();

    for i in 0..count {
        let key_len = r.u32("key length")? as usize;
        let key_bytes = r.take(key_len, "key")?;
        let key = String::from_utf8(key_bytes.to_vec()).map_err(|e| {
            EngineError::Parse(format!("project.binary key {i} is not valid UTF-8: {e}"))
        })?;

        let value_len = r.u32("value length")? as usize;
        let value_bytes = r.take(value_len, "value")?;

        // ⚠️ A key with no `/` is skipped, and that is a crash guard, not
        // tidiness. `ProjectSettings::sections()` returns the whole key when it
        // contains no slash, and `to_godot_format` then does
        // `&key[section.len() + 1..]` — a byte index one past the end, which
        // panics. Every real setting is `section/path`, and the keys here come
        // from a pack we did not write, so the unsectioned case has to be
        // refused rather than trusted.
        if !key.contains('/') {
            skipped.push((key, "unsectioned key"));
            continue;
        }

        match gdvariant::binary::decode_variant(value_bytes) {
            Ok((variant, _)) => match settings_value_from_variant(&variant) {
                Some(v) => settings.set(&key, v),
                None => skipped.push((key, variant_type_name(&variant))),
            },
            Err(e) => {
                // Matches Godot's ERR_CONTINUE_MSG: name the key and carry on.
                tracing::warn!(key = %key, error = ?e, "project.binary: could not decode value");
                skipped.push((key, "undecodable"));
            }
        }
    }

    Ok(BinaryProjectSettings { settings, skipped })
}

/// The `SettingsValue` for a decoded Variant, or `None` if it has no such shape.
///
/// `Vector2i`/`Vector3i` widen into the float vectors because `SettingsValue`
/// has no integer vector — and that is lossless here, since every value Godot
/// stores in them (a window size, a cell count) is far inside f64's exact
/// integer range.
fn settings_value_from_variant(v: &Variant) -> Option<SettingsValue> {
    Some(match v {
        Variant::Bool(b) => SettingsValue::Bool(*b),
        Variant::Int(i) => SettingsValue::Int(*i),
        Variant::Float(f) => SettingsValue::Float(*f),
        Variant::String(s) => SettingsValue::String(s.clone()),
        Variant::StringName(s) => SettingsValue::String(s.to_string()),
        Variant::Vector2(p) => SettingsValue::Vector2(p.x as f64, p.y as f64),
        Variant::Vector2i(p) => SettingsValue::Vector2(p.x as f64, p.y as f64),
        Variant::Vector3(p) => SettingsValue::Vector3(p.x as f64, p.y as f64, p.z as f64),
        Variant::Vector3i(p) => SettingsValue::Vector3(p.x as f64, p.y as f64, p.z as f64),
        Variant::Color(c) => SettingsValue::Color(c.r as f64, c.g as f64, c.b as f64, c.a as f64),
        _ => return None,
    })
}

/// A stable name for the skipped report. Only needs to be good enough to
/// identify what shape was lost.
fn variant_type_name(v: &Variant) -> &'static str {
    match v {
        Variant::Nil => "Nil",
        Variant::NodePath(_) => "NodePath",
        Variant::Rect2(_) | Variant::Rect2i(_) => "Rect2",
        Variant::Vector4(_) | Variant::Vector4i(_) => "Vector4",
        Variant::Transform2D(_) => "Transform2D",
        Variant::Basis(_) => "Basis",
        Variant::Transform3D(_) => "Transform3D",
        Variant::Array(_) => "Array",
        Variant::Dictionary(_) => "Dictionary",
        _ => "unsupported",
    }
}

/// A bounds-checked forward reader.
///
/// Every read names what it wanted, so a truncated file says which field ran
/// out rather than panicking on a slice index.
struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize, what: &str) -> EngineResult<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or_else(|| {
            EngineError::Parse(format!("project.binary: {what} length {n} overflows"))
        })?;
        if end > self.bytes.len() {
            return Err(EngineError::Parse(format!(
                "project.binary truncated: wanted {n} bytes for {what} at offset {}, only {} left",
                self.pos,
                self.bytes.len().saturating_sub(self.pos)
            )));
        }
        let slice = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn u32(&mut self, what: &str) -> EngineResult<u32> {
        let b = self.take(4, what)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u32le(v: u32, out: &mut Vec<u8>) {
        out.extend_from_slice(&v.to_le_bytes());
    }

    /// `"ECFG"`, a count, then one `application/config/name` entry holding a
    /// marshalled `String` (type ordinal 4, `u32 len` + bytes + pad to 4).
    ///
    /// Built from the C++ reader's field order rather than with
    /// `gdvariant::encode_variant`, so a disagreement with Godot shows up here
    /// instead of cancelling out against our own encoder.
    fn one_string_entry() -> Vec<u8> {
        let mut v = b"ECFG".to_vec();
        u32le(1, &mut v);
        let key = "application/config/name";
        u32le(key.len() as u32, &mut v);
        v.extend_from_slice(key.as_bytes());
        let mut val = Vec::new();
        u32le(4, &mut val); // Variant::STRING
        let s = "Exported Game";
        u32le(s.len() as u32, &mut val);
        val.extend_from_slice(s.as_bytes());
        val.resize(val.len() + (4 - s.len() % 4) % 4, 0);
        u32le(val.len() as u32, &mut v);
        v.extend_from_slice(&val);
        v
    }

    #[test]
    fn decodes_a_string_entry() {
        let d = parse_project_binary(&one_string_entry()).expect("decodes");
        assert_eq!(
            d.settings.project_name(),
            "Exported Game",
            "the key is NOT 4-aligned in an ECFG entry; padding it would \
             desynchronise the value that follows"
        );
        assert!(d.skipped.is_empty(), "nothing to skip: {:?}", d.skipped);
    }

    #[test]
    fn a_wrong_magic_is_refused_by_name() {
        let err = parse_project_binary(b"config_version=5\n").expect_err("not ECFG");
        assert!(
            format!("{err}").contains("ECFG"),
            "a text project.godot handed here by mistake must not read as a \
             corrupt binary; got: {err}"
        );
    }

    #[test]
    fn a_truncated_file_is_an_error_not_a_panic() {
        let full = one_string_entry();
        // Past the magic and count, so the entry loop starts and then runs out —
        // the case a length-trusting reader would slice straight past.
        let err = parse_project_binary(&full[..12]).expect_err("truncated");
        assert!(
            format!("{err}").contains("truncated"),
            "the error should say what ran out; got: {err}"
        );
    }

    /// An unsectioned key is reported, not stored.
    ///
    /// 🔑 Storing it would panic later: `to_godot_format` does
    /// `&key[section.len() + 1..]` and `sections()` returns the whole key when
    /// it has no slash.
    #[test]
    fn an_unsectioned_key_is_skipped_rather_than_stored() {
        let mut v = b"ECFG".to_vec();
        u32le(1, &mut v);
        let key = "noslash";
        u32le(key.len() as u32, &mut v);
        v.extend_from_slice(key.as_bytes());
        let mut val = Vec::new();
        u32le(2, &mut val); // Variant::INT, no 64-bit flag
        u32le(7, &mut val);
        u32le(val.len() as u32, &mut v);
        v.extend_from_slice(&val);

        let d = parse_project_binary(&v).expect("decodes");
        assert!(!d.settings.has("noslash"), "must not be stored");
        assert_eq!(
            d.skipped,
            vec![("noslash".to_string(), "unsectioned key")],
            "and it must be REPORTED, so the loss is visible"
        );
        // The guard's whole point: rendering back to text must not panic.
        let _ = d.settings.to_godot_format();
    }
}
