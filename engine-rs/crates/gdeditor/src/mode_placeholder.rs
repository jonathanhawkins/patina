//! Per-mode placeholder panes for partially-implemented main-screen modes.
//!
//! Not every main-screen mode has a fully-implemented central view yet. For
//! those "partial" modes the editor still has to render *something* in the main
//! pane rather than a blank area or a stale view — Godot shows a labelled
//! placeholder. This module decides, per [`MainView`], whether the real view
//! component is mounted or a [`PlaceholderPane`] stands in for it, and exposes
//! that decision on [`MainPane`].
//!
//! It lives in its own module (using only the public API of
//! [`main_screen`](crate::main_screen)) so it composes with the mode switcher
//! without entangling the switcher's own file.

use crate::main_screen::{MainPane, MainView, ViewComponent};

/// What the main pane actually renders for a mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneContent {
    /// The real, implemented view component is mounted.
    View(ViewComponent),
    /// A placeholder is shown because this mode's view isn't implemented yet.
    Placeholder(PlaceholderPane),
}

impl PaneContent {
    /// Whether this content is a placeholder (vs. a real view component).
    pub fn is_placeholder(&self) -> bool {
        matches!(self, PaneContent::Placeholder(_))
    }
}

/// A placeholder pane shown for a partially-implemented main-screen mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceholderPane {
    /// The view this placeholder stands in for.
    pub view: MainView,
    /// User-facing message explaining the view isn't available yet.
    pub message: String,
}

impl MainView {
    /// A short human-readable name for this view (used in placeholder text).
    pub fn display_name(self) -> &'static str {
        match self {
            MainView::Canvas2D => "2D viewport",
            MainView::Spatial3D => "3D viewport",
            MainView::ScriptEditor => "Script editor",
            MainView::GamePreview => "Game view",
            MainView::AssetLibBrowser => "AssetLib browser",
        }
    }

    /// Whether this view has a real, implemented component. Implemented views
    /// mount their component; the rest fall back to a placeholder.
    ///
    /// Every main-screen view now has a real component:
    ///
    /// - the Game view (pat-xwdlg) shows the running game's live frames — the
    ///   frame pump long-polls `/api/viewport?after=<seq>` into
    ///   `#game-screen-img`, with the Godot-style runtime toolbar
    ///   (`editor_play_game_view_wiring_test`, `editor_game_toolbar_runtime_test`);
    /// - the AssetLib browser (pat-4jaq7, pat-s3rrv.227) fetches the project's
    ///   configured catalog (`editor/assetlib/index_url`), renders it, and
    ///   installs a chosen asset's zip (`editor_assetlib_catalog_url_served`),
    ///   with an honest empty state when no catalog is configured.
    ///
    /// They were `false` while those views were labelled placeholders. The
    /// placeholder mechanism below is kept for any future partial view; the
    /// parity-criteria lint (`parity_criteria_lint.rs`) reads this answer.
    pub fn is_implemented(self) -> bool {
        match self {
            MainView::Canvas2D
            | MainView::Spatial3D
            | MainView::ScriptEditor
            | MainView::GamePreview
            | MainView::AssetLibBrowser => true,
        }
    }

    /// The content the main pane renders for this view: the real component when
    /// implemented, otherwise a labelled placeholder.
    pub fn pane_content(self) -> PaneContent {
        if self.is_implemented() {
            PaneContent::View(self.component())
        } else {
            PaneContent::Placeholder(PlaceholderPane {
                view: self,
                message: format!("{} is not available yet", self.display_name()),
            })
        }
    }
}

impl MainPane {
    /// The content the main pane currently renders for the active mode — the
    /// real view component when that mode is implemented, otherwise a
    /// placeholder.
    pub fn pane_content(&self) -> PaneContent {
        self.switcher().main_view().pane_content()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EditorMode;

    /// Acceptance (pat-p9k6o.3): the main pane mounts the real view component
    /// for every implemented mode — now all five, since the Game view and the
    /// AssetLib browser landed — and a placeholder pane still carries a
    /// labelled message naming its view.
    #[test]
    fn editor_mode_pane_placeholder_renders() {
        for view in [
            MainView::Canvas2D,
            MainView::Spatial3D,
            MainView::ScriptEditor,
            MainView::GamePreview,
            MainView::AssetLibBrowser,
        ] {
            assert!(view.is_implemented(), "{} is implemented", view.display_name());
            match view.pane_content() {
                PaneContent::View(c) => assert_eq!(c.view, view),
                PaneContent::Placeholder(_) => {
                    panic!("{} should mount a real view", view.display_name())
                }
            }
        }

        // Through the main pane: every route mounts the real view.
        let mut pane = MainPane::new();
        assert!(
            !pane.pane_content().is_placeholder(),
            "2D default is a real view"
        );
        assert!(pane.route_to(EditorMode::Spatial3D));
        match pane.pane_content() {
            PaneContent::View(c) => assert_eq!(c.component_id, "spatial_editor"),
            PaneContent::Placeholder(_) => panic!("3D should be a real view"),
        }
        assert!(pane.route_to(EditorMode::AssetLib));
        match pane.pane_content() {
            PaneContent::View(c) => {
                assert_eq!(c.view, MainView::AssetLibBrowser);
                assert_eq!(c.component_id, "asset_lib");
            }
            PaneContent::Placeholder(_) => panic!("AssetLib is a real view now"),
        }
        assert!(pane.route_to(EditorMode::Game));
        match pane.pane_content() {
            PaneContent::View(c) => assert_eq!(c.component_id, "game_view"),
            PaneContent::Placeholder(_) => panic!("Game is a real view now"),
        }

        // The placeholder shape itself is unchanged for a future partial view.
        let p = PlaceholderPane {
            view: MainView::GamePreview,
            message: format!("{} is not available yet", MainView::GamePreview.display_name()),
        };
        assert!(PaneContent::Placeholder(p.clone()).is_placeholder());
        assert!(p.message.contains("Game view"));
    }
}
