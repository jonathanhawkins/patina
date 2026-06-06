//! Project Settings **Input Map** tab (pat-vjo4u).
//!
//! Manages the project's input actions: add/remove actions, bind key / mouse /
//! joypad events to them, and set a per-action deadzone. The map serializes to
//! and from project settings (JSON) so it persists, and resolves which actions
//! an input event triggers at runtime.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// An input event that can be bound to an action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputEvent {
    /// A keyboard key, by keycode name (e.g. `"Space"`).
    Key(String),
    /// A mouse button, by index.
    MouseButton(u32),
    /// A joypad button, by index.
    JoyButton(u32),
}

/// A single input action: its deadzone and bound events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    /// Analog deadzone in `[0, 1]`.
    pub deadzone: f32,
    /// The events bound to this action.
    pub events: Vec<InputEvent>,
}

impl Default for Action {
    fn default() -> Self {
        Self {
            deadzone: 0.5,
            events: Vec::new(),
        }
    }
}

/// The project input map: actions by name.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InputMap {
    actions: BTreeMap<String, Action>,
}

impl InputMap {
    /// Creates an empty input map.
    pub fn new() -> Self {
        Self::default()
    }

    /// All action names, sorted.
    pub fn actions(&self) -> Vec<&str> {
        self.actions.keys().map(String::as_str).collect()
    }

    /// Whether an action named `name` exists.
    pub fn has_action(&self, name: &str) -> bool {
        self.actions.contains_key(name)
    }

    /// Adds an action with the default deadzone. Returns false if it exists.
    pub fn add_action(&mut self, name: impl Into<String>) -> bool {
        let name = name.into();
        if self.actions.contains_key(&name) {
            return false;
        }
        self.actions.insert(name, Action::default());
        true
    }

    /// Removes an action. Returns whether it existed.
    pub fn remove_action(&mut self, name: &str) -> bool {
        self.actions.remove(name).is_some()
    }

    /// The deadzone of an action, if it exists.
    pub fn deadzone(&self, name: &str) -> Option<f32> {
        self.actions.get(name).map(|a| a.deadzone)
    }

    /// Sets an action's deadzone (clamped to `[0, 1]`). Returns whether it
    /// exists.
    pub fn set_deadzone(&mut self, name: &str, value: f32) -> bool {
        match self.actions.get_mut(name) {
            Some(a) => {
                a.deadzone = value.clamp(0.0, 1.0);
                true
            }
            None => false,
        }
    }

    /// The events bound to an action, if it exists.
    pub fn events(&self, name: &str) -> Option<&[InputEvent]> {
        self.actions.get(name).map(|a| a.events.as_slice())
    }

    /// Binds `event` to `action` (ignoring duplicates). Returns whether the
    /// action exists.
    pub fn bind(&mut self, action: &str, event: InputEvent) -> bool {
        match self.actions.get_mut(action) {
            Some(a) => {
                if !a.events.contains(&event) {
                    a.events.push(event);
                }
                true
            }
            None => false,
        }
    }

    /// Unbinds `event` from `action`. Returns whether the event was removed.
    pub fn unbind(&mut self, action: &str, event: &InputEvent) -> bool {
        match self.actions.get_mut(action) {
            Some(a) => {
                let before = a.events.len();
                a.events.retain(|e| e != event);
                a.events.len() != before
            }
            None => false,
        }
    }

    /// Whether `action` is bound to `event`.
    pub fn action_has(&self, action: &str, event: &InputEvent) -> bool {
        self.actions
            .get(action)
            .is_some_and(|a| a.events.contains(event))
    }

    /// Resolves which actions `event` triggers at runtime (sorted by name).
    pub fn actions_for_event(&self, event: &InputEvent) -> Vec<&str> {
        self.actions
            .iter()
            .filter(|(_, a)| a.events.contains(event))
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// Serializes the map to project-settings JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("InputMap serializes")
    }

    /// Loads the map from project-settings JSON.
    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }

    /// Builds an [`InputMap`] from the editor's runtime `action -> key-names`
    /// map (`EditorState::input_map`). Each key name becomes an
    /// [`InputEvent::Key`] bound to that action. This is how the live editor
    /// server projects its runtime input table into the editable model.
    pub fn from_runtime_map(runtime: &std::collections::HashMap<String, Vec<String>>) -> Self {
        let mut map = Self::new();
        for (action, keys) in runtime {
            map.add_action(action.clone());
            for key in keys {
                map.bind(action, InputEvent::Key(key.clone()));
            }
        }
        map
    }

    /// Projects the input map back into the editor's runtime
    /// `action -> key-names` map. Only [`InputEvent::Key`] bindings are
    /// representable at runtime (mouse/joypad events are dropped), matching
    /// the key-name lookup used by `EditorState::is_action_pressed`.
    pub fn to_runtime_map(&self) -> std::collections::HashMap<String, Vec<String>> {
        let mut out = std::collections::HashMap::new();
        for name in self.actions() {
            let keys: Vec<String> = self
                .events(name)
                .unwrap_or(&[])
                .iter()
                .filter_map(|e| match e {
                    InputEvent::Key(k) => Some(k.clone()),
                    _ => None,
                })
                .collect();
            out.insert(name.to_string(), keys);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-vjo4u): adding an action and binding an input event
    /// persists to the input map and the action resolves at runtime.
    #[test]
    fn systems_input_map_actions_persist() {
        let mut map = InputMap::new();

        // Add an action (with default deadzone); duplicates are rejected.
        assert!(map.add_action("jump"));
        assert!(!map.add_action("jump"));
        assert!(map.has_action("jump"));
        assert_eq!(map.deadzone("jump"), Some(0.5));

        // Bind key and joypad events.
        assert!(map.bind("jump", InputEvent::Key("Space".to_string())));
        assert!(map.bind("jump", InputEvent::JoyButton(0)));
        // Duplicate binds are ignored.
        assert!(map.bind("jump", InputEvent::Key("Space".to_string())));
        assert_eq!(map.events("jump").unwrap().len(), 2);

        // Set a custom deadzone.
        assert!(map.set_deadzone("jump", 0.2));
        assert_eq!(map.deadzone("jump"), Some(0.2));

        // The action resolves at runtime for its bound event.
        assert!(map.action_has("jump", &InputEvent::Key("Space".to_string())));
        assert_eq!(
            map.actions_for_event(&InputEvent::Key("Space".to_string())),
            vec!["jump"]
        );
        assert!(map
            .actions_for_event(&InputEvent::Key("Escape".to_string()))
            .is_empty());

        // Persist to project settings and reload — everything round-trips.
        let json = map.to_json();
        let loaded = InputMap::from_json(&json).expect("loads");
        assert!(loaded.has_action("jump"));
        assert_eq!(loaded.deadzone("jump"), Some(0.2));
        assert!(loaded.action_has("jump", &InputEvent::JoyButton(0)));
        assert_eq!(loaded, map);

        // Unbinding and removing.
        assert!(map.unbind("jump", &InputEvent::JoyButton(0)));
        assert_eq!(map.events("jump").unwrap().len(), 1);
        assert!(!map.unbind("jump", &InputEvent::JoyButton(0))); // already gone
        assert!(map.remove_action("jump"));
        assert!(!map.has_action("jump"));
        assert!(!map.remove_action("jump"));
    }

    /// Multiple actions can resolve from one event; binds/deadzone reject
    /// missing actions.
    #[test]
    fn multiple_actions_and_missing() {
        let mut map = InputMap::new();
        map.add_action("ui_accept");
        map.add_action("attack");
        map.bind("ui_accept", InputEvent::Key("Enter".to_string()));
        map.bind("attack", InputEvent::Key("Enter".to_string()));
        map.bind("attack", InputEvent::MouseButton(1));

        // One event triggers both actions (sorted by name).
        assert_eq!(
            map.actions_for_event(&InputEvent::Key("Enter".to_string())),
            vec!["attack", "ui_accept"]
        );
        assert_eq!(
            map.actions_for_event(&InputEvent::MouseButton(1)),
            vec!["attack"]
        );

        // Operations on a missing action return false.
        assert!(!map.bind("ghost", InputEvent::Key("X".to_string())));
        assert!(!map.set_deadzone("ghost", 0.1));
        assert!(map.deadzone("ghost").is_none());
    }

    /// The runtime <-> model projection round-trips Key bindings, which is the
    /// bridge the live editor server uses to edit `EditorState::input_map`.
    #[test]
    fn runtime_map_round_trips_key_bindings() {
        use std::collections::HashMap;

        let mut runtime: HashMap<String, Vec<String>> = HashMap::new();
        runtime.insert("jump".to_string(), vec!["Space".to_string()]);
        runtime.insert(
            "move_left".to_string(),
            vec!["a".to_string(), "ArrowLeft".to_string()],
        );

        // Runtime map -> model: each key name becomes a bound Key event.
        let model = InputMap::from_runtime_map(&runtime);
        assert!(model.has_action("jump"));
        assert!(model.action_has("jump", &InputEvent::Key("Space".to_string())));
        assert!(model.action_has("move_left", &InputEvent::Key("ArrowLeft".to_string())));

        // Model -> runtime map: Key events project back to key names.
        let back = model.to_runtime_map();
        assert_eq!(back.get("jump"), Some(&vec!["Space".to_string()]));
        let mut left = back.get("move_left").cloned().unwrap();
        left.sort();
        assert_eq!(left, vec!["ArrowLeft".to_string(), "a".to_string()]);

        // Non-key events are dropped from the runtime projection.
        let mut model2 = InputMap::new();
        model2.add_action("fire");
        model2.bind("fire", InputEvent::Key("Ctrl".to_string()));
        model2.bind("fire", InputEvent::JoyButton(0));
        model2.bind("fire", InputEvent::MouseButton(1));
        assert_eq!(
            model2.to_runtime_map().get("fire"),
            Some(&vec!["Ctrl".to_string()])
        );
    }
}
