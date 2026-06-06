//! Signal documentation source for the Signals dock.
//!
//! Surfaces a signal's documentation/description (sourced from the class
//! reference) so the Signals tab can show it as a hover tooltip and in a details
//! area. Docs are keyed by `(class, signal)` so the same signal name declared on
//! different classes resolves to the right description. Unknown signals have no
//! documentation; the details area then renders a placeholder.

use std::collections::HashMap;

/// Placeholder shown in the details area when a signal has no documentation.
pub const NO_DOC: &str = "No documentation available.";

/// A registry of signal documentation, populated from the class reference.
#[derive(Debug, Clone, Default)]
pub struct SignalDocs {
    docs: HashMap<(String, String), String>,
}

impl SignalDocs {
    /// Creates an empty documentation registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers documentation for `class::signal`, overwriting any existing
    /// entry.
    pub fn insert(
        &mut self,
        class: impl Into<String>,
        signal: impl Into<String>,
        doc: impl Into<String>,
    ) {
        self.docs.insert((class.into(), signal.into()), doc.into());
    }

    /// The documentation for `class::signal`, if known.
    pub fn doc_for(&self, class: &str, signal: &str) -> Option<&str> {
        self.docs
            .get(&(class.to_string(), signal.to_string()))
            .map(|s| s.as_str())
    }

    /// Whether documentation exists for `class::signal`.
    pub fn has_doc(&self, class: &str, signal: &str) -> bool {
        self.docs.contains_key(&(class.to_string(), signal.to_string()))
    }

    /// The text to render in the details area / tooltip for `class::signal`:
    /// the documentation if known, otherwise the [`NO_DOC`] placeholder.
    pub fn details(&self, class: &str, signal: &str) -> String {
        self.doc_for(class, signal)
            .map(|s| s.to_string())
            .unwrap_or_else(|| NO_DOC.to_string())
    }

    /// Number of documented signals.
    pub fn len(&self) -> usize {
        self.docs.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-sfqts): hovering or selecting a signal shows its
    /// documentation text sourced from the class reference; unknown signals
    /// render a placeholder in the details area.
    #[test]
    fn signals_docs_tooltips_render() {
        let mut docs = SignalDocs::new();
        docs.insert("Button", "pressed", "Emitted when the button is pressed.");
        docs.insert(
            "BaseButton",
            "toggled",
            "Emitted when the button toggles between pressed and normal.",
        );

        // Hovering/selecting a documented signal shows its text.
        assert_eq!(
            docs.doc_for("Button", "pressed"),
            Some("Emitted when the button is pressed.")
        );
        assert_eq!(
            docs.doc_for("BaseButton", "toggled"),
            Some("Emitted when the button toggles between pressed and normal.")
        );
        assert!(docs.has_doc("Button", "pressed"));

        // Docs are keyed by (class, signal): the same name on another class is
        // unknown unless documented.
        assert_eq!(docs.doc_for("Node", "pressed"), None);
        assert_eq!(docs.doc_for("Button", "nonexistent"), None);
        assert!(!docs.has_doc("Node", "pressed"));

        // The details area renders the doc text, or a placeholder when unknown.
        assert_eq!(
            docs.details("Button", "pressed"),
            "Emitted when the button is pressed."
        );
        assert_eq!(docs.details("Button", "nope"), NO_DOC);

        assert_eq!(docs.len(), 2);
        assert!(!docs.is_empty());
    }

    /// Re-inserting a signal's doc overwrites the previous text.
    #[test]
    fn signal_doc_insert_overwrites() {
        let mut docs = SignalDocs::new();
        docs.insert("Timer", "timeout", "old");
        docs.insert("Timer", "timeout", "Emitted when the timer reaches 0.");
        assert_eq!(docs.len(), 1);
        assert_eq!(
            docs.doc_for("Timer", "timeout"),
            Some("Emitted when the timer reaches 0.")
        );
    }
}
