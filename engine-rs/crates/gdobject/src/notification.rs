//! Notification dispatch and handling.
//!
//! Godot uses integer notification codes to communicate lifecycle and
//! system events to objects. This module defines the standard constants
//! and a dispatch mechanism that walks the inheritance chain.

use std::fmt;

/// A notification code, matching Godot's integer-tagged notification system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Notification(i32);

impl Notification {
    /// Creates a notification from a raw integer code.
    pub const fn new(code: i32) -> Self {
        Self(code)
    }

    /// Returns the raw integer code.
    pub const fn code(self) -> i32 {
        self.0
    }
}

impl fmt::Display for Notification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self.0 {
            0 => "POSTINITIALIZE",
            1 => "PREDELETE",
            10 => "ENTER_TREE",
            11 => "EXIT_TREE",
            12 => "MOVED_IN_PARENT",
            13 => "READY",
            14 => "PAUSED",
            15 => "UNPAUSED",
            16 => "PHYSICS_PROCESS",
            17 => "PROCESS",
            18 => "PARENTED",
            19 => "UNPARENTED",
            20 => "INSTANCED",
            21 => "DRAG_BEGIN",
            22 => "DRAG_END",
            23 => "PATH_RENAMED",
            24 => "CHILD_ORDER_CHANGED",
            25 => "INTERNAL_PROCESS",
            26 => "INTERNAL_PHYSICS_PROCESS",
            27 => "POST_ENTER_TREE",
            30 => "DRAW",
            _ => return write!(f, "Notification({})", self.0),
        };
        write!(f, "NOTIFICATION_{name}")
    }
}

// ── Standard notification constants ─────────────────────────────────
//
// ⚠️ The VALUES are Godot's (`scene/main/node.h`, the `NOTIFICATION_*` enum).
// Several used to be invented — PHYSICS_PROCESS was 18, PARENTED 20,
// CHILD_ORDER_CHANGED 16, INTERNAL_PROCESS 35 — which is invisible while every
// comparison goes through these constants and wrong the moment a script, a
// probe or a ported test compares the number Godot documents.

/// Sent after the object has been fully initialized.
pub const NOTIFICATION_POSTINITIALIZE: Notification = Notification::new(0);

/// Sent before the object is freed.
pub const NOTIFICATION_PREDELETE: Notification = Notification::new(1);

/// Node has entered the scene tree.
pub const NOTIFICATION_ENTER_TREE: Notification = Notification::new(10);

/// Node has exited the scene tree.
pub const NOTIFICATION_EXIT_TREE: Notification = Notification::new(11);

/// Node has been moved to a different parent.
pub const NOTIFICATION_MOVED_IN_PARENT: Notification = Notification::new(12);

/// Node and all children are ready.
pub const NOTIFICATION_READY: Notification = Notification::new(13);

/// Node has been paused.
pub const NOTIFICATION_PAUSED: Notification = Notification::new(14);

/// Node has been unpaused.
pub const NOTIFICATION_UNPAUSED: Notification = Notification::new(15);

/// The order of children within the parent has changed.
pub const NOTIFICATION_CHILD_ORDER_CHANGED: Notification = Notification::new(24);

/// Called every frame during `_process`.
pub const NOTIFICATION_PROCESS: Notification = Notification::new(17);

/// Called every physics tick during `_physics_process`.
pub const NOTIFICATION_PHYSICS_PROCESS: Notification = Notification::new(16);

/// Node has gained a parent.
pub const NOTIFICATION_PARENTED: Notification = Notification::new(18);

/// Node has lost its parent.
pub const NOTIFICATION_UNPARENTED: Notification = Notification::new(19);

/// Node has been instanced from a packed scene.
pub const NOTIFICATION_INSTANCED: Notification = Notification::new(20);

/// Drag operation started.
pub const NOTIFICATION_DRAG_BEGIN: Notification = Notification::new(21);

/// Drag operation ended.
pub const NOTIFICATION_DRAG_END: Notification = Notification::new(22);

/// The node's path (or an ancestor's) changed — Godot `NOTIFICATION_PATH_RENAMED`.
pub const NOTIFICATION_PATH_RENAMED: Notification = Notification::new(23);

/// Sent on EVERY tree entry, after `_ready` has had its chance — Godot
/// `NOTIFICATION_POST_ENTER_TREE` (`node.cpp` `_propagate_ready`). Unlike
/// `NOTIFICATION_READY` it repeats on a re-entry (a reparent).
pub const NOTIFICATION_POST_ENTER_TREE: Notification = Notification::new(27);

/// Request to draw (2D).
pub const NOTIFICATION_DRAW: Notification = Notification::new(30);

/// Called every frame for internal engine processing (before user `_process`).
pub const NOTIFICATION_INTERNAL_PROCESS: Notification = Notification::new(25);

/// Called every physics tick for internal engine processing (before user `_physics_process`).
pub const NOTIFICATION_INTERNAL_PHYSICS_PROCESS: Notification = Notification::new(26);

/// Trait for types that can receive and handle notifications.
///
/// Implementations should dispatch to the appropriate handler based on
/// the notification code. The `class_name` method allows the dispatch
/// system to log which class handled the notification.
pub trait NotificationHandler {
    /// Handle an incoming notification.
    fn handle_notification(&mut self, what: Notification);

    /// Returns the class name for logging and debugging.
    fn handler_class_name(&self) -> &str;
}

/// A record of a dispatched notification, useful for testing and tracing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationRecord {
    /// The class that handled this notification.
    pub class_name: String,
    /// The notification code.
    pub notification: Notification,
}

/// Dispatches a notification through a chain of class handlers.
///
/// In Godot, notifications walk the inheritance chain from the most-derived
/// class to the base class. This function simulates that by calling each
/// handler in order.
pub fn dispatch_notification_chain(
    handlers: &mut [&mut dyn NotificationHandler],
    what: Notification,
) -> Vec<NotificationRecord> {
    let mut records = Vec::new();
    for handler in handlers.iter_mut() {
        let class = handler.handler_class_name().to_owned();
        handler.handle_notification(what);
        records.push(NotificationRecord {
            class_name: class,
            notification: what,
        });
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_constants_match_godot() {
        assert_eq!(NOTIFICATION_ENTER_TREE.code(), 10);
        assert_eq!(NOTIFICATION_EXIT_TREE.code(), 11);
        assert_eq!(NOTIFICATION_READY.code(), 13);
        assert_eq!(NOTIFICATION_PROCESS.code(), 17);
        // `scene/main/node.h`. PHYSICS_PROCESS was pinned here as 18 — the
        // test named "match_godot" asserted a value Godot never used.
        assert_eq!(NOTIFICATION_PHYSICS_PROCESS.code(), 16);
        assert_eq!(NOTIFICATION_MOVED_IN_PARENT.code(), 12);
        assert_eq!(NOTIFICATION_PAUSED.code(), 14);
        assert_eq!(NOTIFICATION_UNPAUSED.code(), 15);
        assert_eq!(NOTIFICATION_PARENTED.code(), 18);
        assert_eq!(NOTIFICATION_UNPARENTED.code(), 19);
        assert_eq!(NOTIFICATION_INSTANCED.code(), 20);
        assert_eq!(NOTIFICATION_DRAG_BEGIN.code(), 21);
        assert_eq!(NOTIFICATION_DRAG_END.code(), 22);
        assert_eq!(NOTIFICATION_PATH_RENAMED.code(), 23);
        assert_eq!(NOTIFICATION_CHILD_ORDER_CHANGED.code(), 24);
        assert_eq!(NOTIFICATION_INTERNAL_PROCESS.code(), 25);
        assert_eq!(NOTIFICATION_INTERNAL_PHYSICS_PROCESS.code(), 26);
        assert_eq!(NOTIFICATION_POST_ENTER_TREE.code(), 27);
        assert_eq!(NOTIFICATION_DRAW.code(), 30);
        assert_eq!(NOTIFICATION_POSTINITIALIZE.code(), 0);
        assert_eq!(NOTIFICATION_PREDELETE.code(), 1);
    }

    #[test]
    fn notification_display() {
        assert_eq!(format!("{}", NOTIFICATION_READY), "NOTIFICATION_READY");
        assert_eq!(format!("{}", Notification::new(999)), "Notification(999)");
    }

    struct MockHandler {
        class: &'static str,
        received: Vec<Notification>,
    }

    impl NotificationHandler for MockHandler {
        fn handle_notification(&mut self, what: Notification) {
            self.received.push(what);
        }

        fn handler_class_name(&self) -> &str {
            self.class
        }
    }

    #[test]
    fn dispatch_chain_order() {
        let mut derived = MockHandler {
            class: "Player",
            received: vec![],
        };
        let mut base = MockHandler {
            class: "Node2D",
            received: vec![],
        };

        let records =
            dispatch_notification_chain(&mut [&mut derived, &mut base], NOTIFICATION_READY);

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].class_name, "Player");
        assert_eq!(records[1].class_name, "Node2D");
        assert_eq!(derived.received, vec![NOTIFICATION_READY]);
        assert_eq!(base.received, vec![NOTIFICATION_READY]);
    }

    #[test]
    fn dispatch_empty_chain() {
        let records = dispatch_notification_chain(&mut [], NOTIFICATION_READY);
        assert!(records.is_empty());
    }

    #[test]
    fn dispatch_unknown_notification_code() {
        let mut handler = MockHandler {
            class: "Test",
            received: vec![],
        };

        let unknown = Notification::new(9999);
        let records = dispatch_notification_chain(&mut [&mut handler], unknown);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].notification, unknown);
        assert_eq!(handler.received, vec![unknown]);
    }

    #[test]
    fn notification_equality() {
        assert_eq!(NOTIFICATION_READY, NOTIFICATION_READY);
        assert_ne!(NOTIFICATION_READY, NOTIFICATION_PROCESS);
    }

    #[test]
    fn notification_hash_consistent() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(NOTIFICATION_READY);
        assert!(set.contains(&NOTIFICATION_READY));
        assert!(!set.contains(&NOTIFICATION_PROCESS));
    }

    #[test]
    fn notification_new_and_code_roundtrip() {
        let n = Notification::new(42);
        assert_eq!(n.code(), 42);
    }

    #[test]
    fn notification_display_all_known() {
        assert_eq!(
            format!("{}", NOTIFICATION_POSTINITIALIZE),
            "NOTIFICATION_POSTINITIALIZE"
        );
        assert_eq!(
            format!("{}", NOTIFICATION_PREDELETE),
            "NOTIFICATION_PREDELETE"
        );
        assert_eq!(
            format!("{}", NOTIFICATION_ENTER_TREE),
            "NOTIFICATION_ENTER_TREE"
        );
        assert_eq!(
            format!("{}", NOTIFICATION_EXIT_TREE),
            "NOTIFICATION_EXIT_TREE"
        );
        assert_eq!(format!("{}", NOTIFICATION_PAUSED), "NOTIFICATION_PAUSED");
        assert_eq!(
            format!("{}", NOTIFICATION_UNPAUSED),
            "NOTIFICATION_UNPAUSED"
        );
        assert_eq!(
            format!("{}", NOTIFICATION_PARENTED),
            "NOTIFICATION_PARENTED"
        );
        assert_eq!(
            format!("{}", NOTIFICATION_UNPARENTED),
            "NOTIFICATION_UNPARENTED"
        );
        assert_eq!(
            format!("{}", NOTIFICATION_INSTANCED),
            "NOTIFICATION_INSTANCED"
        );
        assert_eq!(format!("{}", NOTIFICATION_DRAW), "NOTIFICATION_DRAW");
        assert_eq!(
            format!("{}", NOTIFICATION_CHILD_ORDER_CHANGED),
            "NOTIFICATION_CHILD_ORDER_CHANGED"
        );
    }

    #[test]
    fn notification_record_clone_and_eq() {
        let record = NotificationRecord {
            class_name: "Player".into(),
            notification: NOTIFICATION_READY,
        };
        let cloned = record.clone();
        assert_eq!(record, cloned);
    }
}
