#!/usr/bin/env python3
"""Generate gdeditor's per-class editor property schema table from the captured
Godot 4.6.1 inspector goldens (pat-ghfv0.x fill phase).

Reads every `engine-rs/fixtures/editor_oracle/inspector_*.json` golden and emits
`engine-rs/crates/gdeditor/src/inspector_schema_generated.rs`, a data table of
`class -> ordered [EditorProp{name,type_name,hint,default}]` where each default
is the class default value taken from an `is_default=true` golden entry (falling
back to the Godot type-zero when a property is overridden in every sample).

This is the single source of truth for the seeded schema: it transcribes the
real editor property list, order, hints, and defaults straight from Godot rather
than hand-authoring ~30 classes.

Run from repo root:  python3 tools/oracle/gen_inspector_schema.py
"""
import glob
import json
import os
import re

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ORACLE_DIR = os.path.join(REPO, "engine-rs/fixtures/editor_oracle")
OUT = os.path.join(REPO, "engine-rs/crates/gdeditor/src/inspector_schema_generated.rs")


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

    lines = []
    lines.append("//! GENERATED — do not edit by hand.")
    lines.append("//!")
    lines.append("//! Per-class editor property schema transcribed from the real Godot 4.6.1")
    lines.append("//! inspector goldens (`fixtures/editor_oracle/inspector_*.json`).")
    lines.append("//! Regenerate with: `python3 tools/oracle/gen_inspector_schema.py`")
    lines.append("//!")
    lines.append(f"//! {len(classes)} classes seeded.")
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
            lines.append(
                f"            EditorProp::new({rust_str(name)}, {rust_str(tname)}, {hint}, "
                f"{rust_str(hint_string)}, {dflt}),"
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

    with open(OUT, "w") as fh:
        fh.write("\n".join(lines))

    total_props = sum(len(c["order"]) for c in classes.values())
    print(f"Wrote {OUT}")
    print(f"  {len(classes)} classes, {total_props} total properties")
    for cls in sorted(classes, key=lambda c: -len(classes[c]["order"])):
        print(f"    {cls:22s} {len(classes[cls]['order'])}")


if __name__ == "__main__":
    main()
