//! Keyboard shortcuts for menu actions.
//!
//! Binds each menu action to a keyboard shortcut so that (a) pressing the
//! shortcut fires the bound action, and (b) the shortcut's accelerator text
//! (e.g. `Ctrl+S`) is shown alongside the item's label in the menu.

/// A keyboard shortcut: a base key plus modifier flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyShortcut {
    /// Control (⌘/Ctrl) modifier.
    pub ctrl: bool,
    /// Shift modifier.
    pub shift: bool,
    /// Alt/Option modifier.
    pub alt: bool,
    /// The base key (case-insensitive).
    pub key: char,
}

impl KeyShortcut {
    /// A shortcut with explicit modifiers.
    pub fn new(ctrl: bool, shift: bool, alt: bool, key: char) -> Self {
        Self {
            ctrl,
            shift,
            alt,
            key,
        }
    }

    /// Convenience: a `Ctrl+<key>` shortcut.
    pub fn ctrl(key: char) -> Self {
        Self::new(true, false, false, key)
    }

    /// The accelerator text shown in the menu, e.g. `Ctrl+Shift+S`.
    pub fn accelerator_text(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("Ctrl+");
        }
        if self.shift {
            s.push_str("Shift+");
        }
        if self.alt {
            s.push_str("Alt+");
        }
        s.push(self.key.to_ascii_uppercase());
        s
    }
}

/// One menu item: its action, display label, and bound shortcut.
#[derive(Debug, Clone)]
struct ShortcutItem<A> {
    action: A,
    label: String,
    shortcut: KeyShortcut,
}

/// A menu's action↔shortcut bindings.
#[derive(Debug, Clone)]
pub struct ShortcutMenu<A> {
    items: Vec<ShortcutItem<A>>,
}

impl<A: Clone + PartialEq> ShortcutMenu<A> {
    /// Creates an empty shortcut menu.
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Binds `action` (shown as `label`) to `shortcut`.
    pub fn bind(&mut self, action: A, label: &str, shortcut: KeyShortcut) {
        self.items.push(ShortcutItem {
            action,
            label: label.to_string(),
            shortcut,
        });
    }

    /// Returns the action whose shortcut matches `pressed`, if any — i.e. the
    /// action the key press fires.
    pub fn fire(&self, pressed: KeyShortcut) -> Option<A> {
        self.items
            .iter()
            .find(|i| i.shortcut == pressed)
            .map(|i| i.action.clone())
    }

    /// The accelerator text for `action`'s shortcut, if bound.
    pub fn accelerator_for(&self, action: &A) -> Option<String> {
        self.items
            .iter()
            .find(|i| &i.action == action)
            .map(|i| i.shortcut.accelerator_text())
    }

    /// The full menu-item label including accelerator text, e.g.
    /// `"Save\tCtrl+S"` (label and accelerator separated by a tab).
    pub fn item_label(&self, action: &A) -> Option<String> {
        self.items
            .iter()
            .find(|i| &i.action == action)
            .map(|i| format!("{}\t{}", i.label, i.shortcut.accelerator_text()))
    }
}

impl<A: Clone + PartialEq> Default for ShortcutMenu<A> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Act {
        Save,
        Open,
        Undo,
    }

    /// Acceptance (pat-o5w2m): a menu action's shortcut both fires the action
    /// when pressed and is shown as accelerator text on the item.
    #[test]
    fn menus_action_shortcuts_bind_and_display() {
        let mut menu = ShortcutMenu::new();
        menu.bind(Act::Save, "Save", KeyShortcut::ctrl('s'));
        menu.bind(Act::Open, "Open", KeyShortcut::ctrl('o'));
        menu.bind(Act::Undo, "Undo", KeyShortcut::new(true, true, false, 'z'));

        // Pressing a bound shortcut fires its action.
        assert_eq!(menu.fire(KeyShortcut::ctrl('s')), Some(Act::Save));
        assert_eq!(menu.fire(KeyShortcut::ctrl('o')), Some(Act::Open));
        assert_eq!(
            menu.fire(KeyShortcut::new(true, true, false, 'z')),
            Some(Act::Undo)
        );

        // An unbound key press fires nothing.
        assert_eq!(menu.fire(KeyShortcut::ctrl('q')), None);
        // Ctrl+S is distinct from Ctrl+Shift+S — modifiers must match exactly.
        assert_eq!(menu.fire(KeyShortcut::new(true, true, false, 's')), None);

        // The shortcut is shown as accelerator text on the item.
        assert_eq!(menu.accelerator_for(&Act::Save).as_deref(), Some("Ctrl+S"));
        assert_eq!(
            menu.accelerator_for(&Act::Undo).as_deref(),
            Some("Ctrl+Shift+Z")
        );

        // The item label carries the accelerator text alongside the name.
        assert_eq!(menu.item_label(&Act::Save).as_deref(), Some("Save\tCtrl+S"));
    }
}
