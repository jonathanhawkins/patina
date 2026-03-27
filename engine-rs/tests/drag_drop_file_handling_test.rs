//! pat-idizv: Drag and drop file handling.
//!
//! Validates:
//! 1. DropData variants (None, Files, Text, Index, Dictionary)
//! 2. DropFilter extension matching (case-insensitive, accept-all, directories)
//! 3. HeadlessDrop OS file drop lifecycle via DropContext
//! 4. HeadlessDrop internal drag-and-drop (start, can_drop, accept, cancel)
//! 5. Drop zone hit testing
//! 6. Filter integration with HeadlessDrop
//! 7. Combined OS + internal drag workflow
//! 8. WindowEvent::FilesDropped / DragEntered / DragExited conversion
//! 9. InputEvent::FilesDropped processing in DropContext
//! 10. ClassDB registration of drag-drop methods on Window and Control

use gdplatform::drag_drop::{DropData, DropFilter, DropHandler, HeadlessDrop};
use gdplatform::input::{DropContext, InputEvent};
use gdplatform::window::WindowEvent;

// ── DropData variants ────────────────────────────────────────────────

#[test]
fn drop_data_none_is_empty() {
    let d = DropData::None;
    assert!(d.is_none());
    assert!(d.as_files().is_none());
    assert!(d.as_text().is_none());
    assert!(d.as_index().is_none());
}

#[test]
fn drop_data_files_variant() {
    let d = DropData::Files(vec!["scene.tscn".into(), "script.gd".into()]);
    assert!(!d.is_none());
    let files = d.as_files().unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files[0], "scene.tscn");
    assert_eq!(files[1], "script.gd");
}

#[test]
fn drop_data_text_variant() {
    let d = DropData::Text("dragged label".into());
    assert!(!d.is_none());
    assert_eq!(d.as_text(), Some("dragged label"));
    assert!(d.as_files().is_none());
    assert!(d.as_index().is_none());
}

#[test]
fn drop_data_index_variant() {
    let d = DropData::Index(7);
    assert_eq!(d.as_index(), Some(7));
    assert!(d.as_text().is_none());
}

#[test]
fn drop_data_dictionary_variant() {
    let d = DropData::Dictionary(vec![
        ("type".into(), "node".into()),
        ("path".into(), "res://Player.tscn".into()),
    ]);
    assert!(!d.is_none());
    assert!(d.as_files().is_none());
}

// ── DropFilter ───────────────────────────────────────────────────────

#[test]
fn filter_accept_all_passes_everything() {
    let f = DropFilter::accept_all();
    assert!(f.accepts("anything.txt"));
    assert!(f.accepts("no_ext"));
    assert!(f.accepts("/path/to/dir/"));
    assert!(f.accepts("image.PNG"));
}

#[test]
fn filter_extensions_basic() {
    let f = DropFilter::extensions(&["tscn", "tres", "gd"]);
    assert!(f.accepts("level.tscn"));
    assert!(f.accepts("material.tres"));
    assert!(f.accepts("player.gd"));
    assert!(!f.accepts("readme.md"));
    assert!(!f.accepts("image.png"));
}

#[test]
fn filter_extensions_case_insensitive() {
    let f = DropFilter::extensions(&["png", "jpg"]);
    assert!(f.accepts("photo.PNG"));
    assert!(f.accepts("photo.Jpg"));
    assert!(f.accepts("photo.png"));
    assert!(!f.accepts("photo.bmp"));
}

#[test]
fn filter_directories_disabled_by_default() {
    let f = DropFilter::extensions(&["tscn"]);
    assert!(!f.accepts("/some/dir/"));
}

#[test]
fn filter_directories_enabled() {
    let f = DropFilter::extensions(&["tscn"]).with_directories(true);
    assert!(f.accepts("/some/dir/"));
    assert!(f.accepts("level.tscn"));
}

#[test]
fn filter_paths_batch() {
    let f = DropFilter::extensions(&["gd", "tscn"]);
    let paths: Vec<String> = vec![
        "player.gd".into(),
        "readme.md".into(),
        "level.tscn".into(),
        "icon.png".into(),
        "main.gd".into(),
    ];
    let accepted = f.filter_paths(&paths);
    assert_eq!(accepted, vec!["player.gd", "level.tscn", "main.gd"]);
}

#[test]
fn filter_empty_extensions_accepts_all_files() {
    let f = DropFilter::extensions(&[]).with_directories(false);
    assert!(f.accepts("anything.txt"));
    assert!(f.accepts("script.gd"));
}

// ── HeadlessDrop — OS file drops ─────────────────────────────────────

#[test]
fn headless_starts_idle() {
    let h = HeadlessDrop::new();
    assert!(!h.is_dragging());
    assert!(h.get_drag_data().is_none());
    assert!(!h.drop_context().is_hovering());
    assert!(!h.drop_context().has_pending_files());
    assert_eq!(h.drop_context().drop_count(), 0);
}

#[test]
fn headless_os_file_drop_lifecycle() {
    let mut h = HeadlessDrop::new();

    // Drag enters
    h.drop_context_mut().drag_enter();
    assert!(h.drop_context().is_hovering());

    // Files dropped
    h.drop_context_mut().drop_files(vec![
        "/Users/dev/scene.tscn".into(),
        "/Users/dev/script.gd".into(),
    ]);
    assert!(!h.drop_context().is_hovering());
    assert_eq!(h.drop_context().pending_count(), 2);
    assert_eq!(h.drop_context().drop_count(), 1);

    // Consumer takes files
    let files = h.drop_context_mut().take_pending_files();
    assert_eq!(files.len(), 2);
    assert!(!h.drop_context().has_pending_files());
    assert_eq!(h.drop_context().drop_count(), 1); // count preserved
}

#[test]
fn headless_multiple_os_drops() {
    let mut h = HeadlessDrop::new();
    h.drop_context_mut().drop_files(vec!["a.png".into()]);
    h.drop_context_mut().drop_files(vec!["b.gd".into(), "c.tscn".into()]);
    assert_eq!(h.drop_context().pending_count(), 3);
    assert_eq!(h.drop_context().drop_count(), 2);
}

// ── HeadlessDrop — internal drag ─────────────────────────────────────

#[test]
fn internal_drag_text() {
    let mut h = HeadlessDrop::new();
    h.start_drag(DropData::Text("label text".into()));
    assert!(h.is_dragging());
    assert!(h.can_drop((50.0, 50.0)));

    let data = h.accept_drop((50.0, 50.0));
    assert_eq!(data.as_text(), Some("label text"));
    assert!(!h.is_dragging());
}

#[test]
fn internal_drag_files() {
    let mut h = HeadlessDrop::new();
    h.start_drag(DropData::Files(vec!["res://icon.png".into()]));
    assert!(h.is_dragging());

    let data = h.accept_drop((0.0, 0.0));
    assert_eq!(data.as_files().unwrap(), &["res://icon.png"]);
}

#[test]
fn internal_drag_index() {
    let mut h = HeadlessDrop::new();
    h.start_drag(DropData::Index(3));
    let data = h.accept_drop((0.0, 0.0));
    assert_eq!(data.as_index(), Some(3));
}

#[test]
fn cancel_drag_clears_data() {
    let mut h = HeadlessDrop::new();
    h.start_drag(DropData::Text("cancel me".into()));
    assert!(h.is_dragging());
    h.cancel_drag();
    assert!(!h.is_dragging());
    assert!(h.get_drag_data().is_none());
}

#[test]
fn cannot_drop_when_not_dragging() {
    let h = HeadlessDrop::new();
    assert!(!h.can_drop((0.0, 0.0)));
}

#[test]
fn accept_drop_when_not_dragging_returns_none() {
    let mut h = HeadlessDrop::new();
    let data = h.accept_drop((100.0, 100.0));
    assert!(data.is_none());
}

// ── Drop zone hit testing ────────────────────────────────────────────

#[test]
fn drop_zone_rejects_outside() {
    let mut h = HeadlessDrop::new();
    h.set_drop_zone(100.0, 100.0, 200.0, 200.0);
    h.start_drag(DropData::Text("zoned".into()));

    assert!(!h.can_drop((50.0, 50.0)));   // before zone
    assert!(h.can_drop((200.0, 200.0)));   // inside zone
    assert!(!h.can_drop((400.0, 200.0)));  // past zone
}

#[test]
fn drop_zone_edges_are_inclusive() {
    let mut h = HeadlessDrop::new();
    h.set_drop_zone(10.0, 10.0, 100.0, 100.0);
    h.start_drag(DropData::Index(1));

    assert!(h.can_drop((10.0, 10.0)));     // top-left edge
    assert!(h.can_drop((110.0, 110.0)));   // bottom-right edge
}

#[test]
fn accept_drop_outside_zone_returns_none() {
    let mut h = HeadlessDrop::new();
    h.set_drop_zone(100.0, 100.0, 50.0, 50.0);
    h.start_drag(DropData::Text("outside".into()));

    let data = h.accept_drop((0.0, 0.0));
    assert!(data.is_none());
    // Drag data is preserved — drop was rejected, not accepted.
    assert!(h.is_dragging());
}

#[test]
fn clear_drop_zone_accepts_anywhere() {
    let mut h = HeadlessDrop::new();
    h.set_drop_zone(100.0, 100.0, 10.0, 10.0);
    h.start_drag(DropData::Text("anywhere".into()));
    assert!(!h.can_drop((0.0, 0.0)));

    h.clear_drop_zone();
    assert!(h.can_drop((0.0, 0.0)));
}

// ── Filter integration ───────────────────────────────────────────────

#[test]
fn filter_rejects_wrong_file_extension() {
    let mut h = HeadlessDrop::with_filter(DropFilter::extensions(&["tscn", "tres"]));
    h.start_drag(DropData::Files(vec!["readme.md".into()]));
    assert!(!h.can_drop((0.0, 0.0)));
}

#[test]
fn filter_accepts_correct_extension() {
    let mut h = HeadlessDrop::with_filter(DropFilter::extensions(&["tscn"]));
    h.start_drag(DropData::Files(vec!["level.tscn".into()]));
    assert!(h.can_drop((0.0, 0.0)));
}

#[test]
fn filter_accepts_if_any_file_matches() {
    let mut h = HeadlessDrop::with_filter(DropFilter::extensions(&["gd"]));
    h.start_drag(DropData::Files(vec![
        "readme.md".into(),
        "player.gd".into(),
    ]));
    // At least one file matches → can_drop returns true.
    assert!(h.can_drop((0.0, 0.0)));
}

#[test]
fn filter_does_not_affect_non_file_data() {
    let mut h = HeadlessDrop::with_filter(DropFilter::extensions(&["tscn"]));
    h.start_drag(DropData::Text("not a file".into()));
    assert!(h.can_drop((0.0, 0.0)));

    h.cancel_drag();
    h.start_drag(DropData::Index(5));
    assert!(h.can_drop((0.0, 0.0)));
}

// ── WindowEvent conversion ───────────────────────────────────────────

#[test]
fn window_event_files_dropped_converts_to_input_event() {
    let we = WindowEvent::FilesDropped {
        paths: vec!["/tmp/a.png".into(), "/tmp/b.tscn".into()],
    };
    let ie = we.to_input_event().unwrap();
    match ie {
        InputEvent::FilesDropped { paths } => {
            assert_eq!(paths.len(), 2);
            assert_eq!(paths[0], "/tmp/a.png");
        }
        _ => panic!("expected FilesDropped"),
    }
}

#[test]
fn window_event_drag_entered_no_input_event() {
    let we = WindowEvent::DragEntered;
    // DragEntered is a window-level event, not an input event.
    assert!(we.to_input_event().is_none());
}

#[test]
fn window_event_drag_exited_no_input_event() {
    let we = WindowEvent::DragExited;
    assert!(we.to_input_event().is_none());
}

// ── DropContext + InputEvent integration ──────────────────────────────

#[test]
fn drop_context_processes_files_dropped_event() {
    let mut ctx = DropContext::new();
    let event = InputEvent::FilesDropped {
        paths: vec!["/home/user/image.png".into()],
    };
    assert!(ctx.process_event(&event));
    assert_eq!(ctx.pending_count(), 1);
    assert_eq!(ctx.drop_count(), 1);
}

#[test]
fn drop_context_ignores_non_drop_events() {
    let mut ctx = DropContext::new();
    let event = InputEvent::MouseMotion {
        position: gdcore::math::Vector2::new(100.0, 200.0),
        relative: gdcore::math::Vector2::new(1.0, 0.0),
    };
    assert!(!ctx.process_event(&event));
    assert_eq!(ctx.drop_count(), 0);
}

// ── Full combined workflow ───────────────────────────────────────────

#[test]
fn full_os_then_internal_workflow() {
    let mut h = HeadlessDrop::new();

    // Phase 1: OS file drop from desktop
    h.drop_context_mut().drag_enter();
    assert!(h.drop_context().is_hovering());
    h.drop_context_mut().drop_files(vec![
        "/Users/dev/project/level.tscn".into(),
    ]);
    assert!(!h.drop_context().is_hovering());
    let os_files = h.drop_context_mut().take_pending_files();
    assert_eq!(os_files, vec!["/Users/dev/project/level.tscn"]);

    // Phase 2: Internal UI drag (e.g., scene tree node reorder)
    h.start_drag(DropData::Index(2));
    assert!(h.is_dragging());
    assert!(h.can_drop((50.0, 80.0)));
    let data = h.accept_drop((50.0, 80.0));
    assert_eq!(data.as_index(), Some(2));
    assert!(!h.is_dragging());

    // Phase 3: Another OS drop
    h.drop_context_mut().drop_files(vec!["script.gd".into()]);
    assert_eq!(h.drop_context().drop_count(), 2);
}

#[test]
fn repeated_drag_accept_cycle() {
    let mut h = HeadlessDrop::new();
    for i in 0..5 {
        h.start_drag(DropData::Index(i));
        let data = h.accept_drop((0.0, 0.0));
        assert_eq!(data.as_index(), Some(i));
        assert!(!h.is_dragging());
    }
}

// ── ClassDB registration ─────────────────────────────────────────────

#[test]
fn classdb_window_has_drag_drop_methods() {
    gdobject::class_db::register_3d_classes();
    assert!(gdobject::class_db::class_has_method(
        "Window",
        "get_files_dropped"
    ));
    assert!(gdobject::class_db::class_has_method(
        "Window",
        "is_drag_hovering"
    ));
}

#[test]
fn classdb_control_has_drag_drop_methods() {
    gdobject::class_db::register_3d_classes();
    assert!(gdobject::class_db::class_has_method(
        "Control",
        "_can_drop_data"
    ));
    assert!(gdobject::class_db::class_has_method(
        "Control",
        "_drop_data"
    ));
    assert!(gdobject::class_db::class_has_method(
        "Control",
        "_get_drag_data"
    ));
    assert!(gdobject::class_db::class_has_method(
        "Control",
        "force_drag"
    ));
}
