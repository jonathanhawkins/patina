//! Help-menu command dispatch, routing each item to its target.
//!
//! Maps the top-bar **Help** menu items (Search Help, Online Docs, Report a
//! Bug, About) to where they go: Search Help raises the in-editor help search
//! dialog, Online Docs and Report a Bug open external links (the docs site and
//! the issue tracker), and About raises the about dialog.
//!
//! This is the action→command layer for the Help menu; it mirrors
//! [`crate::project_menu_commands`] and owns the help-search and about dialog
//! visibility it toggles.

/// An item in the Help menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpMenuAction {
    /// Open the in-editor help search dialog.
    SearchHelp,
    /// Open the online documentation in a browser.
    OnlineDocs,
    /// Open the issue tracker to report a bug.
    ReportABug,
    /// Open the About dialog.
    About,
}

impl HelpMenuAction {
    /// The external URL this item opens, or `None` for items that open an
    /// in-editor dialog instead (Search Help, About).
    pub fn url(&self) -> Option<&'static str> {
        match self {
            HelpMenuAction::OnlineDocs => Some("https://docs.patinaengine.com"),
            HelpMenuAction::ReportABug => {
                Some("https://github.com/patinaengine/patina/issues/new")
            }
            HelpMenuAction::SearchHelp | HelpMenuAction::About => None,
        }
    }
}

/// The outcome of dispatching a Help-menu action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelpCommand {
    /// The help search dialog was raised.
    OpenedHelpSearch,
    /// An external link was opened to the given URL.
    OpenedUrl(String),
    /// The About dialog was raised.
    OpenedAbout,
}

/// The Help menu and the dialogs/links it drives. Owns the help-search and
/// about dialog visibility so dispatching their items raises them directly,
/// and records the last external URL opened.
#[derive(Debug, Default)]
pub struct HelpMenu {
    help_search_visible: bool,
    about_visible: bool,
    last_url: Option<String>,
}

impl HelpMenu {
    /// Creates a Help menu with both dialogs closed and no link opened.
    pub fn new() -> Self {
        Self::default()
    }

    /// The Help-menu items, in display order.
    pub fn items() -> [HelpMenuAction; 4] {
        [
            HelpMenuAction::SearchHelp,
            HelpMenuAction::OnlineDocs,
            HelpMenuAction::ReportABug,
            HelpMenuAction::About,
        ]
    }

    /// Whether the help search dialog is currently shown.
    pub fn is_help_search_visible(&self) -> bool {
        self.help_search_visible
    }

    /// Whether the About dialog is currently shown.
    pub fn is_about_visible(&self) -> bool {
        self.about_visible
    }

    /// The last external URL opened via the Help menu, if any.
    pub fn last_url(&self) -> Option<&str> {
        self.last_url.as_deref()
    }

    /// Dispatches a Help-menu action to its target: raising the help search or
    /// about dialog, or opening an external link, and returns the resulting
    /// command.
    pub fn dispatch(&mut self, action: HelpMenuAction) -> HelpCommand {
        match action {
            HelpMenuAction::SearchHelp => {
                self.help_search_visible = true;
                HelpCommand::OpenedHelpSearch
            }
            HelpMenuAction::About => {
                self.about_visible = true;
                HelpCommand::OpenedAbout
            }
            HelpMenuAction::OnlineDocs | HelpMenuAction::ReportABug => {
                // External-link items always resolve to a target URL.
                let url = action.url().expect("link items have a URL").to_string();
                self.last_url = Some(url.clone());
                HelpCommand::OpenedUrl(url)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-pgt0x): Search Help opens the help search, an
    /// external-link item resolves its target URL, and About opens the about
    /// dialog.
    #[test]
    fn menus_help_actions_route_targets() {
        let mut menu = HelpMenu::new();

        // The menu exposes all four help items, in order.
        assert_eq!(
            HelpMenu::items(),
            [
                HelpMenuAction::SearchHelp,
                HelpMenuAction::OnlineDocs,
                HelpMenuAction::ReportABug,
                HelpMenuAction::About,
            ]
        );

        // Both dialogs start closed and no link has been opened.
        assert!(!menu.is_help_search_visible());
        assert!(!menu.is_about_visible());
        assert_eq!(menu.last_url(), None);

        // Search Help raises the in-editor help search dialog.
        assert_eq!(
            menu.dispatch(HelpMenuAction::SearchHelp),
            HelpCommand::OpenedHelpSearch
        );
        assert!(
            menu.is_help_search_visible(),
            "Search Help raised the help search dialog"
        );

        // Online Docs resolves and opens its external documentation URL.
        let docs = HelpMenuAction::OnlineDocs.url().unwrap();
        assert_eq!(
            menu.dispatch(HelpMenuAction::OnlineDocs),
            HelpCommand::OpenedUrl(docs.to_string()),
            "Online Docs resolves its target URL"
        );
        assert_eq!(menu.last_url(), Some(docs));

        // Report a Bug opens the issue tracker (a different external link).
        let bug = HelpMenuAction::ReportABug.url().unwrap();
        assert_ne!(docs, bug, "the two external links differ");
        assert_eq!(
            menu.dispatch(HelpMenuAction::ReportABug),
            HelpCommand::OpenedUrl(bug.to_string())
        );
        assert_eq!(menu.last_url(), Some(bug));

        // The in-editor dialog items carry no URL; the link items do.
        assert_eq!(HelpMenuAction::SearchHelp.url(), None);
        assert_eq!(HelpMenuAction::About.url(), None);

        // About raises the About dialog.
        assert_eq!(
            menu.dispatch(HelpMenuAction::About),
            HelpCommand::OpenedAbout
        );
        assert!(menu.is_about_visible(), "About raised the about dialog");
    }
}
