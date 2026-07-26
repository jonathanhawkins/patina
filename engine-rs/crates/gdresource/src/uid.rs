//! Unique resource identifiers and path resolution.
//!
//! The [`UidRegistry`] maintains a bidirectional mapping between
//! [`ResourceUid`] values and `res://` paths, allowing resources to be
//! looked up by either identifier.
//!
//! [`id_to_text`] / [`text_to_id`] implement Godot's `uid://…` base-34 codec
//! (ported from `core/io/resource_uid.cpp`), and the registry can be populated
//! from a `.godot/uid_cache.bin` file or from individual `*.uid` sidecar files.

use std::collections::HashMap;
use std::io::{self, Read};
use std::path::Path;

use gdcore::ResourceUid;

// ---------------------------------------------------------------------------
// Godot `uid://` base-34 text codec (core/io/resource_uid.cpp)
// ---------------------------------------------------------------------------

/// The character table used to encode UIDs, ported verbatim from Godot's
/// `uuid_characters` (`resource_uid.cpp`).
///
/// It deliberately omits `'z'` and `'9'`: Godot's constants are off by one, so
/// those two characters are never emitted. This cannot be changed without
/// breaking compatibility (see GH-83843), so the quirk is preserved here.
const UID_CHARS: &[u8; 34] = b"abcdefghijklmnopqrstuvwxy012345678";

/// The numeric base used by the codec: `('z' - 'a') + ('9' - '0')` = 25 + 9.
const UID_BASE: u64 = 34;

/// The number of table entries used by the encoder (`uuid_characters_element_count`).
const UID_ENCODE_DIVISOR: u64 = 34;

/// Godot's sentinel string for an invalid UID.
pub const UID_INVALID_TEXT: &str = "uid://<invalid>";

/// Encodes a UID into its `uid://…` text form (Godot `ResourceUID::id_to_text`).
///
/// Negative ids (including [`ResourceUid::INVALID`]) encode to
/// [`UID_INVALID_TEXT`]. `id == 0` encodes to `"uid://a"`.
pub fn id_to_text(id: i64) -> String {
    if id < 0 {
        return UID_INVALID_TEXT.to_string();
    }

    // Emit base-34 "digits" least-significant first (do-while: always ≥ 1 digit).
    let mut n = id as u64;
    let mut digits: Vec<u8> = Vec::new();
    loop {
        let c = (n % UID_ENCODE_DIVISOR) as usize;
        digits.push(UID_CHARS[c]);
        n /= UID_ENCODE_DIVISOR;
        if n == 0 {
            break;
        }
    }

    let mut txt = String::with_capacity(digits.len() + 6);
    txt.push_str("uid://");
    // The loop produced the number backwards; reverse it.
    for &b in digits.iter().rev() {
        txt.push(b as char);
    }
    txt
}

/// Decodes a `uid://…` text form into a UID (Godot `ResourceUID::text_to_id`).
///
/// Returns [`ResourceUid::INVALID`] if `text` does not begin with `uid://`, is
/// the `uid://<invalid>` sentinel, or contains any character outside `[a-z0-9]`
/// after the prefix.
///
/// The mapping mirrors Godot exactly, including the off-by-one quirk: lowercase
/// letters map `'a'..='z'` → `0..=25` and digits map `'0'..='9'` → `25..=34`, so
/// `'z'` and `'0'` collide and `'9'` yields 34 — values the encoder never emits.
pub fn text_to_id(text: &str) -> ResourceUid {
    if !text.starts_with("uid://") || text == UID_INVALID_TEXT {
        return ResourceUid::INVALID;
    }

    // UID text is ASCII, so byte iteration matches Godot's per-char loop.
    let bytes = text.as_bytes();
    let mut uid: u64 = 0;
    for &c in &bytes[6..] {
        uid = uid.wrapping_mul(UID_BASE);
        if c.is_ascii_lowercase() {
            uid = uid.wrapping_add((c - b'a') as u64);
        } else if c.is_ascii_digit() {
            uid = uid.wrapping_add((c - b'0') as u64 + 25);
        } else {
            return ResourceUid::INVALID;
        }
    }

    ResourceUid::new((uid & 0x7FFF_FFFF_FFFF_FFFF) as i64)
}

/// Parses the content of a `*.uid` sidecar file (a single `uid://…` line).
///
/// Leading/trailing whitespace and blank lines are ignored; the first
/// non-empty line is decoded. Returns [`ResourceUid::INVALID`] if no valid
/// `uid://` line is present.
pub fn parse_uid_sidecar(content: &str) -> ResourceUid {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        return text_to_id(line);
    }
    ResourceUid::INVALID
}

/// A bidirectional registry mapping resource UIDs to paths and vice versa.
#[derive(Debug, Default)]
pub struct UidRegistry {
    uid_to_path: HashMap<i64, String>,
    path_to_uid: HashMap<String, ResourceUid>,
}

impl UidRegistry {
    /// Creates a new, empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a UID-to-path mapping. Overwrites any previous mapping for
    /// either the UID or the path.
    pub fn register(&mut self, uid: ResourceUid, path: impl Into<String>) {
        let path = path.into();
        // Remove stale reverse mapping if this UID was previously bound.
        if let Some(old_path) = self.uid_to_path.get(&uid.raw()) {
            self.path_to_uid.remove(old_path);
        }
        // Remove stale forward mapping if this path was previously bound.
        if let Some(old_uid) = self.path_to_uid.get(&path) {
            self.uid_to_path.remove(&old_uid.raw());
        }
        self.uid_to_path.insert(uid.raw(), path.clone());
        self.path_to_uid.insert(path, uid);
    }

    /// Removes the mapping for a given UID.
    pub fn unregister_uid(&mut self, uid: ResourceUid) {
        if let Some(path) = self.uid_to_path.remove(&uid.raw()) {
            self.path_to_uid.remove(&path);
        }
    }

    /// Removes the mapping for a given path.
    pub fn unregister_path(&mut self, path: &str) {
        if let Some(uid) = self.path_to_uid.remove(path) {
            self.uid_to_path.remove(&uid.raw());
        }
    }

    /// Looks up the path for a given UID.
    pub fn lookup_uid(&self, uid: ResourceUid) -> Option<&str> {
        self.uid_to_path.get(&uid.raw()).map(String::as_str)
    }

    /// Godot `ResourceUID.get_id_path(id)`: the `res://` path a UID resolves to,
    /// or `None`. Alias of [`lookup_uid`](Self::lookup_uid) matching Godot's name.
    pub fn get_id_path(&self, uid: ResourceUid) -> Option<&str> {
        self.lookup_uid(uid)
    }

    /// Looks up the UID for a given path.
    pub fn lookup_path(&self, path: &str) -> Option<ResourceUid> {
        self.path_to_uid.get(path).copied()
    }

    /// Returns the number of registered mappings.
    pub fn len(&self) -> usize {
        self.uid_to_path.len()
    }

    /// Returns `true` if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.uid_to_path.is_empty()
    }

    /// Removes all mappings.
    pub fn clear(&mut self) {
        self.uid_to_path.clear();
        self.path_to_uid.clear();
    }

    /// Loads UID→path mappings from a `uid_cache.bin` byte buffer, registering
    /// each into this registry. Returns the number of entries read.
    ///
    /// Matches Godot's `ResourceUID::load_from_cache` binary format:
    ///
    /// ```text
    /// entry_count: u32 LE
    /// per entry:
    ///   id:      u64 LE
    ///   str_len: u32 LE
    ///   path:    str_len UTF-8 bytes (no NUL terminator)
    /// ```
    pub fn load_from_cache_bytes(&mut self, data: &[u8]) -> io::Result<usize> {
        let mut cursor = io::Cursor::new(data);
        self.load_from_cache_reader(&mut cursor)
    }

    /// Loads UID→path mappings from any reader in the `uid_cache.bin` format.
    pub fn load_from_cache_reader<R: Read>(&mut self, reader: &mut R) -> io::Result<usize> {
        let entry_count = read_u32(reader)?;
        for _ in 0..entry_count {
            let id = read_u64(reader)? as i64;
            let len = read_u32(reader)? as usize;
            let mut buf = vec![0u8; len];
            reader.read_exact(&mut buf)?;
            let path = String::from_utf8(buf).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid UTF-8 in UID cache: {e}"),
                )
            })?;
            self.register(ResourceUid::new(id), path);
        }
        Ok(entry_count as usize)
    }

    /// Registers a mapping described by a `*.uid` sidecar's text content and the
    /// path of the source asset it sits next to.
    ///
    /// Returns the decoded [`ResourceUid`], or [`ResourceUid::INVALID`] (without
    /// registering) if the sidecar has no valid `uid://` line.
    pub fn register_uid_sidecar(
        &mut self,
        source_path: impl Into<String>,
        sidecar_content: &str,
    ) -> ResourceUid {
        let uid = parse_uid_sidecar(sidecar_content);
        if uid.is_valid() {
            self.register(uid, source_path);
        }
        uid
    }

    /// Reads a `*.uid` sidecar file from disk and registers it against its
    /// source asset (the sidecar path with the `.uid` extension removed).
    ///
    /// For example `res://icon.svg.uid` registers the contained UID against
    /// `res://icon.svg`. Returns the decoded UID.
    pub fn load_uid_sidecar_file(&mut self, sidecar_path: &Path) -> io::Result<ResourceUid> {
        let content = std::fs::read_to_string(sidecar_path)?;
        let source_path = sidecar_path
            .to_string_lossy()
            .strip_suffix(".uid")
            .map(|s| s.to_string())
            .unwrap_or_else(|| sidecar_path.to_string_lossy().to_string());
        Ok(self.register_uid_sidecar(source_path, &content))
    }
}

fn read_u32<R: Read>(reader: &mut R) -> io::Result<u32> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

fn read_u64<R: Read>(reader: &mut R) -> io::Result<u64> {
    let mut buf = [0u8; 8];
    reader.read_exact(&mut buf)?;
    Ok(u64::from_le_bytes(buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_lookup() {
        let mut reg = UidRegistry::new();
        let uid = ResourceUid::new(100);
        reg.register(uid, "res://my_resource.tres");

        assert_eq!(reg.lookup_uid(uid), Some("res://my_resource.tres"));
        assert_eq!(reg.lookup_path("res://my_resource.tres"), Some(uid));
    }

    #[test]
    fn lookup_missing_returns_none() {
        let reg = UidRegistry::new();
        assert_eq!(reg.lookup_uid(ResourceUid::new(999)), None);
        assert_eq!(reg.lookup_path("res://nothing"), None);
    }

    #[test]
    fn unregister_uid() {
        let mut reg = UidRegistry::new();
        let uid = ResourceUid::new(1);
        reg.register(uid, "res://a.tres");
        reg.unregister_uid(uid);

        assert_eq!(reg.lookup_uid(uid), None);
        assert_eq!(reg.lookup_path("res://a.tres"), None);
        assert!(reg.is_empty());
    }

    #[test]
    fn unregister_path() {
        let mut reg = UidRegistry::new();
        let uid = ResourceUid::new(2);
        reg.register(uid, "res://b.tres");
        reg.unregister_path("res://b.tres");

        assert_eq!(reg.lookup_uid(uid), None);
        assert_eq!(reg.lookup_path("res://b.tres"), None);
    }

    #[test]
    fn overwrite_mapping() {
        let mut reg = UidRegistry::new();
        let uid = ResourceUid::new(10);
        reg.register(uid, "res://old.tres");
        reg.register(uid, "res://new.tres");

        assert_eq!(reg.lookup_uid(uid), Some("res://new.tres"));
        assert_eq!(reg.lookup_path("res://old.tres"), None);
        assert_eq!(reg.lookup_path("res://new.tres"), Some(uid));
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn register_duplicate_uid_overwrites_previous_path() {
        let mut reg = UidRegistry::new();
        let uid = ResourceUid::new(42);
        reg.register(uid, "res://first.tres");
        reg.register(uid, "res://second.tres");

        assert_eq!(reg.lookup_uid(uid), Some("res://second.tres"));
        assert_eq!(reg.lookup_path("res://first.tres"), None);
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn register_same_path_different_uid_replaces() {
        let mut reg = UidRegistry::new();
        let uid1 = ResourceUid::new(1);
        let uid2 = ResourceUid::new(2);
        reg.register(uid1, "res://shared.tres");
        reg.register(uid2, "res://shared.tres");

        assert_eq!(reg.lookup_path("res://shared.tres"), Some(uid2));
        assert_eq!(reg.lookup_uid(uid1), None); // old uid removed
        assert_eq!(reg.lookup_uid(uid2), Some("res://shared.tres"));
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn unregister_nonexistent_uid_does_not_panic() {
        let mut reg = UidRegistry::new();
        reg.unregister_uid(ResourceUid::new(999));
        assert!(reg.is_empty());
    }

    #[test]
    fn unregister_nonexistent_path_does_not_panic() {
        let mut reg = UidRegistry::new();
        reg.unregister_path("res://nonexistent.tres");
        assert!(reg.is_empty());
    }

    #[test]
    fn clear_removes_all() {
        let mut reg = UidRegistry::new();
        reg.register(ResourceUid::new(1), "res://a.tres");
        reg.register(ResourceUid::new(2), "res://b.tres");
        assert_eq!(reg.len(), 2);

        reg.clear();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    // -----------------------------------------------------------------------
    // Base-34 codec tests (Godot resource_uid.cpp)
    // -----------------------------------------------------------------------

    #[test]
    fn codec_roundtrips_several_ids() {
        for id in [0i64, 1, 25, 33, 34, 100, 1000, 12345, 0x7FFF_FFFF_FFFF_FFFF] {
            let text = id_to_text(id);
            assert!(text.starts_with("uid://"));
            assert_eq!(text_to_id(&text).raw(), id, "roundtrip failed for {id}");
        }
    }

    #[test]
    fn codec_hardcoded_vectors() {
        // Derived by hand-executing Godot's id_to_text (base-34, table a-y,0-8):
        //   0     -> 0%34=0  -> 'a'                      => "uid://a"
        //   33    -> 33%34=33 -> table[33]='8'           => "uid://8"
        //   34    -> [34/34=1 -> 'b'][34%34=0 -> 'a']    => "uid://ba"
        //   1000  -> [1000/34=29 -> '4'][1000%34=14 -> 'o'] => "uid://4o"
        //   12345 -> 10*34² + 23*34 + 3 -> 'k','x','d'   => "uid://kxd"
        assert_eq!(id_to_text(0), "uid://a");
        assert_eq!(id_to_text(33), "uid://8");
        assert_eq!(id_to_text(34), "uid://ba");
        assert_eq!(id_to_text(1000), "uid://4o");
        assert_eq!(id_to_text(12345), "uid://kxd");

        assert_eq!(text_to_id("uid://a").raw(), 0);
        assert_eq!(text_to_id("uid://8").raw(), 33);
        assert_eq!(text_to_id("uid://ba").raw(), 34);
        assert_eq!(text_to_id("uid://4o").raw(), 1000);
        assert_eq!(text_to_id("uid://kxd").raw(), 12345);
    }

    #[test]
    fn id_to_text_negative_is_invalid_sentinel() {
        assert_eq!(id_to_text(-1), "uid://<invalid>");
        assert_eq!(id_to_text(ResourceUid::INVALID.raw()), "uid://<invalid>");
    }

    #[test]
    fn text_to_id_rejects_invalid_inputs() {
        assert!(!text_to_id("res://not_a_uid.tres").is_valid()); // no uid:// prefix
        assert!(!text_to_id("uid://<invalid>").is_valid()); // sentinel
        assert!(!text_to_id("uid://ABC").is_valid()); // uppercase not allowed
        assert!(!text_to_id("uid://ab_cd").is_valid()); // underscore not allowed
    }

    // -----------------------------------------------------------------------
    // uid_cache.bin + .uid sidecar tests
    // -----------------------------------------------------------------------

    /// pat-nzgdu.134: a `uid_cache.bin` mapping resolves a `uid://` to its
    /// `res://` path — `ResourceUID.get_id_path(text_to_id("uid://…"))` returns
    /// the mapped path (the same resolution the loader uses for `load("uid://…")`).
    #[test]
    fn uid_resolves_to_path() {
        // Hand-built uid_cache.bin (entry_count u32, then id u64, len u32, bytes)
        // mapping id 12345 → res://sub.tscn.
        let path = "res://sub.tscn";
        let mut cache: Vec<u8> = Vec::new();
        cache.extend_from_slice(&1u32.to_le_bytes()); // entry_count
        cache.extend_from_slice(&12345u64.to_le_bytes()); // id
        cache.extend_from_slice(&(path.len() as u32).to_le_bytes()); // str_len
        cache.extend_from_slice(path.as_bytes());

        let mut reg = UidRegistry::new();
        assert_eq!(reg.load_from_cache_bytes(&cache).unwrap(), 1);

        // get_id_path resolves the cached uid to its path.
        assert_eq!(
            reg.get_id_path(ResourceUid::new(12345)),
            Some("res://sub.tscn"),
        );
        // The uid text "uid://kxd" decodes to the same id 12345, so resolving
        // via text_to_id → get_id_path returns the same path.
        let uid = text_to_id("uid://kxd");
        assert_eq!(uid.raw(), 12345, "the uid text decodes to the cached id");
        assert_eq!(reg.get_id_path(uid), Some("res://sub.tscn"));
        // The reverse mapping (path → uid) resolves too. (ResourceUid has no
        // Debug impl, so compare the raw ids rather than the values directly.)
        assert_eq!(
            reg.lookup_path("res://sub.tscn").map(|u| u.raw()),
            Some(12345),
        );
    }

    #[test]
    fn load_from_cache_bytes_fixture() {
        // Hand-built uid_cache.bin: entry_count then (id u64, len u32, bytes).
        let mut d: Vec<u8> = Vec::new();
        d.extend_from_slice(&2u32.to_le_bytes()); // entry_count = 2
                                                  // entry 1: id 12345 -> "res://a.tres"
        d.extend_from_slice(&12345u64.to_le_bytes());
        d.extend_from_slice(&12u32.to_le_bytes());
        d.extend_from_slice(b"res://a.tres");
        // entry 2: id 34 -> "res://b.tres"
        d.extend_from_slice(&34u64.to_le_bytes());
        d.extend_from_slice(&12u32.to_le_bytes());
        d.extend_from_slice(b"res://b.tres");

        let mut reg = UidRegistry::new();
        let n = reg.load_from_cache_bytes(&d).unwrap();
        assert_eq!(n, 2);
        assert_eq!(reg.len(), 2);
        assert_eq!(
            reg.lookup_uid(ResourceUid::new(12345)),
            Some("res://a.tres")
        );
        // Resolvable via the decoded uid:// text as well.
        assert_eq!(
            reg.lookup_uid(text_to_id("uid://kxd")),
            Some("res://a.tres")
        );
        assert_eq!(reg.lookup_uid(ResourceUid::new(34)), Some("res://b.tres"));
    }

    #[test]
    fn load_from_cache_bytes_truncated_errors() {
        // Claims 1 entry but provides no body.
        let d = 1u32.to_le_bytes().to_vec();
        assert!(UidRegistry::new().load_from_cache_bytes(&d).is_err());
    }

    #[test]
    fn sidecar_content_registers_source_path() {
        let mut reg = UidRegistry::new();
        let uid = reg.register_uid_sidecar("res://icon.svg", "uid://kxd\n");
        assert_eq!(uid.raw(), 12345);
        assert_eq!(reg.lookup_uid(uid), Some("res://icon.svg"));
        assert_eq!(reg.lookup_path("res://icon.svg"), Some(uid));
    }

    #[test]
    fn sidecar_invalid_content_not_registered() {
        let mut reg = UidRegistry::new();
        let uid = reg.register_uid_sidecar("res://x.svg", "not a uid\n");
        assert!(!uid.is_valid());
        assert!(reg.is_empty());
    }

    #[test]
    fn load_uid_sidecar_file_reads_and_registers() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let sidecar = dir.path().join("icon.svg.uid");
        let mut f = std::fs::File::create(&sidecar).unwrap();
        writeln!(f, "uid://kxd").unwrap();
        drop(f);

        let mut reg = UidRegistry::new();
        let uid = reg.load_uid_sidecar_file(&sidecar).unwrap();
        assert_eq!(uid.raw(), 12345);

        // The source path is the sidecar path with the ".uid" suffix stripped.
        let expected = sidecar
            .to_string_lossy()
            .strip_suffix(".uid")
            .unwrap()
            .to_string();
        assert_eq!(reg.lookup_uid(uid), Some(expected.as_str()));
    }
}
