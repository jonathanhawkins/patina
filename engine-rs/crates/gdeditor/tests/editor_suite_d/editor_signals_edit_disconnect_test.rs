//! Connection flags + edit/disconnect for the signal Connect dialog
//! (pat-jwuv1.3).
//!
//! Godot's ConnectDialog supports advanced options — bound call arguments and
//! the Deferred / One Shot connect flags — and the Connections dock lets you
//! Edit... or Disconnect an existing connection
//! (upstream/godot/editor/scene/connections_dialog.cpp). This exercises the
//! `signal_connect_advanced` model: flags + binds persist onto a built
//! Connection, and a ConnectionList supports edit + disconnect.

use gdeditor::signal_connect_advanced::{AdvancedConnectDialog, ConnectionList};

#[test]
fn editor_signals_edit_disconnect_flags() {
    // Build a connection with Deferred + One Shot flags and bound arguments.
    let mut dlg = AdvancedConnectDialog::new("pressed", "/root/Main", "_on_pressed");
    dlg.set_deferred(true);
    dlg.set_one_shot(true);
    dlg.add_bind("42");
    dlg.add_bind("\"hi\"");
    let conn = dlg.build();
    assert!(
        conn.flags.deferred,
        "deferred flag persists onto the connection"
    );
    assert!(
        conn.flags.one_shot,
        "one-shot flag persists onto the connection"
    );
    assert_eq!(
        conn.binds,
        vec!["42", "\"hi\""],
        "bound args persist in order"
    );

    // Persist it plus a second, plain connection.
    let mut list = ConnectionList::new();
    list.add(conn);
    list.add(AdvancedConnectDialog::new("timeout", "/root/Main", "_on_timeout").build());
    assert_eq!(list.len(), 2);

    // Edit... the first connection: change the method and drop the flags.
    let edited = AdvancedConnectDialog::new("pressed", "/root/Main", "_on_pressed_v2").build();
    assert!(list.edit(0, edited));
    assert_eq!(
        list.all()[0].method,
        "_on_pressed_v2",
        "edit replaces the method"
    );
    assert!(
        !list.all()[0].flags.deferred,
        "edit cleared the deferred flag"
    );
    assert!(
        list.all()[0].binds.is_empty(),
        "edit cleared the bound args"
    );

    // Disconnect the second connection by index.
    assert!(list.disconnect(1));
    assert_eq!(list.len(), 1);
    assert!(!list.disconnect(9), "out-of-range disconnect is a no-op");

    // Disconnect the remaining one by signal/target/method tuple.
    assert_eq!(
        list.disconnect_matching("pressed", "/root/Main", "_on_pressed_v2"),
        1,
        "disconnect_matching removes the tuple"
    );
    assert!(list.is_empty(), "all connections disconnected");
}
