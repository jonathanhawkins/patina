//! pat-0q2kx.124 — the `Time` singleton's full conversion surface, driven from
//! a scene script.
//!
//! Godot ground truth: `doc/classes/Time.xml` — 21 methods converting between
//! Unix time, datetime dictionaries (`year`/`month`/`day`/`weekday`/`hour`/
//! `minute`/`second`, plus `dst` on the system variants) and ISO-8601 strings,
//! honoring a `utc: bool` parameter that **defaults to false** (i.e. local time),
//! plus `get_time_zone_from_system` and `get_offset_string_from_offset_minutes`.
//!
//! Before this bead `call_time_static` implemented four methods; the datetime
//! dict omitted `weekday` and `dst`, the `utc` argument was ignored outright
//! (always UTC — the wrong answer for the default call), and the other
//! seventeen methods raised `UndefinedFunction`.
//!
//! Everything here goes through a `.tscn`-shaped scripted node and the real
//! `Time` dispatch, so it exercises the wiring requirement rather than calling
//! the Rust helpers directly.

use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// Reference instant: Unix 1_720_000_000 is 2024-07-03T09:46:40 UTC, a
/// Wednesday (Godot's `Weekday` counts Sunday as 0, so 3).
///
/// ⚠️ The bead text quotes `2024-07-03T10:26:40` for this timestamp. That is
/// off by 2400 seconds — `10:26:40` is Unix 1_720_002_400. The value asserted
/// here is the arithmetically correct one: 1_720_000_000 = 19907 days ×
/// 86400 + 35200 s, and 35200 s is 09:46:40.
const REF_UNIX: i64 = 1_720_000_000;
const REF_STRING: &str = "2024-07-03T09:46:40";
const REF_WEEKDAY: i64 = 3;

/// Runs `script_src` on a scripted node for one frame and returns the tree so
/// the caller can read the variables the script published onto the node.
fn run_script(script_src: &str) -> (MainLoop, gdscene::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main_id = tree.add_child(root, Node::new("Main", "Node")).unwrap();
    let script = GDScriptNodeInstance::from_source(script_src, main_id).unwrap();
    tree.attach_script(main_id, Box::new(script));

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);
    (ml, main_id)
}

fn prop(ml: &MainLoop, id: gdscene::NodeId, name: &str) -> Variant {
    ml.tree()
        .get_node(id)
        .expect("Main node still in tree")
        .get_property(name)
}

fn int_prop(ml: &MainLoop, id: gdscene::NodeId, name: &str) -> i64 {
    match prop(ml, id, name) {
        Variant::Int(i) => i,
        other => panic!("`{name}` should be an int, got {other:?}"),
    }
}

fn string_prop(ml: &MainLoop, id: gdscene::NodeId, name: &str) -> String {
    match prop(ml, id, name) {
        Variant::String(s) => s,
        other => panic!("`{name}` should be a String, got {other:?}"),
    }
}

/// **The acceptance test.** Every conversion the bead names, asserted through
/// the scene-script dispatch path.
#[test]
fn time_singleton_conversions() {
    // `dict.has(...)` and indexing keep this to constructs the interpreter
    // already supports; each result is published onto the node as a variable.
    let script_src = "\
extends Node
var has_weekday = false
var has_dst = false
var iso = \"\"
var round_trip = 0
var dict_unix = 0
var partial_unix = 0
var empty_unix = -1
var offset_neg = \"\"
var offset_pos = \"\"
var weekday = -1
var zone_bias = 0
var local_unix = 0
var utc_unix = 0
func _process(delta):
    var sys = Time.get_datetime_dict_from_system(true)
    self.has_weekday = sys.has(\"weekday\")
    self.has_dst = sys.has(\"dst\")
    self.iso = Time.get_datetime_string_from_unix_time(1720000000)
    self.round_trip = Time.get_unix_time_from_datetime_string(self.iso)
    var d = Time.get_datetime_dict_from_unix_time(1720000000)
    self.weekday = d[\"weekday\"]
    self.dict_unix = Time.get_unix_time_from_datetime_dict(d)
    var partial = {}
    partial[\"year\"] = 1970
    partial[\"day\"] = 2
    self.partial_unix = Time.get_unix_time_from_datetime_dict(partial)
    self.empty_unix = Time.get_unix_time_from_datetime_dict({})
    self.offset_neg = Time.get_offset_string_from_offset_minutes(-90)
    self.offset_pos = Time.get_offset_string_from_offset_minutes(330)
    var zone = Time.get_time_zone_from_system()
    self.zone_bias = zone[\"bias\"]
    self.local_unix = Time.get_unix_time_from_datetime_string(Time.get_datetime_string_from_system(false))
    self.utc_unix = Time.get_unix_time_from_datetime_string(Time.get_datetime_string_from_system(true))
";

    let (ml, main) = run_script(script_src);

    // `get_datetime_dict_from_system` must carry weekday and dst.
    assert_eq!(
        prop(&ml, main, "has_weekday"),
        Variant::Bool(true),
        "the system datetime dict must contain a `weekday` key",
    );
    assert_eq!(
        prop(&ml, main, "has_dst"),
        Variant::Bool(true),
        "the system datetime dict must contain a `dst` key",
    );

    // Unix -> ISO 8601, and the string round-trips back.
    assert_eq!(
        string_prop(&ml, main, "iso"),
        REF_STRING,
        "unix {REF_UNIX} is {REF_STRING} (19907 days + 35200 s)",
    );
    assert_eq!(
        int_prop(&ml, main, "round_trip"),
        REF_UNIX,
        "get_unix_time_from_datetime_string must invert the string conversion",
    );

    // The dict carries the right weekday and round-trips through unix — the
    // `Time.xml` promise that the output of get_datetime_dict_from_unix_time
    // feeds straight back into get_unix_time_from_datetime_dict.
    assert_eq!(
        int_prop(&ml, main, "weekday"),
        REF_WEEKDAY,
        "2024-07-03 was a Wednesday; Godot's Weekday counts Sunday as 0",
    );
    assert_eq!(
        int_prop(&ml, main, "dict_unix"),
        REF_UNIX,
        "the datetime dict must round-trip back to its own timestamp",
    );

    // Omitted keys default to the epoch's values, extras are ignored, and an
    // empty dictionary yields 0 (Time.xml).
    assert_eq!(
        int_prop(&ml, main, "partial_unix"),
        86_400,
        "{{year: 1970, day: 2}} fills month=1, hour/minute/second=0 -> one day",
    );
    assert_eq!(
        int_prop(&ml, main, "empty_unix"),
        0,
        "an empty datetime dictionary converts to 0",
    );

    // Offset strings are signed and zero-padded.
    assert_eq!(
        string_prop(&ml, main, "offset_neg"),
        "-01:30",
        "get_offset_string_from_offset_minutes(-90)",
    );
    assert_eq!(
        string_prop(&ml, main, "offset_pos"),
        "+05:30",
        "get_offset_string_from_offset_minutes(330)",
    );

    // utc=false vs utc=true differ by exactly the system offset. Asserting the
    // *relationship* rather than "they differ" is deliberate: on a UTC host the
    // bias is legitimately 0 and the two agree, and a test demanding a
    // difference would fail there for no good reason.
    let bias = int_prop(&ml, main, "zone_bias");
    let local = int_prop(&ml, main, "local_unix");
    let utc = int_prop(&ml, main, "utc_unix");
    let delta = local - utc;
    assert!(
        (delta - bias * 60).abs() <= 1,
        "local and UTC system times must differ by the reported zone bias \
         ({bias} min = {} s), but differed by {delta} s",
        bias * 60,
    );
}

/// The `weekday` parameter of `get_datetime_dict_from_datetime_string` is
/// opt-out: Godot omits the key entirely when it is false, because the
/// calculation is comparatively expensive.
#[test]
fn datetime_string_to_dict_honors_the_weekday_flag() {
    let script_src = "\
extends Node
var with_weekday = false
var without_weekday = true
var year = 0
var hour = 0
func _process(delta):
    var a = Time.get_datetime_dict_from_datetime_string(\"2024-07-03T09:46:40\", true)
    self.with_weekday = a.has(\"weekday\")
    self.year = a[\"year\"]
    self.hour = a[\"hour\"]
    var b = Time.get_datetime_dict_from_datetime_string(\"2024-07-03T09:46:40\", false)
    self.without_weekday = b.has(\"weekday\")
";

    let (ml, main) = run_script(script_src);

    assert_eq!(
        prop(&ml, main, "with_weekday"),
        Variant::Bool(true),
        "weekday=true must include the key",
    );
    assert_eq!(
        prop(&ml, main, "without_weekday"),
        Variant::Bool(false),
        "weekday=false must omit the key entirely, not set it to 0",
    );
    assert_eq!(int_prop(&ml, main, "year"), 2024);
    assert_eq!(int_prop(&ml, main, "hour"), 9);
}

/// The date/time slices return only their own keys, and `use_space` swaps the
/// `T` separator for a space.
#[test]
fn date_and_time_slices_and_use_space() {
    let script_src = "\
extends Node
var date_str = \"\"
var time_str = \"\"
var spaced = \"\"
var date_has_hour = true
var time_has_year = true
func _process(delta):
    self.date_str = Time.get_date_string_from_unix_time(1720000000)
    self.time_str = Time.get_time_string_from_unix_time(1720000000)
    self.spaced = Time.get_datetime_string_from_unix_time(1720000000, true)
    var d = Time.get_date_dict_from_unix_time(1720000000)
    self.date_has_hour = d.has(\"hour\")
    var t = Time.get_time_dict_from_unix_time(1720000000)
    self.time_has_year = t.has(\"year\")
";

    let (ml, main) = run_script(script_src);

    assert_eq!(string_prop(&ml, main, "date_str"), "2024-07-03");
    assert_eq!(string_prop(&ml, main, "time_str"), "09:46:40");
    assert_eq!(
        string_prop(&ml, main, "spaced"),
        "2024-07-03 09:46:40",
        "use_space=true separates the date and time with a space",
    );
    assert_eq!(
        prop(&ml, main, "date_has_hour"),
        Variant::Bool(false),
        "a date dict must not carry time keys",
    );
    assert_eq!(
        prop(&ml, main, "time_has_year"),
        Variant::Bool(false),
        "a time dict must not carry date keys",
    );
}
