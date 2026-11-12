//! Scene serialization — save a [`SceneTree`] subtree back to `.tscn` format.
//!
//! [`TscnSaver`] walks a node hierarchy depth-first and produces the text
//! representation that [`PackedScene::from_tscn`](crate::packed_scene::PackedScene::from_tscn)
//! can round-trip.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as FmtWrite;

use gdvariant::Variant;

use crate::node::NodeId;
use crate::packed_scene::{CONNECT_DEFERRED, CONNECT_ONE_SHOT};
use crate::scene_tree::SceneTree;

// ---------------------------------------------------------------------------
// Variant formatting
// ---------------------------------------------------------------------------

/// Formats a [`Variant`] value in `.tscn` property syntax.
///
/// Handles all types that the parser can read back: scalars, strings,
/// math types, node paths, arrays, and dictionaries.
/// Renders a dictionary key to its text form for `.tscn` output. String keys
/// use their raw text; other Variant kinds use their lossy string form.
fn dict_key_display(k: &Variant) -> String {
    match k {
        Variant::String(s) => s.clone(),
        other => other.to_string_lossy(),
    }
}

/// A dictionary key as Godot writes it: a String quoted, a StringName as
/// `&"..."`, anything else in its own constructor syntax.
fn format_dict_key(k: &Variant) -> String {
    match k {
        Variant::String(_) | Variant::StringName(_) => format_variant_value(k),
        Variant::Int(_) | Variant::Float(_) | Variant::Bool(_) => format_variant_value(k),
        other => format!("\"{}\"", other.to_string_lossy()),
    }
}

pub fn format_variant_value(v: &Variant) -> String {
    match v {
        Variant::Nil => "null".to_string(),
        Variant::Bool(b) => b.to_string(),
        Variant::Int(i) => i.to_string(),
        Variant::Float(f) => format_float(*f),
        // A resource held INSIDE a container (an AnimationLibrary's `_data`, a
        // SpriteFrames frame's `texture`) is stored as its reference string;
        // Godot writes the reference itself, `SubResource("id")`.
        Variant::String(s) if s.starts_with("SubResource:") || s.starts_with("ExtResource:") => {
            let (kind, id) = s.split_once(':').unwrap();
            format!("{kind}(\"{id}\")")
        }
        Variant::String(s) => {
            let escaped = s
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\t', "\\t");
            format!("\"{escaped}\"")
        }
        Variant::StringName(sn) => {
            format!("&\"{}\"", sn.as_str())
        }
        Variant::NodePath(np) => {
            format!("NodePath(\"{np}\")")
        }
        Variant::Vector2(v) => {
            format!("Vector2({}, {})", format_f32(v.x), format_f32(v.y))
        }
        // Int vectors print as bare integers — `Vector2i(3, 4)`, never
        // `Vector2i(3.0, 4.0)` — which is what makes the .tscn round-trip
        // byte-identical to what Godot wrote (pat-nzgdu.78).
        Variant::Vector2i(v) => format!("Vector2i({}, {})", v.x, v.y),
        Variant::Vector3(v) => {
            format!(
                "Vector3({}, {}, {})",
                format_f32(v.x),
                format_f32(v.y),
                format_f32(v.z)
            )
        }
        Variant::Vector3i(v) => format!("Vector3i({}, {}, {})", v.x, v.y, v.z),
        Variant::Vector4(v) => {
            format!(
                "Vector4({}, {}, {}, {})",
                format_f32(v.x),
                format_f32(v.y),
                format_f32(v.z),
                format_f32(v.w)
            )
        }
        Variant::Vector4i(v) => format!("Vector4i({}, {}, {}, {})", v.x, v.y, v.z, v.w),
        // Integer components print bare, exactly as they were authored — going
        // through `format_f32` would write `Rect2i(0, 0, 16, 16)` back as
        // floats and stop the file round-tripping.
        Variant::Rect2i(r) => format!(
            "Rect2i({}, {}, {}, {})",
            r.position.x, r.position.y, r.size.x, r.size.y
        ),
        // Sixteen cells in COLUMN-MAJOR order (`m[column][row]`), the order
        // `gdresource::loader::parse_variant_value` reads back. A transposed
        // projection still renders, just wrongly, so nothing downstream would
        // catch the mistake.
        Variant::Projection(p) => {
            let cells: Vec<String> = p.m.iter().flatten().map(|c| format_f32(*c)).collect();
            format!("Projection({})", cells.join(", "))
        }
        Variant::Color(c) => {
            format!(
                "Color({}, {}, {}, {})",
                format_f32(c.r),
                format_f32(c.g),
                format_f32(c.b),
                format_f32(c.a)
            )
        }
        Variant::Rect2(r) => {
            format!(
                "Rect2({}, {}, {}, {})",
                format_f32(r.position.x),
                format_f32(r.position.y),
                format_f32(r.size.x),
                format_f32(r.size.y)
            )
        }
        Variant::Transform2D(t) => {
            format!(
                "Transform2D({}, {}, {}, {}, {}, {})",
                format_f32(t.x.x),
                format_f32(t.x.y),
                format_f32(t.y.x),
                format_f32(t.y.y),
                format_f32(t.origin.x),
                format_f32(t.origin.y)
            )
        }
        Variant::Array(arr) => {
            let items: Vec<String> = arr.to_vec().iter().map(format_variant_value).collect();
            format!("[{}]", items.join(", "))
        }
        Variant::Dictionary(dict) => {
            // Keys are Variants; render each to a stable display string and sort
            // for deterministic output.
            let mut pairs: Vec<(String, String, String)> = dict
                .to_dict()
                .iter()
                .map(|(k, v)| {
                    (
                        dict_key_display(k),
                        format_dict_key(k),
                        format_variant_value(v),
                    )
                })
                .collect();
            pairs.sort_by(|a, b| a.0.cmp(&b.0));
            if pairs.is_empty() {
                return "{}".to_string();
            }
            // Godot's VariantWriter puts every entry on its own line —
            // `{\n"a": 1,\n"b": 2\n}` — and a key keeps its own type (a
            // StringName key is `&"spin"`), pat-l0afa. The one-line form was
            // never what a Godot-saved file holds.
            let pairs: Vec<String> = pairs.iter().map(|(_, k, v)| format!("{k}: {v}")).collect();
            format!("{{\n{}\n}}", pairs.join(",\n"))
        }
        // Packed arrays serialize as `PackedXArray(elem, elem, …)`, with
        // vector/color elements flattened into their components (Godot syntax).
        Variant::PackedByteArray(v) => {
            let items: Vec<String> = v.iter().map(|b| b.to_string()).collect();
            format!("PackedByteArray({})", items.join(", "))
        }
        Variant::PackedInt32Array(v) => {
            let items: Vec<String> = v.iter().map(|i| i.to_string()).collect();
            format!("PackedInt32Array({})", items.join(", "))
        }
        Variant::PackedInt64Array(v) => {
            let items: Vec<String> = v.iter().map(|i| i.to_string()).collect();
            format!("PackedInt64Array({})", items.join(", "))
        }
        Variant::PackedFloat32Array(v) => {
            let items: Vec<String> = v.iter().map(|f| format_f32(*f)).collect();
            format!("PackedFloat32Array({})", items.join(", "))
        }
        Variant::PackedFloat64Array(v) => {
            let items: Vec<String> = v.iter().map(|f| format_float(*f)).collect();
            format!("PackedFloat64Array({})", items.join(", "))
        }
        Variant::PackedStringArray(v) => {
            let items: Vec<String> = v
                .iter()
                .map(|s| {
                    let escaped = s
                        .replace('\\', "\\\\")
                        .replace('"', "\\\"")
                        .replace('\n', "\\n")
                        .replace('\t', "\\t");
                    format!("\"{escaped}\"")
                })
                .collect();
            format!("PackedStringArray({})", items.join(", "))
        }
        Variant::PackedVector2Array(v) => {
            let items: Vec<String> = v
                .iter()
                .flat_map(|p| [format_f32(p.x), format_f32(p.y)])
                .collect();
            format!("PackedVector2Array({})", items.join(", "))
        }
        Variant::PackedVector3Array(v) => {
            let items: Vec<String> = v
                .iter()
                .flat_map(|p| [format_f32(p.x), format_f32(p.y), format_f32(p.z)])
                .collect();
            format!("PackedVector3Array({})", items.join(", "))
        }
        Variant::PackedColorArray(v) => {
            let items: Vec<String> = v
                .iter()
                .flat_map(|c| {
                    [
                        format_f32(c.r),
                        format_f32(c.g),
                        format_f32(c.b),
                        format_f32(c.a),
                    ]
                })
                .collect();
            format!("PackedColorArray({})", items.join(", "))
        }
        Variant::PackedVector4Array(v) => {
            let items: Vec<String> = v
                .iter()
                .flat_map(|p| {
                    [
                        format_f32(p.x),
                        format_f32(p.y),
                        format_f32(p.z),
                        format_f32(p.w),
                    ]
                })
                .collect();
            format!("PackedVector4Array({})", items.join(", "))
        }
        // 3D math types serialize with their Godot constructor syntax, matching
        // the field order `gdresource::loader::parse_variant_value` reads back.
        //
        // ⚠️ Basis / Transform3D write the basis ROW-major (`rows[i][j]`,
        // core/variant/variant_parser.cpp:2110), while `Basis` stores columns —
        // hence `b.x.x, b.y.x, b.z.x, ...`. Column order transposed every saved
        // rotation (pat-sazee).
        Variant::Basis(b) => {
            format!(
                "Basis({}, {}, {}, {}, {}, {}, {}, {}, {})",
                format_f32(b.x.x),
                format_f32(b.y.x),
                format_f32(b.z.x),
                format_f32(b.x.y),
                format_f32(b.y.y),
                format_f32(b.z.y),
                format_f32(b.x.z),
                format_f32(b.y.z),
                format_f32(b.z.z),
            )
        }
        Variant::Transform3D(t) => {
            format!(
                "Transform3D({}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {})",
                format_f32(t.basis.x.x),
                format_f32(t.basis.y.x),
                format_f32(t.basis.z.x),
                format_f32(t.basis.x.y),
                format_f32(t.basis.y.y),
                format_f32(t.basis.z.y),
                format_f32(t.basis.x.z),
                format_f32(t.basis.y.z),
                format_f32(t.basis.z.z),
                format_f32(t.origin.x),
                format_f32(t.origin.y),
                format_f32(t.origin.z),
            )
        }
        Variant::Quaternion(q) => {
            format!(
                "Quaternion({}, {}, {}, {})",
                format_f32(q.x),
                format_f32(q.y),
                format_f32(q.z),
                format_f32(q.w)
            )
        }
        Variant::Aabb(a) => {
            format!(
                "AABB({}, {}, {}, {}, {}, {})",
                format_f32(a.position.x),
                format_f32(a.position.y),
                format_f32(a.position.z),
                format_f32(a.size.x),
                format_f32(a.size.y),
                format_f32(a.size.z)
            )
        }
        Variant::Plane(p) => {
            format!(
                "Plane({}, {}, {}, {})",
                format_f32(p.normal.x),
                format_f32(p.normal.y),
                format_f32(p.normal.z),
                format_f32(p.d)
            )
        }
        // Types that don't have a standard .tscn text representation.
        other => format!("{other}"),
    }
}

/// The `Animation`/`AnimationLibrary` sub-resource text for a scene's
/// AnimationPlayers, plus the per-player `libraries` line each node section needs
/// (pat-0q2kx.54).
struct AnimationTables {
    /// Rendered `[sub_resource]` sections, ready to append.
    text: String,
    /// `node id -> (library sub-resource id, autoplay clip)` for the node writer.
    players: HashMap<NodeId, (String, Option<String>)>,
    /// How many sub-resources were emitted (for `load_steps`).
    count: usize,
}

impl AnimationTables {
    fn resource_count(&self) -> usize {
        self.count
    }

    /// Properties regenerated for `nid` from its attached player, which the node
    /// writer must therefore NOT emit from the node's own (parsed) copies.
    fn skip_keys(&self, nid: NodeId) -> &'static [&'static str] {
        if self.players.contains_key(&nid) {
            &["libraries", "autoplay"]
        } else {
            &[]
        }
    }
}

/// Serializes every AnimationPlayer's clip library in the saved subtree.
///
/// Godot stores an editor-authored animation *inside the scene file*: one
/// `[sub_resource type="Animation"]` per clip carrying `length`/`loop_mode` and
/// `tracks/N/*`, gathered by an `[sub_resource type="AnimationLibrary"]` whose
/// `_data` maps clip names to them. This writes exactly that shape, so
/// `gdscene::animation_tscn` (and Godot itself) reads back what we saved.
///
/// Clip and library ids are derived from the player's node name and the clip
/// name, so re-saving an unchanged scene produces an unchanged file rather than
/// churning ids.
fn collect_animation_tables(tree: &SceneTree, node_ids: &[NodeId]) -> AnimationTables {
    let mut used_stems = std::collections::HashSet::new();
    let mut text = String::new();
    let mut players: HashMap<NodeId, (String, Option<String>)> = HashMap::new();
    let mut count = 0usize;

    for &nid in node_ids {
        let Some(player) = tree.get_animation_player(nid) else {
            continue;
        };
        if player.animations.is_empty() {
            continue;
        }
        let node_name = tree
            .get_node(nid)
            .map(|n| n.name().to_string())
            .unwrap_or_else(|| "AnimationPlayer".to_string());
        let stem = unique_stem(&mut used_stems, sanitize_resource_id(&node_name));

        // Deterministic order: a HashMap iteration order would reshuffle the
        // file on every save and make diffs meaningless.
        let mut names: Vec<&String> = player.animations.keys().collect();
        names.sort();

        let mut entries: Vec<(String, String)> = Vec::new();
        for name in names {
            let anim = &player.animations[name];
            let anim_id = format!("Animation_{stem}_{}", sanitize_resource_id(name));
            writeln!(text, "\n[sub_resource type=\"Animation\" id=\"{anim_id}\"]").unwrap();
            write_animation_body(&mut text, anim);
            entries.push((name.clone(), anim_id));
            count += 1;
        }

        let lib_id = format!("AnimationLibrary_{stem}");
        writeln!(
            text,
            "\n[sub_resource type=\"AnimationLibrary\" id=\"{lib_id}\"]"
        )
        .unwrap();
        writeln!(text, "_data = {{").unwrap();
        for (i, (name, anim_id)) in entries.iter().enumerate() {
            let comma = if i + 1 < entries.len() { "," } else { "" };
            writeln!(text, "\"{name}\": SubResource(\"{anim_id}\"){comma}").unwrap();
        }
        writeln!(text, "}}").unwrap();
        count += 1;

        players.insert(nid, (lib_id, player.autoplay.clone()));
    }

    AnimationTables {
        text,
        players,
        count,
    }
}

/// Writes one `Animation`'s body: its length, loop mode, and track table.
fn write_animation_body(out: &mut String, anim: &crate::animation::Animation) {
    use crate::animation::TrackType;

    writeln!(out, "resource_name = \"{}\"", anim.name).unwrap();
    writeln!(out, "length = {}", format_float(anim.length)).unwrap();
    writeln!(out, "loop_mode = {}", loop_mode_to_int(anim.loop_mode)).unwrap();

    for (i, track) in anim.tracks.iter().enumerate() {
        let Some(kind) = track_type_to_tscn(track.track_type) else {
            // A track kind with no Godot text spelling is skipped rather than
            // written as something Godot would refuse to parse.
            continue;
        };
        writeln!(out, "tracks/{i}/type = \"{kind}\"").unwrap();
        writeln!(out, "tracks/{i}/imported = false").unwrap();
        writeln!(out, "tracks/{i}/enabled = true").unwrap();
        // Godot addresses a track as `NodePath("Child:property")`; an empty
        // property (3D transform tracks) keeps the bare node path.
        let path = if track.property_path.is_empty() {
            track.node_path.clone()
        } else {
            format!("{}:{}", track.node_path, track.property_path)
        };
        writeln!(out, "tracks/{i}/path = NodePath(\"{path}\")").unwrap();
        writeln!(
            out,
            "tracks/{i}/interp = {}",
            interpolation_to_int(track.interpolation)
        )
        .unwrap();
        writeln!(out, "tracks/{i}/loop_wrap = true").unwrap();

        let keys = track.keyframes();
        let times: Vec<String> = keys.iter().map(|k| format_float(k.time)).collect();

        // Bézier and animation tracks carry a different payload than
        // `values`/`transitions`, so they are written whole here and skip the
        // value-track body below (pat-s3rrv.44).
        if track.track_type == TrackType::Bezier {
            // Five floats per key — value, in_handle.x/y, out_handle.x/y — in
            // the flat layout `Animation::_set` reads back as `rv[i * 5 + n]`.
            let points: Vec<String> = keys
                .iter()
                .flat_map(|k| {
                    let v = match &k.value {
                        Variant::Float(f) => *f,
                        Variant::Int(n) => *n as f64,
                        _ => 0.0,
                    };
                    [
                        v,
                        k.in_handle.x as f64,
                        k.in_handle.y as f64,
                        k.out_handle.x as f64,
                        k.out_handle.y as f64,
                    ]
                    .map(format_float)
                })
                .collect();
            // Every handle is written as FREE (0): Patina stores the two handles
            // independently, so claiming BALANCED/MIRRORED could contradict the
            // points that follow it.
            let modes: Vec<String> = keys.iter().map(|_| "0".to_string()).collect();
            writeln!(out, "tracks/{i}/keys = {{").unwrap();
            writeln!(
                out,
                "\"handle_modes\": PackedInt32Array({}),",
                modes.join(", ")
            )
            .unwrap();
            writeln!(
                out,
                "\"points\": PackedFloat32Array({}),",
                points.join(", ")
            )
            .unwrap();
            writeln!(out, "\"times\": PackedFloat32Array({})", times.join(", ")).unwrap();
            writeln!(out, "}}").unwrap();
            continue;
        }
        if track.track_type == TrackType::Animation {
            let clips: Vec<String> = keys
                .iter()
                .map(|k| match &k.value {
                    Variant::String(s) => format!("\"{s}\""),
                    Variant::StringName(s) => format!("\"{}\"", s.as_str()),
                    _ => "\"\"".to_string(),
                })
                .collect();
            writeln!(out, "tracks/{i}/keys = {{").unwrap();
            writeln!(out, "\"clips\": PackedStringArray({}),", clips.join(", ")).unwrap();
            writeln!(out, "\"times\": PackedFloat32Array({})", times.join(", ")).unwrap();
            writeln!(out, "}}").unwrap();
            continue;
        }

        let transitions: Vec<String> = keys.iter().map(|_| "1".to_string()).collect();
        let values: Vec<String> = keys
            .iter()
            .map(|k| format_variant_value(&k.value))
            .collect();
        writeln!(out, "tracks/{i}/keys = {{").unwrap();
        writeln!(out, "\"times\": PackedFloat32Array({}),", times.join(", ")).unwrap();
        writeln!(
            out,
            "\"transitions\": PackedFloat32Array({}),",
            transitions.join(", ")
        )
        .unwrap();
        writeln!(
            out,
            "\"update\": {},",
            update_mode_to_int(track.update_mode)
        )
        .unwrap();
        writeln!(out, "\"values\": [{}]", values.join(", ")).unwrap();
        writeln!(out, "}}").unwrap();
    }
}

/// Appends an AnimationPlayer node's `libraries` (and `autoplay`) properties to
/// its just-written node section, pointing at the library sub-resource emitted
/// for it (pat-0q2kx.54).
///
/// These are written here rather than by `write_node_section` because the
/// library id is minted while serializing the clips, not stored on the node.
fn write_animation_player_props(out: &mut String, anims: &AnimationTables, nid: NodeId) {
    let Some((lib_id, autoplay)) = anims.players.get(&nid) else {
        return;
    };
    writeln!(out, "libraries = {{").unwrap();
    writeln!(out, "\"\": SubResource(\"{lib_id}\")").unwrap();
    writeln!(out, "}}").unwrap();
    if let Some(clip) = autoplay {
        if !clip.is_empty() {
            writeln!(out, "autoplay = \"{clip}\"").unwrap();
        }
    }
}

/// The `SpriteFrames` sub-resources rebuilt from a node's `__sf_*` companions.
struct SpriteFramesTables {
    /// Rendered `[sub_resource type="SpriteFrames"]` sections, ready to append.
    text: String,
    /// `node id -> sub-resource id`, for `sprite_frames = SubResource("...")`.
    nodes: HashMap<NodeId, String>,
    /// The frame textures as `[ext_resource]` rows, merged into the header.
    /// Frame textures carry no uid — they are authored by the editor, not
    /// loaded from a `.tscn` that declared one.
    ext: Vec<(String, String, String, Option<String>)>,
}

impl SpriteFramesTables {
    fn resource_count(&self) -> usize {
        self.nodes.len()
    }
}

/// Serializes every AnimatedSprite2D's frame library in the saved subtree.
///
/// Godot writes the resource as one `[sub_resource type="SpriteFrames"]` whose
/// `animations` array holds `{frames, loop, name, speed}` dictionaries, each
/// frame a `{duration, texture}` pointing at an `[ext_resource]`
/// (`scene/resources/sprite_frames.cpp` `_set_animations`). The editor authors
/// this into the FLAT property bag as `__sf_*` keys (`editor_server.rs`); this
/// folds them back into Godot's shape, so frames authored in the panel survive a
/// save as a real SpriteFrames rather than as invented `__sf_` properties.
///
/// Sub-resource and texture ids are derived from the node, animation and path
/// names, so re-saving an unchanged scene produces an unchanged file.
fn collect_spriteframes_tables(tree: &SceneTree, node_ids: &[NodeId]) -> SpriteFramesTables {
    let mut used_stems = std::collections::HashSet::new();
    let mut text = String::new();
    let mut nodes: HashMap<NodeId, String> = HashMap::new();
    let mut ext: BTreeMap<String, (String, String)> = BTreeMap::new();

    for &nid in node_ids {
        let Some(node) = tree.get_node(nid) else {
            continue;
        };
        let Variant::PackedStringArray(anim_names) = node.get_property("__sf_anims__") else {
            continue;
        };
        if anim_names.is_empty() {
            continue;
        }
        let stem = unique_stem(&mut used_stems, sanitize_resource_id(node.name()));
        let sub_id = format!("SpriteFrames_{stem}");

        let mut anim_blocks: Vec<String> = Vec::new();
        // Authoring order, NOT sorted: the animation list is a user-ordered list
        // in Godot's panel and reordering it on save would churn the file.
        for anim in &anim_names {
            let speed = match node.get_property(&format!("__sf_{anim}_speed")) {
                Variant::Float(f) => f,
                Variant::Int(i) => i as f64,
                _ => 5.0,
            };
            // Only an explicit `false` turns looping off; Godot defaults to true.
            let loops = !matches!(
                node.get_property(&format!("__sf_{anim}_loop")),
                Variant::Bool(false)
            );
            let frames = match node.get_property(&format!("__sf_{anim}_frames")) {
                Variant::PackedStringArray(f) => f,
                _ => Vec::new(),
            };

            let mut frame_blocks: Vec<String> = Vec::new();
            for path in &frames {
                let tex_id = format!("Texture_{}", sanitize_resource_id(path));
                ext.entry(tex_id.clone())
                    .or_insert_with(|| (ext_resource_type_for(path), path.clone()));
                frame_blocks.push(format!(
                    "{{\n\"duration\": 1.0,\n\"texture\": ExtResource(\"{tex_id}\")\n}}"
                ));
            }
            anim_blocks.push(format!(
                "{{\n\"frames\": [{}],\n\"loop\": {loops},\n\"name\": &\"{anim}\",\n\"speed\": {}\n}}",
                frame_blocks.join(", "),
                format_float(speed)
            ));
        }

        writeln!(
            text,
            "\n[sub_resource type=\"SpriteFrames\" id=\"{sub_id}\"]"
        )
        .unwrap();
        writeln!(text, "animations = [{}]", anim_blocks.join(", ")).unwrap();
        nodes.insert(nid, sub_id);
    }

    SpriteFramesTables {
        text,
        nodes,
        ext: ext
            .into_iter()
            // Frame textures are authored by the editor rather than loaded from
            // a `.tscn` that declared a uid, so they contribute none
            // (pat-0q2kx.137).
            .map(|(id, (res_type, path))| (id, res_type, path, None))
            .collect(),
    }
}

/// The `TileSet` sub-resources rebuilt from a node's `__tile_set__*` /
/// `__tile_physics_*` companions.
struct TileSetTables {
    /// Rendered `[sub_resource]` sections, ready to append.
    text: String,
    /// `node id -> TileSet sub-resource id`, for `tile_set = SubResource("…")`.
    nodes: HashMap<NodeId, String>,
    /// Atlas textures as `[ext_resource]` rows, merged into the header.
    ext: Vec<(String, String, String, Option<String>)>,
    /// How many `[sub_resource]` sections were emitted.
    count: usize,
}

impl TileSetTables {
    fn resource_count(&self) -> usize {
        self.count
    }
}

/// Serializes every TileMapLayer's authored TileSet in the saved subtree
/// (pat-s3rrv.65).
///
/// # Why this has to exist
///
/// The `__tile_set__*` companions are a **cache** and [`is_internal_prop`]
/// deliberately drops them: they are property names Godot has never heard of.
/// That was fine while a TileSet could only arrive from a loaded `.tscn`, whose
/// node kept the original `tile_set = SubResource("…")` reference. But a TileSet
/// *authored in the editor* has no such reference, so nothing wrote it — the
/// edit lived in memory, was dropped on save, and the file came back without it.
///
/// So this rebuilds Godot's real shape, which is exactly what
/// `packed_scene`'s reader already consumes:
///
/// ```text
/// [sub_resource type="TileSetAtlasSource" id="TileSetAtlasSource_Layer_0"]
/// texture = ExtResource("Texture_res___atlas_png")
/// texture_region_size = Vector2i(16, 16)
/// 0:0/0 = 0
/// 0:0/0/physics_layer_0/polygon_0/points = PackedVector2Array(-8, -8, 8, -8, 8, 8, -8, 8)
///
/// [sub_resource type="TileSet" id="TileSet_Layer"]
/// tile_size = Vector2i(16, 16)
/// physics_layer_0/collision_layer = 1
/// sources/0 = SubResource("TileSetAtlasSource_Layer_0")
/// ```
fn collect_tileset_tables(tree: &SceneTree, node_ids: &[NodeId]) -> TileSetTables {
    let mut used_stems = std::collections::HashSet::new();
    let mut text = String::new();
    let mut nodes: HashMap<NodeId, String> = HashMap::new();
    let mut ext: std::collections::BTreeMap<String, (String, String)> =
        std::collections::BTreeMap::new();
    let mut count = 0usize;

    for &nid in node_ids {
        let Some(node) = tree.get_node(nid) else {
            continue;
        };
        if !matches!(node.class_name(), "TileMapLayer" | "TileMap") {
            continue;
        }
        let source_ids = crate::tilemap::tile_set_source_ids(
            &node.get_property(crate::tilemap::TILE_SET_SOURCE_IDS),
        );
        if source_ids.is_empty() {
            continue;
        }
        let stem = unique_stem(&mut used_stems, sanitize_resource_id(node.name()));
        let tile_size = match node.get_property(crate::tilemap::TILE_SET_TILE_SIZE) {
            Variant::Vector2i(v) => v,
            _ => gdcore::math::Vector2i::new(16, 16),
        };

        let mut source_refs: Vec<(i32, String)> = Vec::new();
        for source_id in &source_ids {
            let source_id = *source_id;
            let Variant::String(texture) =
                node.get_property(&crate::tilemap::tile_set_source_key(source_id, "texture"))
            else {
                continue;
            };
            let sub_id = format!("TileSetAtlasSource_{stem}_{source_id}");
            let tex_id = format!("Texture_{}", sanitize_resource_id(&texture));
            ext.entry(tex_id.clone())
                .or_insert_with(|| (ext_resource_type_for(&texture), texture.clone()));

            let nav_sources = match node.get_property(crate::tilemap::TILE_NAV_SOURCES) {
                Variant::PackedInt32Array(v) => v,
                _ => Vec::new(),
            };
            // ⚠️ The two polygon families do NOT share an origin, so this is
            // not the collision path with a different key name:
            //
            //   * `__tile_physics_*` points are TILE-CENTRE relative — the
            //     static body is placed at the tile centre
            //     (`gdphysics2d/tilemap_collision.rs:83`) and the points are
            //     used as-is.
            //   * `__tile_nav_polygon__` is TOP-LEFT relative — documented so
            //     at `tilemap.rs:526`, and `tile_navigation_polygons` adds the
            //     cell's top-left corner to every point.
            //
            // Godot stores both centred on the tile, so the navigation polygon
            // has to have half a tile taken off on the way out. Skipping that
            // conversion writes a polygon displaced by half a tile — which
            // still loads, still looks like a navigation mesh, and puts the
            // walkable area over the tile's bottom-right corner.
            let half =
                gdcore::math::Vector2::new(tile_size.x as f32 / 2.0, tile_size.y as f32 / 2.0);
            let nav_local = match node.get_property(crate::tilemap::TILE_NAV_POLYGON) {
                Variant::PackedVector2Array(p) => p,
                _ => Vec::new(),
            };
            let nav_points: Vec<gdcore::math::Vector2> = if nav_local.len() >= 3 {
                nav_local.iter().map(|v| *v - half).collect()
            } else {
                // Godot's default tile navigation polygon is the whole tile,
                // which is also what an empty `__tile_nav_polygon__` means.
                vec![
                    gdcore::math::Vector2::new(-half.x, -half.y),
                    gdcore::math::Vector2::new(half.x, -half.y),
                    gdcore::math::Vector2::new(half.x, half.y),
                    gdcore::math::Vector2::new(-half.x, half.y),
                ]
            };

            // ⚠️ Godot's TileData holds a NavigationPolygon **resource**, not a
            // point array: `TileData::_set` does
            // `Ref<NavigationPolygon> polygon = p_value` (tile_set.cpp:6824), and
            // a PackedVector2Array converts to a NULL Ref — the file loads, no
            // error is raised, and every polygon is silently dropped. Patina's
            // own round trip could not see this: our loader read back exactly
            // what our saver wrote (pat-s3rrv.65).
            //
            // One sub-resource per SOURCE, not per tile, because Patina holds
            // one polygon per layer — every tile of the source shares it. It is
            // emitted BEFORE the atlas source that references it.
            // Occlusion, the same shape as navigation and with the same
            // top-left -> tile-centre rebasing: `__tile_occluder_polygon__` is
            // top-left relative (tilemap.rs) and Godot's TileData polygons are
            // centred (pat-s3rrv.65).
            let occl_sources = match node.get_property(crate::tilemap::TILE_OCCLUDER_SOURCES) {
                Variant::PackedInt32Array(v) => v,
                _ => Vec::new(),
            };
            let occl_local = match node.get_property(crate::tilemap::TILE_OCCLUDER_POLYGON) {
                Variant::PackedVector2Array(p) => p,
                _ => Vec::new(),
            };
            let occl_points: Vec<gdcore::math::Vector2> = if occl_local.len() >= 3 {
                occl_local.iter().map(|v| *v - half).collect()
            } else {
                vec![
                    gdcore::math::Vector2::new(-half.x, -half.y),
                    gdcore::math::Vector2::new(half.x, -half.y),
                    gdcore::math::Vector2::new(half.x, half.y),
                    gdcore::math::Vector2::new(-half.x, half.y),
                ]
            };
            let occl_sub_id = format!("OccluderPolygon2D_{stem}_{source_id}");
            if occl_sources.contains(&source_id) {
                writeln!(
                    text,
                    "\n[sub_resource type=\"OccluderPolygon2D\" id=\"{occl_sub_id}\"]"
                )
                .unwrap();
                writeln!(
                    text,
                    "polygon = {}",
                    format_variant_value(&Variant::PackedVector2Array(occl_points.clone()))
                )
                .unwrap();
                // A tile is solid, so its occluder is a closed polygon rather
                // than a polyline; `closed` defaults true in Godot but is
                // written explicitly because the runtime side asserts it.
                writeln!(text, "closed = true").unwrap();
            }

            let nav_sub_id = format!("NavigationPolygon_{stem}_{source_id}");
            if !nav_sources.is_empty() {
                writeln!(
                    text,
                    "\n[sub_resource type=\"NavigationPolygon\" id=\"{nav_sub_id}\"]"
                )
                .unwrap();
                writeln!(
                    text,
                    "vertices = {}",
                    format_variant_value(&Variant::PackedVector2Array(nav_points.clone()))
                )
                .unwrap();
                // `polygons` is an Array of PackedInt32Array indices into
                // `vertices`: one convex polygon over every vertex in order.
                let indices: Vec<String> = (0..nav_points.len()).map(|i| i.to_string()).collect();
                writeln!(
                    text,
                    "polygons = [PackedInt32Array({})]",
                    indices.join(", ")
                )
                .unwrap();
            }

            writeln!(
                text,
                "\n[sub_resource type=\"TileSetAtlasSource\" id=\"{sub_id}\"]"
            )
            .unwrap();
            writeln!(text, "texture = ExtResource(\"{tex_id}\")").unwrap();
            for key in ["texture_region_size", "margins", "separation"] {
                let value = node.get_property(&crate::tilemap::tile_set_source_key(source_id, key));
                if !value.is_nil() {
                    writeln!(text, "{key} = {}", format_variant_value(&value)).unwrap();
                }
            }

            // Per-tile data. The atlas coordinate lives only inside the
            // companion key's own text, so the tiles are recovered by parsing
            // the keys back — sorted, because a HashMap walk would reshuffle
            // the file on every save.
            let mut tiles: Vec<(gdcore::math::Vector2i, Vec<gdcore::math::Vector2>)> = Vec::new();
            for (name, value) in node.properties() {
                let Some(atlas) = crate::tilemap::tile_physics_points_atlas(name, source_id) else {
                    continue;
                };
                let points = match value {
                    Variant::PackedVector2Array(p) => p.clone(),
                    _ => Vec::new(),
                };
                tiles.push((atlas, points));
            }
            tiles.sort_by_key(|(a, _)| (a.y, a.x));

            // Navigation-enabled tiles for this source (pat-s3rrv.65). A tile
            // may carry navigation WITHOUT collision, so the set is the union
            // of the physics tiles above and the painted cells using this
            // source — deriving it from the physics companions alone would drop
            // every nav-only tile silently.
            let mut nav_tiles: Vec<gdcore::math::Vector2i> = Vec::new();
            if nav_sources.contains(&source_id) {
                let cells = match node.get_property("tile_map_data") {
                    Variant::PackedByteArray(bytes) => crate::tilemap::decode_tile_map_data(&bytes),
                    _ => std::collections::HashMap::new(),
                };
                for data in cells.values() {
                    if data.source_id == source_id && !nav_tiles.contains(&data.atlas_coords) {
                        nav_tiles.push(data.atlas_coords);
                    }
                }
                nav_tiles.sort_by_key(|a| (a.y, a.x));
            }

            let mut occl_tiles: Vec<gdcore::math::Vector2i> = Vec::new();
            if occl_sources.contains(&source_id) {
                let cells = match node.get_property("tile_map_data") {
                    Variant::PackedByteArray(bytes) => crate::tilemap::decode_tile_map_data(&bytes),
                    _ => std::collections::HashMap::new(),
                };
                for data in cells.values() {
                    if data.source_id == source_id && !occl_tiles.contains(&data.atlas_coords) {
                        occl_tiles.push(data.atlas_coords);
                    }
                }
                occl_tiles.sort_by_key(|a| (a.y, a.x));
            }

            // Tiles carrying custom data (pat-s3rrv.65). Built from the
            // PAINTED CELLS like `occl_tiles`, not from the physics companions:
            // custom data is authored per tile and a tile may carry it while
            // being neither solid, walkable nor shadow-casting. Deriving it from
            // `tiles` instead would silently drop exactly those tiles.
            let custom_layer_names = tileset_custom_data_layer_names(node);
            let mut custom_tiles: Vec<gdcore::math::Vector2i> = Vec::new();
            if !custom_layer_names.is_empty() {
                let cells = match node.get_property("tile_map_data") {
                    Variant::PackedByteArray(bytes) => crate::tilemap::decode_tile_map_data(&bytes),
                    _ => std::collections::HashMap::new(),
                };
                for data in cells.values() {
                    if data.source_id != source_id || custom_tiles.contains(&data.atlas_coords) {
                        continue;
                    }
                    let has_any = custom_layer_names.iter().any(|name| {
                        !node
                            .get_property(&crate::tilemap::tile_custom_data_key(
                                source_id,
                                data.atlas_coords,
                                name,
                            ))
                            .is_nil()
                    });
                    if has_any {
                        custom_tiles.push(data.atlas_coords);
                    }
                }
                custom_tiles.sort_by_key(|a| (a.y, a.x));
            }

            let physics_tiles: Vec<gdcore::math::Vector2i> =
                tiles.iter().map(|(a, _)| *a).collect();

            for (atlas, points) in tiles {
                let (ax, ay) = (atlas.x, atlas.y);
                // Godot declares the tile itself before any of its data; without
                // this line the atlas source has no tile at those coordinates
                // and every `<ax>:<ay>/0/...` key below is orphaned.
                writeln!(text, "{ax}:{ay}/0 = 0").unwrap();
                if points.len() >= 3 {
                    writeln!(
                        text,
                        "{ax}:{ay}/0/physics_layer_0/polygon_0/points = {}",
                        format_variant_value(&Variant::PackedVector2Array(points))
                    )
                    .unwrap();
                }
                let one_way =
                    node.get_property(&crate::tilemap::tile_physics_one_way_key(source_id, atlas));
                if let Variant::Bool(b) = one_way {
                    writeln!(text, "{ax}:{ay}/0/physics_layer_0/polygon_0/one_way = {b}").unwrap();
                }

                // ⚠️ Godot keys a tile's custom data by layer INDEX
                // (`custom_data_<i>`, tile_set.cpp:6848) while Patina's
                // companion is keyed by layer NAME, so the enumeration order
                // here IS the mapping — it must match the
                // `custom_data_layer_<i>/name` declarations written above.
                //
                // The value is emitted verbatim; see the loader for why the
                // layer's declared type is deliberately not applied.
                for (index, name) in custom_layer_names.iter().enumerate() {
                    let value = node.get_property(&crate::tilemap::tile_custom_data_key(
                        source_id, atlas, name,
                    ));
                    if value.is_nil() {
                        continue;
                    }
                    writeln!(
                        text,
                        "{ax}:{ay}/0/custom_data_{index} = {}",
                        format_variant_value(&value)
                    )
                    .unwrap();
                }
                if nav_tiles.contains(&atlas) {
                    writeln!(
                        text,
                        "{ax}:{ay}/0/navigation_layer_0/polygon = SubResource(\"{nav_sub_id}\")"
                    )
                    .unwrap();
                }
                if occl_tiles.contains(&atlas) {
                    // ⚠️ THREE components. `occlusion_layer_0/polygon` (two)
                    // also parses, but Godot marks it "Kept for compatibility"
                    // and writes the indexed form — a tile can carry several
                    // occluder polygons per layer where it carries exactly one
                    // navigation polygon.
                    writeln!(
                        text,
                        "{ax}:{ay}/0/occlusion_layer_0/polygon_0/polygon = SubResource(\"{occl_sub_id}\")"
                    )
                    .unwrap();
                }
            }

            // Tiles that carry navigation or occlusion but NOT collision: the
            // loop above walks the physics tiles only, so a tile that is
            // walkable or shadow-casting but not solid has not been declared
            // yet — and an undeclared tile's `<ax>:<ay>/0/...` keys are orphaned
            // on load.
            //
            // ⚠️ One pass over the UNION, not one pass per layer: a tile that is
            // both walkable and occluding would otherwise have its
            // `<ax>:<ay>/0 = 0` declaration written twice.
            let mut layer_only: Vec<gdcore::math::Vector2i> = Vec::new();
            for atlas in nav_tiles
                .iter()
                .chain(occl_tiles.iter())
                .chain(custom_tiles.iter())
            {
                if !physics_tiles.contains(atlas) && !layer_only.contains(atlas) {
                    layer_only.push(*atlas);
                }
            }
            layer_only.sort_by_key(|a| (a.y, a.x));
            for atlas in &layer_only {
                let (ax, ay) = (atlas.x, atlas.y);
                writeln!(text, "{ax}:{ay}/0 = 0").unwrap();
                // Same emission as the physics loop above — a tile reaches this
                // branch precisely because it has no collision, which says
                // nothing about whether it carries custom data.
                for (index, name) in custom_layer_names.iter().enumerate() {
                    let value = node.get_property(&crate::tilemap::tile_custom_data_key(
                        source_id, *atlas, name,
                    ));
                    if value.is_nil() {
                        continue;
                    }
                    writeln!(
                        text,
                        "{ax}:{ay}/0/custom_data_{index} = {}",
                        format_variant_value(&value)
                    )
                    .unwrap();
                }
                if nav_tiles.contains(atlas) {
                    writeln!(
                        text,
                        "{ax}:{ay}/0/navigation_layer_0/polygon = SubResource(\"{nav_sub_id}\")"
                    )
                    .unwrap();
                }
                if occl_tiles.contains(atlas) {
                    writeln!(
                        text,
                        "{ax}:{ay}/0/occlusion_layer_0/polygon_0/polygon = SubResource(\"{occl_sub_id}\")"
                    )
                    .unwrap();
                }
            }

            count += 1;
            source_refs.push((source_id, sub_id));
        }

        if source_refs.is_empty() {
            continue;
        }

        let tileset_id = format!("TileSet_{stem}");
        writeln!(
            text,
            "\n[sub_resource type=\"TileSet\" id=\"{tileset_id}\"]"
        )
        .unwrap();
        writeln!(
            text,
            "tile_size = {}",
            format_variant_value(&Variant::Vector2i(tile_size))
        )
        .unwrap();
        // ⚠️ A tile's collision polygon is stored under `physics_layer_0`, and
        // Godot only reads it when that layer EXISTS. Without this line the
        // polygons are written and then ignored on load — the exact "wired but
        // inert" failure, in the file format rather than in code.
        let has_collision = match node.get_property(crate::tilemap::TILE_PHYSICS_SOURCES) {
            Variant::PackedInt32Array(v) => !v.is_empty(),
            _ => false,
        };
        if has_collision {
            writeln!(text, "physics_layer_0/collision_layer = 1").unwrap();
        }
        // Same rule as the physics layer above: Godot reads a tile's
        // `navigation_layer_0/polygon` only when the navigation layer EXISTS on
        // the TileSet. Without this the polygons are written and then ignored
        // on load (pat-s3rrv.65).
        let has_navigation = match node.get_property(crate::tilemap::TILE_NAV_SOURCES) {
            Variant::PackedInt32Array(v) => !v.is_empty(),
            _ => false,
        };
        if has_navigation {
            writeln!(text, "navigation_layer_0/layers = 1").unwrap();
        }
        // Same rule again: Godot reads a tile's occluder only when the
        // occlusion layer exists on the TileSet. `light_mask` is the layer's
        // own property (tile_set.cpp:4148), not the tile's.
        let has_occlusion = match node.get_property(crate::tilemap::TILE_OCCLUDER_SOURCES) {
            Variant::PackedInt32Array(v) => !v.is_empty(),
            _ => false,
        };
        if has_occlusion {
            writeln!(text, "occlusion_layer_0/light_mask = 1").unwrap();
        }

        // Custom data layers are declared on the TILESET and referenced per
        // tile by index (pat-s3rrv.65). Without these declarations Godot has no
        // layer for a `custom_data_<i>` key to belong to, so the per-tile values
        // written below would be dropped on load — the same rule that makes
        // `physics_layer_0/collision_layer` and `occlusion_layer_0/light_mask`
        // mandatory above.
        //
        // ⚠️ A type is emitted only when the LOADED FILE declared one. A
        // guessed type is worse than an absent one — it would silently coerce
        // the value on load, whereas an absent type leaves the Variant as
        // authored — so nothing here infers a type from a value. `-1`, and any
        // index past the end of the array, means "untyped" and emits no key,
        // which is exactly what an editor-authored TileSet produces.
        //
        // Without this, a `.tscn` written by real Godot lost its
        // `custom_data_layer_<i>/type` declarations on every round trip through
        // Patina: the loader read only `/name`, so the saver could only write
        // `/name` back (pat-s3rrv.65).
        let custom_layer_types = tileset_custom_data_layer_types(node);
        for (index, name) in tileset_custom_data_layer_names(node).iter().enumerate() {
            writeln!(
                text,
                "custom_data_layer_{index}/name = {}",
                format_variant_value(&Variant::String(name.clone()))
            )
            .unwrap();
            if let Some(ty) = custom_layer_types.get(index).copied().filter(|t| *t >= 0) {
                writeln!(text, "custom_data_layer_{index}/type = {ty}").unwrap();
            }
        }
        for (source_id, sub_id) in &source_refs {
            writeln!(text, "sources/{source_id} = SubResource(\"{sub_id}\")").unwrap();
        }
        count += 1;
        nodes.insert(nid, tileset_id);
    }

    TileSetTables {
        text,
        nodes,
        ext: ext
            .into_iter()
            .map(|(id, (res_type, path))| (id, res_type, path, None))
            .collect(),
        count,
    }
}

/// Writes the `tile_set = SubResource("...")` reference for a node whose
/// TileSet was regenerated above.
fn write_tileset_props(out: &mut String, tables: &TileSetTables, nid: NodeId) {
    if let Some(sub_id) = tables.nodes.get(&nid) {
        writeln!(out, "tile_set = SubResource(\"{sub_id}\")").unwrap();
    }
}

/// Properties regenerated from a node's attached player or frame library, which
/// the node writer must therefore NOT also emit from its own parsed copies.
///
/// ⚠️ `sprite_frames` is the subtle one: a node loaded from a scene keeps the
/// raw `SubResource("...")` string in its bag, so writing both that and the
/// regenerated reference would emit the property twice.
fn skip_keys_for(
    anims: &AnimationTables,
    sframes: &SpriteFramesTables,
    tilesets: &TileSetTables,
    nid: NodeId,
) -> Vec<&'static str> {
    let mut keys: Vec<&'static str> = anims.skip_keys(nid).to_vec();
    // Same hazard as `sprite_frames`: a TileMapLayer loaded from a scene keeps
    // the raw `SubResource("…")` string in its bag, so writing both that and
    // the regenerated reference would emit `tile_set` twice.
    if tilesets.nodes.contains_key(&nid) {
        keys.push("tile_set");
    }
    if sframes.nodes.contains_key(&nid) {
        keys.push("sprite_frames");
    }
    keys
}

/// Writes the `sprite_frames = SubResource("...")` reference for a node whose
/// frames were regenerated above.
fn write_spriteframes_props(out: &mut String, sf: &SpriteFramesTables, nid: NodeId) {
    if let Some(sub_id) = sf.nodes.get(&nid) {
        writeln!(out, "sprite_frames = SubResource(\"{sub_id}\")").unwrap();
    }
}

/// Godot's `Animation.LoopMode` integer.
fn loop_mode_to_int(mode: crate::animation::LoopMode) -> i64 {
    match mode {
        crate::animation::LoopMode::None => 0,
        crate::animation::LoopMode::Linear => 1,
        crate::animation::LoopMode::PingPong => 2,
    }
}

/// The `tracks/N/type` spelling, inverse of `animation_tscn::tscn_track_type`.
fn track_type_to_tscn(t: crate::animation::TrackType) -> Option<&'static str> {
    use crate::animation::TrackType;
    Some(match t {
        TrackType::Property => "value",
        TrackType::Method => "method",
        TrackType::Audio => "audio",
        TrackType::Position3D => "position_3d",
        TrackType::Rotation3D => "rotation_3d",
        TrackType::Scale3D => "scale_3d",
        TrackType::BlendShape => "blend_shape",
        TrackType::Bezier => "bezier",
        TrackType::Animation => "animation",
    })
}

/// Godot's `Animation.InterpolationType` integer
/// (`0 = NEAREST, 1 = LINEAR, 2 = CUBIC, 3 = LINEAR_ANGLE, 4 = CUBIC_ANGLE`).
fn interpolation_to_int(i: crate::animation::TrackInterpolation) -> i64 {
    use crate::animation::TrackInterpolation;
    match i {
        // `PerKey` is this port's default — "use each keyframe's own transition"
        // — which Godot has no spelling for. LINEAR is the honest text form: it
        // is what a default-transition keyframe actually interpolates as, and it
        // is what the parser hands back on re-load.
        TrackInterpolation::PerKey => 1,
        TrackInterpolation::Cubic => 2,
        TrackInterpolation::LinearAngle => 3,
        TrackInterpolation::CubicAngle => 4,
    }
}

/// Godot's `Animation.UpdateMode` integer.
fn update_mode_to_int(m: crate::animation::TrackUpdateMode) -> i64 {
    use crate::animation::TrackUpdateMode;
    match m {
        TrackUpdateMode::Continuous => 0,
        TrackUpdateMode::Discrete => 1,
        TrackUpdateMode::Capture => 3,
    }
}

/// `stem`, or `stem_2`, `stem_3`, ... when another node of the scene already
/// took it: ids built from node NAMES collided for two same-named nodes (two
/// moving platforms' `AnimationPlayer`s both wrote
/// `Animation_AnimationPlayer_move`), and a re-load kept only one of them.
fn unique_stem(used: &mut std::collections::HashSet<String>, stem: String) -> String {
    let mut candidate = stem.clone();
    let mut n = 2;
    while !used.insert(candidate.clone()) {
        candidate = format!("{stem}_{n}");
        n += 1;
    }
    candidate
}

/// Reduces a name to the `[A-Za-z0-9_]` set Godot uses for sub-resource ids.
fn sanitize_resource_id(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// Formats an f64 without unnecessary trailing zeros, but always with
/// a decimal point to distinguish from integers.
fn format_float(f: f64) -> String {
    if f.fract() == 0.0 {
        format!("{f:.1}")
    } else {
        format!("{f}")
    }
}

/// Formats an f32 cleanly — whole numbers have no decimal point.
fn format_f32(f: f32) -> String {
    if f.fract() == 0.0 {
        format!("{f:.0}")
    } else {
        format!("{f}")
    }
}

// ---------------------------------------------------------------------------
// TscnSaver
// ---------------------------------------------------------------------------

/// What a scene opened for editing is saved against (see
/// [`TscnSaver::save_tree`]): the file it was read from, and the generator's
/// output for the tree at that moment.
#[derive(Debug, Clone)]
pub struct SaveBaseline {
    pub original: String,
    pub generated: String,
}

/// Saves a [`SceneTree`] subtree as `.tscn` text.
pub struct TscnSaver;

impl TscnSaver {
    /// Serializes the subtree rooted at `root_id` to `.tscn` format.
    ///
    /// The root node is written without a `parent` attribute. All
    /// descendants get a relative `parent` path from the subtree root.
    /// Signal connections from the tree's signal stores are appended as
    /// `[connection]` sections.
    ///
    /// 🔑 A scene opened FOR EDITING carries a [`SaveBaseline`]: the file it
    /// was read from and what [`generate`](Self::generate) produced for it then
    /// (pat-i5igl). Such a scene is saved as an edit of that file
    /// ([`crate::tscn_merge::merge_edits`]): an unedited scene saves
    /// byte-for-byte as it was read, and an edit rewrites only what changed.
    /// Without this, everything the generator cannot rebuild from the node
    /// property bags was lost on save — coin.tscn's `GradientTexture2D ->
    /// Gradient` came back as a bare reference, the `uid`s and `unique_id`s
    /// were dropped, and an instance's untouched foreign nodes were written
    /// out as override entries.
    pub fn save_tree(tree: &SceneTree, root_id: NodeId) -> String {
        let generated = Self::generate(tree, root_id);
        match tree.save_baseline(root_id) {
            Some(b) => crate::tscn_merge::merge_edits(&b.original, &b.generated, &generated),
            None => generated,
        }
    }

    /// Records that `root_id`'s scene is, as of now, the file `original`, so
    /// later [`save_tree`](Self::save_tree) calls save edits of it. Call it
    /// after opening a scene (once every pass that settles the tree has run)
    /// and after every save.
    pub fn capture_baseline(tree: &mut SceneTree, root_id: NodeId, original: String) {
        let generated = Self::generate(tree, root_id);
        tree.set_save_baseline(
            root_id,
            Some(SaveBaseline {
                original,
                generated,
            }),
        );
    }

    /// The `.tscn` text rebuilt from the tree alone, ignoring any baseline:
    /// what [`save_tree`](Self::save_tree) writes for a scene that was never
    /// read from a file.
    pub fn generate(tree: &SceneTree, root_id: NodeId) -> String {
        // Collect subtree in depth-first (top-down) order.
        let mut node_ids = Vec::new();
        tree.collect_subtree_top_down(root_id, &mut node_ids);
        // An instance's nodes belong to the INSTANCED scene's file. Godot
        // writes none of them unless the instance has Editable Children
        // (`SceneState::_parse_node` only visits a foreign node through an
        // editable instance), so neither do we — before this every instanced
        // coin's whole subtree, AnimationPlayer libraries included, was written
        // into level.tscn, all under the same colliding sub-resource ids.
        node_ids.retain(|&id| !is_hidden_foreign(tree, root_id, id));

        // Reconstruct the external/inline resource tables from node references
        // and `__resref__`/`_instance_path`/`_script_path` companions so the
        // saved scene declares them the way Godot's text saver does.
        let tables = collect_resource_tables(tree, &node_ids);
        // Every AnimationPlayer's clips become `Animation` sub-resources plus one
        // `AnimationLibrary` that gathers them, exactly as Godot's text saver
        // writes them (pat-0q2kx.54). Before this, animations authored in the
        // editor existed only in memory: saving a scene dropped them silently.
        let anims = collect_animation_tables(tree, &node_ids);
        // Every AnimatedSprite2D's authored frames become a `SpriteFrames`
        // sub-resource plus `[ext_resource]` rows for its textures, matching
        // Godot's shape (pat-0q2kx.74). Without this the editor's `__sf_*`
        // companion keys would be written verbatim as properties Godot has
        // never heard of.
        let sframes = collect_spriteframes_tables(tree, &node_ids);
        // Every TileMapLayer's authored TileSet becomes a real `TileSet` +
        // `TileSetAtlasSource` sub-resource pair (pat-s3rrv.65). Before this an
        // editor-authored TileSet existed only in memory: the `__tile_set__*`
        // companions are dropped as internal, and there was no `tile_set`
        // reference to fall back on, so saving lost the whole thing.
        let tilesets = collect_tileset_tables(tree, &node_ids);

        let mut out = String::new();
        // Header. Godot emits `load_steps` = (ext + sub resources) + 1 whenever
        // any resources exist; a resource-free scene keeps the bare header.
        let resource_count = tables.ext.len()
            + tables.sub.len()
            + anims.resource_count()
            + sframes.resource_count()
            + sframes.ext.len()
            + tilesets.resource_count()
            + tilesets.ext.len();
        if resource_count > 0 {
            writeln!(out, "[gd_scene load_steps={} format=3]", resource_count + 1).unwrap();
        } else {
            writeln!(out, "[gd_scene format=3]").unwrap();
        }

        // External resource table. Frame textures join the rows the node
        // properties contributed; `or_insert` semantics upstream mean an id
        // already present is not duplicated here either.
        let mut ext_rows: Vec<&(String, String, String, Option<String>)> =
            tables.ext.iter().collect();
        for row in &sframes.ext {
            if !ext_rows.iter().any(|(id, _, _, _)| id == &row.0) {
                ext_rows.push(row);
            }
        }
        for row in &tilesets.ext {
            if !ext_rows.iter().any(|(id, _, _, _)| id == &row.0) {
                ext_rows.push(row);
            }
        }
        if !ext_rows.is_empty() {
            out.push('\n');
            for (id, res_type, path, uid) in &ext_rows {
                // Godot writes `uid=` between `type=` and `path=`, and omits it
                // when the resource has none (pat-0q2kx.137).
                let uid_attr = match uid {
                    Some(u) if !u.is_empty() => format!(" uid=\"{u}\""),
                    _ => String::new(),
                };
                writeln!(
                    out,
                    "[ext_resource type=\"{res_type}\"{uid_attr} path=\"{path}\" id=\"{id}\"]"
                )
                .unwrap();
            }
        }
        // Inline sub-resource sections.
        let sub_bodies = collect_sub_resource_bodies(tree, &node_ids);
        for (id, class) in &tables.sub {
            writeln!(out, "\n[sub_resource type=\"{class}\" id=\"{id}\"]").unwrap();
            if let Some(props) = sub_bodies.get(id) {
                for (key, value) in props {
                    writeln!(out, "{key} = {}", format_variant_value(value)).unwrap();
                }
            }
        }

        // Animation + AnimationLibrary sub-resources, written before the node
        // sections so the players' `libraries` references resolve on re-load.
        out.push_str(&anims.text);
        out.push_str(&sframes.text);
        out.push_str(&tilesets.text);

        // What each instanced scene itself says, to write only overrides.
        let mut instance_bases = InstanceBases::default();

        // Build a map from NodeId -> relative path within the subtree.
        let mut id_to_path: HashMap<NodeId, String> = HashMap::new();
        id_to_path.insert(root_id, ".".to_string());

        for &nid in &node_ids {
            let node = match tree.get_node(nid) {
                Some(n) => n,
                None => continue,
            };

            if nid == root_id {
                // Root node — no parent attribute.
                write_node_section(
                    &mut out,
                    node,
                    None,
                    false,
                    &skip_keys_for(&anims, &sframes, &tilesets, nid),
                    None,
                );
                write_animation_player_props(&mut out, &anims, nid);
                write_spriteframes_props(&mut out, &sframes, nid);
                write_tileset_props(&mut out, &tilesets, nid);
            } else {
                // Compute parent path.
                let parent_id = match node.parent() {
                    Some(pid) => pid,
                    None => continue,
                };
                let parent_path = id_to_path.get(&parent_id).cloned().unwrap_or_default();

                // Compute this node's path for its children.
                let node_path = if parent_path == "." {
                    node.name().to_string()
                } else {
                    format!("{}/{}", parent_path, node.name())
                };
                id_to_path.insert(nid, node_path);

                // A node an INSTANCED scene declared is written as an override
                // entry — no `type=`, no `instance=` — as Godot writes an
                // editable child (pat-hivyf).
                let foreign = crate::packed_scene::is_foreign_node(tree, root_id, nid);
                let base = instance_bases.base_of(tree, root_id, nid);
                write_node_section(
                    &mut out,
                    node,
                    Some(&parent_path),
                    foreign,
                    &skip_keys_for(&anims, &sframes, &tilesets, nid),
                    base,
                );
                write_animation_player_props(&mut out, &anims, nid);
                write_spriteframes_props(&mut out, &sframes, nid);
                write_tileset_props(&mut out, &tilesets, nid);
            }
        }

        // Write connections from signal stores.
        write_connections(&mut out, tree, root_id, &node_ids, &id_to_path);

        // `[editable path=".."]` for every instance whose children are exposed
        // ("Editable Children"), after the connections as Godot writes them.
        // Without it a re-load hid the overrides' nodes again (pat-hivyf).
        for &nid in &node_ids {
            if nid == root_id {
                continue;
            }
            let editable = tree.get_node(nid).is_some_and(|n| {
                matches!(n.get_property("_editable_children"), Variant::Bool(true))
            });
            if editable {
                if let Some(path) = id_to_path.get(&nid) {
                    write!(out, "\n[editable path=\"{path}\"]\n").unwrap();
                }
            }
        }

        undeclared_references_back_to_strings(out)
    }

    /// Serializes only a specific subtree (not necessarily the scene root).
    ///
    /// This is equivalent to [`save_tree`](Self::save_tree) — the given
    /// `subtree_root` is treated as the root of the output `.tscn`.
    pub fn save_subtree(tree: &SceneTree, subtree_root: NodeId) -> String {
        Self::save_tree(tree, subtree_root)
    }
}

/// A `SubResource("id")` the text never declares (a nested sub-resource whose
/// body the tree did not keep) would not load; it is written back as the
/// `"SubResource:id"` string the property bag holds, as before.
fn undeclared_references_back_to_strings(text: String) -> String {
    let declared: std::collections::HashSet<String> = text
        .lines()
        .filter(|l| l.starts_with("[sub_resource "))
        .filter_map(|l| {
            let at = l.find(" id=\"")? + 5;
            let end = l[at..].find('"')?;
            Some(l[at..at + end].to_string())
        })
        .collect();
    let needle = "SubResource(\"";
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(at) = rest.find(needle) {
        out.push_str(&rest[..at]);
        let after = &rest[at + needle.len()..];
        match after.find("\")") {
            Some(end) if !declared.contains(&after[..end]) => {
                out.push_str(&format!("\"SubResource:{}\"", &after[..end]));
                rest = &after[end + 2..];
            }
            _ => {
                out.push_str(needle);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The instanced scenes of one save, each opened once into a scratch tree, so
/// a node of an instance can be compared with what its scene says.
#[derive(Default)]
struct InstanceBases {
    scenes: HashMap<String, Option<(SceneTree, NodeId)>>,
}

impl InstanceBases {
    /// The node `id` stands for in its instance's own scene, if `id` belongs
    /// to an instance of a `.tscn` that can be read.
    fn base_of(
        &mut self,
        tree: &SceneTree,
        scene_root: NodeId,
        id: NodeId,
    ) -> Option<&crate::node::Node> {
        let scope = instance_scope(tree, scene_root, id)?;
        let path = match tree.get_node(scope)?.get_property("_instance_path") {
            Variant::String(p) if p.ends_with(".tscn") => p,
            _ => return None,
        };
        let rel = relative_path(tree, scope, id)?;
        let entry = self.scenes.entry(path.clone()).or_insert_with(|| {
            let file = gdcore::resolve_path(&path).ok()?;
            let text = std::fs::read_to_string(file).ok()?;
            let scene = crate::packed_scene::PackedScene::from_tscn(&text).ok()?;
            let mut scratch = SceneTree::new();
            let r = scratch.root_id();
            let root =
                crate::packed_scene::add_packed_scene_to_tree_from_res(&mut scratch, r, &scene)
                    .ok()?;
            Some((scratch, root))
        });
        let (scratch, root) = entry.as_ref()?;
        let at = if rel == "." {
            Some(*root)
        } else {
            scratch.get_node_relative(*root, &rel)
        };
        scratch.get_node(at?)
    }
}

/// `id`'s path below `ancestor` (`.` for the ancestor itself).
fn relative_path(tree: &SceneTree, ancestor: NodeId, id: NodeId) -> Option<String> {
    let mut parts = Vec::new();
    let mut cursor = id;
    while cursor != ancestor {
        let node = tree.get_node(cursor)?;
        parts.push(node.name().to_string());
        cursor = node.parent()?;
    }
    if parts.is_empty() {
        return Some(".".to_string());
    }
    parts.reverse();
    Some(parts.join("/"))
}

/// Whether `id` is a node of an instanced scene that the edited scene's file
/// does not show: it lies inside an instance whose children are not editable.
fn is_hidden_foreign(tree: &SceneTree, scene_root: NodeId, id: NodeId) -> bool {
    if !crate::packed_scene::is_foreign_node(tree, scene_root, id) {
        return false;
    }
    let mut cursor = tree.get_node(id).and_then(|n| n.parent());
    while let Some(p) = cursor {
        if p == scene_root {
            break;
        }
        let Some(node) = tree.get_node(p) else { break };
        if !node.get_property("_instance_path").is_nil()
            && !matches!(node.get_property("_editable_children"), Variant::Bool(true))
        {
            return true;
        }
        cursor = node.parent();
    }
    false
}

/// The instance a node belongs to: the outermost instance root (a scene-own
/// node with `instance=`) that is `id` itself or contains it as a foreign
/// node, or `None` for a node the edited scene declares itself.
fn instance_scope(tree: &SceneTree, scene_root: NodeId, id: NodeId) -> Option<NodeId> {
    let foreign = crate::packed_scene::is_foreign_node(tree, scene_root, id);
    let mut cursor = Some(id);
    while let Some(c) = cursor {
        if c == scene_root {
            return None;
        }
        let node = tree.get_node(c)?;
        let is_instance = !node.get_property("_instance_path").is_nil();
        if is_instance && !crate::packed_scene::is_foreign_node(tree, scene_root, c) {
            return (c == id || foreign).then_some(c);
        }
        if !foreign {
            return None;
        }
        cursor = node.parent();
    }
    None
}

/// The `uid://...` the loader cached for `property`'s `ExtResource` reference,
/// if the scene declared one (pat-0q2kx.137).
///
/// `script` and `_instance` keep their path under `_script_path`/`_instance_path`
/// rather than `__extres__`, but the uid companion is keyed by the property name
/// in every case.
fn ext_uid_companion(node: &crate::node::Node, property: &str) -> Option<String> {
    match node.get_property(&format!("__extuid__{property}")) {
        Variant::String(u) if !u.is_empty() => Some(u),
        _ => None,
    }
}

/// External and inline resource declarations reconstructed for a saved scene.
struct ResourceTables {
    /// External resources as `(id, res_type, path, uid)`, ordered by id
    /// (pat-0q2kx.137 added the uid).
    ext: Vec<(String, String, String, Option<String>)>,
    /// Inline sub-resources as `(id, class)`, ordered by id.
    sub: Vec<(String, String)>,
}

/// The node-companion prefix that caches a sub-resource's properties, by the
/// property that references it.
///
/// The loader copies an inline sub-resource's whole property bag onto the node
/// under a namespaced prefix so the runtime can reach it after the `PackedScene`
/// is gone (`packed_scene.rs`). That cache is also the only surviving record of
/// the sub-resource's contents at save time.
pub fn companion_prefix_for(property: &str) -> Option<String> {
    // Per-surface overrides beyond surface 0 get an INDEXED prefix, so each
    // surface's material rebuilds into its own `[sub_resource]` body
    // (pat-s3rrv.187). Surface 0 deliberately stays on `__material__`: it is
    // the one the mesh-level priority already flattens there, and moving it
    // would orphan every scene saved before this.
    if let Some(rest) = property.strip_prefix("surface_material_override/") {
        if let Ok(idx) = rest.parse::<usize>() {
            if idx > 0 {
                return Some(format!("__surfmat{idx}__"));
            }
        }
    }
    match property {
        "shape" => Some("__shape__".to_string()),
        "curve" => Some("__curve__".to_string()),
        "material" | "material_override" | "surface_material_override/0" => {
            Some("__material__".to_string())
        }
        _ => None,
    }
}

/// Rebuilds each inline sub-resource's body from the node companions caching it.
///
/// ⚠️ Before this, `[sub_resource type="X" id="Y"]` was written as a bare header
/// with NO properties, so a RectangleShape2D's `size` — or any authored shape
/// geometry — was silently dropped on every save (pat-0q2kx.77). The header
/// alone looks right in a diff, which is why it survived so long.
///
/// `class` is skipped: it is the cache's own bookkeeping and is already the
/// section's `type=`.
fn collect_sub_resource_bodies(
    tree: &SceneTree,
    node_ids: &[NodeId],
) -> BTreeMap<String, Vec<(String, Variant)>> {
    let mut bodies: BTreeMap<String, Vec<(String, Variant)>> = BTreeMap::new();
    for &nid in node_ids {
        let Some(node) = tree.get_node(nid) else {
            continue;
        };
        for (k, v) in node.properties() {
            let Some(property) = k.strip_prefix("__resref__") else {
                continue;
            };
            let Variant::String(meta) = v else { continue };
            let Some((_class, id)) = meta.rsplit_once("::") else {
                continue;
            };
            // A specialised companion still wins where one exists, so every
            // path that worked before this behaves identically (pat-s3rrv.207).
            //
            // ⚠️ That precedence is deliberate and was nearly the other way
            // round. The specialised companions are LIVE: the editor edits
            // `__material__*` in place, so they carry the user's changes, while
            // `__subres__*` is a snapshot of what the file said at load.
            // Preferring the snapshot would have written stale values back and
            // silently discarded every edit to a material or a shape.
            //
            // 🔑 The fallback is what preserves an UNKNOWN class:
            // `companion_prefix_for` is an allowlist of four property names, so
            // before this every other sub-resource was written as a section
            // header with an empty body and its data dropped silently.
            let prefix = match companion_prefix_for(property) {
                Some(prefix) => prefix,
                None => {
                    let verbatim = format!("__subres__{property}__");
                    if !node
                        .properties()
                        .any(|(ck, _)| ck.starts_with(verbatim.as_str()))
                    {
                        continue;
                    }
                    verbatim
                }
            };
            let mut props: Vec<(String, Variant)> = node
                .properties()
                .filter_map(|(ck, cv)| {
                    let key = ck.strip_prefix(prefix.as_str())?;
                    // A Nil companion is an unset value — the editor clears a
                    // replaced resource's keys to Nil so Undo can restore them.
                    if key == "class" || cv.is_nil() {
                        return None;
                    }
                    Some((key.to_string(), cv.clone()))
                })
                .collect();
            // Deterministic order, so re-saving an unchanged scene is a no-op.
            props.sort_by(|a, b| a.0.cmp(&b.0));
            if !props.is_empty() {
                bodies.insert(id.to_string(), props);
            }
        }
    }
    bodies
}

/// Extracts the id from an `ExtResource("id")` reference string.
fn parse_ext_resource_id(s: &str) -> Option<String> {
    let inner = s.strip_prefix("ExtResource(")?.strip_suffix(')')?.trim();
    let inner = inner.strip_prefix('"')?.strip_suffix('"')?;
    Some(inner.to_string())
}

/// The custom data layer NAMES declared on a TileMapLayer's flattened TileSet.
///
/// Accepts both spellings for the same reason the editor's reader does: the
/// route writes a `PackedStringArray`, but a `.tscn` round trip can land the
/// same list as a generic `Array` of strings.
fn tileset_custom_data_layer_names(node: &crate::node::Node) -> Vec<String> {
    match node.get_property(crate::tilemap::TILE_CUSTOM_DATA_LAYERS) {
        Variant::PackedStringArray(v) => v,
        Variant::Array(items) => items
            .to_vec()
            .iter()
            .filter_map(|v| match v {
                Variant::String(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The declared Variant type of each custom data layer, parallel to
/// [`tileset_custom_data_layer_names`]. `-1` means the layer carries no
/// declared type, and a shorter array simply leaves later layers untyped.
///
/// ⚠️ Never inferred from a value — see the call site. This reads back only
/// what the loader preserved from the file.
fn tileset_custom_data_layer_types(node: &crate::node::Node) -> Vec<i32> {
    match node.get_property(crate::tilemap::TILE_CUSTOM_DATA_TYPES) {
        Variant::PackedInt32Array(v) => v,
        Variant::PackedInt64Array(v) => v.iter().map(|t| *t as i32).collect(),
        Variant::Array(items) => items
            .to_vec()
            .iter()
            .map(|v| match v {
                Variant::Int(t) => *t as i32,
                _ => -1,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Whether `key` is engine plumbing kept on a node — a resource cache, an
/// instance marker, runtime state — rather than a property Godot has. The
/// saver never writes one and the inspector never lists one.
pub fn is_internal_property(key: &str) -> bool {
    is_internal_prop(key)
}

/// Property keys that are internal companions, never emitted as scene
/// properties (they are re-encoded as resource references / header attributes).
fn is_internal_prop(key: &str) -> bool {
    key == "signal_connections"
        // The editor used to record signal connections as a `,`-joined string
        // in this property instead of in the tree's signal store
        // (pat-mgixk.13). Nothing at runtime ever read it, so a connection made
        // in the editor never fired — and because `is_internal_prop` did not
        // know the key, the saver wrote it into the `.tscn` as a stray
        // `signal_connections = "pressed:_on_pressed"` property that Godot has
        // never heard of.
        //
        // Connections now go through `SceneTree::connect_signal` and are
        // emitted as real `[connection]` sections below. Filtering the key here
        // is what stops an ALREADY-SAVED scene from carrying it forward: the
        // loader still reads the property into the node, so without this a
        // round trip would preserve it forever.
        || key.starts_with("__resref__")
        // `__shortcut__events` / `__shortcut__actions` cache a Shortcut
        // resource's accelerators for input-time matching (shortcut.rs). The
        // node's own `shortcut` keeps the `SubResource` reference that
        // round-trips; writing the caches would invent properties Godot has
        // never heard of.
        || key.starts_with("__shortcut__")
        // The rect the Control layout last wrote (control.rs).
        || key == crate::control::LAYOUT_RECT_KEY
        // `__resscript__<prop>` caches the script path of a SCRIPTED resource
        // held in `<prop>`, so render code can reach it with only the tree in
        // hand (pat-s3rrv.160). Derived from the sub-resource on load, so
        // writing it back would put engine plumbing into the .tscn.
        || key.starts_with("__resscript__")
        || key.starts_with("__viewport_")
        // `__shape__*` caches a collision shape sub-resource's class and typed
        // geometry for the physics layer (pat-0q2kx.28). The node's own `shape`
        // property still carries the `SubResource` reference that round-trips;
        // emitting the cache would invent properties Godot has never heard of.
        || key.starts_with("__shape__")
        // `__extres__<prop>` caches the path an `ExtResource("id")` property
        // resolves to (pat-0q2kx.6). The property itself still carries the raw
        // reference, which is what round-trips; emitting the cache would invent
        // a property Godot has never heard of.
        || key.starts_with("__extres__")
        // `__mattex__<slot>`: a generated texture's descriptor (pat-cezo4),
        // derived from the material's `SubResource` texture on load.
        || key.starts_with("__mattex__")
        // `__object_*__` holds the Object API surface's per-node runtime state —
        // the `set_block_signals` flag, the `add_user_signal` name list, and the
        // translation enable/domain (pat-mgixk.99). All four are RUNTIME state a
        // script sets, not authored scene data: Godot persists none of them, and
        // `is_internal_prop` is a prefix ALLOWLIST, so without this entry a
        // scene re-saved after a script called `set_block_signals(true)` would
        // carry a literal `__object_block_signals__ = true` property that Godot
        // cannot parse — the same corruption recorded for `__tile_physics_*`
        // and `signal_connections` above.
        || key.starts_with("__object_")
        // `__extuid__<prop>` is the same cache for the reference's `uid://`
        // (pat-0q2kx.137). It is re-emitted on the `[ext_resource]` row, so
        // writing it as a node property too would invent one Godot cannot parse.
        || key.starts_with("__extuid__")
        // `__sf_*` holds the AnimatedSprite2D frame library the editor authors
        // into the flat property bag (pat-0q2kx.74). It is regenerated above as
        // a real `SpriteFrames` sub-resource, so emitting the raw keys too would
        // both duplicate the data and invent properties Godot cannot parse.
        || key.starts_with("__sf_")
        // `__tile_set__*` caches the resolved TileSet — its cell size and each
        // atlas source's texture and region grid — so the renderer can turn a
        // `tile_map_data` cell into a texture region without the `PackedScene`
        // (pat-0q2kx.108). The node's `tile_set` property still carries the
        // `SubResource` reference that round-trips; the cache itself is not a
        // property Godot has ever heard of. `tile_map_data` is NOT a companion
        // and is written back verbatim, which is what makes a painted layer
        // survive a save.
        || key.starts_with("__tile_set__")
        // The per-tile companions are the same kind of cache and were NOT
        // filtered (pat-s3rrv.65): any scene with tile collision re-saved with
        // `__tile_physics_0_0_0_points = PackedVector2Array(…)` as a literal
        // node property, which Godot cannot parse. They are regenerated above
        // as real `TileSetAtlasSource` keys, so emitting the raw ones would
        // both duplicate the data and corrupt the file.
        || key.starts_with(crate::tilemap::TILE_PHYSICS_PREFIX)
        || key == crate::tilemap::TILE_PHYSICS_SOURCES
        || key.starts_with(crate::tilemap::TILE_CUSTOM_DATA_PREFIX)
        || key == crate::tilemap::TILE_CUSTOM_DATA_LAYERS
        || key.starts_with("__tile_nav_")
        // ⚠️ The occlusion companions were missed when the others were filtered
        // (pat-s3rrv.65). `__tile_occluder_sources__` and
        // `__tile_occluder_polygon__` are the same kind of cache and are
        // regenerated above as a real `OccluderPolygon2D` sub-resource plus
        // `occlusion_layer_0/polygon_0/polygon`, so writing the raw keys too
        // both duplicates the data and emits properties Godot cannot parse —
        // exactly the corruption described for `__tile_physics_*`.
        //
        // It also made the whole-tile default explicit: the raw key round-tripped
        // an empty `PackedVector2Array` back onto the node, so an untouched tile
        // came back looking edited instead of absent.
        || key.starts_with("__tile_occluder_")
        || key == "_instance"
        || key == "_instance_path"
        // Runtime marker: the file a script-instantiated scene came from
        // (pat-i4jgd), never a property line.
        || key == crate::world_environment::SCENE_FILE_PATH_KEY
        // Editor-only instance markers: "Editable Children" is written as an
        // `[editable path=..]` section and the dock's source label is the
        // instance's own `instance=` (pat-hivyf), never as property lines.
        || key == "_editable_children"
        || key == "_instance_source"
        // Runtime marker set when a Play tree expands an `instance=`
        // placeholder (`packed_scene::expand_instance_placeholders`).
        || key == crate::packed_scene::INSTANCE_EXPANDED_KEY
        || key == "_script_path"
        || key == crate::packed_scene::PROPS_BEFORE_SCRIPT_KEY
        // `_camera2d_scroll_center` / `_camera2d_scroll_rotation` are the
        // Camera2D's running scroll state, republished every frame by
        // `behavioral_2d::process_camera2d_scroll` (pat-s3rrv.84). They are
        // frame state, not authored data — writing them would invent properties
        // Godot has never heard of, and would pin a saved scene to whatever
        // centre the camera happened to be easing through when it was saved.
        || key.starts_with("_camera2d_scroll_")
        // `__gradient_offsets` / `__gradient_colors` / `__width_curve_samples`
        // cache a Line2D's `gradient` and `width_curve` sub-resources, flattened
        // onto the node so the rasterizer can reach them (pat-s3rrv.85). The
        // node's own `gradient` / `width_curve` properties still carry the
        // `SubResource` references that round-trip; emitting the caches too
        // would invent properties Godot has never heard of, exactly as for
        // `__shape__*` above — and `__width_curve_samples` would additionally
        // write a 101-entry baked table in place of the four authored points.
        || key.starts_with("__gradient_")
        || key.starts_with("__width_curve_")
        // `__surfmat<N>__*` caches a per-surface material override's body so it
        // can be rebuilt into a `[sub_resource]` on save (pat-s3rrv.187). The
        // node's own `surface_material_override/N` still carries the reference
        // that round-trips; emitting the cache too would invent properties Godot
        // has never heard of, exactly as for `__shape__*` above.
        || key.starts_with("__surfmat")
        // ⚠️ FOUR MORE that were never filtered, found while auditing this list
        // for pat-s3rrv.207 — the same omission its own comments record for
        // `__tile_physics_*` and `__tile_occluder_*`, twice. Every one of these
        // is a sub-resource body flattened onto the node so a render or layout
        // path can reach it, and every one was being written back into the
        // `[node]` section as a literal property Godot cannot parse:
        //
        //   `__multimesh__*`      MultiMesh body          (pat-s3rrv.112)
        //   `__ls__*`             LabelSettings body      (pat-0q2kx.*)
        //   `__click_mask__*`     TextureButton BitMap    (pat-0q2kx.257)
        //   `__skin__*`           Skin binds              (pat-s3rrv.200)
        //   `__canvas_texture__*` CanvasTexture channels  (pat-s3rrv.203)
        //
        // In each case the node's own property still carries the `SubResource`
        // reference that round-trips, so the cache is pure engine plumbing —
        // exactly the argument made for `__shape__*` above.
        || key.starts_with("__multimesh__")
        || key.starts_with("__ls__")
        || key.starts_with("__label_settings__")
        || key.starts_with("__click_mask__")
        || key.starts_with("__skin__")
        || key.starts_with("__canvas_texture__")
        // `__subres__<prop>__<key>` is the verbatim body of ANY inline
        // sub-resource, kept so the saver can write it back under its original
        // class even when the engine has no idea what that class is
        // (pat-s3rrv.207). It is rebuilt into a `[sub_resource]` section, so
        // emitting it as a node property too would duplicate the data.
        || key.starts_with("__subres__")
        // The allowlist above missed `__material__*`, `__mesh__*`,
        // `__physics_material__*` and `__canvas_material__*` (sub-resource
        // bodies flattened onto MeshInstance3D / CollisionObject / CanvasItem
        // nodes), so every save of a 3D scene wrote them as node properties
        // Godot has never heard of (pat-l0afa). No Godot storage property
        // starts with `__`, so every such key is engine plumbing.
        || key.starts_with("__")
}

/// The `type=` an external resource is declared as in a `[ext_resource]` header,
/// inferred from the file's extension the way Godot's importer assigns one.
/// Anything unrecognised is a plain `Resource`, which is what Godot writes for a
/// `.tres` whose class it cannot infer from the path alone.
fn ext_resource_type_for(path: &str) -> String {
    let ext = match path.rsplit_once('.') {
        Some((_, ext)) => ext.to_ascii_lowercase(),
        None => String::new(),
    };
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "svg" | "bmp" | "tga" | "ktx" => "Texture2D",
        "wav" | "ogg" | "mp3" => "AudioStream",
        "tscn" | "scn" => "PackedScene",
        "gd" => "Script",
        "gdshader" | "glsl" => "Shader",
        "ttf" | "otf" | "woff" | "woff2" => "FontFile",
        "obj" | "gltf" | "glb" => "Mesh",
        _ => "Resource",
    }
    .to_string()
}

/// Walks the subtree and reconstructs the ext/sub resource tables from node
/// references. Script paths come from `_script_path`, instanced sub-scene paths
/// from `_instance_path`, any other external reference from its
/// `__extres__<prop>` companion, and inline sub-resources from `__resref__<prop>`
/// companions (which carry `class::id`). Ids are preserved from the original
/// `ExtResource("id")` / `SubResource("id")` references so a save/load round-trip
/// is stable.
fn collect_resource_tables(tree: &SceneTree, node_ids: &[NodeId]) -> ResourceTables {
    let mut ext_by_id: BTreeMap<String, (String, String, Option<String>)> = BTreeMap::new();
    let mut sub_by_id: BTreeMap<String, String> = BTreeMap::new();

    for &nid in node_ids {
        let node = match tree.get_node(nid) {
            Some(n) => n,
            None => continue,
        };

        // Script external resource: raw `script = ExtResource("id")` reference
        // paired with the resolved `_script_path` companion.
        if let (Variant::String(script_ref), Variant::String(path)) = (
            node.get_property("script"),
            node.get_property("_script_path"),
        ) {
            if let Some(id) = parse_ext_resource_id(&script_ref) {
                let uid = ext_uid_companion(node, "script");
                ext_by_id
                    .entry(id)
                    .or_insert(("Script".to_string(), path, uid));
            }
        }

        // Instanced sub-scene external resource: `_instance` ref + resolved path.
        if let (Variant::String(inst_ref), Variant::String(path)) = (
            node.get_property("_instance"),
            node.get_property("_instance_path"),
        ) {
            if let Some(id) = parse_ext_resource_id(&inst_ref) {
                let uid = ext_uid_companion(node, "_instance");
                ext_by_id
                    .entry(id)
                    .or_insert(("PackedScene".to_string(), path, uid));
            }
        }

        // Inline sub-resources referenced via `__resref__<prop>` = `class::id`.
        for (k, v) in node.properties() {
            if k.starts_with("__resref__") {
                if let Variant::String(meta) = v {
                    if let Some((class, id)) = meta.rsplit_once("::") {
                        if !id.is_empty() {
                            sub_by_id
                                .entry(id.to_string())
                                .or_insert_with(|| class.to_string());
                        }
                    }
                }
            }
        }

        // Any *other* property holding an `ExtResource("id")` reference — a
        // texture, an audio stream, a font — paired with the `__extres__<prop>`
        // companion the loader writes for it (packed_scene.rs). Without this the
        // property keeps its reference on save but its `[ext_resource]` header
        // line is dropped, leaving a dangling id. `script` and `_instance` are
        // handled above and re-inserting is a no-op thanks to `or_insert`.
        for (k, v) in node.properties() {
            if is_internal_prop(k) {
                continue;
            }
            let Variant::String(s) = v else { continue };
            let id = match parse_ext_resource_id(s) {
                Some(id) => id,
                None => match s.strip_prefix("ExtResource:") {
                    Some(id) => id.to_string(),
                    None => continue,
                },
            };
            let path = match node.get_property(&format!("__extres__{k}")) {
                Variant::String(p) if !p.is_empty() => p,
                _ => continue,
            };
            let res_type = ext_resource_type_for(&path);
            let uid = ext_uid_companion(node, k);
            ext_by_id.entry(id).or_insert((res_type, path, uid));
        }
    }

    ResourceTables {
        ext: ext_by_id
            .into_iter()
            .map(|(id, (ty, path, uid))| (id, ty, path, uid))
            .collect(),
        sub: sub_by_id.into_iter().collect(),
    }
}

/// Writes a `[node ...]` section for a single node.
/// `skip_keys` names properties the caller regenerates itself — an
/// AnimationPlayer's `libraries`/`autoplay` are rebuilt from the attached player
/// (`write_animation_player_props`), so writing the node's own parsed copies too
/// would duplicate the lines on every re-save (pat-0q2kx.54).
fn write_node_section(
    out: &mut String,
    node: &crate::node::Node,
    parent_path: Option<&str>,
    foreign: bool,
    skip_keys: &[&str],
    instance_base: Option<&crate::node::Node>,
) {
    out.push('\n');

    let name = node.name();
    let class = node.class_name();

    // Emit group membership in the node header (`groups=["a", "b"]`), matching
    // the loader's parse format, so groups survive a save/load round-trip.
    // Sorted for deterministic output.
    let groups_attr = {
        let mut groups: Vec<&str> = node.groups().iter().map(String::as_str).collect();
        if groups.is_empty() {
            String::new()
        } else {
            groups.sort_unstable();
            let list: Vec<String> = groups.iter().map(|g| format!("\"{g}\"")).collect();
            format!(" groups=[{}]", list.join(", "))
        }
    };

    // A node that instances a sub-scene is written `instance=ExtResource("id")`
    // with NO `type=` attribute, exactly as Godot does.
    let instance_attr = match node.get_property("_instance") {
        Variant::String(s) if !foreign => {
            parse_ext_resource_id(&s).map(|id| format!(" instance=ExtResource(\"{id}\")"))
        }
        _ => None,
    };
    let type_attr = if instance_attr.is_some() || foreign {
        String::new()
    } else {
        format!(" type=\"{class}\"")
    };
    let parent_attr = match parent_path {
        None => String::new(),
        Some(parent) => format!(" parent=\"{parent}\""),
    };
    let instance_attr = instance_attr.unwrap_or_default();
    writeln!(
        out,
        "[node name=\"{name}\"{type_attr}{parent_attr}{groups_attr}{instance_attr}]"
    )
    .unwrap();

    // Map each property that references an inline sub-resource to its id, from
    // the `__resref__<prop>` companions.
    let mut sub_ref: HashMap<&str, String> = HashMap::new();
    for (k, v) in node.properties() {
        if let Some(prop) = k.strip_prefix("__resref__") {
            if let Variant::String(meta) = v {
                if let Some((_class, id)) = meta.rsplit_once("::") {
                    sub_ref.insert(prop, id.to_string());
                }
            }
        }
    }

    // Write properties sorted for determinism. Internal companion keys are
    // skipped — they were folded into the resource tables / header attributes.
    //
    // 🔑 A value equal to the CLASS DEFAULT is omitted, as Godot's
    // `SceneState::_parse_node` does (it stores a property only when it
    // differs from the instantiated default). The loader seeds every ClassDB
    // default onto each node, so without this a load -> save wrote ~20 default
    // lines per node and no scene survived a round trip byte-for-byte. With an
    // empty ClassDB the default list is empty and nothing is dropped.
    let class_defaults = gdobject::class_db::get_property_list(node.class_name());
    let is_class_default = |k: &str, v: &Variant| {
        class_defaults
            .iter()
            .rev()
            .find(|p| p.name == k)
            .is_some_and(|p| &p.default_value == v)
    };
    // 🔑 A Node3D stores ONE property, `transform` (pat-l0afa). `position`,
    // `rotation`, `scale`, `quaternion`, `basis` and `rotation_degrees` are
    // editor views of it (PROPERTY_USAGE_EDITOR, no STORAGE, in
    // `Node3D::_bind_methods`), so Godot's saver never writes them; Patina
    // mirrors all of them onto the node (`node3d::store_local_transform`) and
    // wrote every one back next to `transform`. The value written is the
    // canonical local transform, so an edit made through any view is kept.
    let node3d_transform = crate::node3d::looks_like_node3d(node)
        .then(|| Variant::Transform3D(crate::node3d::local_transform_of(node)));
    const NODE3D_VIEWS: &[&str] = &[
        "position",
        "rotation",
        "rotation_degrees",
        "scale",
        "quaternion",
        "basis",
        "transform",
    ];
    let mut owned: Vec<(String, Variant)> = node
        .properties()
        .filter(|(k, _)| !is_internal_prop(k) && !skip_keys.contains(&k.as_str()))
        .filter(|(k, _)| node3d_transform.is_none() || !NODE3D_VIEWS.contains(&k.as_str()))
        .filter(|(k, v)| !is_class_default(k, v) || sub_ref.contains_key(k.as_str()))
        // 🔑 An instance (or an editable child of one) writes only what
        // differs from the instanced scene, as Godot's `SceneState::_parse_node`
        // compares against the instance's own state. Without this every coin
        // in level.tscn repeated coin.tscn's `script`, collision layers and
        // `monitorable` as overrides.
        .filter(|(k, v)| instance_base.is_none_or(|b| b.get_property(k) != **v))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if let Some(t) = node3d_transform {
        let identity = Variant::Transform3D(gdcore::math3d::Transform3D::IDENTITY);
        let base_t = instance_base
            .filter(|b| crate::node3d::looks_like_node3d(b))
            .map(|b| Variant::Transform3D(crate::node3d::local_transform_of(b)));
        let unchanged = match base_t {
            Some(b) => b == t,
            None => t == identity,
        };
        if !unchanged && !skip_keys.contains(&"transform") {
            owned.push(("transform".to_string(), t));
        }
    }
    owned.sort_by(|a, b| a.0.cmp(&b.0));
    let props: Vec<(&String, &Variant)> = owned.iter().map(|(k, v)| (k, v)).collect();
    for (key, value) in props {
        // Property that references an inline sub-resource (mesh/shape/etc): emit
        // the canonical `SubResource("id")` regardless of any resolved marker.
        if let Some(id) = sub_ref.get(key.as_str()) {
            writeln!(out, "{key} = SubResource(\"{id}\")").unwrap();
            continue;
        }
        // Resource references stored as strings are re-encoded as their unquoted
        // constructor form (e.g. `script = ExtResource("id")`).
        if let Variant::String(s) = value {
            if let Some(id) = parse_ext_resource_id(s) {
                writeln!(out, "{key} = ExtResource(\"{id}\")").unwrap();
                continue;
            }
            if let Some(id) = s.strip_prefix("ExtResource:") {
                writeln!(out, "{key} = ExtResource(\"{id}\")").unwrap();
                continue;
            }
            if let Some(id) = s.strip_prefix("SubResource:") {
                writeln!(out, "{key} = SubResource(\"{id}\")").unwrap();
                continue;
            }
        }
        writeln!(out, "{} = {}", key, format_variant_value(value)).unwrap();
    }

    // Metadata set at runtime lives in the node's `meta` map, which is a
    // SEPARATE store from `properties` (pat-s3rrv.151). A `metadata/foo` row
    // read from a .tscn lands in `properties` and round-trips through the loop
    // above, but anything written by `set_meta` was dropped silently on save —
    // so editor- or script-authored metadata never reached the file.
    //
    // Only keys with no property row of their own are emitted, so a value that
    // came from the file is written once, by the loop above, and not duplicated
    // here. Sorted for the same determinism the property loop wants.
    let mut metas: Vec<(&str, Variant)> = node
        .get_meta_list()
        .into_iter()
        .filter(|k| node.get_property(&format!("metadata/{k}")).is_nil())
        .map(|k| (k, node.get_meta(k)))
        .collect();
    metas.sort_by_key(|(k, _)| k.to_owned());
    for (key, value) in metas {
        writeln!(out, "metadata/{} = {}", key, format_variant_value(&value)).unwrap();
    }
}

/// Writes `[connection]` sections for all signals in the subtree.
fn write_connections(
    out: &mut String,
    tree: &SceneTree,
    _root_id: NodeId,
    node_ids: &[NodeId],
    id_to_path: &HashMap<NodeId, String>,
) {
    // Build reverse lookup: ObjectId -> NodeId for resolving connection targets.
    let mut object_id_to_node_id: HashMap<gdcore::ObjectId, NodeId> = HashMap::new();
    for &nid in node_ids {
        if let Some(node) = tree.get_node(nid) {
            let _ = node; // we need nid.object_id()
            object_id_to_node_id.insert(nid.object_id(), nid);
        }
    }

    // Collect connections in a deterministic order.
    struct ConnInfo {
        signal_name: String,
        from_path: String,
        to_path: String,
        method: String,
        /// Godot `ConnectFlags` bitfield rebuilt from the live connection, so a
        /// loaded-then-saved scene keeps the behavior it was authored with
        /// (pat-0q2kx.65). Omitted from the output when zero, as Godot does.
        flags: u32,
        binds: Vec<Variant>,
        unbinds: usize,
    }

    let mut conn_infos: Vec<ConnInfo> = Vec::new();

    // Iterate nodes in tree order for deterministic output.
    for &nid in node_ids {
        let store = match tree.signal_store(nid) {
            Some(s) => s,
            None => continue,
        };

        let from_path = match id_to_path.get(&nid) {
            Some(p) => p.clone(),
            None => continue,
        };

        let mut signal_names: Vec<&str> = store.signal_names();
        signal_names.sort();

        for sig_name in signal_names {
            let signal = match store.get_signal(sig_name) {
                Some(s) => s,
                None => continue,
            };

            for conn in signal.connections() {
                // A connection inside one instance (both ends in the same
                // instanced scene) is that scene's own `[connection]`; Godot
                // does not repeat it in the scene that instances it.
                if let Some(&to_nid) = object_id_to_node_id.get(&conn.target_id) {
                    let from_scope = instance_scope(tree, _root_id, nid);
                    if from_scope.is_some() && from_scope == instance_scope(tree, _root_id, to_nid)
                    {
                        continue;
                    }
                }
                // Resolve target ObjectId -> NodeId -> path.
                let to_path = object_id_to_node_id
                    .get(&conn.target_id)
                    .and_then(|nid| id_to_path.get(nid))
                    .cloned()
                    .unwrap_or_else(|| ".".to_string());

                let mut flags = 0u32;
                if conn.deferred {
                    flags |= CONNECT_DEFERRED;
                }
                if conn.one_shot {
                    flags |= CONNECT_ONE_SHOT;
                }
                conn_infos.push(ConnInfo {
                    signal_name: sig_name.to_string(),
                    from_path: from_path.clone(),
                    to_path,
                    method: conn.method.clone(),
                    flags,
                    binds: conn.binds.clone(),
                    unbinds: conn.unbinds,
                });
            }
        }
    }

    for info in &conn_infos {
        // Godot writes flags/binds/unbinds only when they carry information, so
        // an ordinary connection round-trips byte-identical.
        let mut extra = String::new();
        if info.flags != 0 {
            extra.push_str(&format!(" flags={}", info.flags));
        }
        if !info.binds.is_empty() {
            let items: Vec<String> = info.binds.iter().map(format_variant_value).collect();
            extra.push_str(&format!(" binds=[{}]", items.join(", ")));
        }
        if info.unbinds != 0 {
            extra.push_str(&format!(" unbinds={}", info.unbinds));
        }
        writeln!(
            out,
            "\n[connection signal=\"{}\" from=\"{}\" to=\"{}\" method=\"{}\"{}]",
            info.signal_name, info.from_path, info.to_path, info.method, extra,
        )
        .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;
    use crate::packed_scene::{add_packed_scene_to_tree, PackedScene};
    use crate::scene_tree::SceneTree;
    use gdcore::math::{Color, Rect2, Transform2D, Vector2, Vector3};
    use gdcore::math3d::{Aabb, Basis, Plane, Quaternion, Transform3D};
    use gdcore::node_path::NodePath;
    use gdobject::signal::Connection;
    use gdvariant::Dictionary;

    // -- format_variant_value tests -----------------------------------------

    #[test]
    fn format_nil() {
        assert_eq!(format_variant_value(&Variant::Nil), "null");
    }

    #[test]
    fn format_bool() {
        assert_eq!(format_variant_value(&Variant::Bool(true)), "true");
        assert_eq!(format_variant_value(&Variant::Bool(false)), "false");
    }

    #[test]
    fn format_int() {
        assert_eq!(format_variant_value(&Variant::Int(42)), "42");
        assert_eq!(format_variant_value(&Variant::Int(-7)), "-7");
    }

    #[test]
    fn format_float() {
        assert_eq!(format_variant_value(&Variant::Float(3.14)), "3.14");
        assert_eq!(format_variant_value(&Variant::Float(5.0)), "5.0");
    }

    #[test]
    fn format_string() {
        assert_eq!(
            format_variant_value(&Variant::String("hello".into())),
            "\"hello\""
        );
    }

    #[test]
    fn format_string_with_escapes() {
        assert_eq!(
            format_variant_value(&Variant::String("line1\nline2\ttab".into())),
            r#""line1\nline2\ttab""#
        );
    }

    #[test]
    fn format_vector2() {
        let v = Vector2::new(10.0, 20.5);
        assert_eq!(
            format_variant_value(&Variant::Vector2(v)),
            "Vector2(10, 20.5)"
        );
    }

    #[test]
    fn format_vector3() {
        let v = Vector3::new(1.0, 2.0, 3.5);
        assert_eq!(
            format_variant_value(&Variant::Vector3(v)),
            "Vector3(1, 2, 3.5)"
        );
    }

    #[test]
    fn format_color() {
        let c = Color::new(0.5, 0.6, 0.7, 1.0);
        assert_eq!(
            format_variant_value(&Variant::Color(c)),
            "Color(0.5, 0.6, 0.7, 1)"
        );
    }

    #[test]
    fn format_rect2() {
        let r = Rect2::new(Vector2::new(1.0, 2.0), Vector2::new(3.0, 4.0));
        assert_eq!(
            format_variant_value(&Variant::Rect2(r)),
            "Rect2(1, 2, 3, 4)"
        );
    }

    #[test]
    fn format_transform2d() {
        let t = Transform2D::IDENTITY;
        assert_eq!(
            format_variant_value(&Variant::Transform2D(t)),
            "Transform2D(1, 0, 0, 1, 0, 0)"
        );
    }

    #[test]
    fn format_node_path() {
        let np = NodePath::new("/root/Player");
        assert_eq!(
            format_variant_value(&Variant::NodePath(np)),
            "NodePath(\"/root/Player\")"
        );
    }

    #[test]
    fn format_array() {
        let arr = Variant::Array(
            vec![
                Variant::Int(1),
                Variant::String("two".into()),
                Variant::Bool(true),
            ]
            .into(),
        );
        assert_eq!(format_variant_value(&arr), "[1, \"two\", true]");
    }

    #[test]
    fn format_empty_array() {
        assert_eq!(
            format_variant_value(&Variant::Array(Default::default())),
            "[]"
        );
    }

    #[test]
    fn format_dictionary() {
        let mut d = Dictionary::new();
        d.insert("b".to_string(), Variant::Int(2));
        d.insert("a".to_string(), Variant::Int(1));
        let result = format_variant_value(&Variant::Dictionary(d.into()));
        // Keys are sorted, one entry per line, as Godot's VariantWriter writes
        // them (pat-l0afa).
        assert_eq!(result, "{\n\"a\": 1,\n\"b\": 2\n}");
    }

    #[test]
    fn format_empty_dictionary() {
        assert_eq!(
            format_variant_value(&Variant::Dictionary(Default::default())),
            "{}"
        );
    }

    // -- TscnSaver tests ----------------------------------------------------

    #[test]
    fn save_empty_scene() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root = Node::new("Main", "Node");
        let scene_root_id = tree.add_child(root, scene_root).unwrap();

        let output = TscnSaver::save_tree(&tree, scene_root_id);
        assert!(output.contains("[gd_scene format=3]"));
        assert!(output.contains("[node name=\"Main\" type=\"Node\"]"));
        // Root node should have no parent attribute.
        assert!(!output.contains("parent="));
    }

    #[test]
    fn save_with_properties() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Player", "Node2D");
        node.set_property("position", Variant::Vector2(Vector2::new(100.0, 200.0)));
        node.set_property("health", Variant::Int(100));
        let nid = tree.add_child(root, node).unwrap();

        let output = TscnSaver::save_tree(&tree, nid);
        assert!(output.contains("position = Vector2(100, 200)"));
        assert!(output.contains("health = 100"));
    }

    #[test]
    fn save_with_nested_children() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let scene_root = Node::new("World", "Node");
        let world_id = tree.add_child(root, scene_root).unwrap();

        let level = Node::new("Level", "Node");
        let level_id = tree.add_child(world_id, level).unwrap();

        let enemies = Node::new("Enemies", "Node");
        let enemies_id = tree.add_child(level_id, enemies).unwrap();

        let mut boss = Node::new("Boss", "Node2D");
        boss.set_property("health", Variant::Int(500));
        tree.add_child(enemies_id, boss).unwrap();

        let output = TscnSaver::save_tree(&tree, world_id);

        assert!(output.contains("[node name=\"World\" type=\"Node\"]"));
        assert!(output.contains("[node name=\"Level\" type=\"Node\" parent=\".\"]"));
        assert!(output.contains("[node name=\"Enemies\" type=\"Node\" parent=\"Level\"]"));
        assert!(output.contains("[node name=\"Boss\" type=\"Node2D\" parent=\"Level/Enemies\"]"));
        assert!(output.contains("health = 500"));
    }

    #[test]
    fn save_with_signals() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let scene_root = Node::new("Root", "Control");
        let root_id = tree.add_child(root, scene_root).unwrap();

        let button = Node::new("Button", "Button");
        let button_id = tree.add_child(root_id, button).unwrap();

        // Connect button "pressed" signal to root.
        let conn = Connection::new(root_id.object_id(), "_on_button_pressed");
        tree.connect_signal(button_id, "pressed", conn);

        let output = TscnSaver::save_tree(&tree, root_id);

        assert!(output.contains(
            "[connection signal=\"pressed\" from=\"Button\" to=\".\" method=\"_on_button_pressed\"]"
        ));
    }

    #[test]
    fn save_subtree_is_same_as_save_tree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = Node::new("Sub", "Node2D");
        let nid = tree.add_child(root, node).unwrap();

        let a = TscnSaver::save_tree(&tree, nid);
        let b = TscnSaver::save_subtree(&tree, nid);
        assert_eq!(a, b);
    }

    #[test]
    fn roundtrip_simple_scene() {
        let tscn = r#"
[gd_scene format=3]

[node name="Root" type="Node"]

[node name="Player" type="Node2D" parent="."]
position = Vector2(100, 200)

[node name="Sprite" type="Sprite2D" parent="Player"]
"#;
        // Parse -> instance -> add to tree -> save -> re-parse.
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

        let saved = TscnSaver::save_tree(&tree, scene_root_id);

        // Re-parse the saved output.
        let scene2 = PackedScene::from_tscn(&saved).unwrap();
        assert_eq!(scene2.node_count(), 3);

        // Instance and verify structure.
        let nodes2 = scene2.instance().unwrap();
        assert_eq!(nodes2[0].name(), "Root");
        assert_eq!(nodes2[0].class_name(), "Node");
        assert_eq!(nodes2[1].name(), "Player");
        assert_eq!(nodes2[1].class_name(), "Node2D");
        assert_eq!(
            nodes2[1].get_property("position"),
            Variant::Vector2(Vector2::new(100.0, 200.0))
        );
        assert_eq!(nodes2[2].name(), "Sprite");
        assert_eq!(nodes2[2].class_name(), "Sprite2D");
    }

    #[test]
    fn roundtrip_with_connections() {
        let tscn = r#"
[gd_scene format=3]

[node name="Root" type="Control"]

[node name="Button" type="Button" parent="."]

[connection signal="pressed" from="Button" to="." method="_on_pressed"]
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

        let saved = TscnSaver::save_tree(&tree, scene_root_id);

        // Verify connection section is present.
        assert!(saved.contains("[connection signal=\"pressed\""));
        assert!(saved.contains("from=\"Button\""));
        assert!(saved.contains("to=\".\""));
        assert!(saved.contains("method=\"_on_pressed\""));

        // Re-parse.
        let scene2 = PackedScene::from_tscn(&saved).unwrap();
        assert_eq!(scene2.connection_count(), 1);
    }

    #[test]
    fn save_single_root_node() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = Node::new("Alone", "Node");
        let nid = tree.add_child(root, node).unwrap();

        let output = TscnSaver::save_tree(&tree, nid);
        assert!(output.contains("[gd_scene format=3]"));
        assert!(output.contains("[node name=\"Alone\" type=\"Node\"]"));
        // Should have no connection sections.
        assert!(!output.contains("[connection"));
    }

    #[test]
    fn properties_are_sorted() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("N", "Node");
        node.set_property("z_prop", Variant::Int(3));
        node.set_property("a_prop", Variant::Int(1));
        node.set_property("m_prop", Variant::Int(2));
        let nid = tree.add_child(root, node).unwrap();

        let output = TscnSaver::save_tree(&tree, nid);

        let a_pos = output.find("a_prop").unwrap();
        let m_pos = output.find("m_prop").unwrap();
        let z_pos = output.find("z_prop").unwrap();
        assert!(a_pos < m_pos);
        assert!(m_pos < z_pos);
    }

    #[test]
    fn roundtrip_deep_nesting() {
        let tscn = r#"
[gd_scene format=3]

[node name="World" type="Node"]

[node name="Level" type="Node" parent="."]

[node name="Enemies" type="Node" parent="Level"]

[node name="Boss" type="Node2D" parent="Level/Enemies"]
health = 100
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

        let saved = TscnSaver::save_tree(&tree, scene_root_id);
        let scene2 = PackedScene::from_tscn(&saved).unwrap();
        assert_eq!(scene2.node_count(), 4);

        let nodes2 = scene2.instance().unwrap();
        assert_eq!(nodes2[3].name(), "Boss");
        assert_eq!(nodes2[3].get_property("health"), Variant::Int(100));
    }

    // -- Packed array .tscn round-trips (pat-u3l8c.11) ----------------------

    /// Format a packed value to `.tscn` text and parse it back; the packed
    /// type and contents must survive unchanged.
    fn packed_roundtrip(v: Variant) {
        let text = format_variant_value(&v);
        let parsed = gdresource::loader::parse_variant_value(&text)
            .unwrap_or_else(|e| panic!("re-parse of {text:?} failed: {e:?}"));
        assert_eq!(parsed, v, "round-trip changed value; text was {text:?}");
    }

    #[test]
    fn packed_arrays_keep_type_through_tscn_roundtrip() {
        packed_roundtrip(Variant::PackedByteArray(vec![0, 1, 2, 255]));
        packed_roundtrip(Variant::PackedByteArray(vec![]));
        packed_roundtrip(Variant::PackedInt32Array(vec![1, -2, 3]));
        packed_roundtrip(Variant::PackedInt64Array(vec![1, -2, 9_000_000_000]));
        packed_roundtrip(Variant::PackedFloat32Array(vec![0.0, 1.5, -3.25]));
        packed_roundtrip(Variant::PackedFloat64Array(vec![0.0, 1.5, -3.25]));
        packed_roundtrip(Variant::PackedStringArray(vec![
            "alpha".into(),
            "with space".into(),
        ]));
        packed_roundtrip(Variant::PackedVector2Array(vec![
            Vector2::new(1.0, 2.0),
            Vector2::new(-3.0, 4.0),
        ]));
        packed_roundtrip(Variant::PackedVector3Array(vec![Vector3::new(
            1.0, 2.0, 3.0,
        )]));
        packed_roundtrip(Variant::PackedColorArray(vec![Color::new(
            1.0, 0.0, 0.5, 1.0,
        )]));
    }

    #[test]
    fn packed_array_saved_as_constructor_syntax() {
        assert_eq!(
            format_variant_value(&Variant::PackedInt32Array(vec![1, 2, 3])),
            "PackedInt32Array(1, 2, 3)"
        );
        assert_eq!(
            format_variant_value(&Variant::PackedVector2Array(vec![Vector2::new(1.0, 2.0)])),
            "PackedVector2Array(1, 2)"
        );
        assert_eq!(
            format_variant_value(&Variant::PackedByteArray(vec![])),
            "PackedByteArray()"
        );
    }

    // -- 3D math type serialization (pat-nzgdu.18) --------------------------

    #[test]
    fn transform3d_saved_as_constructor_syntax() {
        let t = Variant::Transform3D(Transform3D {
            basis: Basis {
                x: Vector3::new(1.0, 0.0, 0.0),
                y: Vector3::new(0.0, 1.0, 0.0),
                z: Vector3::new(0.0, 0.0, 1.0),
            },
            origin: Vector3::new(5.0, 6.0, 7.0),
        });
        assert_eq!(
            format_variant_value(&t),
            "Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 5, 6, 7)"
        );
    }

    #[test]
    fn math3d_types_keep_value_through_tscn_roundtrip() {
        packed_roundtrip(Variant::Transform3D(Transform3D {
            basis: Basis {
                x: Vector3::new(1.0, 2.0, 3.0),
                y: Vector3::new(4.0, 5.0, 6.0),
                z: Vector3::new(7.0, 8.0, 9.0),
            },
            origin: Vector3::new(-1.5, 0.0, 2.25),
        }));
        packed_roundtrip(Variant::Basis(Basis {
            x: Vector3::new(1.0, 0.0, 0.0),
            y: Vector3::new(0.0, 1.0, 0.0),
            z: Vector3::new(0.0, 0.0, 1.0),
        }));
        packed_roundtrip(Variant::Quaternion(Quaternion::new(0.0, 0.0, 0.0, 1.0)));
        packed_roundtrip(Variant::Aabb(Aabb::new(
            Vector3::new(0.0, 1.0, 2.0),
            Vector3::new(3.0, 4.0, 5.0),
        )));
        packed_roundtrip(Variant::Plane(Plane::new(Vector3::new(0.0, 1.0, 0.0), 5.0)));
    }

    // -- Lossless ext/sub resource round-trip (pat-nzgdu.18) ----------------

    /// An external resource referenced by an ordinary property — not `script`,
    /// not a sub-scene `instance` — must keep its `[ext_resource]` header across a
    /// save. Before pat-0q2kx.13 only `script` and `_instance` were collected, so
    /// a `texture`/`stream`/font reference saved as a dangling `ExtResource("id")`
    /// with no header declaring it, which Godot cannot load.
    #[test]
    fn tscn_save_keeps_ext_resource_for_ordinary_properties() {
        let tscn = r#"[gd_scene load_steps=3 format=3]

[ext_resource type="Texture2D" path="res://icon.png" id="1_tex"]
[ext_resource type="AudioStream" path="res://beep.wav" id="2_snd"]

[node name="Root" type="Node2D"]

[node name="Icon" type="Sprite2D" parent="."]
texture = ExtResource("1_tex")

[node name="Beeper" type="AudioStreamPlayer" parent="."]
stream = ExtResource("2_snd")
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

        let saved = TscnSaver::save_tree(&tree, scene_root_id);

        // Both headers come back, each with the type inferred from its extension.
        assert!(
            saved.contains(r#"[ext_resource type="Texture2D" path="res://icon.png" id="1_tex"]"#),
            "texture ext_resource header lost on save:\n{saved}"
        );
        assert!(
            saved.contains(r#"[ext_resource type="AudioStream" path="res://beep.wav" id="2_snd"]"#),
            "audio stream ext_resource header lost on save:\n{saved}"
        );
        // …and the properties still reference them by id.
        assert!(
            saved.contains(r#"texture = ExtResource("1_tex")"#),
            "texture reference lost:\n{saved}"
        );
        assert!(
            saved.contains(r#"stream = ExtResource("2_snd")"#),
            "stream reference lost:\n{saved}"
        );
        // The `__extres__` companions are bookkeeping and must never be emitted.
        assert!(
            !saved.contains("__extres__"),
            "internal companion leaked into the scene:\n{saved}"
        );

        // The saved text must load again with the references intact — a dangling
        // id would survive a string assert but not a reload.
        let reloaded = PackedScene::from_tscn(&saved).expect("saved scene reloads");
        let mut tree2 = SceneTree::new();
        let root2 = tree2.root_id();
        let reloaded_root = add_packed_scene_to_tree(&mut tree2, root2, &reloaded).unwrap();
        let icon = tree2
            .get_node_or_null(reloaded_root, "Icon")
            .expect("Icon survives the round-trip");
        assert_eq!(
            tree2
                .get_node(icon)
                .unwrap()
                .get_property("__extres__texture"),
            Variant::String("res://icon.png".into()),
            "the reloaded texture must resolve to the same path"
        );
    }

    #[test]
    fn ext_resource_type_is_inferred_from_the_extension() {
        assert_eq!(ext_resource_type_for("res://icon.png"), "Texture2D");
        assert_eq!(ext_resource_type_for("res://a/b/beep.WAV"), "AudioStream");
        assert_eq!(ext_resource_type_for("res://child.tscn"), "PackedScene");
        assert_eq!(ext_resource_type_for("res://player.gd"), "Script");
        assert_eq!(ext_resource_type_for("res://theme.tres"), "Resource");
        assert_eq!(ext_resource_type_for("res://noext"), "Resource");
    }

    /// A complex scene — root with a script `ext_resource`, a child instancing a
    /// sub-scene, a node with an inline `sub_resource`, and a `Transform3D`
    /// property — must survive a save/load round-trip with its resource tables,
    /// script path, instance ref, and Transform3D value intact.
    #[test]
    fn tscn_save_roundtrip_ext_and_sub_resources() {
        let tscn = r#"[gd_scene load_steps=4 format=3 uid="uid://testscene"]

[ext_resource type="Script" path="res://player.gd" id="1_script"]
[ext_resource type="PackedScene" path="res://child.tscn" id="2_scene"]

[sub_resource type="BoxShape3D" id="Box_1"]

[node name="Root" type="Node3D"]
script = ExtResource("1_script")
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 5, 6, 7)

[node name="Body" type="CollisionShape3D" parent="."]
shape = SubResource("Box_1")

[node name="Child" parent="." instance=ExtResource("2_scene")]
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

        let saved = TscnSaver::save_tree(&tree, scene_root_id);

        // Sanity: the saved text declares the reconstructed tables & header.
        assert!(
            saved.contains("[gd_scene load_steps=4 format=3]"),
            "missing header with load_steps:\n{saved}"
        );
        assert!(
            saved.contains(
                "[ext_resource type=\"Script\" path=\"res://player.gd\" id=\"1_script\"]"
            ),
            "missing script ext_resource:\n{saved}"
        );
        assert!(
            saved.contains(
                "[ext_resource type=\"PackedScene\" path=\"res://child.tscn\" id=\"2_scene\"]"
            ),
            "missing instanced-scene ext_resource:\n{saved}"
        );
        assert!(
            saved.contains("[sub_resource type=\"BoxShape3D\" id=\"Box_1\"]"),
            "missing sub_resource section:\n{saved}"
        );
        assert!(
            saved.contains("script = ExtResource(\"1_script\")"),
            "script should be an unquoted ExtResource ref:\n{saved}"
        );
        assert!(
            saved.contains("instance=ExtResource(\"2_scene\")"),
            "instanced child should carry an instance= attribute:\n{saved}"
        );
        assert!(
            saved.contains("shape = SubResource(\"Box_1\")"),
            "shape should reference the sub_resource:\n{saved}"
        );

        // Re-parse the saved text and diff the resource tables against the
        // original — the round-trip diff must be empty for these.
        let scene2 = PackedScene::from_tscn(&saved).unwrap();

        let ext1 = scene.ext_resources();
        let ext2 = scene2.ext_resources();
        assert_eq!(
            ext2.len(),
            ext1.len(),
            "ext_resource count changed:\n{saved}"
        );
        for (id, e1) in ext1 {
            let e2 = ext2
                .get(id)
                .unwrap_or_else(|| panic!("ext_resource {id} lost on round-trip:\n{saved}"));
            assert_eq!(e2.res_type, e1.res_type, "ext_resource {id} type changed");
            assert_eq!(e2.path, e1.path, "ext_resource {id} path changed");
        }

        assert_eq!(
            scene2.sub_resource_type("Box_1"),
            Some("BoxShape3D"),
            "sub_resource type lost on round-trip:\n{saved}"
        );

        // Script path, instance ref, and Transform3D value survive intact.
        let nodes1 = scene.instance().unwrap();
        let nodes2 = scene2.instance().unwrap();

        let root1 = &nodes1[0];
        let root2 = nodes2.iter().find(|n| n.name() == "Root").expect("Root");
        assert_eq!(
            root2.get_property("_script_path"),
            root1.get_property("_script_path"),
            "script path changed"
        );
        assert_eq!(
            root2.get_property("transform"),
            root1.get_property("transform"),
            "Transform3D value changed"
        );

        let child1 = nodes1.iter().find(|n| n.name() == "Child").expect("Child");
        let child2 = nodes2.iter().find(|n| n.name() == "Child").expect("Child");
        assert_eq!(
            child2.get_property("_instance"),
            child1.get_property("_instance"),
            "instance ref changed"
        );
    }
}
