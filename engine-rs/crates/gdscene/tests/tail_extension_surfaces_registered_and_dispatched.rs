//! pat-s3rrv.229 — the tail extension surfaces are declared in ClassDB.
//!
//! Godot's small extension classes let a script plug into an engine subsystem:
//! `extends Translation` supplies a runtime-computed locale, `extends Logger`
//! intercepts engine output, `extends VideoStream` + `VideoStreamPlayback`
//! implements a custom decoder, and XRInterfaceExtension /
//! GLTFDocumentExtension / EditorVCSInterface round out the tail. Each is
//! defined ENTIRELY by its virtuals — a class registered with none of them
//! exists in name only: `class_get_method_list` reports nothing to override, so
//! neither the editor nor a script author can discover the contract.
//!
//! `Logger` was absent from ClassDB altogether, so `extends Logger` could not
//! resolve a base class. The other six were registered with 0 of their
//! 2/1/13/32/23/23 virtuals.
//!
//! ## Where the table comes from
//!
//! Every row is transcribed from the class's own doc XML — the `<method>`
//! entries whose names begin with `_`, with the parameter count from
//! `<param>` and the const flag from `qualifiers="virtual const"`. Deriving an
//! arity from a name is how a table like this goes quietly wrong, so nothing
//! here is derived.
//!
//! Sources: `upstream/godot/doc/classes/{Translation,Logger,VideoStream,
//! VideoStreamPlayback,XRInterfaceExtension,EditorVCSInterface}.xml` and
//! `upstream/godot/modules/gltf/doc_classes/GLTFDocumentExtension.xml`.

use gdobject::class_db::{class_exists, get_class_info, register_all_classes};

/// `(class, parent, [(virtual, argument count, is const)])`.
type Surface = (
    &'static str,
    &'static str,
    &'static [(&'static str, usize, bool)],
);

const EXPECTED: &[Surface] = &[
    (
        "Translation",
        "Resource",
        &[("_get_message", 2, true), ("_get_plural_message", 4, true)],
    ),
    (
        "Logger",
        "RefCounted",
        &[("_log_error", 8, false), ("_log_message", 2, false)],
    ),
    (
        "VideoStream",
        "Resource",
        &[("_instantiate_playback", 0, false)],
    ),
    (
        "VideoStreamPlayback",
        "Resource",
        &[
            ("_get_channels", 0, true),
            ("_get_length", 0, true),
            ("_get_mix_rate", 0, true),
            ("_get_playback_position", 0, true),
            ("_get_texture", 0, true),
            ("_is_paused", 0, true),
            ("_is_playing", 0, true),
            ("_play", 0, false),
            ("_seek", 1, false),
            ("_set_audio_track", 1, false),
            ("_set_paused", 1, false),
            ("_stop", 0, false),
            ("_update", 1, false),
        ],
    ),
    (
        "XRInterfaceExtension",
        "XRInterface",
        &[
            ("_end_frame", 0, false),
            ("_get_anchor_detection_is_enabled", 0, true),
            ("_get_camera_feed_id", 0, true),
            ("_get_camera_transform", 0, false),
            ("_get_capabilities", 0, true),
            ("_get_color_texture", 0, false),
            ("_get_depth_texture", 0, false),
            ("_get_name", 0, true),
            ("_get_play_area", 0, true),
            ("_get_play_area_mode", 0, true),
            ("_get_projection_for_view", 4, false),
            ("_get_render_target_size", 0, false),
            ("_get_suggested_pose_names", 1, true),
            ("_get_suggested_tracker_names", 0, true),
            ("_get_system_info", 0, true),
            ("_get_tracking_status", 0, true),
            ("_get_transform_for_view", 2, false),
            ("_get_velocity_texture", 0, false),
            ("_get_view_count", 0, false),
            ("_get_vrs_texture", 0, false),
            ("_get_vrs_texture_format", 0, false),
            ("_initialize", 0, false),
            ("_is_initialized", 0, true),
            ("_post_draw_viewport", 2, false),
            ("_pre_draw_viewport", 1, false),
            ("_pre_render", 0, false),
            ("_process", 0, false),
            ("_set_anchor_detection_is_enabled", 1, false),
            ("_set_play_area_mode", 1, true),
            ("_supports_play_area_mode", 1, true),
            ("_trigger_haptic_pulse", 6, false),
            ("_uninitialize", 0, false),
        ],
    ),
    (
        "GLTFDocumentExtension",
        "Resource",
        &[
            ("_convert_scene_node", 3, false),
            ("_export_node", 4, false),
            ("_export_object_model_property", 6, false),
            ("_export_post", 1, false),
            ("_export_post_convert", 2, false),
            ("_export_preflight", 2, false),
            ("_export_preserialize", 1, false),
            ("_generate_scene_node", 3, false),
            ("_get_image_file_extension", 0, false),
            ("_get_saveable_image_formats", 0, false),
            ("_get_supported_extensions", 0, false),
            ("_import_node", 4, false),
            ("_import_object_model_property", 3, false),
            ("_import_post", 2, false),
            ("_import_post_parse", 1, false),
            ("_import_pre_generate", 1, false),
            ("_import_preflight", 2, false),
            ("_parse_image_data", 4, false),
            ("_parse_node_extensions", 3, false),
            ("_parse_texture_json", 3, false),
            ("_save_image_at_path", 5, false),
            ("_serialize_image_to_bytes", 5, false),
            ("_serialize_texture_json", 4, false),
        ],
    ),
    (
        "EditorVCSInterface",
        "Object",
        &[
            ("_checkout_branch", 1, false),
            ("_commit", 1, false),
            ("_create_branch", 1, false),
            ("_create_remote", 2, false),
            ("_discard_file", 1, false),
            ("_fetch", 1, false),
            ("_get_branch_list", 0, false),
            ("_get_current_branch_name", 0, false),
            ("_get_diff", 2, false),
            ("_get_line_diff", 2, false),
            ("_get_modified_files_data", 0, false),
            ("_get_previous_commits", 1, false),
            ("_get_remotes", 0, false),
            ("_get_vcs_name", 0, false),
            ("_initialize", 1, false),
            ("_pull", 1, false),
            ("_push", 2, false),
            ("_remove_branch", 1, false),
            ("_remove_remote", 1, false),
            ("_set_credentials", 5, false),
            ("_shut_down", 0, false),
            ("_stage_file", 1, false),
            ("_unstage_file", 1, false),
        ],
    ),
];

// ===========================================================================
// The acceptance test
// ===========================================================================

/// **The acceptance test.** `class_exists` and the method list answer correctly
/// for all seven classes, with their full virtual lists.
#[test]
fn tail_extension_surfaces_are_registered() {
    register_all_classes();

    // A miscount in the table itself would weaken every assertion below
    // without failing any of them.
    let declared: usize = EXPECTED.iter().map(|(_, _, v)| v.len()).sum();
    assert_eq!(
        declared, 96,
        "the doc XML defines 96 virtuals across these seven classes"
    );

    for (class, parent, virtuals) in EXPECTED {
        assert!(class_exists(class), "{class} must be registered");

        let info = get_class_info(class).expect("registered class has info");
        assert_eq!(
            &info.parent_class, parent,
            "{class} must inherit {parent}, or `extends {class}` cannot resolve its base"
        );

        for (name, argc, is_const) in *virtuals {
            let m = info
                .methods
                .iter()
                .find(|m| m.name == *name)
                .unwrap_or_else(|| panic!("{class} must declare {name}"));
            assert!(m.is_virtual, "{class}.{name} must be marked virtual");
            assert_eq!(
                m.argument_count, *argc,
                "{class}.{name} takes {argc} arguments per its doc XML"
            );
            assert_eq!(
                m.is_const, *is_const,
                "{class}.{name} const-ness must match qualifiers in its doc XML"
            );
        }
    }
}

/// The control for the test above: a virtual is reported because it was
/// declared, not because the lookup says yes to anything.
///
/// Without this, a `find` that matched loosely — or a `has_method` that walked
/// into a permissive base — would satisfy every assertion above.
#[test]
fn an_undeclared_virtual_is_absent() {
    register_all_classes();

    for (class, _, _) in EXPECTED {
        let info = get_class_info(class).expect("registered class has info");
        assert!(
            !info.methods.iter().any(|m| m.name == "_no_such_virtual"),
            "{class} must not report a virtual it never declared"
        );
    }
}

/// A duplicate declaration does not error — the later one silently shadows the
/// earlier — so the only way to catch one is to count.
#[test]
fn no_virtual_is_declared_twice() {
    register_all_classes();

    for (class, _, virtuals) in EXPECTED {
        let info = get_class_info(class).expect("registered class has info");
        for (name, _, _) in *virtuals {
            let hits = info.methods.iter().filter(|m| m.name == *name).count();
            assert_eq!(hits, 1, "{class} declares {name} {hits} times");
        }
    }
}

// ===========================================================================
// Dispatch — a script Translation and a script Logger actually run
// ===========================================================================
//
// These live in `gdscene` rather than `gdscript-interop` because the dispatch
// reaches the script through the `SceneAccess::call_script_method` seam, and
// only `gdscene` implements it. A test in `gdscript-interop` could supply a
// mock, which would prove the mock works while the real path stayed dead.
//
// ⚠️ Each scripted node owns its OWN `Interpreter`, so the registries these
// tests exercise have to be process-wide. A per-interpreter registry passes no
// test here at all: the whole point is that a DIFFERENT script's `tr()` and
// `print()` see what this one registered.

use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Reads a script `var` off the attached instance.
///
/// Script vars and node properties are separate stores, so `get_property`
/// would answer `Nil` here and fail for the wrong reason.
fn script_var(ml: &MainLoop, node: gdscene::node::NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .expect("script attached")
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("script has no var `{name}`"))
        .default_value
}

/// Builds a tree of scripted `Node`s and fires `_ready` in the order given.
///
/// The order is the point: a provider has to register itself before the
/// consumer asks, which is exactly how an autoload beats the scene it serves.
fn scene_with(scripts: &[(&str, &str)]) -> (MainLoop, Vec<gdscene::node::NodeId>) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut ids = Vec::new();
    for (name, src) in scripts {
        let node = tree
            .add_child(root, Node::new(*name, "Node"))
            .expect("child adds");
        let script = GDScriptNodeInstance::from_source(src, node).expect("script parses");
        tree.attach_script(node, Box::new(script));
        ids.push(node);
    }
    let mut ml = MainLoop::new(tree);
    for id in &ids {
        ml.tree_mut().process_script_ready(*id);
    }
    ml.step(1.0 / 60.0);
    (ml, ids)
}

/// A script `extends Translation` that answers only for `es`, and only for one
/// key — so both halves of the acceptance are visible in the same run.
const LOCALISER: &str = "\
extends Translation

func _ready():
    TranslationServer.add_translation(self)

func _get_message(src_message, context):
    if TranslationServer.get_locale() != \"es\":
        return \"\"
    if src_message == \"HELLO\":
        return \"HOLA\"
    return \"\"
";

/// **The acceptance test.** A script `extends Translation`, added to the
/// TranslationServer with its locale selected, changes what `tr()` returns for
/// a DIFFERENT script — while an unmapped key falls through unchanged.
#[test]
fn script_translation_answers_tr_and_unmapped_keys_fall_through() {
    let driver = "\
extends Node

var hello = \"\"
var unmapped = \"\"

func _ready():
    TranslationServer.set_locale(\"es\")
    hello = tr(\"HELLO\")
    unmapped = tr(\"GOODBYE\")
";

    let (ml, ids) = scene_with(&[("Localiser", LOCALISER), ("Driver", driver)]);

    assert_eq!(
        script_var(&ml, ids[1], "hello"),
        Variant::String("HOLA".into()),
        "tr(\"HELLO\") must come from the script Translation, not the catalog"
    );
    // The other half, and the one a "does tr() return something?" assertion
    // would pass on a broken engine: `_get_message` returning "" means "not
    // mine", so the key has to survive unchanged rather than become empty.
    assert_eq!(
        script_var(&ml, ids[1], "unmapped"),
        Variant::String("GOODBYE".into()),
        "an unmapped key must fall through to the catalog unchanged"
    );
}

/// The control. With the locale switched away, the same registered script
/// declines and `tr()` falls back — so the assertion above is measuring the
/// dispatch and not a constant.
#[test]
fn a_declining_script_translation_falls_back_to_the_catalog() {
    let driver = "\
extends Node

var hello = \"\"

func _ready():
    TranslationServer.set_locale(\"fr\")
    hello = tr(\"HELLO\")
";

    let (ml, ids) = scene_with(&[("Localiser", LOCALISER), ("Driver", driver)]);

    assert_eq!(
        script_var(&ml, ids[1], "hello"),
        Variant::String("HELLO".into()),
        "with another locale selected the script declines and the key is unchanged"
    );
}

/// **The acceptance test** for the other half: a script `extends Logger`
/// receives one `_log_message` per `print()` from a scene script.
#[test]
fn script_logger_receives_one_log_message_per_print() {
    let sink = "\
extends Logger

var lines = 0
var last = \"\"

func _ready():
    OS.add_logger(self)

func _log_message(message, error):
    lines += 1
    last = message
";
    let driver = "\
extends Node

func _ready():
    print(\"first\")
    print(\"second\")
";

    let (ml, ids) = scene_with(&[("Sink", sink), ("Driver", driver)]);

    // An exact count, not `>= 1`: one print must produce exactly one
    // `_log_message`, and a fan-out that fires per registered logger per
    // argument — or twice through two code paths — passes a loose assertion.
    assert_eq!(
        script_var(&ml, ids[0], "lines"),
        Variant::Int(2),
        "two print() calls must produce exactly two _log_message calls"
    );
    assert_eq!(
        script_var(&ml, ids[0], "last"),
        Variant::String("second".into()),
        "the logger must receive the printed text, in order"
    );
}

/// A logger that prints from inside `_log_message` must not recurse.
///
/// This is the natural thing to write in a logger, so the guard is not
/// hypothetical — without it this test does not fail, it hangs or overflows.
#[test]
fn a_logger_that_prints_does_not_recurse() {
    let sink = "\
extends Logger

var lines = 0

func _ready():
    OS.add_logger(self)

func _log_message(message, error):
    lines += 1
    print(\"echo\")
";
    let driver = "\
extends Node

func _ready():
    print(\"once\")
";

    let (ml, ids) = scene_with(&[("Sink", sink), ("Driver", driver)]);

    assert_eq!(
        script_var(&ml, ids[0], "lines"),
        Variant::Int(1),
        "the re-entrant print must not come back round as another _log_message"
    );
}

/// Removing the object stops the dispatch, so registration is a real list and
/// not a latch that is on once anything registers.
#[test]
fn removing_a_logger_stops_the_dispatch() {
    let sink = "\
extends Logger

var lines = 0

func _ready():
    OS.add_logger(self)
    OS.remove_logger(self)

func _log_message(message, error):
    lines += 1
";
    let driver = "\
extends Node

func _ready():
    print(\"ignored\")
";

    let (ml, ids) = scene_with(&[("Sink", sink), ("Driver", driver)]);

    assert_eq!(
        script_var(&ml, ids[0], "lines"),
        Variant::Int(0),
        "a removed logger must stop receiving lines"
    );
}
