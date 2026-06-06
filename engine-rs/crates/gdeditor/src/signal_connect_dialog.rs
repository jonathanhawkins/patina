//! **Connect-a-Signal dialog** (pat-xw1xw).
//!
//! Models the dialog that connects one of a node's signals to a target node and
//! method chosen from the scene. The user picks a target node (which must be in
//! the scene), optionally overrides the suggested receiver method name, and on
//! confirm the dialog produces a [`Connection`] that the editor persists — after
//! which it appears under the signal in the Signals dock.

/// A persisted signal connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    /// The connected signal name.
    pub signal: String,
    /// The emitting (source) node.
    pub source: String,
    /// The target node receiving the signal.
    pub target: String,
    /// The receiver method on the target.
    pub method: String,
}

/// The Connect-a-Signal dialog state.
#[derive(Debug, Clone)]
pub struct ConnectDialog {
    source_node: String,
    signal: String,
    scene_nodes: Vec<String>,
    selected_target: Option<String>,
    method_override: Option<String>,
}

impl ConnectDialog {
    /// Opens the dialog to connect `signal` from `source_node`, choosing a
    /// target among `scene_nodes`.
    pub fn new(source_node: impl Into<String>, signal: impl Into<String>, scene_nodes: &[&str]) -> Self {
        Self {
            source_node: source_node.into(),
            signal: signal.into(),
            scene_nodes: scene_nodes.iter().map(|s| s.to_string()).collect(),
            selected_target: None,
            method_override: None,
        }
    }

    /// The nodes available to connect to.
    pub fn targets(&self) -> &[String] {
        &self.scene_nodes
    }

    /// The currently-selected target node, if any.
    pub fn selected_target(&self) -> Option<&str> {
        self.selected_target.as_deref()
    }

    /// Selects `node` as the target. Returns whether it is a valid scene node.
    pub fn select_target(&mut self, node: &str) -> bool {
        if self.scene_nodes.iter().any(|n| n == node) {
            self.selected_target = Some(node.to_string());
            true
        } else {
            false
        }
    }

    /// The suggested receiver method name (`_on_<source>_<signal>`), used unless
    /// overridden.
    pub fn default_method(&self) -> String {
        format!("_on_{}_{}", self.source_node, self.signal)
    }

    /// The method the connection will use (override if set, else the default).
    pub fn method(&self) -> String {
        self.method_override
            .clone()
            .unwrap_or_else(|| self.default_method())
    }

    /// Overrides the receiver method name.
    pub fn set_method(&mut self, name: impl Into<String>) {
        self.method_override = Some(name.into());
    }

    /// Whether the dialog can be confirmed (a target node is selected).
    pub fn can_confirm(&self) -> bool {
        self.selected_target.is_some()
    }

    /// Confirms the dialog, producing the connection — or `None` if no target
    /// has been selected.
    pub fn confirm(&self) -> Option<Connection> {
        let target = self.selected_target.clone()?;
        Some(Connection {
            signal: self.signal.clone(),
            source: self.source_node.clone(),
            target,
            method: self.method(),
        })
    }
}

/// Stores persisted connections, queryable by signal.
#[derive(Debug, Clone, Default)]
pub struct ConnectionStore {
    connections: Vec<Connection>,
}

impl ConnectionStore {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Persists a connection.
    pub fn add(&mut self, connection: Connection) {
        self.connections.push(connection);
    }

    /// All connections for a given signal (as shown nested under it).
    pub fn connections_for(&self, signal: &str) -> Vec<&Connection> {
        self.connections
            .iter()
            .filter(|c| c.signal == signal)
            .collect()
    }

    /// All persisted connections.
    pub fn all(&self) -> &[Connection] {
        &self.connections
    }

    /// The number of persisted connections.
    pub fn len(&self) -> usize {
        self.connections.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-xw1xw): connecting a signal to a target node+method via
    /// the dialog creates a persisted connection shown under the signal.
    #[test]
    fn signals_connect_dialog_creates_connection() {
        let mut dlg = ConnectDialog::new("Button", "pressed", &["Main", "HUD", "Player"]);
        assert_eq!(dlg.targets(), ["Main", "HUD", "Player"]);

        // Can't confirm until a target is chosen.
        assert!(!dlg.can_confirm());
        assert!(dlg.confirm().is_none());

        // Only scene nodes are valid targets.
        assert!(!dlg.select_target("Ghost"));
        assert!(dlg.select_target("HUD"));
        assert_eq!(dlg.selected_target(), Some("HUD"));
        assert!(dlg.can_confirm());

        // A receiver method name is suggested from the emitter + signal.
        assert_eq!(dlg.default_method(), "_on_Button_pressed");

        // Confirming creates the connection with the default method.
        let conn = dlg.confirm().expect("target selected");
        assert_eq!(conn.signal, "pressed");
        assert_eq!(conn.source, "Button");
        assert_eq!(conn.target, "HUD");
        assert_eq!(conn.method, "_on_Button_pressed");

        // Persisting it makes it queryable under the signal.
        let mut store = ConnectionStore::new();
        store.add(conn);
        let under_pressed = store.connections_for("pressed");
        assert_eq!(under_pressed.len(), 1);
        assert_eq!(under_pressed[0].target, "HUD");
        assert!(store.connections_for("released").is_empty());

        // A custom receiver method overrides the suggestion.
        let mut dlg2 = ConnectDialog::new("Area2D", "body_entered", &["World"]);
        assert!(dlg2.select_target("World"));
        dlg2.set_method("_on_player_entered");
        let conn2 = dlg2.confirm().unwrap();
        assert_eq!(conn2.method, "_on_player_entered");
        assert_eq!(conn2.signal, "body_entered");
    }

    /// Acceptance (pat-jwuv1.2): driving the Connect-a-Signal dialog to confirm
    /// creates a live connection — persisted, queryable by signal, and shown
    /// nested under the signal in the Signals dock tree.
    #[test]
    fn editor_signals_connect_dialog_live() {
        use crate::signals_tree::{self, ClassSignals, SignalDef};

        // Open the dialog to connect Button's `pressed` signal.
        let scene = ["Main", "Player", "Hud"];
        let mut dialog = ConnectDialog::new("Button", "pressed", &scene);
        // Nothing is connectable until a target is chosen.
        assert!(!dialog.can_confirm());
        assert_eq!(dialog.confirm(), None);
        let target_names: Vec<&str> = dialog.targets().iter().map(|s| s.as_str()).collect();
        assert_eq!(target_names, ["Main", "Player", "Hud"]);

        // A node not in the scene can't be targeted.
        assert!(!dialog.select_target("Ghost"));
        // Selecting a real target suggests `_on_<source>_<signal>`.
        assert!(dialog.select_target("Main"));
        assert_eq!(dialog.selected_target(), Some("Main"));
        assert_eq!(dialog.method(), "_on_Button_pressed");
        assert!(dialog.can_confirm());

        // Override the receiver method, then confirm -> a Connection.
        dialog.set_method("_on_button_pressed");
        let conn = dialog.confirm().expect("a target is selected");
        assert_eq!(conn.signal, "pressed");
        assert_eq!(conn.source, "Button");
        assert_eq!(conn.target, "Main");
        assert_eq!(conn.method, "_on_button_pressed");

        // Persist it -> live and queryable by signal.
        let mut store = ConnectionStore::new();
        assert!(store.is_empty());
        store.add(conn.clone());
        assert_eq!(store.len(), 1);
        let for_pressed = store.connections_for("pressed");
        assert_eq!(for_pressed.len(), 1);
        assert_eq!(for_pressed[0].target, "Main");
        assert!(store.connections_for("draw").is_empty());

        // Live in the dock: the persisted connection shows nested under its
        // signal in the Signals dock tree.
        let classes = [ClassSignals {
            class: "Button".to_string(),
            signals: vec![
                SignalDef::new("pressed", &[]),
                SignalDef::new("toggled", &["on"]),
            ],
        }];
        let tree_conns: Vec<signals_tree::Connection> = store
            .all()
            .iter()
            .map(|c| signals_tree::Connection {
                signal: c.signal.clone(),
                target: c.target.clone(),
                method: c.method.clone(),
            })
            .collect();
        let tree = signals_tree::build_tree(&classes, &tree_conns);
        let pressed = &tree.groups[0].signals[0];
        assert_eq!(pressed.name, "pressed");
        assert_eq!(pressed.connections.len(), 1);
        assert_eq!(pressed.connections[0].target, "Main");
        assert_eq!(pressed.connections[0].method, "_on_button_pressed");
        // A signal with no connection stays empty.
        assert!(tree.groups[0].signals[1].connections.is_empty());
    }
}
