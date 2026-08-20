//! Localization: a per-locale [`Translation`] message catalog (Godot
//! `Translation`), `.po` / `.csv` parsers, and a [`TranslationServer`] that
//! resolves keys through a locale fallback chain (Godot `TranslationServer`).
//!
//! This is the data/logic core. Wiring `.po`/`.csv` files through the resource
//! loader, exposing `tr()`/`tr_n()` to GDScript, and seeding the active locale
//! from `project.godot` are separate integration steps.

use std::collections::HashMap;

/// A single-locale message catalog: maps message keys to translated text
/// (Godot `Translation`).
#[derive(Debug, Clone, Default)]
pub struct Translation {
    locale: String,
    messages: HashMap<String, String>,
    /// Messages carrying a `msgctxt`, keyed by `(context, key)` (pat-s3rrv.128).
    ///
    /// 🔑 Held separately from `messages` rather than folded into a composite
    /// key, because Godot treats the two as **disjoint entries**:
    /// `translation_map.getptr({context, src_text})` is an exact pair lookup
    /// with no fall back to the empty context (`core/string/translation.cpp:169`).
    /// So `tr("FILE")` and `tr("FILE", "menu")` may legitimately resolve to
    /// different strings, or one may exist while the other does not.
    contexts: HashMap<(String, String), String>,
}

impl Translation {
    /// Creates an empty catalog for `locale` (e.g. `"en"`, `"fr_FR"`).
    pub fn new(locale: impl Into<String>) -> Self {
        Self {
            locale: locale.into(),
            messages: HashMap::new(),
            contexts: HashMap::new(),
        }
    }

    /// The locale this catalog is for.
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// Adds or overwrites a message.
    pub fn add_message(&mut self, key: impl Into<String>, text: impl Into<String>) {
        self.messages.insert(key.into(), text.into());
    }

    /// Returns the translated text for `key` (Godot `Translation.get_message`).
    /// Records a message under a `msgctxt`.
    pub fn add_message_with_context(
        &mut self,
        context: impl Into<String>,
        key: impl Into<String>,
        text: impl Into<String>,
    ) {
        let context = context.into();
        if context.is_empty() {
            self.add_message(key, text);
            return;
        }
        self.contexts.insert((context, key.into()), text.into());
    }

    /// Looks a message up under a `msgctxt`.
    ///
    /// ⚠️ An empty context reads the ordinary map, and a non-empty one reads
    /// ONLY the context map — it does not fall back to the context-free entry,
    /// matching Godot's exact-pair lookup. Falling back would silently return
    /// the wrong translation for a word whose meaning depends on its context,
    /// which is the entire reason `msgctxt` exists.
    pub fn get_message_with_context(&self, context: &str, key: &str) -> Option<&str> {
        if context.is_empty() {
            return self.get_message(key);
        }
        self.contexts
            .get(&(context.to_string(), key.to_string()))
            .map(|s| s.as_str())
    }

    pub fn get_message(&self, key: &str) -> Option<&str> {
        self.messages.get(key).map(String::as_str)
    }

    /// Number of messages in the catalog.
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Whether the catalog has no messages.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

/// Strips one pair of surrounding double quotes from a gettext string and
/// resolves C-style escapes (`\n`, `\t`, `\r`, `\"`, `\\`).
fn unquote_po(s: &str) -> String {
    let s = s.trim();
    let inner = s
        .strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .unwrap_or(s);
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Commits the currently-accumulated `msgid`/`msgstr` pair into `t`, if both
/// are present and the key is non-empty (the empty `msgid` is the PO header).
fn flush_po_entry(
    t: &mut Translation,
    ctx: &mut Option<String>,
    id: &mut Option<String>,
    text: &mut Option<String>,
) {
    // ⚠️ The context is taken unconditionally, even when the entry is
    // incomplete — otherwise a `msgctxt` whose entry is malformed would leak
    // onto the NEXT message and silently mis-file it.
    let context = ctx.take().unwrap_or_default();
    if let (Some(k), Some(v)) = (id.take(), text.take()) {
        if !k.is_empty() {
            t.add_message_with_context(context, k, v);
        }
    }
}

/// Parses a gettext `.po` source into a [`Translation`] for `locale`.
///
/// Recognizes `msgid "…"` / `msgstr "…"` pairs with adjacent multi-line string
/// continuation (`"…"` on its own line); comments (`#…`) and blank lines are
/// skipped. Plural forms (`msgid_plural` / `msgstr[n]`) are not yet retained.
pub fn parse_po(locale: impl Into<String>, source: &str) -> Translation {
    let mut t = Translation::new(locale);
    let mut cur_ctx: Option<String> = None;
    let mut cur_id: Option<String> = None;
    let mut cur_str: Option<String> = None;
    // 1 = accumulating into msgid, 2 = into msgstr, 3 = into msgctxt,
    // 0 = neither.
    let mut mode = 0u8;

    for raw in source.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("msgctxt ") {
            // 🔑 `msgctxt` PRECEDES its `msgid`, so it opens the next entry:
            // the previous one is committed here, not when the msgid arrives.
            // Treating it as part of the entry already in hand would attach it
            // to the message before it.
            flush_po_entry(&mut t, &mut cur_ctx, &mut cur_id, &mut cur_str);
            cur_ctx = Some(unquote_po(rest));
            mode = 3;
        } else if let Some(rest) = line.strip_prefix("msgid ") {
            // A new entry begins — commit the previous one first, unless a
            // `msgctxt` line already did so and is waiting for this msgid.
            if cur_ctx.is_none() {
                flush_po_entry(&mut t, &mut cur_ctx, &mut cur_id, &mut cur_str);
            }
            cur_id = Some(unquote_po(rest));
            cur_str = None;
            mode = 1;
        } else if let Some(rest) = line.strip_prefix("msgstr ") {
            cur_str = Some(unquote_po(rest));
            mode = 2;
        } else if line.starts_with('"') {
            let piece = unquote_po(line);
            match mode {
                3 => {
                    if let Some(s) = cur_ctx.as_mut() {
                        s.push_str(&piece);
                    }
                }
                1 => {
                    if let Some(s) = cur_id.as_mut() {
                        s.push_str(&piece);
                    }
                }
                2 => {
                    if let Some(s) = cur_str.as_mut() {
                        s.push_str(&piece);
                    }
                }
                _ => {}
            }
        }
    }
    flush_po_entry(&mut t, &mut cur_ctx, &mut cur_id, &mut cur_str);
    t
}

/// Splits one CSV row on commas, honoring `"…"`-quoted fields (which may embed
/// commas) and the `""` escaped-quote convention.
fn split_csv_row(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                if in_quotes && chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_quotes = !in_quotes;
                }
            }
            ',' if !in_quotes => fields.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    fields.push(cur);
    fields
}

/// Parses a Godot translation CSV — a header row `keys,<locale>,<locale>,…`
/// followed by `key,text,text,…` rows — into one [`Translation`] per locale
/// column. Returns an empty vec if there is no header or no locale columns.
pub fn parse_csv(source: &str) -> Vec<Translation> {
    let mut lines = source.lines().filter(|l| !l.trim().is_empty());
    let header = match lines.next() {
        Some(h) => split_csv_row(h),
        None => return Vec::new(),
    };
    if header.len() < 2 {
        return Vec::new();
    }
    // header[0] is the key column ("keys"); header[1..] name the locales.
    let mut translations: Vec<Translation> = header[1..]
        .iter()
        .map(|loc| Translation::new(loc.trim()))
        .collect();
    for line in lines {
        let fields = split_csv_row(line);
        let key = fields.first().map(|k| k.trim()).unwrap_or("");
        if key.is_empty() {
            continue;
        }
        for (i, t) in translations.iter_mut().enumerate() {
            if let Some(text) = fields.get(i + 1) {
                t.add_message(key, text.trim());
            }
        }
    }
    translations
}

/// Holds per-locale [`Translation`] catalogs plus an active locale and a
/// fallback locale, resolving keys through Godot's fallback chain
/// (Godot `TranslationServer`).
#[derive(Debug, Clone)]
pub struct TranslationServer {
    translations: Vec<Translation>,
    locale: String,
    fallback: String,
    /// Named translation namespaces (`TranslationDomain`, Godot 4.4).
    ///
    /// The unnamed `""` domain is NOT stored here — it is the `translations`
    /// field above, kept in place so every existing caller and the interpreter's
    /// `TranslationServer.*` dispatch keep working untouched.
    domains: HashMap<String, TranslationDomain>,
    /// Pseudolocalization for the default domain.
    pseudolocalization: PseudolocalizationConfig,
}

impl TranslationServer {
    /// Creates a server defaulting to the `"en"` locale and fallback.
    pub fn new() -> Self {
        Self {
            translations: Vec::new(),
            locale: "en".to_string(),
            fallback: "en".to_string(),
            domains: HashMap::new(),
            pseudolocalization: PseudolocalizationConfig::default(),
        }
    }

    /// Registers a translation catalog.
    pub fn add_translation(&mut self, translation: Translation) {
        self.translations.push(translation);
    }

    /// Sets the active locale (`TranslationServer.set_locale`).
    pub fn set_locale(&mut self, locale: impl Into<String>) {
        self.locale = locale.into();
    }

    /// The active locale.
    pub fn get_locale(&self) -> &str {
        &self.locale
    }

    /// Sets the fallback locale used when a key is missing in the active one.
    pub fn set_fallback(&mut self, locale: impl Into<String>) {
        self.fallback = locale.into();
    }

    /// Translates `key` for the active locale (Godot `tr()` / `translate`).
    ///
    /// Resolution order: exact active locale → its language part
    /// (`fr_FR` → `fr`) → fallback locale → the key itself (returned verbatim
    /// when nothing matches, matching Godot).
    pub fn translate(&self, key: &str) -> String {
        for candidate in self.locale_chain() {
            if let Some(text) = self.lookup(&candidate, key) {
                return self.apply_pseudolocalization(text);
            }
        }
        // 🔑 A MISS is pseudolocalized too. Godot returns
        // `pseudolocalize(p_message)` when nothing matched
        // (translation_domain.cpp:365), so an untranslated string still shows
        // as pseudolocalized — which is the whole point of the mode: it makes
        // the strings that were never run through the catalog stand out.
        self.apply_pseudolocalization(key)
    }

    /// Runs the pseudolocalization pipeline when it is enabled.
    ///
    /// This sits inside `translate` deliberately: the interpreter's `tr()` and
    /// `tr_n()` builtins already route here, so toggling the mode changes what
    /// a running script — and therefore a Label bound to it — renders, with no
    /// change to the dispatch layer.
    fn apply_pseudolocalization(&self, text: &str) -> String {
        if self.pseudolocalization.enabled {
            self.pseudolocalization.pseudolocalize(text)
        } else {
            text.to_string()
        }
    }

    /// `TranslationServer.translate(message, context)` (pat-s3rrv.128).
    ///
    /// ⚠️ A miss returns the KEY, not an empty string — Godot's server falls
    /// back to the source text so an untranslated UI still reads sensibly
    /// rather than going blank.
    pub fn translate_with_context(&self, key: &str, context: &str) -> String {
        for candidate in self.locale_chain() {
            if let Some(text) = self.lookup_with_context(&candidate, key, context) {
                return text.to_string();
            }
        }
        key.to_string()
    }

    fn lookup_with_context(&self, locale: &str, key: &str, context: &str) -> Option<&str> {
        self.translations.iter().find_map(|t| {
            let matches =
                t.locale() == locale || t.locale().split(['_', '-']).next() == Some(locale);
            if matches {
                t.get_message_with_context(context, key)
            } else {
                None
            }
        })
    }

    /// The ordered locale-resolution chain for the active locale.
    fn locale_chain(&self) -> Vec<String> {
        let mut chain = vec![self.locale.clone()];
        if let Some(lang) = self.locale.split(['_', '-']).next() {
            if lang != self.locale.as_str() {
                chain.push(lang.to_string());
            }
        }
        if !chain.contains(&self.fallback) {
            chain.push(self.fallback.clone());
        }
        chain
    }

    /// Finds the first catalog matching `locale` (exact, or by language part —
    /// an `fr_FR` catalog satisfies a request for `fr`) that contains `key`.
    fn lookup(&self, locale: &str, key: &str) -> Option<&str> {
        self.translations.iter().find_map(|t| {
            let matches =
                t.locale() == locale || t.locale().split(['_', '-']).next() == Some(locale);
            if matches {
                t.get_message(key)
            } else {
                None
            }
        })
    }
}

impl Default for TranslationServer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation_csv_po_and_fallback_chain() {
        // CSV: a key column plus two locale columns.
        let csv = "keys,en,fr\nHELLO,Hello,Bonjour\nBYE,Bye,Au revoir\n";
        let cats = parse_csv(csv);
        assert_eq!(cats.len(), 2);
        assert_eq!(cats[0].locale(), "en");
        assert_eq!(cats[1].locale(), "fr");
        assert_eq!(cats[0].get_message("HELLO"), Some("Hello"));
        assert_eq!(cats[1].get_message("HELLO"), Some("Bonjour"));

        // PO: a `de` catalog with comments and blank lines.
        let po = "# a comment\nmsgid \"HELLO\"\nmsgstr \"Hallo\"\n\nmsgid \"BYE\"\nmsgstr \"Tschuess\"\n";
        let de = parse_po("de", po);
        assert_eq!(de.locale(), "de");
        assert_eq!(de.get_message("HELLO"), Some("Hallo"));
        assert_eq!(de.get_message("BYE"), Some("Tschuess"));

        // Server + fallback chain.
        let mut server = TranslationServer::new();
        for c in cats {
            server.add_translation(c);
        }
        server.add_translation(de);
        server.set_fallback("en");

        server.set_locale("fr");
        assert_eq!(server.get_locale(), "fr");
        assert_eq!(server.translate("HELLO"), "Bonjour"); // active locale hit

        server.set_locale("fr_FR"); // language fallback fr_FR -> fr
        assert_eq!(server.translate("BYE"), "Au revoir");

        server.set_locale("de");
        assert_eq!(server.translate("HELLO"), "Hallo");

        // Active locale es has no catalog -> fallback en.
        server.set_locale("es");
        assert_eq!(server.translate("HELLO"), "Hello");

        // A key absent everywhere returns itself.
        server.set_locale("fr");
        assert_eq!(server.translate("MISSING_KEY"), "MISSING_KEY");
    }

    #[test]
    fn po_escapes_and_csv_quoted_fields() {
        let po = "msgid \"NL\"\nmsgstr \"line1\\nline2\"\n";
        assert_eq!(parse_po("en", po).get_message("NL"), Some("line1\nline2"));

        // Quoted CSV field with an embedded comma.
        let csv = "keys,en\nGREETING,\"Hello, world\"\n";
        let cats = parse_csv(csv);
        assert_eq!(cats[0].get_message("GREETING"), Some("Hello, world"));
    }
}

// ---------------------------------------------------------------------------
// Process-global server (pat-0q2kx.147)
// ---------------------------------------------------------------------------

/// The engine-wide [`TranslationServer`].
///
/// Deliberately process-global rather than thread-local: the boot path loads a
/// project's catalogs, and script evaluation may run on a different thread. A
/// `thread_local` store — which is what the interpreter used to keep — cannot
/// see anything boot loaded, which is exactly why project translations never
/// reached `tr()`.
static GLOBAL_SERVER: std::sync::OnceLock<std::sync::Mutex<TranslationServer>> =
    std::sync::OnceLock::new();

fn global_server() -> &'static std::sync::Mutex<TranslationServer> {
    GLOBAL_SERVER.get_or_init(|| std::sync::Mutex::new(TranslationServer::new()))
}

/// Reads the global server.
///
/// A poisoned lock is recovered from rather than propagated: a translation
/// lookup failing is never worth taking the process down, and Godot's
/// `tr()` has no failure mode at all.
pub fn with_translation_server<R>(f: impl FnOnce(&TranslationServer) -> R) -> R {
    let guard = global_server().lock().unwrap_or_else(|e| e.into_inner());
    f(&guard)
}

/// Mutates the global server.
pub fn with_translation_server_mut<R>(f: impl FnOnce(&mut TranslationServer) -> R) -> R {
    let mut guard = global_server().lock().unwrap_or_else(|e| e.into_inner());
    f(&mut guard)
}

/// Translates `key` through the global server — the one entry point `tr()`,
/// `atr()` and `TranslationServer.translate()` all share.
pub fn translate(key: &str) -> String {
    with_translation_server(|s| s.translate(key))
}

/// Engine-wide `tr(key, context)`.
pub fn translate_with_context(key: &str, context: &str) -> String {
    with_translation_server(|s| s.translate_with_context(key, context))
}

impl TranslationServer {
    /// Every locale that has a loaded catalog, in load order and de-duplicated
    /// (`TranslationServer.get_loaded_locales`).
    pub fn get_loaded_locales(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for t in &self.translations {
            if !out.iter().any(|l| l == t.locale()) {
                out.push(t.locale().to_string());
            }
        }
        out
    }

    /// The fallback locale.
    pub fn get_fallback(&self) -> &str {
        &self.fallback
    }

    /// Drops every catalog for `locale` (`TranslationServer.remove_translation`).
    pub fn remove_translation(&mut self, locale: &str) {
        self.translations.retain(|t| t.locale() != locale);
    }

    /// Drops every catalog.
    pub fn clear(&mut self) {
        self.translations.clear();
    }

    /// Adds a single message, creating the locale's catalog if needed.
    ///
    /// This is `TranslationServer.add_message`; it appends to the last catalog
    /// for that locale so repeated calls accumulate instead of each one
    /// shadowing the last.
    pub fn add_message(&mut self, locale: &str, key: &str, text: &str) {
        if let Some(t) = self
            .translations
            .iter_mut()
            .rev()
            .find(|t| t.locale() == locale)
        {
            t.add_message(key, text);
            return;
        }
        let mut t = Translation::new(locale);
        t.add_message(key, text);
        self.translations.push(t);
    }
}

// ===========================================================================
// TranslationDomain + pseudolocalization (pat-s3rrv.212)
//
// Ported from `upstream/godot/core/string/translation_domain.cpp`:
// `pseudolocalize` (:480), `_double_vowels` (:111),
// `_replace_with_accented_string`, `_wrap_with_fakebidi_characters`,
// `_add_padding`, `_is_placeholder`, and the `_character_to_accented` table
// (:42). Defaults are from `PseudolocalizationConfig` in the matching header.
// ===========================================================================

/// Godot's ASCII-to-accented substitution table (`translation_domain.cpp:42`).
///
/// ⚠️ **Transcribed, never derived.** There is no rule here: `B` becomes `ß`,
/// `K` becomes `ĸ`, `N` becomes the CYRILLIC `й` and `n` the GREEK `ή`, while a
/// dozen entries are a plain letter plus a combining mark (`F́`, `d́`, `h̀`, `m̀`,
/// `q́`, `x́`) and so are two code points rather than one. Any "add an accent"
/// heuristic disagrees with Godot on roughly half the alphabet.
const CHARACTER_TO_ACCENTED: [(char, &str); 52] = [
    ('A', "Å"),
    ('B', "ß"),
    ('C', "Ç"),
    ('D', "Ð"),
    ('E', "É"),
    ('F', "F́"),
    ('G', "Ĝ"),
    ('H', "Ĥ"),
    ('I', "Ĩ"),
    ('J', "Ĵ"),
    ('K', "ĸ"),
    ('L', "Ł"),
    ('M', "Ḿ"),
    ('N', "й"),
    ('O', "Ö"),
    ('P', "Ṕ"),
    ('Q', "Q́"),
    ('R', "Ř"),
    ('S', "Ŝ"),
    ('T', "Ŧ"),
    ('U', "Ũ"),
    ('V', "Ṽ"),
    ('W', "Ŵ"),
    ('X', "X́"),
    ('Y', "Ÿ"),
    ('Z', "Ž"),
    ('a', "á"),
    ('b', "ḅ"),
    ('c', "ć"),
    ('d', "d́"),
    ('e', "é"),
    ('f', "f́"),
    ('g', "ǵ"),
    ('h', "h̀"),
    ('i', "í"),
    ('j', "ǰ"),
    ('k', "ḱ"),
    ('l', "ł"),
    ('m', "m̀"),
    ('n', "ή"),
    ('o', "ô"),
    ('p', "ṕ"),
    ('q', "q́"),
    ('r', "ŕ"),
    ('s', "š"),
    ('t', "ŧ"),
    ('u', "ü"),
    ('v', "ṽ"),
    ('w', "ŵ"),
    ('x', "x́"),
    ('y', "ý"),
    ('z', "ź"),
];

/// Unicode RIGHT-TO-LEFT OVERRIDE, opened around fake-bidi text.
const FAKE_BIDI_PREFIX: char = '\u{202e}';
/// Unicode POP DIRECTIONAL FORMATTING, closing a fake-bidi run.
const FAKE_BIDI_SUFFIX: char = '\u{202c}';

/// Pseudolocalization knobs, mirroring Godot's `PseudolocalizationConfig`.
///
/// Every default is that struct's, and two of them matter more than they look:
/// `accents_enabled` is **true** while everything else is off, and
/// `expansion_ratio` is **0.0**, so switching pseudolocalization on with no
/// further configuration accents the text and adds `[`/`]` but does NOT
/// lengthen it.
#[derive(Debug, Clone, PartialEq)]
pub struct PseudolocalizationConfig {
    /// Whether pseudolocalization is applied at all.
    pub enabled: bool,
    /// Replace ASCII letters with accented look-alikes.
    pub accents_enabled: bool,
    /// Double every vowel, to simulate a longer translation.
    pub double_vowels_enabled: bool,
    /// Wrap the text in RTL override characters.
    pub fake_bidi_enabled: bool,
    /// Replace the message body with `*` padding instead of translating it.
    pub override_enabled: bool,
    /// Leave `%s`-style placeholders untouched by every transform.
    pub skip_placeholders_enabled: bool,
    /// How much to lengthen the string, as a fraction of its ORIGINAL length.
    pub expansion_ratio: f32,
    /// Written before the message (and before the expansion underscores).
    pub prefix: String,
    /// Written after the message (and after the expansion underscores).
    pub suffix: String,
}

impl Default for PseudolocalizationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            accents_enabled: true,
            double_vowels_enabled: false,
            fake_bidi_enabled: false,
            override_enabled: false,
            skip_placeholders_enabled: true,
            expansion_ratio: 0.0,
            prefix: "[".to_string(),
            suffix: "]".to_string(),
        }
    }
}

/// True when `chars[index]` begins a `%`-placeholder Godot leaves alone.
///
/// Mirrors `_is_placeholder`: a `%` followed by one of `scdoxXf`, and never at
/// the final position.
fn is_placeholder(chars: &[char], index: usize) -> bool {
    index + 1 < chars.len()
        && chars[index] == '%'
        && matches!(chars[index + 1], 's' | 'c' | 'd' | 'o' | 'x' | 'X' | 'f')
}

/// Applies `f` to each character, copying `%`-placeholders through untouched
/// when `skip` is set.
///
/// The three per-character transforms differ only in `f`, and each one has to
/// repeat the same placeholder-skipping dance; sharing it keeps them from
/// drifting apart, which is the way this kind of code usually breaks.
fn map_skipping_placeholders(
    message: &str,
    skip: bool,
    mut f: impl FnMut(char, &mut String),
) -> String {
    let chars: Vec<char> = message.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if skip && is_placeholder(&chars, i) {
            out.push(chars[i]);
            out.push(chars[i + 1]);
            i += 2;
            continue;
        }
        f(chars[i], &mut out);
        i += 1;
    }
    out
}

impl PseudolocalizationConfig {
    fn double_vowels(&self, message: &str) -> String {
        map_skipping_placeholders(message, self.skip_placeholders_enabled, |c, out| {
            out.push(c);
            if matches!(c, 'a' | 'e' | 'i' | 'o' | 'u' | 'A' | 'E' | 'I' | 'O' | 'U') {
                out.push(c);
            }
        })
    }

    fn replace_with_accents(&self, message: &str) -> String {
        map_skipping_placeholders(message, self.skip_placeholders_enabled, |c, out| {
            match CHARACTER_TO_ACCENTED.iter().find(|(k, _)| *k == c) {
                Some((_, accented)) => out.push_str(accented),
                None => out.push(c),
            }
        })
    }

    fn override_string(&self, message: &str) -> String {
        map_skipping_placeholders(message, self.skip_placeholders_enabled, |_, out| {
            out.push('*');
        })
    }

    /// ⚠️ The RTL override is re-opened after every newline AND around every
    /// skipped placeholder, because the override is popped by both.
    fn wrap_fake_bidi(&self, message: &str) -> String {
        let chars: Vec<char> = message.chars().collect();
        let mut out = String::new();
        out.push(FAKE_BIDI_PREFIX);
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '\n' {
                out.push(FAKE_BIDI_SUFFIX);
                out.push(chars[i]);
                out.push(FAKE_BIDI_PREFIX);
                i += 1;
            } else if self.skip_placeholders_enabled && is_placeholder(&chars, i) {
                out.push(FAKE_BIDI_SUFFIX);
                out.push(chars[i]);
                out.push(chars[i + 1]);
                out.push(FAKE_BIDI_PREFIX);
                i += 2;
            } else {
                out.push(chars[i]);
                i += 1;
            }
        }
        out.push(FAKE_BIDI_SUFFIX);
        out
    }

    /// 🔑 `length` is the ORIGINAL message's length, not the transformed one.
    /// Godot captures it before any substitution runs, so doubling the vowels
    /// does not also double the padding.
    fn add_padding(&self, message: &str, length: usize) -> String {
        let count = ((length as f32) * self.expansion_ratio / 2.0) as usize;
        let underscores = "_".repeat(count);
        format!(
            "{}{}{}{}{}",
            self.prefix, underscores, message, underscores, self.suffix
        )
    }

    /// The full pipeline, in Godot's order: override, double vowels, accents,
    /// fake bidi, then padding computed from the ORIGINAL length.
    ///
    /// An empty message is returned untouched — Godot returns early, so an
    /// empty string does NOT come back as just the prefix and suffix.
    pub fn pseudolocalize(&self, message: &str) -> String {
        if message.is_empty() {
            return String::new();
        }
        let length = message.chars().count();
        let mut out = message.to_string();
        if self.override_enabled {
            out = self.override_string(&out);
        }
        if self.double_vowels_enabled {
            out = self.double_vowels(&out);
        }
        if self.accents_enabled {
            out = self.replace_with_accents(&out);
        }
        if self.fake_bidi_enabled {
            out = self.wrap_fake_bidi(&out);
        }
        self.add_padding(&out, length)
    }
}

/// A named translation namespace with its own catalogs and its own
/// pseudolocalization settings (Godot 4.4 `TranslationDomain`, RefCounted).
///
/// Godot keeps the unnamed domain and the named ones in one map; here the
/// unnamed one stays on [`TranslationServer`] itself so that every existing
/// caller — including the interpreter's `TranslationServer.*` dispatch — is
/// untouched by this addition.
#[derive(Debug, Clone, Default)]
pub struct TranslationDomain {
    translations: Vec<Translation>,
    /// This domain's own pseudolocalization settings, independent of the
    /// server's. Godot gives each domain its own config, so a debug overlay can
    /// be pseudolocalized while the game's UI is not.
    pub pseudolocalization: PseudolocalizationConfig,
    /// When empty, the domain follows the server's locale; otherwise it pins
    /// its own (`TranslationDomain.locale_override`).
    pub locale_override: String,
}

impl TranslationDomain {
    /// An empty domain.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a catalog to this domain.
    pub fn add_translation(&mut self, translation: Translation) {
        self.translations.push(translation);
    }

    /// Drops every catalog whose locale matches.
    pub fn remove_translation(&mut self, locale: &str) {
        self.translations.retain(|t| t.locale() != locale);
    }

    /// Drops all catalogs.
    pub fn clear(&mut self) {
        self.translations.clear();
    }

    /// How many catalogs this domain holds.
    pub fn len(&self) -> usize {
        self.translations.len()
    }

    /// Whether the domain holds no catalogs.
    pub fn is_empty(&self) -> bool {
        self.translations.is_empty()
    }

    /// The locales this domain has catalogs for.
    pub fn get_loaded_locales(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .translations
            .iter()
            .map(|t| t.locale().to_string())
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// Translates `key` against `locale`, then `fallback`, then returns the key.
    ///
    /// The resolution mirrors `TranslationDomain::translate`: the active locale
    /// (or this domain's override) first, then the fallback, and a miss returns
    /// the message itself. Pseudolocalization applies to BOTH outcomes.
    pub fn translate(&self, key: &str, locale: &str, fallback: &str) -> String {
        let active = if self.locale_override.is_empty() {
            locale
        } else {
            self.locale_override.as_str()
        };
        for candidate in locale_chain_for(active, fallback) {
            if let Some(text) = self.lookup(&candidate, key) {
                return self.finish(text);
            }
        }
        self.finish(key)
    }

    fn finish(&self, text: &str) -> String {
        if self.pseudolocalization.enabled {
            self.pseudolocalization.pseudolocalize(text)
        } else {
            text.to_string()
        }
    }

    fn lookup(&self, locale: &str, key: &str) -> Option<&str> {
        self.translations.iter().find_map(|t| {
            let matches =
                t.locale() == locale || t.locale().split(['_', '-']).next() == Some(locale);
            if matches {
                t.get_message(key)
            } else {
                None
            }
        })
    }
}

/// The locale resolution order: the exact locale, its language part, then the
/// fallback and ITS language part.
///
/// ⚠️ `fr_FR` must try `fr` before giving up — a catalog shipped as `fr` has to
/// serve a `fr_FR` user, which is the case that makes region-tagged locales
/// usable at all.
fn locale_chain_for(locale: &str, fallback: &str) -> Vec<String> {
    let mut chain = Vec::new();
    let mut push = |s: &str| {
        if !s.is_empty() && !chain.iter().any(|c| c == s) {
            chain.push(s.to_string());
        }
    };
    push(locale);
    if let Some(lang) = locale.split(['_', '-']).next() {
        push(lang);
    }
    push(fallback);
    if let Some(lang) = fallback.split(['_', '-']).next() {
        push(lang);
    }
    chain
}

impl TranslationServer {
    /// Whether a named domain exists (`TranslationServer.has_domain`).
    ///
    /// The unnamed `""` domain always exists, matching Godot.
    pub fn has_domain(&self, name: &str) -> bool {
        name.is_empty() || self.domains.contains_key(name)
    }

    /// Returns the named domain, creating it when absent
    /// (`TranslationServer.get_or_add_domain`).
    pub fn get_or_add_domain(&mut self, name: &str) -> &mut TranslationDomain {
        self.domains.entry(name.to_string()).or_default()
    }

    /// Reads a named domain without creating it.
    pub fn get_domain(&self, name: &str) -> Option<&TranslationDomain> {
        self.domains.get(name)
    }

    /// Removes a named domain (`TranslationServer.remove_domain`).
    ///
    /// ⚠️ Removing the unnamed domain is a no-op: Godot's default domain cannot
    /// be removed, and silently clearing every default catalog here would be a
    /// far worse outcome than ignoring the call.
    pub fn remove_domain(&mut self, name: &str) {
        if !name.is_empty() {
            self.domains.remove(name);
        }
    }

    /// The names of the loaded named domains, sorted for determinism.
    pub fn get_loaded_domains(&self) -> Vec<String> {
        let mut out: Vec<String> = self.domains.keys().cloned().collect();
        out.sort();
        out
    }

    /// Translates `key` through a named domain, falling back to the default
    /// domain when the name is empty or unknown.
    pub fn translate_in_domain(&self, domain: &str, key: &str) -> String {
        match self.domains.get(domain) {
            Some(d) => d.translate(key, &self.locale, &self.fallback),
            None => self.translate(key),
        }
    }

    /// Whether pseudolocalization is on for the default domain.
    pub fn is_pseudolocalization_enabled(&self) -> bool {
        self.pseudolocalization.enabled
    }

    /// Turns pseudolocalization on or off for the default domain
    /// (`TranslationServer.pseudolocalization_enabled`).
    pub fn set_pseudolocalization_enabled(&mut self, enabled: bool) {
        self.pseudolocalization.enabled = enabled;
    }

    /// The default domain's pseudolocalization settings, for configuring the
    /// individual knobs.
    pub fn pseudolocalization(&self) -> &PseudolocalizationConfig {
        &self.pseudolocalization
    }

    /// Mutable access to the default domain's pseudolocalization settings.
    ///
    /// Godot exposes each knob as its own setter; one accessor to the config is
    /// the same surface with far fewer places to get a name wrong.
    pub fn pseudolocalization_mut(&mut self) -> &mut PseudolocalizationConfig {
        &mut self.pseudolocalization
    }
}
