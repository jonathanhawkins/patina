//! `MenuBar` / `MenuButton` open-state machine (pat-0q2kx.177).
//!
//! Godot ground truth: `scene/gui/menu_bar.cpp` (a Control-drawn strip of
//! titles, one per child `PopupMenu`, with switch-on-hover while a menu is
//! open) and `scene/gui/menu_button.cpp` (a flat button that opens its child
//! `PopupMenu` beneath itself on press).
//!
//! The [`PopupMenu`](crate::popup::PopupMenu) model already existed — items,
//! separators, checkable entries, `activate_item` returning the id that becomes
//! `id_pressed` — with **zero non-test callers**. Nothing could open one. This
//! is the state machine that decides *which* menu is open, which is the part
//! that has real behaviour to get wrong; drawing the strip and routing real
//! clicks sit on top of it.

/// Which menu of a bar is open, and which title the pointer is over.
///
/// Godot keeps "open" and "hovered" apart because hovering only *switches*
/// menus while one is already open — moving the pointer across a closed bar
/// must not open anything.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MenuBarModel {
    titles: Vec<String>,
    open: Option<usize>,
    hovered: Option<usize>,
}

impl MenuBarModel {
    /// A bar whose titles come from its child `PopupMenu`s, in order.
    pub fn new(titles: Vec<String>) -> Self {
        Self {
            titles,
            open: None,
            hovered: None,
        }
    }

    /// The title strip, one entry per child menu.
    pub fn titles(&self) -> &[String] {
        &self.titles
    }

    /// The open menu's index, if any.
    pub fn open_index(&self) -> Option<usize> {
        self.open
    }

    /// The hovered title's index, if any.
    pub fn hovered_index(&self) -> Option<usize> {
        self.hovered
    }

    /// Whether any menu is open — what the popup layer checks to decide if it
    /// should be capturing input.
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// A click on title `index`. Returns the newly open index, or `None` when
    /// the click closed the menu.
    ///
    /// Clicking the already-open title closes it, matching Godot: a menu button
    /// toggles rather than re-opening.
    pub fn click_title(&mut self, index: usize) -> Option<usize> {
        if index >= self.titles.len() {
            return self.open;
        }
        if self.open == Some(index) {
            self.open = None;
        } else {
            self.open = Some(index);
        }
        self.open
    }

    /// The pointer moved over title `index`.
    ///
    /// Switch-on-hover: this only changes which menu is open **while one
    /// already is**. Hovering across a closed bar just tracks the highlight —
    /// opening menus on hover alone would make the bar impossible to move the
    /// pointer past.
    pub fn hover_title(&mut self, index: usize) -> Option<usize> {
        if index >= self.titles.len() {
            return self.open;
        }
        self.hovered = Some(index);
        if self.open.is_some() {
            self.open = Some(index);
        }
        self.open
    }

    /// The pointer left the strip.
    pub fn clear_hover(&mut self) {
        self.hovered = None;
    }

    /// Closes whatever is open — what an item activation or a click elsewhere
    /// does.
    pub fn close(&mut self) {
        self.open = None;
    }
}

/// A `MenuButton`: a single title that toggles its own child `PopupMenu`.
///
/// Godot's MenuButton is the degenerate one-entry case of the bar, but it is a
/// separate class with its own `about_to_popup` signal, so it gets its own tiny
/// model rather than pretending to be a one-title bar.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MenuButtonModel {
    open: bool,
}

impl MenuButtonModel {
    /// A closed menu button.
    pub fn new() -> Self {
        Self { open: false }
    }

    /// Whether its popup is showing.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// A press toggles the popup. Returns `true` when it just opened, which is
    /// when Godot emits `about_to_popup` — the signal fires on opening only,
    /// not on the closing press.
    pub fn press(&mut self) -> bool {
        self.open = !self.open;
        self.open
    }

    /// Closes the popup, as activating an item or clicking away does.
    pub fn close(&mut self) {
        self.open = false;
    }
}
