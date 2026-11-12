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

    /// Height of the title strip.
    pub const TITLE_HEIGHT: f32 = 24.0;

    /// Horizontal padding either side of a title's text.
    pub const TITLE_PADDING: f32 = 10.0;

    /// Nominal advance per character in a title.
    ///
    /// ⚠️ Patina has no font metrics at this layer — the popup draw passes text
    /// through without measuring it — so the strip uses a synthetic advance.
    /// The number itself barely matters; what matters is that
    /// [`title_at`](Self::title_at) and the draw pass derive their layout from
    /// this SAME rule. If the width used to map a click differs from the width
    /// used to place the titles, every click past the first title opens the
    /// wrong menu, and it looks correct until you try the last one — the same
    /// trap [`PopupMenu::ROW_HEIGHT`](crate::popup::PopupMenu::ROW_HEIGHT)
    /// exists to avoid vertically.
    pub const TITLE_CHAR_WIDTH: f32 = 8.0;

    /// Width of one title's clickable cell.
    pub fn title_width(title: &str) -> f32 {
        title.chars().count() as f32 * Self::TITLE_CHAR_WIDTH + Self::TITLE_PADDING * 2.0
    }

    /// Left edge of title `index` within the strip.
    pub fn title_x(&self, index: usize) -> f32 {
        self.titles
            .iter()
            .take(index)
            .map(|t| Self::title_width(t))
            .sum()
    }

    /// Total width of the strip.
    pub fn strip_width(&self) -> f32 {
        self.title_x(self.titles.len())
    }

    /// The title under `x` (an offset within the bar), or `None` past the end.
    ///
    /// Answers regardless of whether a menu is open — opening is the caller's
    /// decision, and hover-to-switch needs the same mapping a press uses.
    pub fn title_at(&self, x: f32) -> Option<usize> {
        if x < 0.0 {
            return None;
        }
        let mut edge = 0.0;
        for (i, t) in self.titles.iter().enumerate() {
            edge += Self::title_width(t);
            if x < edge {
                return Some(i);
            }
        }
        None
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
