//! The `--debug-*` launch flags the editor passes to a play session
//! (pat-s3rrv.82).
//!
//! # Why this exists
//!
//! `gdeditor`'s Debug menu builds a launch command from its checked toggles —
//! `DebugToggles::play_session_flags()` emits one `--debug-*` argument per
//! checked item (`debug_menu_toggles.rs:86-93`). The runner's argument parser
//! ended with:
//!
//! ```text
//! other => return Err(format!("unknown argument: {other}")),
//! ```
//!
//! …and knew none of them. So **checking any item in the editor's Debug menu
//! made Play fail outright** with `unknown argument: --debug-navigation`, rather
//! than starting with that overlay enabled. The feature could not be switched on
//! from the only UI that offers it.
//!
//! Parsing lives here, in the library, rather than beside `parse_args` in
//! `main.rs`: a binary's private parser cannot be tested, and this is exactly
//! the kind of vocabulary agreement between two crates that deserves a test.
//!
//! ⚠️ The names are not invented here. They are the strings
//! `DebugToggles::flag` produces, and the two lists must stay in step — a flag
//! the editor emits and the runner does not know is a hard launch failure, not
//! a degraded overlay.

/// Which debug overlays / sync behaviours a play session was launched with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DebugFlags {
    /// `--debug-collisions`: draw collision shapes.
    pub collisions: bool,
    /// `--debug-navigation`: draw navigation meshes and links.
    pub navigation: bool,
    /// `--debug-paths`: draw `Path2D`/`Path3D` curves.
    pub paths: bool,
    /// `--debug-scene-sync`: re-apply scene edits into the running session.
    pub scene_sync: bool,
    /// `--debug-script-sync`: re-apply script edits into the running session.
    pub script_sync: bool,
}

impl DebugFlags {
    /// Applies `arg` if it is a known debug flag, reporting whether it was.
    ///
    /// Returning `false` rather than erroring keeps the decision with the
    /// caller: `parse_args` still rejects genuinely unknown arguments, so a
    /// typo is not silently swallowed.
    pub fn apply(&mut self, arg: &str) -> bool {
        match arg {
            "--debug-collisions" => self.collisions = true,
            "--debug-navigation" => self.navigation = true,
            "--debug-paths" => self.paths = true,
            "--debug-scene-sync" => self.scene_sync = true,
            "--debug-script-sync" => self.script_sync = true,
            _ => return false,
        }
        true
    }

    /// Whether any debug flag is set.
    pub fn any(&self) -> bool {
        self.collisions || self.navigation || self.paths || self.scene_sync || self.script_sync
    }

    /// The flags as `(name, enabled)` pairs, for the runner's JSON report.
    ///
    /// Names match Godot's own vocabulary (`debug_navigation_hint` and friends
    /// on `SceneTree`) minus the `_hint` suffix, so a consumer reading this
    /// report and a script reading the tree see the same words.
    pub fn as_pairs(&self) -> [(&'static str, bool); 5] {
        [
            ("debug_collisions", self.collisions),
            ("debug_navigation", self.navigation),
            ("debug_paths", self.paths),
            ("debug_scene_sync", self.scene_sync),
            ("debug_script_sync", self.script_sync),
        ]
    }
}
