//! **Edit / disconnect a signal connection** (pat-thvpi).
//!
//! From the signal tree the user can edit an existing connection — change its
//! target, bound arguments, or flags (deferred / one-shot) — or disconnect it,
//! which removes it from the node and from the tree. Connections are addressed
//! by a stable id so editing/disconnecting one doesn't disturb the others.

/// Per-connection flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ConnectionFlags {
    /// Emit the call deferred (at idle) rather than immediately.
    pub deferred: bool,
    /// Disconnect automatically after the first emission.
    pub one_shot: bool,
}

/// A signal connection with its receiver, bound arguments, and flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    /// The connected signal name.
    pub signal: String,
    /// The target node receiving the signal.
    pub target: String,
    /// The receiver method.
    pub method: String,
    /// Extra arguments bound to the call.
    pub binds: Vec<String>,
    /// Connection flags.
    pub flags: ConnectionFlags,
}

/// A stable identifier for a stored connection.
pub type ConnectionId = u64;

/// Stores connections, supporting edit and disconnect by stable id.
#[derive(Debug, Clone, Default)]
pub struct ConnectionStore {
    items: Vec<(ConnectionId, Connection)>,
    next_id: ConnectionId,
}

impl ConnectionStore {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a connection, returning its stable id.
    pub fn add(&mut self, connection: Connection) -> ConnectionId {
        let id = self.next_id;
        self.next_id += 1;
        self.items.push((id, connection));
        id
    }

    /// The number of connections.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The connection with `id`, if present.
    pub fn get(&self, id: ConnectionId) -> Option<&Connection> {
        self.items.iter().find(|(i, _)| *i == id).map(|(_, c)| c)
    }

    /// Edits the connection with `id` via `f`. Returns whether it existed.
    pub fn edit(&mut self, id: ConnectionId, f: impl FnOnce(&mut Connection)) -> bool {
        match self.items.iter_mut().find(|(i, _)| *i == id) {
            Some((_, c)) => {
                f(c);
                true
            }
            None => false,
        }
    }

    /// Convenience: retarget a connection.
    pub fn set_target(&mut self, id: ConnectionId, target: impl Into<String>) -> bool {
        self.edit(id, |c| c.target = target.into())
    }

    /// Convenience: replace a connection's bound arguments.
    pub fn set_binds(&mut self, id: ConnectionId, binds: Vec<String>) -> bool {
        self.edit(id, |c| c.binds = binds)
    }

    /// Convenience: replace a connection's flags.
    pub fn set_flags(&mut self, id: ConnectionId, flags: ConnectionFlags) -> bool {
        self.edit(id, |c| c.flags = flags)
    }

    /// Disconnects (removes) the connection with `id`. Returns whether it
    /// existed.
    pub fn disconnect(&mut self, id: ConnectionId) -> bool {
        let before = self.items.len();
        self.items.retain(|(i, _)| *i != id);
        self.items.len() != before
    }

    /// All connections for a signal (as shown under it in the tree).
    pub fn connections_for(&self, signal: &str) -> Vec<&Connection> {
        self.items
            .iter()
            .filter(|(_, c)| c.signal == signal)
            .map(|(_, c)| c)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(signal: &str, target: &str, method: &str) -> Connection {
        Connection {
            signal: signal.to_string(),
            target: target.to_string(),
            method: method.to_string(),
            binds: Vec::new(),
            flags: ConnectionFlags::default(),
        }
    }

    /// Acceptance (pat-thvpi): editing a connection updates its
    /// target/binds/flags and disconnect removes it from the node and the tree.
    #[test]
    fn signals_disconnect_and_edit_connection() {
        let mut store = ConnectionStore::new();
        let a = store.add(conn("pressed", "HUD", "_on_pressed"));
        let b = store.add(conn("pressed", "Player", "_on_jump"));
        assert_eq!(store.len(), 2);
        assert_eq!(store.connections_for("pressed").len(), 2);

        // Edit connection `a`: change target, binds, and flags.
        assert!(store.edit(a, |c| {
            c.target = "Menu".to_string();
            c.method = "_on_play".to_string();
            c.binds = vec!["1".to_string(), "true".to_string()];
            c.flags.deferred = true;
            c.flags.one_shot = true;
        }));
        let edited = store.get(a).unwrap();
        assert_eq!(edited.target, "Menu");
        assert_eq!(edited.method, "_on_play");
        assert_eq!(edited.binds, vec!["1", "true"]);
        assert!(edited.flags.deferred);
        assert!(edited.flags.one_shot);

        // Convenience setters work too.
        assert!(store.set_target(a, "Title"));
        assert!(store.set_binds(a, vec!["x".to_string()]));
        assert!(store.set_flags(a, ConnectionFlags::default()));
        let again = store.get(a).unwrap();
        assert_eq!(again.target, "Title");
        assert_eq!(again.binds, vec!["x"]);
        assert!(!again.flags.deferred);

        // Disconnect `a`: it's removed from the node and the tree; `b` remains.
        assert!(store.disconnect(a));
        assert_eq!(store.len(), 1);
        assert!(store.get(a).is_none());
        let remaining = store.connections_for("pressed");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].target, "Player");

        // Disconnecting or editing a missing id is a no-op returning false.
        assert!(!store.disconnect(a));
        assert!(!store.edit(a, |_| {}));
        assert!(!store.set_target(a, "Nope"));

        // `b` is still intact and editable.
        assert!(store.set_target(b, "Boss"));
        assert_eq!(store.get(b).unwrap().target, "Boss");
    }

    /// Acceptance (pat-jwuv1.3): from the signals dock, an existing connection
    /// can be edited to toggle its deferred / one-shot flags and bound
    /// arguments, and disconnected — addressed by stable id so siblings are
    /// undisturbed.
    #[test]
    fn editor_signals_edit_disconnect_flags() {
        let mut store = ConnectionStore::new();
        let a = store.add(conn("body_entered", "Player", "_on_body_entered"));
        let b = store.add(conn("body_entered", "Enemy", "_on_body_entered"));
        assert_eq!(store.connections_for("body_entered").len(), 2);

        // Edit `a`: turn on deferred + one-shot and attach bound args.
        assert!(store.set_flags(
            a,
            ConnectionFlags {
                deferred: true,
                one_shot: true,
            }
        ));
        assert!(store.set_binds(a, vec!["42".to_string(), "true".to_string()]));
        let edited = store.get(a).unwrap();
        assert!(edited.flags.deferred, "deferred flag persisted");
        assert!(edited.flags.one_shot, "one-shot flag persisted");
        assert_eq!(edited.binds, vec!["42", "true"], "binds persisted");

        // Editing `a` left `b`'s flags/binds untouched.
        let sibling = store.get(b).unwrap();
        assert!(!sibling.flags.deferred);
        assert!(!sibling.flags.one_shot);
        assert!(sibling.binds.is_empty());

        // Toggling a single flag back off is independent of the other.
        assert!(store.edit(a, |c| c.flags.one_shot = false));
        let again = store.get(a).unwrap();
        assert!(again.flags.deferred, "deferred unchanged");
        assert!(!again.flags.one_shot, "one-shot cleared independently");

        // Disconnect `a`: it leaves the node and the tree; `b` remains.
        assert!(store.disconnect(a));
        assert!(store.get(a).is_none());
        let remaining = store.connections_for("body_entered");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].target, "Enemy");

        // Editing/disconnecting the now-missing id is a no-op.
        assert!(!store.disconnect(a));
        assert!(!store.set_flags(a, ConnectionFlags::default()));
    }
}
