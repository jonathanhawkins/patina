//! pat-nzgdu.5 — parametrized `@export` annotations carry hint metadata into
//! `list_properties`.
//!
//! Before this bead a script with any parenthesized annotation
//! (`@export_range(0,100,5)`, `@export_enum(...)`, `@rpc(...)`) either failed to
//! parse or, once parsing was added, dropped the argument list on the floor so
//! the inspector had no hint. This test drives the public
//! `GDScriptInstance` API end to end: the script must instantiate, and
//! `list_properties()` must report the exported vars with the correct Godot
//! `PROPERTY_HINT_*` code and hint string.

use gdscript_interop::interpreter::{GDScriptInstance, PROPERTY_HINT_ENUM, PROPERTY_HINT_RANGE};
use gdscript_interop::{ScriptInstance, ScriptPropertyInfo};

// A script exercising the whole annotation surface the bead calls out:
// a range export, an enum export, a standalone group marker, and an @rpc func.
const SCRIPT: &str = "\
@export_range(0, 100, 5) var speed: float = 10.0\n\
@export_enum(\"Idle\", \"Run\") var state: int = 0\n\
@export_group(\"Movement\")\n\
\n\
@rpc(\"any_peer\")\n\
func hit():\n\
\tpass\n\
";

#[test]
fn gdscript_export_annotations_wired() {
    // Parses + instantiates without error (parametrized annotations + group +
    // @rpc func all accepted).
    let instance = GDScriptInstance::from_source("Player", SCRIPT)
        .expect("script with parametrized @export annotations should parse + instantiate");

    let props: Vec<ScriptPropertyInfo> = instance.list_properties();
    let find = |n: &str| props.iter().find(|p| p.name == n);

    // @export_range(0,100,5) var speed -> PROPERTY_HINT_RANGE + "0,100,5".
    let speed = find("speed").expect("exported 'speed' should be listed");
    assert_eq!(
        speed.hint, PROPERTY_HINT_RANGE,
        "speed should carry a range hint, got {}",
        speed.hint
    );
    assert_eq!(
        speed.hint_string, "0,100,5",
        "range hint_string should be 'min,max,step'"
    );

    // @export_enum("Idle","Run") var state -> PROPERTY_HINT_ENUM + "Idle,Run".
    let state = find("state").expect("exported 'state' should be listed");
    assert_eq!(
        state.hint, PROPERTY_HINT_ENUM,
        "state should carry an enum hint, got {}",
        state.hint
    );
    assert_eq!(
        state.hint_string, "Idle,Run",
        "enum hint_string should be the comma-joined choices"
    );
}
