#!/usr/bin/env python3
"""Generate gdeditor's per-class editor property schema table from the captured
Godot 4.6.1 inspector goldens (pat-ghfv0.x fill phase).

Reads every `engine-rs/fixtures/editor_oracle/inspector_*.json` golden and emits
`engine-rs/crates/gdeditor/src/inspector_schema_generated.rs`, a data table of
`class -> ordered [EditorProp{name,type_name,hint,default,true_default}]` where
`default` is the value Godot's inspector SHOWS (taken from an `is_default=true`
golden entry, else the unanimous shown value, else the Godot type-zero) and
`true_default` is Godot's real ClassDB default used to decide `is_default` — the
two differ for project-settings-/context-derived and always-overridden props
(see `true_defaults.json` from the ClassDB probe / `true_default_expr`;
pat-ghfv0.41 + pat-ghfv0.42).

This is the single source of truth for the seeded schema: it transcribes the
real editor property list, order, hints, and defaults straight from Godot rather
than hand-authoring ~30 classes.

Run from repo root:  python3 tools/oracle/gen_inspector_schema.py

True ClassDB defaults for props without an `is_default=true` golden sample come
from `fixtures/editor_oracle/true_defaults.json` (produced by
`tools/oracle/editor_true_defaults_probe.gd` via ClassDB.class_get_property_default_value).
There is no hand-curated KNOWN_TRUE_DEFAULTS table after pat-ghfv0.42.
"""
import glob
import json
import os
import re

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ORACLE_DIR = os.path.join(REPO, "engine-rs/fixtures/editor_oracle")
TRUE_DEFAULTS_PATH = os.path.join(ORACLE_DIR, "true_defaults.json")
OUT = os.path.join(REPO, "engine-rs/crates/gdeditor/src/inspector_schema_generated.rs")
DOC_DIR = os.path.join(REPO, "upstream/godot/doc/classes")

# Classes the editor must show a real schema for that no captured scene happens
# to contain (pat-0q2kx.128). The goldens only cover what the oracle scenes used,
# so Line2D / Button / GPUParticles2D / … resolved to `None` and the inspector
# fell back to a stored-only, near-empty list for freshly created nodes.
#
# For these the schema is synthesized from the vendored `doc/classes/*.xml`,
# which carries both membership and Godot's documented defaults, walked
# base-first up the `inherits` chain the same way a golden's flattened list
# reads. A captured golden always wins over synthesis.
SYNTHESIZE_FROM_DOCS = [
    # 2D nodes
    "Line2D",
    "Polygon2D",
    "AnimatedSprite2D",
    "GPUParticles2D",
    "CPUParticles2D",
    "PointLight2D",
    "DirectionalLight2D",
    "LightOccluder2D",
    "TouchScreenButton",
    "RemoteTransform2D",
    # 3D nodes
    "GPUParticles3D",
    "CPUParticles3D",
    "Label3D",
    "Decal",
    "SpotLight3D",
    "ReflectionProbe",
    # GUI
    "Button",
    "LineEdit",
    "TextEdit",
    "RichTextLabel",
    "OptionButton",
    "CheckBox",
    "CheckButton",
    "TextureRect",
    "TextureButton",
    "ProgressBar",
    "HSlider",
    "VSlider",
    "ItemList",
    "Tree",
    "TabBar",
    "ScrollContainer",
    # Resources
    "StandardMaterial3D",
    "BoxMesh",
    "SphereMesh",
    "CylinderMesh",
    "PlaneMesh",
    "Gradient",
    "Curve",
]

# Doc XML defaults that name a constructor with no arguments (`PackedVector2Array()`,
# `Callable()`, …) carry no value — treat them as the type's empty form.
_EMPTY_CTOR = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*\(\s*\)$")


def _doc_path(cls):
    return os.path.join(DOC_DIR, f"{cls}.xml")


def doc_class(cls, _cache={}):
    """Parse `doc/classes/<cls>.xml` into (inherits, [(name, type, default)]).

    Returns `None` when the class has no vendored doc page. `default` is the raw
    attribute string, or `None` when the doc declares no default (Godot shows
    those as unset — a resource slot, typically).
    """
    if cls in _cache:
        return _cache[cls]
    path = _doc_path(cls)
    if not os.path.exists(path):
        _cache[cls] = None
        return None
    import xml.etree.ElementTree as ET

    try:
        root = ET.parse(path).getroot()
    except ET.ParseError:
        _cache[cls] = None
        return None
    inherits = root.get("inherits")
    members = []
    for m in root.findall("./members/member"):
        name = m.get("name")
        if not name:
            continue
        members.append((name, m.get("type", ""), m.get("default")))
    result = (inherits, members)
    _cache[cls] = result
    return result


def doc_default_value(type_name, raw):
    """Convert a doc XML `default="…"` string into the shape `rust_default` takes.

    The golden path feeds `rust_default` JSON values; the vector-ish branches
    already accept a `"(x, y)"`-style string, so most doc defaults pass straight
    through. The ones that must NOT are the scalars: doc writes `"false"`, and a
    non-empty Python string is truthy, so a bool would come out `true`.
    """
    if raw is None:
        return None
    raw = raw.strip()
    if type_name == "bool":
        return raw == "true"
    if type_name == "int":
        try:
            return int(raw, 0)
        except ValueError:
            return 0
    if type_name == "float":
        try:
            return float(raw.rstrip("f"))
        except ValueError:
            return 0.0
    if type_name in ("String", "StringName", "NodePath"):
        # `""`, `&""`, `^""`, `NodePath("")` → the inner text.
        inner = re.search(r'"(.*)"', raw)
        return inner.group(1) if inner else ""
    if type_name == "Array" or type_name.startswith("Packed"):
        return []
    if _EMPTY_CTOR.match(raw):
        # `Callable()`, `RID()`, … — no value to show.
        return None
    if raw == "null":
        return None
    # Color(…) / Vector2(…) / Rect2(…) / AABB(…): `rust_default` parses the
    # numbers straight out of the string.
    return raw


def synthesize_entry(cls):
    """Build a golden-shaped entry for `cls` from the vendored doc XML.

    Walks the `inherits` chain base-first so the flattened order matches how a
    captured golden reads (Node members, then CanvasItem, then Node2D, then the
    leaf). Hints are unavailable in the doc XML, so they are 0/"" — the property
    still shows with its name, type and Godot default, which is the difference
    between a populated inspector and an empty one.
    """
    chain = []
    seen_cls = set()
    cur = cls
    while cur and cur not in seen_cls:
        doc = doc_class(cur)
        if doc is None:
            break
        seen_cls.add(cur)
        chain.append((cur, doc[1]))
        cur = doc[0]
    if not chain:
        return None
    chain.reverse()  # base-first

    entry = {"order": [], "hint": {}, "hint_string": {}, "type": {}, "default": {}, "allvals": {}}
    for _owner, members in chain:
        for name, tname, raw in members:
            if name in entry["type"]:
                continue  # a subclass redeclaration keeps the base's position
            entry["order"].append(name)
            entry["type"][name] = tname
            entry["hint"][name] = 0
            entry["hint_string"][name] = ""
            entry["default"][name] = doc_default_value(tname, raw)
    return entry


def load_api_parents():
    """`child -> parent` for every class in the captured extension_api dump."""
    path = os.path.join(ORACLE_DIR, "godot_extension_api.json")
    if not os.path.exists(path):
        return {}
    data = json.load(open(path))
    out = {}
    for c in data.get("classes", []):
        name, inherits = c.get("name"), c.get("inherits")
        if name and inherits:
            out[name] = inherits
    return out


def fmt_f(v):
    """Rust float literal (always has a decimal point / exponent)."""
    f = float(v)
    s = repr(f)
    if "." not in s and "e" not in s and "E" not in s and "inf" not in s and "nan" not in s:
        s += ".0"
    return s


def rust_str(s):
    """Emit a valid Rust string literal (Rust source is UTF-8, and its unicode
    escape is `\\u{XXXX}`, unlike JSON's `\\uXXXX`)."""
    if not isinstance(s, str):
        s = ""
    out = ['"']
    for ch in s:
        o = ord(ch)
        if ch == "\\":
            out.append("\\\\")
        elif ch == '"':
            out.append('\\"')
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\t":
            out.append("\\t")
        elif ch == "\r":
            out.append("\\r")
        elif o < 0x20:
            out.append(f"\\u{{{o:x}}}")
        else:
            out.append(ch)  # printable ASCII or non-ASCII UTF-8, emitted literally
    out.append('"')
    return "".join(out)


_NUMS = re.compile(r"-?\d+(?:\.\d+)?")


def parse_nums(s):
    return [float(x) for x in _NUMS.findall(s)]


def vec_components(value, n):
    """Return n float components from a golden vector-ish value (dict {x,y,z},
    string '(x, y[, z])', or list)."""
    keys = ["x", "y", "z", "w"][:n]
    if isinstance(value, dict):
        return [float(value.get(k, 0.0)) for k in keys]
    if isinstance(value, str):
        comps = parse_nums(value)
        return (comps + [0.0] * n)[:n]
    if isinstance(value, list):
        return (list(map(float, value)) + [0.0] * n)[:n]
    return [0.0] * n


def rust_default(type_name, value):
    """Map a golden (type_name, default value) to a Rust `Variant` expression."""
    # A null golden value means Godot shows the property unset (e.g. an
    # `is_default` theme override / resource slot) — reproduce it as Nil, not a
    # type-zero, so it compares equal to Godot's null.
    if value is None:
        return "Variant::Nil"
    t = type_name
    if t == "int":
        try:
            return f"Variant::Int({int(value)})"
        except (TypeError, ValueError):
            return "Variant::Int(0)"
    if t == "bool":
        return f"Variant::Bool({'true' if value else 'false'})"
    if t == "float":
        try:
            return f"Variant::Float({fmt_f(value)})"
        except (TypeError, ValueError):
            return "Variant::Float(0.0)"
    if t == "String":
        return "Variant::String(String::new())" if not value else f"Variant::String(String::from({rust_str(value)}))"
    if t == "StringName":
        return f"Variant::StringName(StringName::new({rust_str(value if isinstance(value, str) else '')}))"
    if t in ("Object", "Nil"):
        return "Variant::Nil"
    if t == "NodePath":
        return "Variant::NodePath(NodePath::empty())" if not value else f"Variant::NodePath(NodePath::new({rust_str(value)}))"
    if t == "Color":
        if isinstance(value, dict):
            r, g, b, a = (value.get(k, 0.0) for k in "rgba")
        else:
            comps = vec_components(value, 4)
            r, g, b, a = comps
        return f"Variant::Color(Color {{ r: {fmt_f(r)}, g: {fmt_f(g)}, b: {fmt_f(b)}, a: {fmt_f(a)} }})"
    if t in ("Vector2", "Vector2i"):  # Patina Variant has no integer vectors -> Vector2
        x, y = vec_components(value, 2)
        return f"Variant::Vector2(Vector2 {{ x: {fmt_f(x)}, y: {fmt_f(y)} }})"
    if t in ("Vector3", "Vector3i"):
        x, y, z = vec_components(value, 3)
        return f"Variant::Vector3(Vector3 {{ x: {fmt_f(x)}, y: {fmt_f(y)}, z: {fmt_f(z)} }})"
    if t == "Rect2":
        if isinstance(value, dict):
            px, py = vec_components(value.get("position", {}), 2)
            sx, sy = vec_components(value.get("size", {}), 2)
        else:
            n = parse_nums(value if isinstance(value, str) else "")
            px, py, sx, sy = (n + [0.0] * 4)[:4]
        return (f"Variant::Rect2(Rect2 {{ position: Vector2 {{ x: {fmt_f(px)}, y: {fmt_f(py)} }}, "
                f"size: Vector2 {{ x: {fmt_f(sx)}, y: {fmt_f(sy)} }} }})")
    if t == "AABB":
        n = parse_nums(value if isinstance(value, str) else "")
        px, py, pz, sx, sy, sz = (n + [0.0] * 6)[:6]
        return (f"Variant::Aabb(Aabb {{ position: Vector3 {{ x: {fmt_f(px)}, y: {fmt_f(py)}, z: {fmt_f(pz)} }}, "
                f"size: Vector3 {{ x: {fmt_f(sx)}, y: {fmt_f(sy)}, z: {fmt_f(sz)} }} }})")
    if t == "Array" or t.startswith("Packed"):
        return "Variant::Array(Vec::new())"
    # Unknown/exotic types: expose the property name with a nil placeholder value.
    return "Variant::Nil"


def load_true_defaults_table():
    """Load ClassDB true defaults from the probe artifact (pat-ghfv0.42).

    Returns a dict keyed by (class, prop) -> Rust Variant expression string.
    The probe JSON is the durable source — not a hand-curated Python table.
    """
    table = {}
    if not os.path.isfile(TRUE_DEFAULTS_PATH):
        return table
    with open(TRUE_DEFAULTS_PATH) as f:
        doc = json.load(f)
    classes = doc.get("classes") or {}
    for cls, props in classes.items():
        if not isinstance(props, dict):
            continue
        for name, entry in props.items():
            if not isinstance(entry, dict):
                continue
            tname = entry.get("type") or "Nil"
            value = entry.get("value")
            # Map probe type names to rust_default's type strings.
            # NOTE: rust_default() matches primitive types in lowercase
            # ("bool"/"int"/"float") but compound types capitalized
            # ("String"/"Vector2"/"Vector3"/"Color"). The probe emits capitalized
            # primitive names ("Bool"/"Int"/"Float"), so these MUST map down to
            # lowercase — otherwise every bool/int/float true-default falls through
            # rust_default and silently becomes Variant::Nil (pat-ghfv0.42 regression).
            type_map = {
                "bool": "bool",
                "int": "int",
                "float": "float",
                "String": "String",
                "Vector2": "Vector2",
                "Vector3": "Vector3",
                "Color": "Color",
                "Nil": "Nil",
                "Bool": "bool",
                "Int": "int",
                "Float": "float",
            }
            t = type_map.get(tname, tname)
            if t == "Nil" or value is None and tname in ("Nil", "nil", ""):
                table[(cls, name)] = "Variant::Nil"
            else:
                table[(cls, name)] = rust_default(t, value if value is not None else {})
    return table


# Populated once at import / main — sourced from true_defaults.json (probe).
TRUE_DEFAULTS_FROM_PROBE = None


def true_defaults_table():
    global TRUE_DEFAULTS_FROM_PROBE
    if TRUE_DEFAULTS_FROM_PROBE is None:
        TRUE_DEFAULTS_FROM_PROBE = load_true_defaults_table()
    return TRUE_DEFAULTS_FROM_PROBE


def true_default_expr(cls, name, tname, entry, display_default):
    """Rust `Variant` expression for a prop's TRUE ClassDB default, used for
    `is_default` (distinct from the shown `display_default`). Precedence:
      1. an `is_default=true` golden sample directly reveals the real default;
      2. probe-sourced ClassDB default from true_defaults.json (pat-ghfv0.42);
      3. transform defaults (position -> origin, scale -> unit vector);
      4. otherwise the display default (the two coincide for the vast majority).
    `surface_material_override/N` slots are handled by the server (always an
    override), so they fall through to (4) here."""
    if name in entry["default"]:
        # An is_default=true sample IS Godot's class default for this class.
        return rust_default(tname, entry["default"][name])
    probe = true_defaults_table()
    if (cls, name) in probe:
        return probe[(cls, name)]
    if name == "position":
        return rust_default(tname, {})  # origin: (0,0) / (0,0,0)
    if name == "scale":
        if tname in ("Vector2", "Vector2i"):
            return "Variant::Vector2(Vector2 { x: 1.0, y: 1.0 })"
        return "Variant::Vector3(Vector3 { x: 1.0, y: 1.0, z: 1.0 })"
    return display_default


def main():
    files = sorted(glob.glob(os.path.join(ORACLE_DIR, "inspector_*.json")))
    # class -> {"order": [propnames from the maximal instance], "hint": {}, "type": {}, "default": {}}
    classes = {}
    for f in files:
        data = json.load(open(f))
        for node in data.get("nodes", []):
            if node.get("path") == "/root":
                continue  # editor host Window, out of oracle scope
            cls = node.get("class")
            props = node.get("inspector_properties", [])
            if not cls or not props:
                continue
            entry = classes.setdefault(
                cls,
                {"order": [], "hint": {}, "hint_string": {}, "type": {}, "default": {}, "allvals": {}},
            )
            # Use the maximal (superset) instance to fix ordering + membership.
            if len(props) > len(entry["order"]):
                entry["order"] = [p["name"] for p in props]
            for p in props:
                name = p["name"]
                entry["hint"][name] = p.get("hint", 0)
                entry["hint_string"][name] = p.get("hint_string", "")
                entry["type"][name] = p.get("type_name", "")
                entry["allvals"].setdefault(name, []).append(p.get("value"))
                # Record the class default from an is_default=true occurrence.
                if p.get("is_default") and name not in entry["default"]:
                    entry["default"][name] = p.get("value")

    # Fill in the classes no captured scene contains (pat-0q2kx.128). A golden
    # always wins: synthesis only runs for a class the goldens never saw.
    synthesized = []
    missing_docs = []
    for cls in SYNTHESIZE_FROM_DOCS:
        if cls in classes:
            continue
        entry = synthesize_entry(cls)
        if entry is None or not entry["order"]:
            missing_docs.append(cls)
            continue
        classes[cls] = entry
        synthesized.append(cls)

    lines = []
    lines.append("//! GENERATED — do not edit by hand.")
    lines.append("//!")
    lines.append("//! Per-class editor property schema transcribed from the real Godot 4.6.1")
    lines.append("//! inspector goldens (`fixtures/editor_oracle/inspector_*.json`), plus")
    lines.append("//! classes synthesized from the vendored `upstream/godot/doc/classes/*.xml`")
    lines.append("//! for the ones no captured scene contains (pat-0q2kx.128). A golden always")
    lines.append("//! wins; synthesized entries carry Godot's documented defaults but no hints.")
    lines.append("//! Regenerate with: `python3 tools/oracle/gen_inspector_schema.py`")
    lines.append("//!")
    lines.append(f"//! {len(classes)} classes seeded ({len(synthesized)} synthesized from doc XML).")
    lines.append("")
    lines.append("use gdcore::math::{Color, Rect2, Vector2, Vector3};")
    lines.append("use gdcore::math3d::Aabb;")
    lines.append("use gdcore::node_path::NodePath;")
    lines.append("use gdcore::string_name::StringName;")
    lines.append("use gdvariant::Variant;")
    lines.append("")
    lines.append("use crate::inspector_schema::EditorProp;")
    lines.append("")
    lines.append("/// Returns the full flattened editor property schema for `class_name`,")
    lines.append("/// or `None` if the class has no captured golden.")
    lines.append("pub fn generated_schema(class_name: &str) -> Option<Vec<EditorProp>> {")
    lines.append("    let props: Vec<EditorProp> = match class_name {")

    for cls in sorted(classes):
        entry = classes[cls]
        lines.append(f'        {rust_str(cls)} => vec![')
        for name in entry["order"]:
            tname = entry["type"].get(name, "")
            hint = entry["hint"].get(name, 0)
            hint_string = entry["hint_string"].get(name, "")
            if name in entry["default"]:
                dflt = rust_default(tname, entry["default"][name])
            else:
                # No `is_default` sample. If every captured node agrees on one
                # value, that unanimous value is what Godot's inspector shows by
                # default (e.g. CollisionShape `debug_color` from project
                # settings) — seed it. Otherwise fall back to the type-zero.
                vals = entry["allvals"].get(name, [])
                keys = {json.dumps(v, sort_keys=True) for v in vals}
                if len(keys) == 1 and vals:
                    dflt = rust_default(tname, vals[0])
                else:
                    dflt = rust_default(tname, "" if tname in ("String", "StringName", "NodePath") else {})
            true_dflt = true_default_expr(cls, name, tname, entry, dflt)
            lines.append(
                f"            EditorProp::new({rust_str(name)}, {rust_str(tname)}, {hint}, "
                f"{rust_str(hint_string)}, {dflt}, {true_dflt}),"
            )
        lines.append("        ],")

    lines.append("        _ => return None,")
    lines.append("    };")
    lines.append("    Some(props)")
    lines.append("}")
    lines.append("")
    lines.append("/// Every class with a generated schema (for tests / diagnostics).")
    lines.append("pub const GENERATED_CLASSES: &[&str] = &[")
    for cls in sorted(classes):
        lines.append(f"    {rust_str(cls)},")
    lines.append("];")
    lines.append("")

    # Complete `inherits` map straight from extension_api.json (pat-0q2kx.128).
    # `inspector_schema::parent_of` keeps its curated table as the authority and
    # falls back here, so ANY class — not just the ones in the oracle scenes —
    # resolves up to its nearest seeded base instead of returning None.
    parents = load_api_parents()
    lines.append("/// Every `inherits` edge Godot 4.6.1 declares, from")
    lines.append("/// `fixtures/editor_oracle/godot_extension_api.json`.")
    lines.append("///")
    lines.append("/// [`crate::inspector_schema::parent_of`] consults its own curated table first")
    lines.append("/// and falls back to this, so a class outside the captured oracle scenes still")
    lines.append("/// walks up to its nearest seeded base rather than losing its schema.")
    lines.append("pub fn generated_parent_of(class_name: &str) -> Option<&'static str> {")
    lines.append("    const PARENTS: &[(&str, &str)] = &[")
    for child in sorted(parents):
        lines.append(f"        ({rust_str(child)}, {rust_str(parents[child])}),")
    lines.append("    ];")
    lines.append("    PARENTS")
    lines.append("        .binary_search_by(|(c, _)| c.cmp(&class_name))")
    lines.append("        .ok()")
    lines.append("        .map(|i| PARENTS[i].1)")
    lines.append("}")
    lines.append("")

    with open(OUT, "w") as fh:
        fh.write("\n".join(lines))

    total_props = sum(len(c["order"]) for c in classes.values())
    print(f"Wrote {OUT}")
    if synthesized:
        print(f"  synthesized from doc XML ({len(synthesized)}): {', '.join(sorted(synthesized))}")
    if missing_docs:
        print(f"  NO doc page, still unseeded ({len(missing_docs)}): {', '.join(sorted(missing_docs))}")
    print(f"  {len(classes)} classes, {total_props} total properties")
    for cls in sorted(classes, key=lambda c: -len(classes[c]["order"])):
        print(f"    {cls:22s} {len(classes[cls]['order'])}")


if __name__ == "__main__":
    main()
