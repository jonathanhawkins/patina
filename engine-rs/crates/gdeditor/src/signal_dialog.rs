//! Signal connection dialog with method picker.
//!
//! Provides a dialog model for connecting signals between nodes in the editor.
//! Mirrors Godot's signal connection dialog which allows the user to:
//!
//! - Select a signal from the source node
//! - Pick a target node from the scene tree
//! - Choose or create a method on the target node
//! - Configure connection flags (deferred, one-shot)
//!
//! The [`SignalConnectionDialog`] is a headless model — it tracks dialog state
//! and validates connections without rendering UI.

use gdscene::node::NodeId;
use gdscene::SceneTree;

// ---------------------------------------------------------------------------
// ConnectionFlags
// ---------------------------------------------------------------------------

/// Flags that modify how a signal connection behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ConnectionFlags {
    /// Call the method on the next idle frame (deferred).
    pub deferred: bool,
    /// Automatically disconnect after the first emission.
    pub one_shot: bool,
}

impl ConnectionFlags {
    /// Returns the Godot-compatible integer flag value.
    pub fn to_godot_flags(self) -> u32 {
        let mut flags = 0u32;
        if self.deferred {
            flags |= 1; // CONNECT_DEFERRED
        }
        if self.one_shot {
            flags |= 4; // CONNECT_ONE_SHOT
        }
        flags
    }

    /// Parses flags from a Godot integer value.
    pub fn from_godot_flags(flags: u32) -> Self {
        Self {
            deferred: flags & 1 != 0,
            one_shot: flags & 4 != 0,
        }
    }
}

// ---------------------------------------------------------------------------
// MethodEntry — a method available for connection
// ---------------------------------------------------------------------------

/// A method that can be the target of a signal connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodEntry {
    /// Method name.
    pub name: String,
    /// Whether this is a user-defined method (vs. built-in).
    pub is_user_defined: bool,
    /// Number of parameters the method accepts.
    pub param_count: usize,
}

impl MethodEntry {
    /// Creates a built-in method entry.
    pub fn builtin(name: impl Into<String>, param_count: usize) -> Self {
        Self {
            name: name.into(),
            is_user_defined: false,
            param_count,
        }
    }

    /// Creates a user-defined method entry.
    pub fn user_defined(name: impl Into<String>, param_count: usize) -> Self {
        Self {
            name: name.into(),
            is_user_defined: true,
            param_count,
        }
    }
}

// ---------------------------------------------------------------------------
// SignalEntry — a signal available on a node
// ---------------------------------------------------------------------------

/// A signal that can be connected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalEntry {
    /// Signal name (e.g. "pressed", "body_entered").
    pub name: String,
    /// Number of arguments the signal emits.
    pub arg_count: usize,
}

impl SignalEntry {
    pub fn new(name: impl Into<String>, arg_count: usize) -> Self {
        Self {
            name: name.into(),
            arg_count,
        }
    }
}

// ---------------------------------------------------------------------------
// ConnectionResult — validated connection ready to apply
// ---------------------------------------------------------------------------

/// A validated signal connection ready to be applied to the scene tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionResult {
    /// Source node emitting the signal.
    pub source_node: NodeId,
    /// Signal name on the source node.
    pub signal_name: String,
    /// Target node receiving the callback.
    pub target_node: NodeId,
    /// Method name on the target node.
    pub method_name: String,
    /// Connection flags.
    pub flags: ConnectionFlags,
}

// ---------------------------------------------------------------------------
// ValidationError
// ---------------------------------------------------------------------------

/// Errors from validating a signal connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// No source node selected.
    NoSourceNode,
    /// No signal selected.
    NoSignalSelected,
    /// No target node selected.
    NoTargetNode,
    /// No method selected or entered.
    NoMethod,
    /// The source node doesn't exist in the tree.
    SourceNodeNotFound,
    /// The target node doesn't exist in the tree.
    TargetNodeNotFound,
    /// The selected signal doesn't exist on the source node.
    SignalNotFound(String),
    /// A connection with the same signal+target+method already exists.
    DuplicateConnection,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSourceNode => write!(f, "no source node selected"),
            Self::NoSignalSelected => write!(f, "no signal selected"),
            Self::NoTargetNode => write!(f, "no target node selected"),
            Self::NoMethod => write!(f, "no method selected"),
            Self::SourceNodeNotFound => write!(f, "source node not found in tree"),
            Self::TargetNodeNotFound => write!(f, "target node not found in tree"),
            Self::SignalNotFound(s) => write!(f, "signal '{}' not found on source node", s),
            Self::DuplicateConnection => write!(f, "connection already exists"),
        }
    }
}

// ---------------------------------------------------------------------------
// SignalConnectionDialog
// ---------------------------------------------------------------------------

/// The signal connection dialog state machine.
///
/// Models the editor dialog for connecting a signal from one node to a
/// method on another node. Tracks selections, provides method suggestions,
/// validates the connection, and produces a [`ConnectionResult`].
#[derive(Debug, Clone)]
pub struct SignalConnectionDialog {
    /// The source node whose signal will be connected.
    source_node: Option<NodeId>,
    /// Available signals on the source node.
    available_signals: Vec<SignalEntry>,
    /// The currently selected signal index.
    selected_signal: Option<usize>,
    /// The target node that will receive the callback.
    target_node: Option<NodeId>,
    /// Available methods on the target node.
    available_methods: Vec<MethodEntry>,
    /// The selected method name (may be typed manually).
    selected_method: Option<String>,
    /// Connection flags.
    flags: ConnectionFlags,
    /// Whether to auto-generate a method name from the signal.
    auto_name: bool,
}

impl Default for SignalConnectionDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalConnectionDialog {
    /// Creates a new empty dialog.
    pub fn new() -> Self {
        Self {
            source_node: None,
            available_signals: Vec::new(),
            selected_signal: None,
            target_node: None,
            available_methods: Vec::new(),
            selected_method: None,
            flags: ConnectionFlags::default(),
            auto_name: true,
        }
    }

    /// Opens the dialog for a specific source node and signal.
    pub fn open_for_signal(
        source_node: NodeId,
        signals: Vec<SignalEntry>,
        signal_index: usize,
    ) -> Self {
        Self {
            source_node: Some(source_node),
            available_signals: signals,
            selected_signal: Some(signal_index),
            target_node: None,
            available_methods: Vec::new(),
            selected_method: None,
            flags: ConnectionFlags::default(),
            auto_name: true,
        }
    }

    // -- Source node & signals ------------------------------------------------

    /// Sets the source node and populates available signals.
    pub fn set_source_node(&mut self, node_id: NodeId, signals: Vec<SignalEntry>) {
        self.source_node = Some(node_id);
        self.available_signals = signals;
        self.selected_signal = None;
        // Reset auto-generated method name.
        if self.auto_name {
            self.selected_method = None;
        }
    }

    /// Returns the source node, if set.
    pub fn source_node(&self) -> Option<NodeId> {
        self.source_node
    }

    /// Returns the available signals.
    pub fn available_signals(&self) -> &[SignalEntry] {
        &self.available_signals
    }

    /// Selects a signal by index.
    pub fn select_signal(&mut self, index: usize) {
        if index < self.available_signals.len() {
            self.selected_signal = Some(index);
            // Auto-generate method name.
            if self.auto_name {
                let signal_name = &self.available_signals[index].name;
                self.selected_method = Some(format!("_on_{}", signal_name));
            }
        }
    }

    /// Returns the currently selected signal.
    pub fn selected_signal(&self) -> Option<&SignalEntry> {
        self.selected_signal
            .and_then(|i| self.available_signals.get(i))
    }

    /// Returns the selected signal name.
    pub fn selected_signal_name(&self) -> Option<&str> {
        self.selected_signal().map(|s| s.name.as_str())
    }

    // -- Target node & methods -----------------------------------------------

    /// Sets the target node and populates available methods.
    pub fn set_target_node(&mut self, node_id: NodeId, methods: Vec<MethodEntry>) {
        self.target_node = Some(node_id);
        self.available_methods = methods;
    }

    /// Returns the target node, if set.
    pub fn target_node(&self) -> Option<NodeId> {
        self.target_node
    }

    /// Returns the available methods on the target node.
    pub fn available_methods(&self) -> &[MethodEntry] {
        &self.available_methods
    }

    /// Filters available methods to those compatible with the selected signal's
    /// argument count.
    pub fn compatible_methods(&self) -> Vec<&MethodEntry> {
        let arg_count = self
            .selected_signal()
            .map(|s| s.arg_count)
            .unwrap_or(0);

        self.available_methods
            .iter()
            .filter(|m| m.param_count >= arg_count || m.param_count == 0)
            .collect()
    }

    /// Selects a method by name.
    pub fn select_method(&mut self, name: impl Into<String>) {
        self.selected_method = Some(name.into());
        self.auto_name = false;
    }

    /// Returns the selected method name.
    pub fn selected_method(&self) -> Option<&str> {
        self.selected_method.as_deref()
    }

    /// Sets auto-naming mode. When enabled, selecting a signal auto-generates
    /// a method name like `_on_<signal_name>`.
    pub fn set_auto_name(&mut self, enabled: bool) {
        self.auto_name = enabled;
    }

    /// Returns the auto-generated method name for the given signal.
    pub fn auto_method_name(signal_name: &str) -> String {
        format!("_on_{}", signal_name)
    }

    // -- Flags ---------------------------------------------------------------

    /// Returns the current connection flags.
    pub fn flags(&self) -> ConnectionFlags {
        self.flags
    }

    /// Sets the deferred flag.
    pub fn set_deferred(&mut self, deferred: bool) {
        self.flags.deferred = deferred;
    }

    /// Sets the one-shot flag.
    pub fn set_one_shot(&mut self, one_shot: bool) {
        self.flags.one_shot = one_shot;
    }

    // -- Validation & result -------------------------------------------------

    /// Validates the current dialog state and returns a connection result
    /// or a list of validation errors.
    pub fn validate(&self, tree: &SceneTree) -> Result<ConnectionResult, Vec<ValidationError>> {
        let mut errors = Vec::new();

        let source = match self.source_node {
            Some(id) => id,
            None => {
                errors.push(ValidationError::NoSourceNode);
                return Err(errors);
            }
        };

        let signal_name = match self.selected_signal_name() {
            Some(name) => name.to_string(),
            None => {
                errors.push(ValidationError::NoSignalSelected);
                return Err(errors);
            }
        };

        let target = match self.target_node {
            Some(id) => id,
            None => {
                errors.push(ValidationError::NoTargetNode);
                return Err(errors);
            }
        };

        let method_name = match &self.selected_method {
            Some(name) if !name.is_empty() => name.clone(),
            _ => {
                errors.push(ValidationError::NoMethod);
                return Err(errors);
            }
        };

        // Verify nodes exist.
        if tree.get_node(source).is_none() {
            errors.push(ValidationError::SourceNodeNotFound);
        }
        if tree.get_node(target).is_none() {
            errors.push(ValidationError::TargetNodeNotFound);
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(ConnectionResult {
            source_node: source,
            signal_name,
            target_node: target,
            method_name,
            flags: self.flags,
        })
    }

    /// Resets the dialog to its initial state.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Returns `true` if all required fields are filled.
    pub fn is_ready(&self) -> bool {
        self.source_node.is_some()
            && self.selected_signal.is_some()
            && self.target_node.is_some()
            && self.selected_method.as_ref().map_or(false, |m| !m.is_empty())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use gdscene::node::Node;

    fn make_tree() -> (SceneTree, NodeId, NodeId) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let button = Node::new("Button", "Button");
        let player = Node::new("Player", "CharacterBody2D");
        let btn_id = tree.add_child(root, button).unwrap();
        let player_id = tree.add_child(root, player).unwrap();
        (tree, btn_id, player_id)
    }

    fn sample_signals() -> Vec<SignalEntry> {
        vec![
            SignalEntry::new("pressed", 0),
            SignalEntry::new("toggled", 1),
            SignalEntry::new("button_down", 0),
        ]
    }

    fn sample_methods() -> Vec<MethodEntry> {
        vec![
            MethodEntry::builtin("_ready", 0),
            MethodEntry::builtin("_process", 1),
            MethodEntry::user_defined("_on_pressed", 0),
            MethodEntry::user_defined("handle_toggle", 1),
        ]
    }

    // -- ConnectionFlags -----------------------------------------------------

    #[test]
    fn flags_default() {
        let flags = ConnectionFlags::default();
        assert!(!flags.deferred);
        assert!(!flags.one_shot);
        assert_eq!(flags.to_godot_flags(), 0);
    }

    #[test]
    fn flags_deferred() {
        let flags = ConnectionFlags {
            deferred: true,
            one_shot: false,
        };
        assert_eq!(flags.to_godot_flags(), 1);
    }

    #[test]
    fn flags_one_shot() {
        let flags = ConnectionFlags {
            deferred: false,
            one_shot: true,
        };
        assert_eq!(flags.to_godot_flags(), 4);
    }

    #[test]
    fn flags_both() {
        let flags = ConnectionFlags {
            deferred: true,
            one_shot: true,
        };
        assert_eq!(flags.to_godot_flags(), 5);
    }

    #[test]
    fn flags_from_godot() {
        let flags = ConnectionFlags::from_godot_flags(5);
        assert!(flags.deferred);
        assert!(flags.one_shot);

        let flags = ConnectionFlags::from_godot_flags(0);
        assert!(!flags.deferred);
        assert!(!flags.one_shot);
    }

    // -- MethodEntry ---------------------------------------------------------

    #[test]
    fn method_entry_builtin() {
        let m = MethodEntry::builtin("_ready", 0);
        assert_eq!(m.name, "_ready");
        assert!(!m.is_user_defined);
        assert_eq!(m.param_count, 0);
    }

    #[test]
    fn method_entry_user_defined() {
        let m = MethodEntry::user_defined("on_hit", 2);
        assert!(m.is_user_defined);
        assert_eq!(m.param_count, 2);
    }

    // -- SignalEntry ---------------------------------------------------------

    #[test]
    fn signal_entry() {
        let s = SignalEntry::new("pressed", 0);
        assert_eq!(s.name, "pressed");
        assert_eq!(s.arg_count, 0);
    }

    // -- Dialog lifecycle ----------------------------------------------------

    #[test]
    fn dialog_starts_empty() {
        let dialog = SignalConnectionDialog::new();
        assert!(dialog.source_node().is_none());
        assert!(dialog.target_node().is_none());
        assert!(dialog.selected_signal().is_none());
        assert!(dialog.selected_method().is_none());
        assert!(!dialog.is_ready());
    }

    #[test]
    fn dialog_set_source_and_select_signal() {
        let (_, btn_id, _) = make_tree();
        let mut dialog = SignalConnectionDialog::new();
        dialog.set_source_node(btn_id, sample_signals());

        assert_eq!(dialog.source_node(), Some(btn_id));
        assert_eq!(dialog.available_signals().len(), 3);

        dialog.select_signal(0);
        assert_eq!(dialog.selected_signal_name(), Some("pressed"));
    }

    #[test]
    fn dialog_auto_method_name() {
        let (_, btn_id, _) = make_tree();
        let mut dialog = SignalConnectionDialog::new();
        dialog.set_source_node(btn_id, sample_signals());

        dialog.select_signal(0);
        assert_eq!(dialog.selected_method(), Some("_on_pressed"));

        dialog.select_signal(1);
        assert_eq!(dialog.selected_method(), Some("_on_toggled"));
    }

    #[test]
    fn dialog_manual_method_disables_auto_name() {
        let (_, btn_id, _) = make_tree();
        let mut dialog = SignalConnectionDialog::new();
        dialog.set_source_node(btn_id, sample_signals());
        dialog.select_signal(0);

        dialog.select_method("custom_handler");
        assert_eq!(dialog.selected_method(), Some("custom_handler"));

        // Selecting another signal should NOT overwrite manual name.
        dialog.select_signal(1);
        assert_eq!(dialog.selected_method(), Some("custom_handler"));
    }

    #[test]
    fn dialog_set_target_and_methods() {
        let (_, _, player_id) = make_tree();
        let mut dialog = SignalConnectionDialog::new();
        dialog.set_target_node(player_id, sample_methods());

        assert_eq!(dialog.target_node(), Some(player_id));
        assert_eq!(dialog.available_methods().len(), 4);
    }

    #[test]
    fn dialog_compatible_methods() {
        let (_, btn_id, player_id) = make_tree();
        let mut dialog = SignalConnectionDialog::new();
        dialog.set_source_node(btn_id, sample_signals());
        dialog.select_signal(1); // toggled has 1 arg
        dialog.set_target_node(player_id, sample_methods());

        let compatible = dialog.compatible_methods();
        // _ready(0) is compatible (0 params accepts anything),
        // _process(1) compatible, handle_toggle(1) compatible,
        // _on_pressed(0) compatible (0 params)
        assert!(compatible.len() >= 3);
    }

    #[test]
    fn dialog_flags() {
        let mut dialog = SignalConnectionDialog::new();
        assert!(!dialog.flags().deferred);
        assert!(!dialog.flags().one_shot);

        dialog.set_deferred(true);
        assert!(dialog.flags().deferred);

        dialog.set_one_shot(true);
        assert!(dialog.flags().one_shot);
    }

    #[test]
    fn dialog_is_ready() {
        let (_, btn_id, player_id) = make_tree();
        let mut dialog = SignalConnectionDialog::new();

        assert!(!dialog.is_ready());

        dialog.set_source_node(btn_id, sample_signals());
        assert!(!dialog.is_ready());

        dialog.select_signal(0);
        assert!(!dialog.is_ready());

        dialog.set_target_node(player_id, sample_methods());
        // auto-name was set when selecting signal
        assert!(dialog.is_ready());
    }

    #[test]
    fn dialog_open_for_signal() {
        let (_, btn_id, _) = make_tree();
        let dialog = SignalConnectionDialog::open_for_signal(btn_id, sample_signals(), 0);
        assert_eq!(dialog.source_node(), Some(btn_id));
        assert_eq!(dialog.selected_signal_name(), Some("pressed"));
    }

    // -- Validation ----------------------------------------------------------

    #[test]
    fn validate_empty_dialog() {
        let tree = SceneTree::new();
        let dialog = SignalConnectionDialog::new();
        let result = dialog.validate(&tree);
        assert!(result.is_err());
    }

    #[test]
    fn validate_no_target() {
        let (tree, btn_id, _) = make_tree();
        let mut dialog = SignalConnectionDialog::new();
        dialog.set_source_node(btn_id, sample_signals());
        dialog.select_signal(0);

        let result = dialog.validate(&tree);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains(&ValidationError::NoTargetNode));
    }

    #[test]
    fn validate_success() {
        let (tree, btn_id, player_id) = make_tree();
        let mut dialog = SignalConnectionDialog::new();
        dialog.set_source_node(btn_id, sample_signals());
        dialog.select_signal(0);
        dialog.set_target_node(player_id, sample_methods());

        let result = dialog.validate(&tree);
        assert!(result.is_ok());

        let conn = result.unwrap();
        assert_eq!(conn.source_node, btn_id);
        assert_eq!(conn.signal_name, "pressed");
        assert_eq!(conn.target_node, player_id);
        assert_eq!(conn.method_name, "_on_pressed");
        assert_eq!(conn.flags.to_godot_flags(), 0);
    }

    #[test]
    fn validate_with_flags() {
        let (tree, btn_id, player_id) = make_tree();
        let mut dialog = SignalConnectionDialog::new();
        dialog.set_source_node(btn_id, sample_signals());
        dialog.select_signal(0);
        dialog.set_target_node(player_id, sample_methods());
        dialog.set_deferred(true);
        dialog.set_one_shot(true);

        let conn = dialog.validate(&tree).unwrap();
        assert!(conn.flags.deferred);
        assert!(conn.flags.one_shot);
        assert_eq!(conn.flags.to_godot_flags(), 5);
    }

    #[test]
    fn validate_missing_source_node() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let player = Node::new("Player", "Node2D");
        let player_id = tree.add_child(root, player).unwrap();

        let fake_id = NodeId::next();
        let mut dialog = SignalConnectionDialog::new();
        dialog.set_source_node(fake_id, sample_signals());
        dialog.select_signal(0);
        dialog.set_target_node(player_id, sample_methods());

        let result = dialog.validate(&tree);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains(&ValidationError::SourceNodeNotFound));
    }

    #[test]
    fn validate_no_method() {
        let (tree, btn_id, player_id) = make_tree();
        let mut dialog = SignalConnectionDialog::new();
        dialog.set_source_node(btn_id, sample_signals());
        dialog.select_signal(0);
        dialog.set_target_node(player_id, sample_methods());
        dialog.selected_method = None; // clear auto-name
        dialog.auto_name = false;

        let result = dialog.validate(&tree);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains(&ValidationError::NoMethod));
    }

    // -- Reset ---------------------------------------------------------------

    #[test]
    fn dialog_reset() {
        let (_, btn_id, player_id) = make_tree();
        let mut dialog = SignalConnectionDialog::new();
        dialog.set_source_node(btn_id, sample_signals());
        dialog.select_signal(0);
        dialog.set_target_node(player_id, sample_methods());

        assert!(dialog.is_ready());
        dialog.reset();
        assert!(!dialog.is_ready());
        assert!(dialog.source_node().is_none());
    }

    // -- Auto method name helper ---------------------------------------------

    #[test]
    fn auto_method_name_generation() {
        assert_eq!(
            SignalConnectionDialog::auto_method_name("pressed"),
            "_on_pressed"
        );
        assert_eq!(
            SignalConnectionDialog::auto_method_name("body_entered"),
            "_on_body_entered"
        );
    }

    // -- ValidationError display ---------------------------------------------

    #[test]
    fn validation_error_display() {
        assert_eq!(
            ValidationError::NoSourceNode.to_string(),
            "no source node selected"
        );
        assert_eq!(
            ValidationError::SignalNotFound("foo".into()).to_string(),
            "signal 'foo' not found on source node"
        );
        assert_eq!(
            ValidationError::DuplicateConnection.to_string(),
            "connection already exists"
        );
    }

    // -- ConnectionResult fields ---------------------------------------------

    #[test]
    fn connection_result_fields() {
        let (tree, btn_id, player_id) = make_tree();
        let mut dialog = SignalConnectionDialog::new();
        dialog.set_source_node(btn_id, sample_signals());
        dialog.select_signal(2); // button_down
        dialog.set_target_node(player_id, sample_methods());
        dialog.select_method("my_handler");

        let conn = dialog.validate(&tree).unwrap();
        assert_eq!(conn.signal_name, "button_down");
        assert_eq!(conn.method_name, "my_handler");
    }

    #[test]
    fn select_signal_out_of_bounds() {
        let (_, btn_id, _) = make_tree();
        let mut dialog = SignalConnectionDialog::new();
        dialog.set_source_node(btn_id, sample_signals());
        dialog.select_signal(99); // out of bounds
        assert!(dialog.selected_signal().is_none());
    }
}
