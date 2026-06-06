//! **Signals dock tree** (pat-h52gt).
//!
//! The Signals tab shows the selected node's signals as a tree: signals are
//! grouped under the class that declares them (in the node's inheritance order),
//! each signal shows its `name(params)` signature, and any existing connections
//! are nested as children under their signal.

/// A signal declaration: a name and its parameter names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalDef {
    /// Signal name.
    pub name: String,
    /// Parameter names, in order.
    pub params: Vec<String>,
}

impl SignalDef {
    /// Convenience constructor.
    pub fn new(name: impl Into<String>, params: &[&str]) -> Self {
        Self {
            name: name.into(),
            params: params.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// The `name(p1, p2)` signature string.
    pub fn signature(&self) -> String {
        format!("{}({})", self.name, self.params.join(", "))
    }
}

/// The signals a single class in the inheritance chain declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassSignals {
    /// The declaring class name.
    pub class: String,
    /// The signals it declares.
    pub signals: Vec<SignalDef>,
}

/// An existing connection of a signal to a receiver method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    /// The signal that is connected.
    pub signal: String,
    /// The receiver (target node/script).
    pub target: String,
    /// The method the signal is connected to.
    pub method: String,
}

/// A connection node nested under a signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionNode {
    /// The receiver.
    pub target: String,
    /// The connected method.
    pub method: String,
}

/// A signal node in the tree: its name, signature, and connections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalNode {
    /// Signal name.
    pub name: String,
    /// `name(params)` signature.
    pub signature: String,
    /// Existing connections nested under this signal.
    pub connections: Vec<ConnectionNode>,
}

/// A class group node holding its signals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassNode {
    /// The declaring class.
    pub class: String,
    /// Its signals.
    pub signals: Vec<SignalNode>,
}

/// The full signals tree for a node.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SignalTree {
    /// Class groups, in inheritance order.
    pub groups: Vec<ClassNode>,
}

impl SignalTree {
    /// Total number of signals across all groups.
    pub fn signal_count(&self) -> usize {
        self.groups.iter().map(|g| g.signals.len()).sum()
    }

    /// Returns a copy of the tree narrowed to signals whose name contains
    /// `query` (case-insensitive substring). An empty query returns the full
    /// tree unchanged; class groups left with no matching signals are dropped.
    pub fn filtered(&self, query: &str) -> SignalTree {
        if query.is_empty() {
            return self.clone();
        }
        let needle = query.to_lowercase();
        let groups = self
            .groups
            .iter()
            .filter_map(|g| {
                let signals: Vec<SignalNode> = g
                    .signals
                    .iter()
                    .filter(|s| s.name.to_lowercase().contains(&needle))
                    .cloned()
                    .collect();
                if signals.is_empty() {
                    None
                } else {
                    Some(ClassNode {
                        class: g.class.clone(),
                        signals,
                    })
                }
            })
            .collect();
        SignalTree { groups }
    }
}

/// Builds the signals tree from the node's per-class signals (in inheritance
/// order) and its existing connections. Each signal nests the connections whose
/// `signal` matches its name.
pub fn build_tree(classes: &[ClassSignals], connections: &[Connection]) -> SignalTree {
    let groups = classes
        .iter()
        .map(|cs| ClassNode {
            class: cs.class.clone(),
            signals: cs
                .signals
                .iter()
                .map(|sig| SignalNode {
                    name: sig.name.clone(),
                    signature: sig.signature(),
                    connections: connections
                        .iter()
                        .filter(|c| c.signal == sig.name)
                        .map(|c| ConnectionNode {
                            target: c.target.clone(),
                            method: c.method.clone(),
                        })
                        .collect(),
                })
                .collect(),
        })
        .collect();
    SignalTree { groups }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-h52gt): selecting a node lists its signals grouped by
    /// class with signatures, and signals with connections show them as
    /// children.
    #[test]
    fn signals_dock_tree_lists_signals_and_connections() {
        // The node's inheritance chain (derived first), each with its signals.
        let classes = vec![
            ClassSignals {
                class: "Button".to_string(),
                signals: vec![
                    SignalDef::new("pressed", &[]),
                    SignalDef::new("toggled", &["button_pressed"]),
                ],
            },
            ClassSignals {
                class: "BaseButton".to_string(),
                signals: vec![SignalDef::new("button_down", &[])],
            },
        ];
        let connections = vec![
            Connection {
                signal: "pressed".to_string(),
                target: "res://ui/menu.gd".to_string(),
                method: "_on_button_pressed".to_string(),
            },
            Connection {
                signal: "pressed".to_string(),
                target: "res://ui/hud.gd".to_string(),
                method: "_on_play".to_string(),
            },
        ];

        let tree = build_tree(&classes, &connections);

        // Grouped by declaring class, in inheritance order.
        assert_eq!(tree.groups.len(), 2);
        assert_eq!(tree.groups[0].class, "Button");
        assert_eq!(tree.groups[1].class, "BaseButton");
        assert_eq!(tree.signal_count(), 3);

        // `pressed` shows its signature and both connections as children.
        let pressed = &tree.groups[0].signals[0];
        assert_eq!(pressed.name, "pressed");
        assert_eq!(pressed.signature, "pressed()");
        assert_eq!(pressed.connections.len(), 2);
        assert_eq!(pressed.connections[0].method, "_on_button_pressed");
        assert_eq!(pressed.connections[1].target, "res://ui/hud.gd");

        // `toggled` shows its parameter in the signature and has no connections.
        let toggled = &tree.groups[0].signals[1];
        assert_eq!(toggled.signature, "toggled(button_pressed)");
        assert!(toggled.connections.is_empty());

        // The base class group lists its own signal.
        let button_down = &tree.groups[1].signals[0];
        assert_eq!(button_down.name, "button_down");
        assert_eq!(button_down.signature, "button_down()");
        assert!(button_down.connections.is_empty());
    }

    /// Multi-parameter signatures are comma-separated; an empty node has an
    /// empty tree.
    #[test]
    fn signatures_and_empty() {
        assert_eq!(
            SignalDef::new("hit", &["damage", "source"]).signature(),
            "hit(damage, source)"
        );
        let empty = build_tree(&[], &[]);
        assert_eq!(empty.signal_count(), 0);
        assert!(empty.groups.is_empty());
    }

    /// Acceptance (pat-nv674): typing in the filter narrows the signal tree to
    /// name matches, and clearing restores the full tree.
    #[test]
    fn signals_filter_search_narrows_tree() {
        let classes = vec![
            ClassSignals {
                class: "Button".to_string(),
                signals: vec![
                    SignalDef::new("pressed", &[]),
                    SignalDef::new("toggled", &["button_pressed"]),
                ],
            },
            ClassSignals {
                class: "BaseButton".to_string(),
                signals: vec![
                    SignalDef::new("button_down", &[]),
                    SignalDef::new("button_up", &[]),
                ],
            },
        ];
        let tree = build_tree(&classes, &[]);
        assert_eq!(tree.signal_count(), 4);

        // Typing narrows the tree to matching signals (substring on name).
        let filtered = tree.filtered("button");
        assert_eq!(filtered.signal_count(), 2); // button_down, button_up
                                                 // The non-matching class group is dropped entirely.
        assert_eq!(filtered.groups.len(), 1);
        assert_eq!(filtered.groups[0].class, "BaseButton");

        // Matching is case-insensitive.
        assert_eq!(tree.filtered("PRESSED").signal_count(), 1); // pressed

        // A query matching nothing yields an empty tree.
        assert_eq!(tree.filtered("zzz").signal_count(), 0);
        assert!(tree.filtered("zzz").groups.is_empty());

        // Clearing the filter restores the full tree.
        let restored = tree.filtered("");
        assert_eq!(restored, tree);
        assert_eq!(restored.signal_count(), 4);
    }

    /// Acceptance (pat-jwuv1.1): the Signals dock builds a tree of the node's
    /// signals grouped under the class that declares them, in inheritance order
    /// (most-derived first), each signal showing its `name(params)` signature
    /// with existing connections nested beneath; grouping survives a search.
    #[test]
    fn editor_signals_dock_tree() {
        // A Button node's signals, per declaring class in inheritance order.
        let classes = vec![
            ClassSignals {
                class: "Button".into(),
                signals: vec![
                    SignalDef::new("pressed", &[]),
                    SignalDef::new("toggled", &["toggled_on"]),
                ],
            },
            ClassSignals {
                class: "BaseButton".into(),
                signals: vec![
                    SignalDef::new("button_down", &[]),
                    SignalDef::new("button_up", &[]),
                ],
            },
            ClassSignals {
                class: "CanvasItem".into(),
                signals: vec![
                    SignalDef::new("visibility_changed", &[]),
                    SignalDef::new("draw", &[]),
                ],
            },
            ClassSignals {
                class: "Node".into(),
                signals: vec![
                    SignalDef::new("ready", &[]),
                    SignalDef::new("renamed", &[]),
                    SignalDef::new("tree_entered", &[]),
                ],
            },
        ];
        let connections = vec![Connection {
            signal: "pressed".into(),
            target: "Main".into(),
            method: "_on_button_pressed".into(),
        }];

        let tree = build_tree(&classes, &connections);

        // One group per declaring class, in inheritance order.
        assert_eq!(
            tree.groups
                .iter()
                .map(|g| g.class.as_str())
                .collect::<Vec<_>>(),
            ["Button", "BaseButton", "CanvasItem", "Node"]
        );
        // Each group holds exactly its own class's signals (no leakage).
        assert_eq!(
            tree.groups[0]
                .signals
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            ["pressed", "toggled"]
        );
        assert_eq!(
            tree.groups[3]
                .signals
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            ["ready", "renamed", "tree_entered"]
        );
        // Signatures render as name(params).
        assert_eq!(tree.groups[0].signals[0].signature, "pressed()");
        assert_eq!(tree.groups[0].signals[1].signature, "toggled(toggled_on)");
        // Total count sums across groups.
        assert_eq!(tree.signal_count(), 2 + 2 + 2 + 3);

        // The existing connection nests under its signal (and only there).
        let pressed = &tree.groups[0].signals[0];
        assert_eq!(pressed.connections.len(), 1);
        assert_eq!(pressed.connections[0].target, "Main");
        assert_eq!(pressed.connections[0].method, "_on_button_pressed");
        assert!(tree.groups[3].signals[0].connections.is_empty());

        // Searching narrows to the matching class group, preserving grouping.
        let filtered = tree.filtered("button");
        assert_eq!(
            filtered
                .groups
                .iter()
                .map(|g| g.class.as_str())
                .collect::<Vec<_>>(),
            ["BaseButton"]
        );
        assert_eq!(filtered.signal_count(), 2);
    }
}
