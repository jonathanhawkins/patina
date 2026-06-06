//! The Create Node dialog's **description panel**.
//!
//! Selecting a type in the dialog populates a side panel with that type's
//! summary / help text so the user knows what they're about to create. Types
//! without a known description (or an empty one) fall back to a graceful
//! placeholder rather than showing a blank panel.
//!
//! Descriptions live in a small built-in table here, independent of the larger
//! `create_dialog` module.

/// Shown when the selected type has no description, or nothing is selected.
pub const PLACEHOLDER: &str = "No description available for this type.";

/// The built-in summary for `class_name`, if one is known and non-empty.
pub fn description_for(class_name: &str) -> Option<&'static str> {
    let text = match class_name {
        "Node" => "The base class for all scene-tree nodes.",
        "Node2D" => "A 2D game object with position, rotation, and scale.",
        "Sprite2D" => "Draws a single texture in 2D.",
        "AnimatedSprite2D" => "Plays sprite-sheet or frame animations in 2D.",
        "CollisionShape2D" => "Provides a collision shape to a 2D physics body or area.",
        "Area2D" => "Detects overlap and influences physics within a 2D region.",
        "CharacterBody2D" => "A 2D body for player- or AI-controlled movement.",
        "Camera2D" => "Controls what part of the 2D world is visible.",
        "StaticBody2D" => "An immovable 2D physics body, for level geometry.",
        "RigidBody2D" => "A 2D body fully simulated by the physics engine.",
        _ => "",
    };
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// The description panel: the selected type and the text currently shown.
#[derive(Debug, Clone, Default)]
pub struct DescriptionPanel {
    selected: Option<String>,
    has_description: bool,
}

impl DescriptionPanel {
    /// Creates an empty panel (nothing selected; placeholder shown).
    pub fn new() -> Self {
        Self::default()
    }

    /// Selects `class_name`, populating the panel with its description or the
    /// placeholder when none is known. An empty name clears the selection.
    pub fn select(&mut self, class_name: &str) {
        if class_name.is_empty() {
            self.clear();
            return;
        }
        self.has_description = description_for(class_name).is_some();
        self.selected = Some(class_name.to_string());
    }

    /// The class currently selected, if any.
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// Whether the selected type has a real (non-placeholder) description.
    pub fn has_description(&self) -> bool {
        self.has_description
    }

    /// The text to render in the panel: the type's description, or the
    /// placeholder when none is available.
    pub fn text(&self) -> &str {
        match self.selected.as_deref() {
            Some(name) => description_for(name).unwrap_or(PLACEHOLDER),
            None => PLACEHOLDER,
        }
    }

    /// Clears the selection, returning to the placeholder.
    pub fn clear(&mut self) {
        self.selected = None;
        self.has_description = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-7j12m): selecting a type populates the description panel
    /// with its help text, and unknown/empty descriptions render a graceful
    /// placeholder.
    #[test]
    fn create_node_description_panel() {
        let mut panel = DescriptionPanel::new();

        // Nothing selected: the panel shows the placeholder.
        assert!(panel.selected().is_none());
        assert!(!panel.has_description());
        assert_eq!(panel.text(), PLACEHOLDER);

        // Selecting a known type populates its summary/help text.
        panel.select("Sprite2D");
        assert_eq!(panel.selected(), Some("Sprite2D"));
        assert!(panel.has_description());
        assert_eq!(panel.text(), "Draws a single texture in 2D.");

        // Selecting another known type swaps the text.
        panel.select("Camera2D");
        assert_eq!(panel.text(), "Controls what part of the 2D world is visible.");
        assert!(panel.has_description());

        // An unknown type renders the graceful placeholder (but stays selected).
        panel.select("ZzNotARealTypeZz");
        assert_eq!(panel.selected(), Some("ZzNotARealTypeZz"));
        assert!(!panel.has_description());
        assert_eq!(panel.text(), PLACEHOLDER);

        // An empty selection clears back to the placeholder.
        panel.select("");
        assert!(panel.selected().is_none());
        assert_eq!(panel.text(), PLACEHOLDER);
    }

    /// The description table and the panel agree about what's known.
    #[test]
    fn description_lookup_matches_panel() {
        assert!(description_for("Area2D").is_some());
        assert!(description_for("Nonexistent").is_none());

        let mut panel = DescriptionPanel::new();
        panel.select("Area2D");
        assert_eq!(panel.text(), description_for("Area2D").unwrap());
    }
}
