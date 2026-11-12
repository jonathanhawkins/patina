//! **AnimationPlayer panel** (pat-zhnpp).
//!
//! The panel lists the AnimationPlayer's animations (from its AnimationLibrary)
//! and supports creating, renaming, duplicating, and deleting them. Each
//! operation updates the library and the current selection: creating or
//! duplicating selects the new animation, renaming follows the selection, and
//! deleting the selected animation moves the selection to another (or clears it
//! when none remain).

/// A single animation entry (name plus its length in seconds).
#[derive(Debug, Clone, PartialEq)]
pub struct Animation {
    /// The animation's name (its key in the library).
    pub name: String,
    /// Length in seconds.
    pub length: f32,
}

/// The AnimationPlayer panel: an ordered animation library plus a selection.
#[derive(Debug, Clone, Default)]
pub struct AnimationPanel {
    anims: Vec<Animation>,
    selected: Option<String>,
}

impl AnimationPanel {
    /// Creates an empty panel.
    pub fn new() -> Self {
        Self::default()
    }

    /// The animation names, in order.
    pub fn names(&self) -> Vec<&str> {
        self.anims.iter().map(|a| a.name.as_str()).collect()
    }

    /// The number of animations.
    pub fn len(&self) -> usize {
        self.anims.len()
    }

    /// Whether the library is empty.
    pub fn is_empty(&self) -> bool {
        self.anims.is_empty()
    }

    /// Whether an animation named `name` exists.
    pub fn contains(&self, name: &str) -> bool {
        self.anims.iter().any(|a| a.name == name)
    }

    /// The currently-selected animation name, if any.
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// The length of `name`, if present.
    pub fn length_of(&self, name: &str) -> Option<f32> {
        self.anims.iter().find(|a| a.name == name).map(|a| a.length)
    }

    /// Selects `name`. Returns whether it exists.
    pub fn select(&mut self, name: &str) -> bool {
        if self.contains(name) {
            self.selected = Some(name.to_string());
            true
        } else {
            false
        }
    }

    /// Creates a new empty animation named `name` and selects it. Returns false
    /// if the name is already taken.
    pub fn create(&mut self, name: impl Into<String>) -> bool {
        let name = name.into();
        if self.contains(&name) {
            return false;
        }
        self.anims.push(Animation {
            name: name.clone(),
            length: 1.0,
        });
        self.selected = Some(name);
        true
    }

    /// Renames `old` to `new`, following the selection. Returns false if `old`
    /// is missing or `new` is already taken.
    pub fn rename(&mut self, old: &str, new: &str) -> bool {
        if !self.contains(old) || self.contains(new) {
            return false;
        }
        if let Some(a) = self.anims.iter_mut().find(|a| a.name == old) {
            a.name = new.to_string();
        }
        if self.selected.as_deref() == Some(old) {
            self.selected = Some(new.to_string());
        }
        true
    }

    /// Duplicates `src` to `dst` (copying its length) and selects the copy.
    /// Returns false if `src` is missing or `dst` is already taken.
    pub fn duplicate(&mut self, src: &str, dst: impl Into<String>) -> bool {
        let dst = dst.into();
        if self.contains(&dst) {
            return false;
        }
        let length = match self.length_of(src) {
            Some(l) => l,
            None => return false,
        };
        self.anims.push(Animation {
            name: dst.clone(),
            length,
        });
        self.selected = Some(dst);
        true
    }

    /// Deletes `name`. If it was selected, the selection moves to the first
    /// remaining animation (or clears when none remain). Returns whether it
    /// existed.
    pub fn delete(&mut self, name: &str) -> bool {
        match self.anims.iter().position(|a| a.name == name) {
            Some(i) => {
                self.anims.remove(i);
                if self.selected.as_deref() == Some(name) {
                    self.selected = self.anims.first().map(|a| a.name.clone());
                }
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-zhnpp): the panel lists the player's animations, and
    /// create/rename/duplicate/delete operate on the library and update the
    /// selection.
    #[test]
    fn anim_player_panel_manage() {
        let mut panel = AnimationPanel::new();
        assert!(panel.is_empty());

        // Create lists the animations and selects the newest.
        assert!(panel.create("idle"));
        assert!(panel.create("run"));
        assert_eq!(panel.names(), vec!["idle", "run"]);
        assert_eq!(panel.selected(), Some("run"));
        // Duplicate name is rejected.
        assert!(!panel.create("idle"));

        // Selecting another animation.
        assert!(panel.select("idle"));
        assert_eq!(panel.selected(), Some("idle"));
        assert!(!panel.select("missing"));

        // Rename follows the selection.
        assert!(panel.rename("idle", "idle_loop"));
        assert!(!panel.contains("idle"));
        assert!(panel.contains("idle_loop"));
        assert_eq!(panel.selected(), Some("idle_loop"));
        // Renaming onto an existing name fails.
        assert!(!panel.rename("run", "idle_loop"));

        // Duplicate copies and selects the copy.
        assert!(panel.duplicate("run", "run_copy"));
        assert!(panel.contains("run_copy"));
        assert_eq!(panel.selected(), Some("run_copy"));
        assert!(!panel.duplicate("run", "run_copy")); // dst exists
        assert!(!panel.duplicate("ghost", "x")); // src missing

        // Deleting the selected animation moves the selection elsewhere.
        assert_eq!(panel.names(), vec!["idle_loop", "run", "run_copy"]);
        assert!(panel.delete("run_copy"));
        assert!(!panel.contains("run_copy"));
        assert_eq!(panel.selected(), Some("idle_loop")); // first remaining

        // Deleting everything clears the selection.
        assert!(panel.delete("idle_loop"));
        assert!(panel.delete("run"));
        assert!(panel.is_empty());
        assert!(panel.selected().is_none());
        // Deleting a missing animation is a no-op.
        assert!(!panel.delete("idle_loop"));
    }
}
