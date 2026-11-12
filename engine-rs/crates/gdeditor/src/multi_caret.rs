//! **Multiple carets / column selection** for the script editor.
//!
//! The editor can hold more than one caret at once: the user adds carets
//! directly, or grows the set by selecting the next occurrence of the current
//! word. While several carets are active, typing inserts the same text at every
//! caret simultaneously, and each caret advances past what it inserted. Pressing
//! Escape collapses the set back to a single (primary) caret.
//!
//! Carets are tracked as byte offsets into the buffer, kept sorted and unique.

/// A text buffer with one or more carets (byte offsets into the text).
#[derive(Debug, Clone)]
pub struct MultiCaret {
    text: String,
    carets: Vec<usize>,
}

impl MultiCaret {
    /// Creates a buffer with a single caret at the start.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            carets: vec![0],
        }
    }

    /// The current buffer text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The caret offsets, sorted ascending.
    pub fn carets(&self) -> &[usize] {
        &self.carets
    }

    /// The number of active carets.
    pub fn caret_count(&self) -> usize {
        self.carets.len()
    }

    /// The primary caret (the lowest offset).
    pub fn primary_caret(&self) -> usize {
        self.carets[0]
    }

    /// Replaces the caret set with a single caret at `pos`.
    pub fn set_caret(&mut self, pos: usize) {
        self.carets = vec![pos.min(self.text.len())];
    }

    /// Adds a caret at `pos` (clamped to the buffer), keeping the set sorted and
    /// unique. Returns whether a new caret was added.
    pub fn add_caret(&mut self, pos: usize) -> bool {
        let pos = pos.min(self.text.len());
        if self.carets.contains(&pos) {
            return false;
        }
        self.carets.push(pos);
        self.carets.sort_unstable();
        true
    }

    /// Adds a caret at the end of the next occurrence of `needle` after the
    /// last caret (the select-next-occurrence gesture). Returns whether one was
    /// found and added.
    pub fn select_next_occurrence(&mut self, needle: &str) -> bool {
        if needle.is_empty() {
            return false;
        }
        let from = *self.carets.iter().max().unwrap();
        if let Some(rel) = self.text[from..].find(needle) {
            let end = from + rel + needle.len();
            return self.add_caret(end);
        }
        false
    }

    /// Inserts `s` at every caret simultaneously. Each caret ends up positioned
    /// just after the text it inserted, and carets to the right shift over by
    /// the text inserted to their left.
    pub fn insert(&mut self, s: &str) {
        self.carets.sort_unstable();
        self.carets.dedup();

        let add = s.len();
        let mut out = String::with_capacity(self.text.len() + add * self.carets.len());
        let mut new_carets = Vec::with_capacity(self.carets.len());
        let mut last = 0;
        let mut shift = 0;

        for &c in &self.carets {
            out.push_str(&self.text[last..c]);
            out.push_str(s);
            last = c;
            new_carets.push(c + shift + add);
            shift += add;
        }
        out.push_str(&self.text[last..]);

        self.text = out;
        self.carets = new_carets;
    }

    /// Collapses the caret set back to the single primary caret (Escape).
    pub fn collapse(&mut self) {
        let primary = self.primary_caret();
        self.carets = vec![primary];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-dqbca): adding carets and typing inserts the same text at
    /// every caret, and Escape collapses back to a single caret.
    #[test]
    fn script_core_multi_caret() {
        let mut mc = MultiCaret::new("foo foo foo");

        // Place the primary caret just after the first "foo".
        mc.set_caret(3);
        assert_eq!(mc.caret_count(), 1);

        // Select-next-occurrence grows the caret set to the end of each "foo".
        assert!(mc.select_next_occurrence("foo")); // 2nd foo -> offset 7
        assert!(mc.select_next_occurrence("foo")); // 3rd foo -> offset 11
        assert_eq!(mc.carets(), &[3, 7, 11]);
        // No further occurrences after the last caret.
        assert!(!mc.select_next_occurrence("foo"));

        // Typing inserts the same text at every caret at once.
        mc.insert("X");
        assert_eq!(mc.text(), "fooX fooX fooX");
        // Each caret advanced past the inserted "X".
        assert_eq!(mc.carets(), &[4, 9, 14]);

        // Typing again keeps inserting at all carets.
        mc.insert("!");
        assert_eq!(mc.text(), "fooX! fooX! fooX!");

        // Escape collapses to a single (primary) caret.
        mc.collapse();
        assert_eq!(mc.caret_count(), 1);
        assert_eq!(mc.primary_caret(), 5);

        // A single caret behaves like an ordinary insertion point.
        mc.insert("Z");
        assert_eq!(mc.text(), "fooX!Z fooX! fooX!");
    }

    /// Manually-added carets insert simultaneously, and duplicate carets collapse.
    #[test]
    fn add_caret_and_dedup() {
        let mut mc = MultiCaret::new("a b c");
        mc.set_caret(0);
        assert!(mc.add_caret(2));
        assert!(mc.add_caret(4));
        // Re-adding an existing caret is a no-op.
        assert!(!mc.add_caret(2));
        assert_eq!(mc.carets(), &[0, 2, 4]);

        mc.insert("-");
        assert_eq!(mc.text(), "-a -b -c");
        assert_eq!(mc.carets(), &[1, 4, 7]);
    }
}
