//! The main-screen mode switcher (top-bar 2D / 3D / Script / AssetLib buttons).
//!
//! Godot's editor centers on a main-screen switcher: a row of mutually
//! exclusive buttons that swap the central view between the 2D canvas, the 3D
//! spatial editor, the script editor, and the AssetLib browser. Exactly one
//! button is active at a time, and the active button is highlighted. This
//! models that selection state so the UI can render which view is shown and
//! which button is pressed.

use crate::EditorMode;

/// One main-screen mode button and whether it is the active (pressed) one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeButton {
    /// The editor mode this button selects.
    pub mode: EditorMode,
    /// Whether this is the currently active button.
    pub active: bool,
}

/// The central pane the editor renders for the active main-screen mode.
///
/// Selecting a mode button doesn't just highlight the button — it swaps the
/// whole central view. Each mode renders its own real view, so the central
/// area is never stuck on the 2D viewport. The runtime maps each `MainView`
/// to the corresponding pane widget (2D viewport, 3D viewport, script editor,
/// game/play view, AssetLib browser).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainView {
    /// The 2D canvas viewport.
    Canvas2D,
    /// The 3D spatial viewport.
    Spatial3D,
    /// The script/code editor pane.
    ScriptEditor,
    /// The running game / play view.
    GamePreview,
    /// The AssetLib browser.
    AssetLibBrowser,
}

/// A concrete central-pane view component the editor mounts for a [`MainView`].
///
/// The runtime maps each `component_id` to a real widget; the main pane mounts
/// exactly one component at a time and swaps it when the active mode changes.
/// The ids mirror Godot's main-screen plugin names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewComponent {
    /// Which view this component renders.
    pub view: MainView,
    /// Stable identifier the runtime uses to mount the widget.
    pub component_id: &'static str,
}

impl MainView {
    /// The view component the editor mounts to render this view.
    pub fn component(self) -> ViewComponent {
        let component_id = match self {
            MainView::Canvas2D => "canvas_item_editor",
            MainView::Spatial3D => "spatial_editor",
            MainView::ScriptEditor => "script_editor",
            MainView::GamePreview => "game_view",
            MainView::AssetLibBrowser => "asset_lib",
        };
        ViewComponent {
            view: self,
            component_id,
        }
    }
}

/// The main-screen mode switcher. Tracks which central view is shown and
/// reflects it in the button states. The button set is exactly the four
/// main-screen modes (2D, 3D, Script, AssetLib), in toolbar order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MainScreenSwitcher {
    active: EditorMode,
}

impl Default for MainScreenSwitcher {
    fn default() -> Self {
        Self::new()
    }
}

impl MainScreenSwitcher {
    /// The main-screen modes, in toolbar display order. This is the button set
    /// the switcher toggles between (the `Game` mode is driven by the run
    /// controls, not this switcher).
    pub const MODES: [EditorMode; 4] = [
        EditorMode::Canvas2D,
        EditorMode::Spatial3D,
        EditorMode::Script,
        EditorMode::AssetLib,
    ];

    /// Creates a switcher defaulting to the 2D canvas view (Godot's default).
    pub fn new() -> Self {
        Self {
            active: EditorMode::Canvas2D,
        }
    }

    /// The mode whose view is currently shown in the central editor.
    pub fn active(&self) -> EditorMode {
        self.active
    }

    /// The central pane the editor renders for `mode`. This is the actual view
    /// shown in the main area — each mode maps to its own real view rather than
    /// every mode falling back to the 2D viewport.
    pub fn view_for(mode: EditorMode) -> MainView {
        match mode {
            EditorMode::Canvas2D => MainView::Canvas2D,
            EditorMode::Spatial3D => MainView::Spatial3D,
            EditorMode::Script => MainView::ScriptEditor,
            EditorMode::Game => MainView::GamePreview,
            EditorMode::AssetLib => MainView::AssetLibBrowser,
        }
    }

    /// The central pane currently rendered, following the active mode. The UI
    /// renders this view in the main editor area; switching modes swaps it.
    pub fn main_view(&self) -> MainView {
        Self::view_for(self.active)
    }

    /// Whether `mode` is the active (pressed) button.
    pub fn is_active(&self, mode: EditorMode) -> bool {
        self.active == mode
    }

    /// Selects `mode`, switching the central view to it. Returns `true` if the
    /// view actually changed (selecting the already-active mode is a no-op).
    pub fn select(&mut self, mode: EditorMode) -> bool {
        if self.active == mode {
            return false;
        }
        self.active = mode;
        true
    }

    /// The mode buttons with their active state, in toolbar order — exactly
    /// one is active. This is what the top-bar renders.
    pub fn button_states(&self) -> Vec<ModeButton> {
        Self::MODES
            .iter()
            .map(|&mode| ModeButton {
                mode,
                active: self.active == mode,
            })
            .collect()
    }
}

/// The editor's central pane.
///
/// The main pane mounts exactly one view component at a time — the one for the
/// active main-screen mode — and re-routes to a different component whenever the
/// active mode changes. This is what makes 2D/3D/Script/AssetLib actually swap
/// the central widget rather than leaving a fixed view mounted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MainPane {
    switcher: MainScreenSwitcher,
}

impl Default for MainPane {
    fn default() -> Self {
        Self::new()
    }
}

impl MainPane {
    /// Creates a main pane routed to the default (2D) mode's view component.
    pub fn new() -> Self {
        Self {
            switcher: MainScreenSwitcher::new(),
        }
    }

    /// The switcher driving which mode (and therefore which component) is active.
    pub fn switcher(&self) -> &MainScreenSwitcher {
        &self.switcher
    }

    /// The active main-screen mode.
    pub fn active_mode(&self) -> EditorMode {
        self.switcher.active()
    }

    /// The view component the main pane currently routes to (mounts). Always the
    /// component for the active mode.
    pub fn routed_component(&self) -> ViewComponent {
        self.switcher.main_view().component()
    }

    /// Switches the active mode and re-routes the main pane to that mode's view
    /// component. Returns `true` if the routed component actually changed
    /// (routing to the already-active mode is a no-op).
    pub fn route_to(&mut self, mode: EditorMode) -> bool {
        self.switcher.select(mode)
    }

    /// Notifies the main pane that the editor's node selection changed.
    ///
    /// The active main-screen mode is editor-global, not per-node, so selecting
    /// a different node never re-routes the central pane — the same mode and
    /// mounted component persist. Returns the (unchanged) routed component.
    pub fn on_selection_changed(&self) -> ViewComponent {
        self.routed_component()
    }

    /// Notifies the main pane that the active scene/document changed (New Scene,
    /// Open Scene, or a tab switch).
    ///
    /// Like selection, the main-screen mode persists across scene changes — the
    /// editor stays in whatever mode you were in rather than snapping back to
    /// the 2D view — so the routed component is preserved. Returns the
    /// (unchanged) routed component.
    pub fn on_scene_changed(&self) -> ViewComponent {
        self.routed_component()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-3tdle): selecting a mode button switches the central
    /// editor view between 2D, 3D, Script, and AssetLib, and the active mode is
    /// reflected in the button state (exactly one button pressed at a time).
    #[test]
    fn top_bar_editor_mode_switch() {
        let mut switcher = MainScreenSwitcher::new();

        // Defaults to the 2D canvas, with only the 2D button active, and the
        // central pane renders the 2D viewport.
        assert_eq!(switcher.active(), EditorMode::Canvas2D);
        assert!(switcher.is_active(EditorMode::Canvas2D));
        assert!(!switcher.is_active(EditorMode::Spatial3D));
        assert_eq!(switcher.main_view(), MainView::Canvas2D);

        // The switcher exposes exactly the four main-screen buttons in order.
        let labels: Vec<&str> = switcher
            .button_states()
            .iter()
            .map(|b| b.mode.label())
            .collect();
        assert_eq!(labels, vec!["2D", "3D", "Script", "AssetLib"]);

        // Selecting each mode switches the central view and reflects the active
        // button — exactly one button is pressed at a time. `select` reports a
        // real switch only when the mode actually changes.
        for &mode in &MainScreenSwitcher::MODES {
            let was_active = switcher.active();
            let changed = switcher.select(mode);
            assert_eq!(
                changed,
                was_active != mode,
                "select reports a change iff the view actually switched"
            );
            assert_eq!(switcher.active(), mode, "view switched to {}", mode.label());

            // The central pane follows the active mode — it renders that mode's
            // real view, not a fixed 2D viewport.
            assert_eq!(
                switcher.main_view(),
                MainScreenSwitcher::view_for(mode),
                "central pane renders {}'s real view",
                mode.label()
            );

            let states = switcher.button_states();
            let active_count = states.iter().filter(|b| b.active).count();
            assert_eq!(active_count, 1, "exactly one button active");
            assert!(
                states.iter().find(|b| b.mode == mode).unwrap().active,
                "the selected button is the active one"
            );
        }

        // Re-selecting the already-active mode is a no-op (no view change).
        let mode = switcher.active();
        assert!(!switcher.select(mode), "re-selecting active mode does nothing");
        assert_eq!(switcher.active(), mode);
    }

    /// Acceptance (pat-p9k6o): switching modes renders each mode's real view in
    /// the central pane, instead of always showing the 2D viewport. Switching
    /// to SCRIPT replaces the 2D viewport with the script editor pane, 3D shows
    /// the 3D viewport, and ASSETLIB shows the asset browser.
    #[test]
    fn main_screen_switch_renders_each_modes_real_view() {
        let mut switcher = MainScreenSwitcher::new();

        // Starts on the 2D viewport.
        assert_eq!(switcher.main_view(), MainView::Canvas2D);

        // SCRIPT replaces the 2D viewport with the script editor pane.
        assert!(switcher.select(EditorMode::Script));
        assert_eq!(switcher.main_view(), MainView::ScriptEditor);
        assert_ne!(switcher.main_view(), MainView::Canvas2D);

        // 3D shows the 3D spatial viewport.
        assert!(switcher.select(EditorMode::Spatial3D));
        assert_eq!(switcher.main_view(), MainView::Spatial3D);

        // ASSETLIB shows the asset library browser.
        assert!(switcher.select(EditorMode::AssetLib));
        assert_eq!(switcher.main_view(), MainView::AssetLibBrowser);

        // 2D switches back to the canvas viewport.
        assert!(switcher.select(EditorMode::Canvas2D));
        assert_eq!(switcher.main_view(), MainView::Canvas2D);

        // Every main-screen mode maps to a distinct central view — no two modes
        // share a pane, so the switcher never gets stuck on one view.
        let views: Vec<MainView> = MainScreenSwitcher::MODES
            .iter()
            .map(|&m| MainScreenSwitcher::view_for(m))
            .collect();
        for (i, a) in views.iter().enumerate() {
            for b in &views[i + 1..] {
                assert_ne!(a, b, "each mode renders a distinct central view");
            }
        }

        // The run-driven Game mode maps to the play view.
        assert_eq!(
            MainScreenSwitcher::view_for(EditorMode::Game),
            MainView::GamePreview
        );
    }

    /// Acceptance (pat-p9k6o.1): the main pane routes to (mounts) the active
    /// mode's view component, and re-routes to a distinct component whenever the
    /// active mode changes.
    #[test]
    fn editor_main_pane_routes_to_active_mode() {
        let mut pane = MainPane::new();

        // Defaults to the 2D mode → routes to the 2D canvas component.
        assert_eq!(pane.active_mode(), EditorMode::Canvas2D);
        let comp = pane.routed_component();
        assert_eq!(comp.view, MainView::Canvas2D);
        assert_eq!(comp.component_id, "canvas_item_editor");

        // Routing to a new mode mounts that mode's component.
        let cases = [
            (EditorMode::Script, MainView::ScriptEditor, "script_editor"),
            (EditorMode::Spatial3D, MainView::Spatial3D, "spatial_editor"),
            (EditorMode::AssetLib, MainView::AssetLibBrowser, "asset_lib"),
            (EditorMode::Canvas2D, MainView::Canvas2D, "canvas_item_editor"),
        ];
        for (mode, view, id) in cases {
            let changed = pane.route_to(mode);
            assert!(changed, "routing to {} re-mounts a component", mode.label());
            assert_eq!(pane.active_mode(), mode);
            let comp = pane.routed_component();
            assert_eq!(comp.view, view, "main pane routes to {}'s view", mode.label());
            assert_eq!(comp.component_id, id);
            // The routed component is always consistent with the switcher.
            assert_eq!(comp.view, pane.switcher().main_view());
        }

        // Re-routing to the already-active mode is a no-op (no re-mount).
        assert!(!pane.route_to(EditorMode::Canvas2D));
        assert_eq!(pane.routed_component().component_id, "canvas_item_editor");

        // Every main-screen mode routes to a distinct component id.
        let ids: Vec<&str> = MainScreenSwitcher::MODES
            .iter()
            .map(|&m| MainScreenSwitcher::view_for(m).component().component_id)
            .collect();
        for (i, a) in ids.iter().enumerate() {
            for b in &ids[i + 1..] {
                assert_ne!(a, b, "each mode routes to a distinct view component");
            }
        }
    }

    /// Acceptance (pat-p9k6o.2): the active main-screen mode is editor-global —
    /// it persists across node-selection changes and scene/document changes.
    /// Only an explicit mode switch changes the central view; selecting nodes or
    /// switching scenes never snaps the pane back to the 2D view.
    #[test]
    fn editor_active_mode_persists() {
        let mut pane = MainPane::new();

        // Switch into SCRIPT mode; the pane mounts the script editor component.
        assert!(pane.route_to(EditorMode::Script));
        assert_eq!(pane.active_mode(), EditorMode::Script);
        let script_component = pane.routed_component();
        assert_eq!(script_component.component_id, "script_editor");

        // Repeated node-selection changes never re-route the pane: the active
        // mode and the mounted component persist.
        for _ in 0..3 {
            let after = pane.on_selection_changed();
            assert_eq!(
                pane.active_mode(),
                EditorMode::Script,
                "the active mode persists across selection changes"
            );
            assert_eq!(
                after, script_component,
                "a selection change keeps the same component mounted"
            );
        }

        // Scene/document changes (New Scene, Open Scene, tab switch) likewise
        // preserve the active mode.
        for _ in 0..3 {
            let after = pane.on_scene_changed();
            assert_eq!(
                pane.active_mode(),
                EditorMode::Script,
                "the active mode persists across scene changes"
            );
            assert_eq!(
                after, script_component,
                "a scene change keeps the same component mounted"
            );
        }

        // The invariant holds for another mode too — selection/scene events
        // don't reset 3D back to 2D.
        assert!(pane.route_to(EditorMode::Spatial3D));
        pane.on_selection_changed();
        pane.on_scene_changed();
        assert_eq!(
            pane.active_mode(),
            EditorMode::Spatial3D,
            "a different active mode also persists across selection + scene changes"
        );
        assert_eq!(pane.routed_component().component_id, "spatial_editor");

        // Only an explicit mode switch changes the central view.
        assert!(pane.route_to(EditorMode::Canvas2D));
        assert_eq!(pane.active_mode(), EditorMode::Canvas2D);
        assert!(
            !pane.route_to(EditorMode::Canvas2D),
            "re-routing to the already-active mode is a no-op"
        );
    }
}
