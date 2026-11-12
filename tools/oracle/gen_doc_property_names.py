#!/usr/bin/env python3
"""Generate engine-rs/crates/gdobject/src/godot_doc_properties.rs.

Every Godot class's documented properties (`<members>`) and its parent, read
from the vendored Godot docs (upstream/godot/doc/classes/*.xml and
upstream/godot/modules/*/doc_classes/*.xml). The runtime uses it to answer
"does a Godot object of class X have a property named Y?" — what GDScript's
`obj.y` needs to raise Godot's "Invalid access to property or key" for a name
that does not exist (pat-w0q0w), without depending on Patina's own ClassDB,
whose property lists are incomplete.

Run from the repo root:  python3 tools/oracle/gen_doc_property_names.py
"""

import glob
import os
import xml.etree.ElementTree as ET

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, "engine-rs", "crates", "gdobject", "src", "godot_doc_properties.rs")


def main():
    files = sorted(glob.glob(os.path.join(ROOT, "upstream/godot/doc/classes/*.xml")))
    files += sorted(glob.glob(os.path.join(ROOT, "upstream/godot/modules/*/doc_classes/*.xml")))
    if not files:
        raise SystemExit("no Godot doc XML found; run scripts/worktree-setup.sh first")
    classes = {}
    for f in files:
        root = ET.parse(f).getroot()
        name = root.get("name")
        parent = root.get("inherits") or ""
        members = sorted({m.get("name") for m in root.iter("member") if m.get("name")})
        classes[name] = (parent, members)
    lines = [
        "//! GENERATED — do not edit by hand.",
        "//!",
        "//! Every Godot 4.6 class's documented properties and its parent, from the",
        "//! vendored `upstream/godot/doc/classes/*.xml` (+ modules' doc_classes).",
        "//! Regenerate with: `python3 tools/oracle/gen_doc_property_names.py`",
        "",
        "/// `(class, parent, sorted property names)`, sorted by class name.",
        "pub static CLASSES: &[(&str, &str, &[&str])] = &[",
    ]
    for name in sorted(classes):
        parent, members = classes[name]
        body = ", ".join('"%s"' % m for m in members)
        lines.append('    ("%s", "%s", &[%s]),' % (name, parent, body))
    lines.append("];")
    lines.append("")
    with open(OUT, "w") as fh:
        fh.write("\n".join(lines))
    print("wrote %s (%d classes)" % (OUT, len(classes)))


if __name__ == "__main__":
    main()
