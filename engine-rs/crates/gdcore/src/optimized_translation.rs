//! Decoding for Godot's binary `OptimizedTranslation` resource (pat-s3rrv.213).
//!
//! Ported from the vendored reference, not from a description of the format:
//! `upstream/godot/core/string/optimized_translation.{h,cpp}` for the hash /
//! bucket layout and `upstream/godot/thirdparty/misc/smaz.c` for the string
//! codec. The reverse codebook below was extracted from `smaz.c` mechanically
//! rather than retyped, and verified entry-for-entry against it. ⚠️ That matters
//! more than it looks: **51** of the 254 entries differ from another entry only
//! in leading/trailing whitespace — `"a"`, `" a"`, `"a "` and `" a "` are four
//! separate codes — so a hand transcription would corrupt the table silently and
//! mis-decode only the strings that happened to use the dropped variant.
//!
//! # The format
//!
//! Godot's CSV/PO importer emits a `.translation` resource. With `compress` on
//! it is an `OptimizedTranslation`, carrying three arrays:
//!
//! - `hash_table` — `htsize` slots; `hash_table[hash(0, key) % htsize]` is an
//!   index into `bucket_table`, or `0xFFFFFFFF` for "no such key".
//! - `bucket_table` — a `u32` arena. A bucket is `{ size: i32, func: u32 }`
//!   followed by `size` entries of `{ key, str_offset, comp_size, uncomp_size }`.
//!   The second hash uses the bucket's own `func` as its seed, which is how
//!   collisions in the first table are resolved.
//! - `strings` — a byte arena. An entry is stored raw when
//!   `comp_size == uncomp_size`, and SMAZ-compressed otherwise.
//!
//! ⚠️ Everything here is bounds-checked and returns `None` rather than
//! panicking. A truncated or malformed `.translation` must degrade to "no
//! translation" and let the project boot continue, which is what the existing
//! loader does for an unreadable file.

/// SMAZ reverse codebook, transcribed from `upstream/godot/thirdparty/misc/smaz.c`.
pub const SMAZ_REVERSE_CODEBOOK: [&str; 254] = [
    " ", "the", "e", "t", "a", "of", "o", "and", "i", "n", "s", "e ", "r", " th", " t", "in", "he",
    "th", "h", "he ", "to", "\r\n", "l", "s ", "d", " a", "an", "er", "c", " o", "d ", "on", " of",
    "re", "of ", "t ", ", ", "is", "u", "at", "   ", "n ", "or", "which", "f", "m", "as", "it",
    "that", "\n", "was", "en", "  ", " w", "es", " an", " i", "\r", "f ", "g", "p", "nd", " s",
    "nd ", "ed ", "w", "ed", "http://", "for", "te", "ing", "y ", "The", " c", "ti", "r ", "his",
    "st", " in", "ar", "nt", ",", " to", "y", "ng", " h", "with", "le", "al", "to ", "b", "ou",
    "be", "were", " b", "se", "o ", "ent", "ha", "ng ", "their", "\"", "hi", "from", " f", "in ",
    "de", "ion", "me", "v", ".", "ve", "all", "re ", "ri", "ro", "is ", "co", "f t", "are", "ea",
    ". ", "her", " m", "er ", " p", "es ", "by", "they", "di", "ra", "ic", "not", "s, ", "d t",
    "at ", "ce", "la", "h ", "ne", "as ", "tio", "on ", "n t", "io", "we", " a ", "om", ", a",
    "s o", "ur", "li", "ll", "ch", "had", "this", "e t", "g ", "e\r\n", " wh", "ere", " co", "e o",
    "a ", "us", " d", "ss", "\n\r\n", "\r\n\r", "=\"", " be", " e", "s a", "ma", "one", "t t",
    "or ", "but", "el", "so", "l ", "e s", "s,", "no", "ter", " wa", "iv", "ho", "e a", " r",
    "hat", "s t", "ns", "ch ", "wh", "tr", "ut", "/", "have", "ly ", "ta", " ha", " on", "tha",
    "-", " l", "ati", "en ", "pe", " re", "there", "ass", "si", " fo", "wa", "ec", "our", "who",
    "its", "z", "fo", "rs", ">", "ot", "un", "<", "im", "th ", "nc", "ate", "><", "ver", "ad",
    " we", "ly", "ee", " n", "id", " cl", "ac", "il", "</", "rt", " wi", "div", "e, ", " it",
    "whi", " ma", "ge", "x", "e c", "men", ".com",
];

/// The bucket hash, from `OptimizedTranslation::hash` in the vendored header.
///
/// An FNV-1a variant seeded either with the FNV prime (`d == 0`, the first
/// lookup) or with the bucket's own `func` (the second lookup).
///
/// ⚠️ Hashes the UTF-8 **bytes**, and the multiply wraps. Using `char`s here
/// would agree with Godot for ASCII keys and silently diverge for any other.
pub fn hash(d: u32, text: &str) -> u32 {
    let mut d = if d == 0 { 0x0100_0193 } else { d };
    for byte in text.as_bytes() {
        d = d.wrapping_mul(0x0100_0193) ^ (*byte as u32);
    }
    d
}

/// Appends `bytes`, refusing to exceed `capacity`.
///
/// Mirrors `smaz.c`, which checks the remaining output room before every write
/// and reports failure rather than overrunning. That check is what makes a
/// corrupt stream return `None` instead of allocating wildly.
fn push_within(out: &mut Vec<u8>, bytes: &[u8], capacity: usize) -> Option<()> {
    if out.len() + bytes.len() > capacity {
        return None;
    }
    out.extend_from_slice(bytes);
    Some(())
}

/// Decompresses a SMAZ stream, or `None` if it is malformed or overruns.
///
/// Three cases, straight from `smaz_decompress`:
/// `254` = the next byte verbatim, `255` = a length byte (stored as `len - 1`)
/// followed by that many verbatim bytes, anything else = a codebook index.
pub fn smaz_decompress(input: &[u8], uncompressed_len: usize) -> Option<String> {
    let mut out: Vec<u8> = Vec::with_capacity(uncompressed_len);
    let mut i = 0usize;
    while i < input.len() {
        match input[i] {
            254 => {
                let byte = *input.get(i + 1)?;
                push_within(&mut out, &[byte], uncompressed_len)?;
                i += 2;
            }
            255 => {
                // ⚠️ Stored as `len - 1`, so a run of 1 byte is encoded as 0 and
                // the maximum run is 256. Reading it as a plain length loses the
                // last byte of every verbatim run.
                let len = (*input.get(i + 1)? as usize) + 1;
                let end = i.checked_add(2)?.checked_add(len)?;
                push_within(&mut out, input.get(i + 2..end)?, uncompressed_len)?;
                i = end;
            }
            code => {
                // Safe by construction: 254 and 255 are handled above, so this
                // is always in 0..=253 and the table has 254 entries.
                let entry = SMAZ_REVERSE_CODEBOOK[code as usize];
                push_within(&mut out, entry.as_bytes(), uncompressed_len)?;
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// Drops one trailing NUL, the terminator `CharString` sizing folds into the
/// stored length. A string that has none is returned unchanged.
fn strip_trailing_nul(mut text: String) -> String {
    if text.ends_with('\0') {
        text.pop();
    }
    text
}

/// A decoded `OptimizedTranslation` resource.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OptimizedTranslation {
    /// The locale this catalogue is for, e.g. `"fr"`.
    pub locale: String,
    /// First-level table; `0xFFFFFFFF` means "no key hashes here".
    pub hash_table: Vec<u32>,
    /// Bucket arena — see the module docs for the layout.
    pub bucket_table: Vec<u32>,
    /// String arena, raw or SMAZ-compressed per entry.
    pub strings: Vec<u8>,
}

impl OptimizedTranslation {
    /// The translation for `key`, or `None` when the catalogue has no entry.
    ///
    /// A `None` here is what makes `tr()` fall back to the key itself.
    pub fn get_message(&self, key: &str) -> Option<String> {
        let htsize = self.hash_table.len();
        if htsize == 0 {
            return None;
        }
        let slot = (hash(0, key) as usize) % htsize;
        let bucket_index = *self.hash_table.get(slot)?;
        if bucket_index == 0xFFFF_FFFF {
            return None;
        }

        let base = bucket_index as usize;
        // `size` is a signed `int` in the C struct; a negative value is corrupt.
        let size = *self.bucket_table.get(base)?;
        if size > i32::MAX as u32 {
            return None;
        }
        let func = *self.bucket_table.get(base + 1)?;

        // ⚠️ The SECOND hash is seeded with the bucket's own `func`. Reusing the
        // first hash finds the right bucket and then matches no element in it.
        let elem_hash = hash(func, key);
        for i in 0..size as usize {
            let elem = base + 2 + i * 4;
            if *self.bucket_table.get(elem)? == elem_hash {
                return self.read_entry(elem);
            }
        }
        None
    }

    /// Reads the string an element points at, decompressing when needed.
    ///
    /// ⚠️ **`uncomp_size` counts the NUL terminator.** `generate()` stores
    /// `orig_len = src_s.size()` where `src_s` is a `CharString`, and
    /// `ustring.h:182` documents `size()` as "the number of characters in the
    /// buffer, **including** the terminating NUL" (`length()` is `size() - 1`).
    /// So a three-character translation is stored as four bytes with
    /// `uncomp_size == 4`, and the writer's empty-string case makes it explicit:
    /// it emits `orig_len = 1` with a single `0` byte.
    ///
    /// Taking the length at face value therefore yields `"Bonjour\0"` for every
    /// entry of a real `.translation`. The trailing NUL is stripped here.
    ///
    /// Godot itself is inconsistent about this: the compressed branch ends with
    /// `String::utf8(uncomp.get_data())`, which stops at the NUL, while the
    /// uncompressed branch passes an explicit length and so keeps it. The NUL is
    /// an artifact of `CharString` sizing rather than part of the message, so it
    /// is dropped in both branches here.
    fn read_entry(&self, elem: usize) -> Option<String> {
        let str_offset = *self.bucket_table.get(elem + 1)? as usize;
        let comp_size = *self.bucket_table.get(elem + 2)? as usize;
        let uncomp_size = *self.bucket_table.get(elem + 3)? as usize;

        let text = if comp_size == uncomp_size {
            let end = str_offset.checked_add(uncomp_size)?;
            let bytes = self.strings.get(str_offset..end)?;
            String::from_utf8(bytes.to_vec()).ok()?
        } else {
            let end = str_offset.checked_add(comp_size)?;
            let bytes = self.strings.get(str_offset..end)?;
            smaz_decompress(bytes, uncomp_size)?
        };

        Some(strip_trailing_nul(text))
    }

    /// Always empty — the source texts are not stored.
    ///
    /// ⚠️ Not a stub. Godot's `OptimizedTranslation::get_message_list` warns and
    /// returns nothing, because the format keys on a HASH of the source text and
    /// never keeps the text itself. Anything that needs the keys must read the
    /// original `.po`/`.csv`.
    pub fn get_message_list(&self) -> Vec<String> {
        Vec::new()
    }

    /// Always zero, for the same reason as [`Self::get_message_list`].
    pub fn get_message_count(&self) -> usize {
        0
    }

    /// Every *translated* string in the catalogue.
    ///
    /// The counterpart to the two above: the translations ARE stored, so this
    /// one walks every bucket and returns them even though the keys are gone.
    pub fn get_translated_message_list(&self) -> Vec<String> {
        let mut out = Vec::new();
        for slot in 0..self.hash_table.len() {
            let bucket_index = match self.hash_table.get(slot) {
                Some(&p) if p != 0xFFFF_FFFF => p as usize,
                _ => continue,
            };
            let size = match self.bucket_table.get(bucket_index) {
                Some(&s) if s <= i32::MAX as u32 => s as usize,
                _ => continue,
            };
            for i in 0..size {
                let elem = bucket_index + 2 + i * 4;
                if let Some(text) = self.read_entry(elem) {
                    out.push(text);
                }
            }
        }
        out
    }
}
