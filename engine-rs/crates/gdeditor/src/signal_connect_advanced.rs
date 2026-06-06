//! Connect dialog **advanced options** (pat-ktpjc).
//!
//! The Connect-a-Signal dialog's advanced section lets the user attach extra
//! bound arguments to the call and set the connect flags (deferred / one-shot).
//! These settings are persisted on the resulting connection and remain visible
//! when the connection is later edited.

/// The deferred / one-shot connection flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ConnectionFlags {
    /// Emit deferred (at idle) instead of immediately.
    pub deferred: bool,
    /// Auto-disconnect after the first emission.
    pub one_shot: bool,
}

/// A connection carrying its advanced options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    /// The connected signal.
    pub signal: String,
    /// The target node.
    pub target: String,
    /// The receiver method.
    pub method: String,
    /// Extra arguments bound to the call, in order.
    pub binds: Vec<String>,
    /// The connect flags.
    pub flags: ConnectionFlags,
}

/// The advanced-options state of the Connect dialog.
#[derive(Debug, Clone)]
pub struct AdvancedConnectDialog {
    signal: String,
    target: String,
    method: String,
    binds: Vec<String>,
    flags: ConnectionFlags,
}

impl AdvancedConnectDialog {
    /// Opens advanced options for connecting `signal` to `target`.`method`.
    pub fn new(
        signal: impl Into<String>,
        target: impl Into<String>,
        method: impl Into<String>,
    ) -> Self {
        Self {
            signal: signal.into(),
            target: target.into(),
            method: method.into(),
            binds: Vec::new(),
            flags: ConnectionFlags::default(),
        }
    }

    /// Appends a bound argument.
    pub fn add_bind(&mut self, value: impl Into<String>) {
        self.binds.push(value.into());
    }

    /// Removes the bound argument at `index`. Returns whether one was removed.
    pub fn remove_bind(&mut self, index: usize) -> bool {
        if index < self.binds.len() {
            self.binds.remove(index);
            true
        } else {
            false
        }
    }

    /// The current bound arguments.
    pub fn binds(&self) -> &[String] {
        &self.binds
    }

    /// Sets the deferred flag.
    pub fn set_deferred(&mut self, on: bool) {
        self.flags.deferred = on;
    }

    /// Sets the one-shot flag.
    pub fn set_one_shot(&mut self, on: bool) {
        self.flags.one_shot = on;
    }

    /// The current flags.
    pub fn flags(&self) -> ConnectionFlags {
        self.flags
    }

    /// Builds the connection, persisting the binds and flags onto it.
    pub fn build(&self) -> Connection {
        Connection {
            signal: self.signal.clone(),
            target: self.target.clone(),
            method: self.method.clone(),
            binds: self.binds.clone(),
            flags: self.flags,
        }
    }
}

/// A simple persistence list for connections.
#[derive(Debug, Clone, Default)]
pub struct ConnectionList {
    connections: Vec<Connection>,
}

impl ConnectionList {
    /// Creates an empty list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Persists a connection.
    pub fn add(&mut self, connection: Connection) {
        self.connections.push(connection);
    }

    /// All persisted connections.
    pub fn all(&self) -> &[Connection] {
        &self.connections
    }

    /// The number of persisted connections.
    pub fn len(&self) -> usize {
        self.connections.len()
    }

    /// Whether the list is empty.
    pub fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-ktpjc): a connection made with bound args and
    /// deferred/one-shot flags persists those settings and they are visible when
    /// editing the connection.
    #[test]
    fn signals_connection_flags_and_binds_persist() {
        let mut dlg = AdvancedConnectDialog::new("body_entered", "Player", "_on_body_entered");

        // Add two bound arguments and enable both flags.
        dlg.add_bind("42");
        dlg.add_bind("\"hello\"");
        dlg.set_deferred(true);
        dlg.set_one_shot(true);
        assert_eq!(dlg.binds(), ["42", "\"hello\""]);
        assert_eq!(
            dlg.flags(),
            ConnectionFlags {
                deferred: true,
                one_shot: true
            }
        );

        // Building persists the binds and flags on the connection.
        let conn = dlg.build();
        assert_eq!(conn.binds, vec!["42", "\"hello\""]);
        assert!(conn.flags.deferred);
        assert!(conn.flags.one_shot);

        // Persisting and reading it back (as when re-opening to edit) keeps them.
        let mut list = ConnectionList::new();
        list.add(conn);
        let stored = &list.all()[0];
        assert_eq!(stored.binds, vec!["42", "\"hello\""]);
        assert!(stored.flags.deferred);
        assert!(stored.flags.one_shot);

        // A plain connection has no binds and flags off by default.
        let plain = AdvancedConnectDialog::new("pressed", "HUD", "_on_pressed").build();
        assert!(plain.binds.is_empty());
        assert!(!plain.flags.deferred);
        assert!(!plain.flags.one_shot);
    }

    /// Binds can be removed before building.
    #[test]
    fn binds_can_be_removed() {
        let mut dlg = AdvancedConnectDialog::new("s", "t", "m");
        dlg.add_bind("a");
        dlg.add_bind("b");
        dlg.add_bind("c");
        assert!(dlg.remove_bind(1)); // drop "b"
        assert_eq!(dlg.binds(), ["a", "c"]);
        assert!(!dlg.remove_bind(9)); // out of range
        assert_eq!(dlg.build().binds, vec!["a", "c"]);
    }
}
