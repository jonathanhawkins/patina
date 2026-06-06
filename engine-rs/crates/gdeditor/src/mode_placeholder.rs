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
    /// The 2D viewport, 3D viewport, and script editor have real views; the
    /// game/play view and the AssetLib browser are not implemented yet.
    pub fn is_implemented(self) -> bool {
        match self {
            MainView::Canvas2D | MainView::Spatial3D | MainView::ScriptEditor => true,
            MainView::GamePreview | MainView::AssetLibBrowser => false,
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

    /// Acceptance (pat-p9k6o.3): the main pane renders a labelled placeholder
    /// for partially-implemented modes, and the real view component for
    /// implemented modes.
    #[test]
    fn editor_mode_pane_placeholder_renders() {
        // Implemented views mount their real component.
        for view in [
            MainView::Canvas2D,
            MainView::Spatial3D,
            MainView::ScriptEditor,
        ] {
            assert!(view.is_implemented());
            match view.pane_content() {
                PaneContent::View(c) => assert_eq!(c.view, view),
                PaneContent::Placeholder(_) => {
                    panic!("{} should mount a real view", view.display_name())
                }
            }
        }

        // Partial views render a labelled placeholder instead.
        for view in [MainView::GamePreview, MainView::AssetLibBrowser] {
            assert!(!view.is_implemented());
            let content = view.pane_content();
            assert!(content.is_placeholder());
            match content {
                PaneContent::Placeholder(p) => {
                    assert_eq!(p.view, view);
                    assert!(!p.message.is_empty(), "placeholder carries a message");
                    assert!(p.message.contains(view.display_name()));
                }
                PaneContent::View(_) => panic!("{} should be a placeholder", view.display_name()),
            }
        }

        // Through the main pane: routing to an implemented mode mounts the real
        // view; routing to a partial mode (AssetLib) shows the placeholder.
        let mut pane = MainPane::new();
        assert!(!pane.pane_content().is_placeholder(), "2D default is a real view");

        assert!(pane.route_to(EditorMode::Spatial3D));
        match pane.pane_content() {
            PaneContent::View(c) => assert_eq!(c.component_id, "spatial_editor"),
            PaneContent::Placeholder(_) => panic!("3D should be a real view"),
        }

        assert!(pane.route_to(EditorMode::AssetLib));
        let content = pane.pane_content();
        assert!(content.is_placeholder(), "AssetLib renders a placeholder pane");
        if let PaneContent::Placeholder(p) = content {
            assert_eq!(p.view, MainView::AssetLibBrowser);
        }
    }
}
