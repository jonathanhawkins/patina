//! The play-custom-scene control.
//!
//! Besides running the project's configured *main* scene, the editor can run an
//! arbitrary chosen scene. Activating the play-custom-scene control launches the
//! specified scene and records it as the *last-played custom scene* so it can be
//! replayed quickly without re-selecting it.

/// Tracks custom-scene play requests and remembers the most recent one for
/// quick replay.
#[derive(Debug, Clone, Default)]
pub struct PlayCustomScene {
    /// The path of the scene currently running, if any.
    running: Option<String>,
    /// The last custom scene that was played, for quick replay.
    last_played: Option<String>,
}

/// The outcome of a play request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayOutcome {
    /// The given scene was launched.
    Launched(String),
    /// Nothing was launched (e.g. quick-replay with no prior custom scene).
    Nothing,
}

impl PlayCustomScene {
    /// Creates a control with no scene running and no replay history.
    pub fn new() -> Self {
        Self::default()
    }

    /// The scene currently running, if any.
    pub fn running(&self) -> Option<&str> {
        self.running.as_deref()
    }

    /// The last custom scene that was played, if any.
    pub fn last_played(&self) -> Option<&str> {
        self.last_played.as_deref()
    }

    /// Runs a chosen scene other than the main scene: launches `scene_path` and
    /// records it as the last-played custom scene for quick replay.
    pub fn play(&mut self, scene_path: &str) -> PlayOutcome {
        self.running = Some(scene_path.to_string());
        self.last_played = Some(scene_path.to_string());
        PlayOutcome::Launched(scene_path.to_string())
    }

    /// Re-runs the last-played custom scene, if there is one. Returns
    /// [`PlayOutcome::Nothing`] when no custom scene has been played yet.
    pub fn replay_last(&mut self) -> PlayOutcome {
        match self.last_played.clone() {
            Some(scene) => {
                self.running = Some(scene.clone());
                PlayOutcome::Launched(scene)
            }
            None => PlayOutcome::Nothing,
        }
    }

    /// Stops the running scene (the last-played record is retained).
    pub fn stop(&mut self) {
        self.running = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-nz9qs): selecting play-custom-scene runs the specified
    /// scene and records it as the last-played custom scene for quick replay.
    #[test]
    fn top_bar_play_custom_scene() {
        let mut control = PlayCustomScene::new();
        assert_eq!(control.running(), None);
        assert_eq!(control.last_played(), None);

        // Selecting play-custom-scene runs the chosen scene (not the main one).
        let outcome = control.play("res://levels/boss.tscn");
        assert_eq!(
            outcome,
            PlayOutcome::Launched("res://levels/boss.tscn".to_string()),
            "the specified scene is launched"
        );
        assert_eq!(control.running(), Some("res://levels/boss.tscn"));
        // It is recorded as the last-played custom scene.
        assert_eq!(control.last_played(), Some("res://levels/boss.tscn"));

        // Stopping clears the running scene but keeps the replay record.
        control.stop();
        assert_eq!(control.running(), None);
        assert_eq!(
            control.last_played(),
            Some("res://levels/boss.tscn"),
            "the last-played scene is remembered after stopping"
        );

        // Quick replay re-runs the remembered scene without re-selecting it.
        let replay = control.replay_last();
        assert_eq!(
            replay,
            PlayOutcome::Launched("res://levels/boss.tscn".to_string()),
            "quick replay relaunches the last custom scene"
        );
        assert_eq!(control.running(), Some("res://levels/boss.tscn"));

        // Playing a different custom scene updates the last-played record.
        control.play("res://ui/menu.tscn");
        assert_eq!(control.running(), Some("res://ui/menu.tscn"));
        assert_eq!(control.last_played(), Some("res://ui/menu.tscn"));
        control.stop();
        assert_eq!(
            control.replay_last(),
            PlayOutcome::Launched("res://ui/menu.tscn".to_string()),
            "replay follows the most recently played scene"
        );
    }

    /// Quick replay with no prior custom scene launches nothing.
    #[test]
    fn replay_without_history_is_noop() {
        let mut control = PlayCustomScene::new();
        assert_eq!(control.replay_last(), PlayOutcome::Nothing);
        assert_eq!(control.running(), None);
        assert_eq!(control.last_played(), None);
    }
}
