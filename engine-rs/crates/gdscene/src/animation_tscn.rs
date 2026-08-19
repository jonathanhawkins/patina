//! Build [`Animation`] / [`AnimationPlayer`] objects out of parsed `.tscn` data
//! (pat-0q2kx.1).
//!
//! Godot stores animations inside a scene as `[sub_resource type="Animation"]`
//! blocks keyed by `tracks/N/type|path|interp|keys`, gathered by an
//! `[sub_resource type="AnimationLibrary"]` whose `_data` dictionary maps clip
//! names to those animations. An `AnimationPlayer` node then points at one or
//! more libraries through `libraries = {"": SubResource("...")}` and names the
//! clip to start on ready in `autoplay`
//! (upstream/godot/scene/resources/animation.cpp `Animation::_set`,
//! upstream/godot/scene/resources/animation_library.cpp `_set_data`,
//! upstream/godot/scene/animation/animation_player.cpp).
//!
//! Patina already had the sampling engine ([`crate::animation`]) and the tree
//! hook ([`crate::SceneTree::attach_animation_player`]), but nothing built a
//! player from a loaded scene — `attach_animation_player` had no non-test
//! caller. This module is that missing step; [`attach_scene_animation_players`]
//! runs from the scene-instancing helpers in [`crate::packed_scene`].
//!
//! It lives in `gdscene` rather than `gdresource` (where the rest of the `.tscn`
//! value parsing lives) because [`Animation`] is a `gdscene` type and
//! `gdresource` is a dependency of `gdscene`, not the other way round.

use std::collections::HashMap;

use gdcore::math::{Vector2, Vector3};
use gdcore::math3d::Quaternion;
use gdvariant::Variant;

use crate::animation::{
    Animation, AnimationPlayer, AnimationTrack, KeyFrame, LoopMode, TrackInterpolation, TrackType,
    TrackUpdateMode, TransitionType,
};
use crate::node::NodeId;
use crate::packed_scene::PackedScene;
use crate::scene_tree::SceneTree;

/// Godot's `AnimationMixer.root_node` default (`NodePath("..")`) — track paths
/// resolve from the player's *parent*, not the player itself
/// (upstream/godot/scene/animation/animation_mixer.cpp:2499).
const GODOT_DEFAULT_ROOT_NODE: &str = "..";

// ---------------------------------------------------------------------------
// Animation (one `[sub_resource type="Animation"]`)
// ---------------------------------------------------------------------------

/// Builds an [`Animation`] named `name` from the property map of an
/// `[sub_resource type="Animation"]` section.
///
/// Recognized keys mirror `Animation::_set`: `length`, `loop_mode`, and
/// `tracks/<N>/{type,path,interp,enabled,keys}`. Tracks Patina cannot model
/// (`bezier`, `animation`) and disabled tracks are skipped rather than
/// producing a broken track.
pub fn animation_from_tscn_props(name: &str, props: &HashMap<String, Variant>) -> Animation {
    let length = props.get("length").and_then(variant_to_f64).unwrap_or(1.0);
    let mut anim = Animation::new(name, length);
    anim.loop_mode = match props.get("loop_mode").and_then(variant_to_i64) {
        Some(1) => LoopMode::Linear,
        Some(2) => LoopMode::PingPong,
        _ => LoopMode::None,
    };

    for idx in track_indices(props) {
        if let Some(track) = track_from_props(props, idx) {
            anim.tracks.push(track);
        }
    }

    anim
}

/// Returns the track indices present in `props`, ascending, so tracks keep the
/// authored order regardless of `HashMap` iteration order.
fn track_indices(props: &HashMap<String, Variant>) -> Vec<usize> {
    let mut indices: Vec<usize> = props
        .keys()
        .filter_map(|k| {
            let rest = k.strip_prefix("tracks/")?;
            let (num, what) = rest.split_once('/')?;
            // Keyed off `type` alone so each track contributes exactly one index.
            if what != "type" {
                return None;
            }
            num.parse::<usize>().ok()
        })
        .collect();
    indices.sort_unstable();
    indices
}

/// Reads the `tracks/<idx>/<what>` entry of an Animation sub-resource.
fn track_key<'a>(
    props: &'a HashMap<String, Variant>,
    idx: usize,
    what: &str,
) -> Option<&'a Variant> {
    props.get(&format!("tracks/{idx}/{what}"))
}

/// Builds track `idx` from the `tracks/<idx>/*` keys, or `None` when the track
/// is disabled, of an unsupported type, or has no usable keyframes.
fn track_from_props(props: &HashMap<String, Variant>, idx: usize) -> Option<AnimationTrack> {
    // Godot skips disabled tracks when building its caches.
    if let Some(Variant::Bool(false)) = track_key(props, idx, "enabled") {
        return None;
    }

    let type_name = match track_key(props, idx, "type") {
        Some(Variant::String(s)) => s.clone(),
        _ => return None,
    };
    let track_type = tscn_track_type(&type_name)?;

    // `path` is a NodePath: names address the node relative to the mixer root,
    // subnames the animated property (`Sprite2D:position:x`). 3D transform
    // tracks carry no subname — the track type names the property.
    let (node_path, mut property_path) = match track_key(props, idx, "path") {
        Some(Variant::NodePath(np)) => {
            let names: Vec<&str> = (0..np.get_name_count())
                .filter_map(|i| np.get_name(i))
                .collect();
            (names.join("/"), np.get_concatenated_subnames())
        }
        // A `path = "Sprite2D:position"` string form (hand-written scenes).
        Some(Variant::String(s)) => match s.split_once(':') {
            Some((node, prop)) => (node.to_string(), prop.to_string()),
            None => (s.clone(), String::new()),
        },
        _ => (String::new(), String::new()),
    };
    if property_path.is_empty() {
        property_path = implicit_property_path(track_type)?.to_string();
    }

    let mut track = AnimationTrack::with_type(node_path, property_path, track_type);

    // `interp` selects the per-track curve; Godot's NEAREST/LINEAR are expressed
    // in Patina as the per-keyframe `TransitionType` under `PerKey`.
    let interp = track_key(props, idx, "interp")
        .and_then(variant_to_i64)
        .unwrap_or(1);
    track.interpolation = match interp {
        2 => TrackInterpolation::Cubic,
        3 => TrackInterpolation::LinearAngle,
        4 => TrackInterpolation::CubicAngle,
        _ => TrackInterpolation::PerKey,
    };
    let per_key_transition = if interp == 0 {
        TransitionType::Nearest
    } else {
        TransitionType::Linear
    };

    let keys = track_key(props, idx, "keys")?;
    match track_type {
        TrackType::Position3D | TrackType::Scale3D | TrackType::Rotation3D
            if matches!(keys, Variant::PackedFloat32Array(_) | Variant::Array(_)) =>
        {
            fill_transform_track(&mut track, track_type, keys, per_key_transition);
        }
        // Both of these use `times` alongside a payload array that is neither
        // `values` nor interleaved transform floats, so neither fits the
        // dictionary/transform fillers (pat-s3rrv.44).
        TrackType::Bezier => fill_bezier_track(&mut track, keys),
        TrackType::Animation => fill_animation_track(&mut track, keys),
        _ => fill_dictionary_track(&mut track, track_type, keys, per_key_transition),
    }

    (!track.keyframes().is_empty()).then_some(track)
}

/// Maps a `.tscn` `tracks/N/type` string to a [`TrackType`].
///
/// A type Patina cannot sample is reported as unsupported rather than silently
/// mis-read as a value track, which would drive the property with the wrong
/// curve instead of leaving it alone.
fn tscn_track_type(name: &str) -> Option<TrackType> {
    match name {
        "value" => Some(TrackType::Property),
        "method" => Some(TrackType::Method),
        "audio" => Some(TrackType::Audio),
        "position_3d" => Some(TrackType::Position3D),
        "rotation_3d" => Some(TrackType::Rotation3D),
        "scale_3d" => Some(TrackType::Scale3D),
        "blend_shape" => Some(TrackType::BlendShape),
        "bezier" => Some(TrackType::Bezier),
        "animation" => Some(TrackType::Animation),
        _ => None,
    }
}

/// The property a track drives when its `path` carries no subname — Godot's 3D
/// transform tracks name the property through the track type itself.
fn implicit_property_path(track_type: TrackType) -> Option<&'static str> {
    match track_type {
        TrackType::Position3D => Some("position"),
        TrackType::Rotation3D => Some("quaternion"),
        TrackType::Scale3D => Some("scale"),
        // An animation track addresses a nested AnimationPlayer as a whole, not
        // one of its properties, so its `path` carries no subname by design.
        // The label mirrors how audio tracks use `property_path` (pat-s3rrv.44).
        TrackType::Animation => Some("animation"),
        // A value/method/audio/blend-shape/bezier track with no subname has
        // nothing to address; Godot's own cache build rejects it too. A bézier
        // track in particular MUST name a scalar subname such as `position:x`.
        _ => None,
    }
}

/// Fills a bézier track from the `{"times": ..., "points": ...}` form
/// (pat-s3rrv.44).
///
/// `points` is a FLAT array of **five floats per key** — `value`, `in_handle.x`,
/// `in_handle.y`, `out_handle.x`, `out_handle.y` — matching Godot's
/// `rv[i * 5 + n]` indexing in `animation.cpp`'s `_set`. It is not one entry per
/// key, so a key count taken from `points.len()` rather than from `times` would
/// be five times too large.
///
/// `handle_modes` (free / balanced / mirrored) is an EDITOR affordance that
/// constrains how dragging one handle moves the other; it does not alter the
/// sampled curve, so it is deliberately not read.
fn fill_bezier_track(track: &mut AnimationTrack, keys: &Variant) {
    let Variant::Dictionary(d) = keys else {
        return;
    };
    let times = d.get("times").map(variant_to_f64_vec).unwrap_or_default();
    let points = d.get("points").map(variant_to_f64_vec).unwrap_or_default();

    for (i, &time) in times.iter().enumerate() {
        let base = i * 5;
        // A truncated `points` array must drop the unbacked keys rather than
        // read a neighbour's handles as this key's value.
        let Some(chunk) = points.get(base..base + 5) else {
            break;
        };
        track.add_keyframe(KeyFrame::bezier(
            time,
            chunk[0],
            Vector2::new(chunk[1] as f32, chunk[2] as f32),
            Vector2::new(chunk[3] as f32, chunk[4] as f32),
        ));
    }
}

/// Fills an animation track from the `{"times": ..., "clips": ...}` form
/// (pat-s3rrv.44).
///
/// Each key names the clip to start on the nested player at that time. Godot
/// writes the clips as a `PackedStringArray`; `StringName` entries are accepted
/// too, since a hand-written scene may spell them `&"name"`.
fn fill_animation_track(track: &mut AnimationTrack, keys: &Variant) {
    let Variant::Dictionary(d) = keys else {
        return;
    };
    let times = d.get("times").map(variant_to_f64_vec).unwrap_or_default();
    let clips: Vec<String> = match d.get("clips") {
        Some(Variant::PackedStringArray(v)) => v.clone(),
        Some(Variant::Array(items)) => items
            .iter()
            .map(|v| match v {
                Variant::String(s) => s.clone(),
                Variant::StringName(s) => s.as_str().to_string(),
                // A non-string entry names no clip. It becomes an empty
                // PLACEHOLDER rather than being dropped: `clips` is indexed in
                // lockstep with `times`, so compacting it here would shift every
                // later clip onto the wrong keyframe time.
                _ => String::new(),
            })
            .collect(),
        _ => Vec::new(),
    };

    for (i, &time) in times.iter().enumerate() {
        let Some(clip) = clips.get(i) else { break };
        if clip.is_empty() {
            continue;
        }
        track.add_keyframe(KeyFrame::linear(time, Variant::String(clip.clone())));
    }
}

/// Fills a value / method / audio / blend-shape track from the
/// `{"times": ..., "transitions": ..., "values": ...}` dictionary form.
fn fill_dictionary_track(
    track: &mut AnimationTrack,
    track_type: TrackType,
    keys: &Variant,
    per_key_transition: TransitionType,
) {
    let Variant::Dictionary(d) = keys else {
        return;
    };
    let times = d.get("times").map(variant_to_f64_vec).unwrap_or_default();
    let values: Vec<Variant> = match d.get("values") {
        Some(Variant::Array(items)) => items.clone(),
        Some(other) => variant_to_f64_vec(other)
            .into_iter()
            .map(Variant::Float)
            .collect(),
        None => Vec::new(),
    };
    // Godot's per-key `transitions` entry is an easing *exponent*, not a curve
    // selector, and is 1.0 (plain linear) for every key the editor writes.
    // Patina's `TransitionType` has no exponent form, so the exponent is not
    // read — the track-level `interp` above carries the curve.

    // `update` only applies to value tracks (Godot writes it there only).
    if track_type == TrackType::Property {
        track.update_mode = match d.get("update").and_then(variant_to_i64) {
            // 1 = discrete, 2 = trigger (fires at the key, never interpolates).
            Some(1) | Some(2) => TrackUpdateMode::Discrete,
            Some(3) => TrackUpdateMode::Capture,
            _ => TrackUpdateMode::Continuous,
        };
    }

    for (i, &time) in times.iter().enumerate() {
        let Some(value) = values.get(i) else { continue };
        let value = match track_type {
            TrackType::Method => match method_key_to_array(value) {
                Some(v) => v,
                None => continue,
            },
            _ => value.clone(),
        };
        track.add_keyframe(KeyFrame::new(time, value, per_key_transition));
    }
}

/// Converts Godot's method-track keyframe `{"method": &"name", "args": [...]}`
/// into the `[name, arg0, ...]` array [`crate::animation`] samples.
fn method_key_to_array(value: &Variant) -> Option<Variant> {
    match value {
        // Already in the array form (hand-written scenes / Patina's own saver).
        Variant::Array(_) => Some(value.clone()),
        Variant::Dictionary(d) => {
            let method = match d.get("method")? {
                Variant::String(s) => s.clone(),
                Variant::StringName(s) => s.as_str().to_string(),
                _ => return None,
            };
            let mut items = vec![Variant::String(method)];
            if let Some(Variant::Array(args)) = d.get("args") {
                items.extend(args.iter().cloned());
            }
            Some(Variant::Array(items))
        }
        _ => None,
    }
}

/// Fills a 3D transform track from Godot's flat float layout: position/scale
/// keys are `(time, transition, x, y, z)` and rotation keys are
/// `(time, transition, x, y, z, w)` (animation.cpp `Animation::_set`).
fn fill_transform_track(
    track: &mut AnimationTrack,
    track_type: TrackType,
    keys: &Variant,
    per_key_transition: TransitionType,
) {
    let floats = variant_to_f64_vec(keys);
    let stride = if track_type == TrackType::Rotation3D {
        6
    } else {
        5
    };
    if stride > floats.len() {
        return;
    }
    for chunk in floats.chunks_exact(stride) {
        let time = chunk[0];
        let value = if track_type == TrackType::Rotation3D {
            Variant::Quaternion(Quaternion {
                x: chunk[2] as f32,
                y: chunk[3] as f32,
                z: chunk[4] as f32,
                w: chunk[5] as f32,
            })
        } else {
            Variant::Vector3(Vector3::new(
                chunk[2] as f32,
                chunk[3] as f32,
                chunk[4] as f32,
            ))
        };
        track.add_keyframe(KeyFrame::new(time, value, per_key_transition));
    }
}

// ---------------------------------------------------------------------------
// AnimationPlayer (one `[node type="AnimationPlayer"]`)
// ---------------------------------------------------------------------------

/// Collects every animation reachable from an `[sub_resource
/// type="AnimationLibrary"]`, as `(clip_name, animation)`.
///
/// `library_name` is the key the player filed the library under; Godot
/// addresses clips of a non-default library as `"<library>/<clip>"` and clips of
/// the default (empty-named) library as just `"<clip>"`.
fn animations_from_library(
    scene: &PackedScene,
    library_name: &str,
    library_id: &str,
) -> Vec<(String, Animation)> {
    let Some(props) = scene.sub_resource_props(library_id) else {
        return Vec::new();
    };
    let Some(Variant::Dictionary(data)) = props.get("_data") else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for (clip, anim_ref) in data.iter() {
        let Variant::String(clip) = clip else {
            continue;
        };
        let Some(anim_id) = sub_resource_id(anim_ref) else {
            continue;
        };
        let Some(anim_props) = scene.sub_resource_props(&anim_id) else {
            continue;
        };
        let full_name = if library_name.is_empty() {
            clip.clone()
        } else {
            format!("{library_name}/{clip}")
        };
        out.push((
            full_name.clone(),
            animation_from_tscn_props(&full_name, anim_props),
        ));
    }
    out
}

/// Builds an [`AnimationPlayer`] from an `AnimationPlayer` node's `.tscn`
/// properties, resolving `libraries` against `scene`'s inline sub-resources.
///
/// Returns `None` when the node declares no resolvable library, so a player with
/// nothing to play is not attached (matching Godot, where such a node animates
/// nothing).
pub fn build_animation_player(
    scene: &PackedScene,
    props: &HashMap<String, Variant>,
) -> Option<AnimationPlayer> {
    let mut player = AnimationPlayer::new();

    // Godot's AnimationMixer resolves track paths from `root_node`, which
    // defaults to the player's parent — not the player itself.
    player.root_node = match props.get("root_node") {
        Some(Variant::NodePath(np)) => {
            let names: Vec<&str> = (0..np.get_name_count())
                .filter_map(|i| np.get_name(i))
                .collect();
            if names.is_empty() {
                GODOT_DEFAULT_ROOT_NODE.to_string()
            } else {
                names.join("/")
            }
        }
        Some(Variant::String(s)) if !s.is_empty() => s.clone(),
        _ => GODOT_DEFAULT_ROOT_NODE.to_string(),
    };

    if let Some(speed) = props.get("speed_scale").and_then(variant_to_f64) {
        player.speed_scale = speed;
    }
    if let Some(Variant::NodePath(np)) = props.get("root_motion_track") {
        let sub = np.get_concatenated_subnames();
        if !sub.is_empty() {
            player.root_motion_track = Some(sub);
        }
    }

    let mut any = false;
    if let Some(Variant::Dictionary(libraries)) = props.get("libraries") {
        for (lib_name, lib_ref) in libraries.iter() {
            let lib_name = match lib_name {
                Variant::String(s) => s.as_str(),
                _ => continue,
            };
            let Some(lib_id) = sub_resource_id(lib_ref) else {
                continue;
            };
            for (_, anim) in animations_from_library(scene, lib_name, &lib_id) {
                player.add_animation(anim);
                any = true;
            }
        }
    }
    if !any {
        return None;
    }

    if let Some(Variant::String(autoplay)) = props.get("autoplay") {
        if !autoplay.is_empty() {
            player.autoplay = Some(autoplay.clone());
        }
    }

    Some(player)
}

/// Walks the subtree rooted at `root_id` and attaches an [`AnimationPlayer`] to
/// every `AnimationPlayer` node the scene declared, starting each one's
/// `autoplay` clip (Godot's autoplay-on-ready).
///
/// This is the wiring that turns a `.tscn` AnimationPlayer into a running
/// animation; without it `attach_animation_player` had no production caller.
pub fn attach_scene_animation_players(
    tree: &mut SceneTree,
    root_id: NodeId,
    scene: &PackedScene,
) -> usize {
    let mut subtree = Vec::new();
    tree.collect_subtree_top_down(root_id, &mut subtree);

    // Build first (immutable node reads), attach second, so the tree is not
    // borrowed mutably while the node's property map is being read.
    let mut built = Vec::new();
    for &node_id in &subtree {
        let Some(node) = tree.get_node(node_id) else {
            continue;
        };
        if node.class_name() != "AnimationPlayer" {
            continue;
        }
        let props: HashMap<String, Variant> = node
            .properties()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if let Some(mut player) = build_animation_player(scene, &props) {
            // Godot autoplays when the player enters the tree; the scene has
            // just been added, so start it now. The queued `animation_started`
            // signal drains on the first `process_animations`.
            player.play_autoplay();
            built.push((node_id, player));
        }
    }

    let attached = built.len();
    for (node_id, player) in built {
        // The node came from the walk above, so this cannot fail; a stale id
        // would only mean the player is skipped.
        let _ = tree.attach_animation_player(node_id, player);
    }
    attached
}

// ---------------------------------------------------------------------------
// Variant helpers
// ---------------------------------------------------------------------------

/// Extracts the sub-resource id from a parsed `SubResource("id")` reference,
/// which the `.tscn` value parser stores as `Variant::String("SubResource:id")`.
fn sub_resource_id(value: &Variant) -> Option<String> {
    match value {
        Variant::String(s) => s.strip_prefix("SubResource:").map(String::from),
        _ => None,
    }
}

fn variant_to_f64(v: &Variant) -> Option<f64> {
    match v {
        Variant::Float(f) => Some(*f),
        Variant::Int(i) => Some(*i as f64),
        _ => None,
    }
}

fn variant_to_i64(v: &Variant) -> Option<i64> {
    match v {
        Variant::Int(i) => Some(*i),
        Variant::Float(f) => Some(*f as i64),
        _ => None,
    }
}

/// Reads a numeric sequence out of any of the shapes Godot writes for times /
/// transitions / flat transform keys: a `PackedFloat32Array`, a
/// `PackedFloat64Array`, or a plain `Array` of numbers.
fn variant_to_f64_vec(v: &Variant) -> Vec<f64> {
    match v {
        Variant::PackedFloat32Array(a) => a.iter().map(|f| *f as f64).collect(),
        Variant::PackedFloat64Array(a) => a.clone(),
        Variant::PackedInt32Array(a) => a.iter().map(|i| *i as f64).collect(),
        Variant::PackedInt64Array(a) => a.iter().map(|i| *i as f64).collect(),
        Variant::Array(items) => items.iter().filter_map(variant_to_f64).collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(source: &str) -> PackedScene {
        PackedScene::from_tscn(source).expect("scene parses")
    }

    /// The multi-line `tracks/N/keys` dictionary survives `.tscn` parsing —
    /// before pat-0q2kx.1 the sub-resource body dropped every container value.
    #[test]
    fn multiline_sub_resource_keys_are_parsed() {
        let s = scene(
            r#"[gd_scene format=3]

[sub_resource type="Animation" id="Animation_a"]
length = 2.0
tracks/0/type = "value"
tracks/0/path = NodePath("Sprite2D:position")
tracks/0/interp = 1
tracks/0/keys = {
"times": PackedFloat32Array(0, 2),
"transitions": PackedFloat32Array(1, 1),
"update": 0,
"values": [Vector2(0, 0), Vector2(100, 0)]
}

[node name="Main" type="Node2D"]
"#,
        );
        let props = s.sub_resource_props("Animation_a").expect("sub-resource");
        assert!(
            matches!(props.get("tracks/0/keys"), Some(Variant::Dictionary(_))),
            "multi-line keys dictionary parsed, got {:?}",
            props.get("tracks/0/keys")
        );

        let anim = animation_from_tscn_props("walk", props);
        assert_eq!(anim.length, 2.0);
        assert_eq!(anim.tracks.len(), 1, "one value track");
        assert_eq!(anim.tracks[0].node_path, "Sprite2D");
        assert_eq!(anim.tracks[0].property_path, "position");
        assert_eq!(anim.tracks[0].keyframes().len(), 2);
    }

    /// `loop_mode` and `interp` map onto Patina's loop/interpolation enums.
    #[test]
    fn loop_mode_and_interpolation_map_from_godot_ints() {
        let mut props = HashMap::new();
        props.insert("length".to_string(), Variant::Float(1.0));
        props.insert("loop_mode".to_string(), Variant::Int(1));
        props.insert(
            "tracks/0/type".to_string(),
            Variant::String("value".to_string()),
        );
        props.insert(
            "tracks/0/path".to_string(),
            Variant::NodePath(gdcore::node_path::NodePath::new("Node:scale")),
        );
        props.insert("tracks/0/interp".to_string(), Variant::Int(2));
        let mut keys = gdvariant::Dictionary::new();
        keys.insert("times", Variant::PackedFloat32Array(vec![0.0, 1.0]));
        keys.insert(
            "values",
            Variant::Array(vec![Variant::Float(1.0), Variant::Float(2.0)]),
        );
        props.insert("tracks/0/keys".to_string(), Variant::Dictionary(keys));

        let anim = animation_from_tscn_props("a", &props);
        assert_eq!(anim.loop_mode, LoopMode::Linear);
        assert_eq!(anim.tracks[0].interpolation, TrackInterpolation::Cubic);
    }

    /// A disabled track is dropped, matching Godot's cache build.
    #[test]
    fn disabled_track_is_skipped() {
        let mut props = HashMap::new();
        props.insert(
            "tracks/0/type".to_string(),
            Variant::String("value".to_string()),
        );
        props.insert("tracks/0/enabled".to_string(), Variant::Bool(false));
        props.insert(
            "tracks/0/path".to_string(),
            Variant::NodePath(gdcore::node_path::NodePath::new("Node:scale")),
        );
        let mut keys = gdvariant::Dictionary::new();
        keys.insert("times", Variant::PackedFloat32Array(vec![0.0]));
        keys.insert("values", Variant::Array(vec![Variant::Float(1.0)]));
        props.insert("tracks/0/keys".to_string(), Variant::Dictionary(keys));

        assert!(animation_from_tscn_props("a", &props).tracks.is_empty());
    }

    /// Godot's method-track keyframe dictionary becomes the `[name, args...]`
    /// array the sampler dispatches.
    #[test]
    fn method_track_dictionary_becomes_call_array() {
        let mut call = gdvariant::Dictionary::new();
        call.insert("method", Variant::String("on_step".to_string()));
        call.insert("args", Variant::Array(vec![Variant::Int(7)]));
        let converted = method_key_to_array(&Variant::Dictionary(call)).expect("converted");
        assert_eq!(
            converted,
            Variant::Array(vec![Variant::String("on_step".into()), Variant::Int(7)])
        );
    }

    /// 3D position keys are the flat `(time, transition, x, y, z)` layout.
    #[test]
    fn position_3d_track_reads_flat_float_layout() {
        let mut props = HashMap::new();
        props.insert(
            "tracks/0/type".to_string(),
            Variant::String("position_3d".to_string()),
        );
        props.insert(
            "tracks/0/path".to_string(),
            Variant::NodePath(gdcore::node_path::NodePath::new("Mesh")),
        );
        props.insert(
            "tracks/0/keys".to_string(),
            Variant::PackedFloat32Array(vec![0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 5.0, 0.0, 0.0]),
        );

        let anim = animation_from_tscn_props("a", &props);
        assert_eq!(anim.tracks.len(), 1);
        assert_eq!(anim.tracks[0].property_path, "position");
        let kfs = anim.tracks[0].keyframes();
        assert_eq!(kfs.len(), 2);
        assert_eq!(kfs[1].time, 1.0);
        assert_eq!(kfs[1].value, Variant::Vector3(Vector3::new(5.0, 0.0, 0.0)));
    }

    /// `bezier` and `animation` now map to their own track types (pat-s3rrv.44).
    ///
    /// This test previously asserted both were `None`. That was the honest
    /// contract while the sampler could not model them — mapping `bezier` to
    /// `Property` would have driven the property along a straight line through
    /// keys the artist curved. Now that both are sampled, the assertion inverts.
    #[test]
    fn bezier_and_animation_map_to_their_own_track_types() {
        assert_eq!(tscn_track_type("bezier"), Some(TrackType::Bezier));
        assert_eq!(tscn_track_type("animation"), Some(TrackType::Animation));
    }

    /// A type Patina still cannot sample must stay unmapped rather than fall
    /// back to a value track — the guard the inverted test above no longer
    /// provides.
    #[test]
    fn a_genuinely_unknown_track_type_is_still_skipped() {
        assert!(tscn_track_type("not_a_real_track_type").is_none());
        assert!(tscn_track_type("").is_none());
    }

    /// `libraries` + `autoplay` produce a player whose clips are keyed the way
    /// Godot names them, defaulting `root_node` to the player's parent.
    #[test]
    fn player_built_from_libraries_and_autoplay() {
        let s = scene(
            r#"[gd_scene format=3]

[sub_resource type="Animation" id="Animation_a"]
length = 1.0
tracks/0/type = "value"
tracks/0/path = NodePath("Sprite2D:position")
tracks/0/keys = {
"times": PackedFloat32Array(0, 1),
"values": [Vector2(0, 0), Vector2(10, 0)]
}

[sub_resource type="AnimationLibrary" id="AnimationLibrary_a"]
_data = {
"slide": SubResource("Animation_a")
}

[node name="Main" type="Node2D"]

[node name="AnimationPlayer" type="AnimationPlayer" parent="."]
libraries = {
"": SubResource("AnimationLibrary_a")
}
autoplay = "slide"
"#,
        );
        // Pull the player node's properties out of the instanced scene.
        let nodes = s.instance().expect("instance");
        let player_node = nodes
            .iter()
            .find(|n| n.class_name() == "AnimationPlayer")
            .expect("AnimationPlayer node");
        let props: HashMap<String, Variant> = player_node
            .properties()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let player = build_animation_player(&s, &props).expect("player built");
        assert_eq!(player.autoplay.as_deref(), Some("slide"));
        assert!(player.animations.contains_key("slide"));
        assert_eq!(
            player.root_node, "..",
            "root_node defaults to the player's parent, like Godot"
        );
    }

    /// A named library prefixes its clips, exactly as Godot addresses them.
    #[test]
    fn named_library_prefixes_clip_names() {
        let s = scene(
            r#"[gd_scene format=3]

[sub_resource type="Animation" id="Animation_a"]
length = 1.0
tracks/0/type = "value"
tracks/0/path = NodePath("Sprite2D:position")
tracks/0/keys = {
"times": PackedFloat32Array(0),
"values": [Vector2(0, 0)]
}

[sub_resource type="AnimationLibrary" id="Lib_a"]
_data = {
"walk": SubResource("Animation_a")
}

[node name="Main" type="Node2D"]
"#,
        );
        let anims = animations_from_library(&s, "hero", "Lib_a");
        assert_eq!(anims.len(), 1);
        assert_eq!(anims[0].0, "hero/walk");
    }

    /// A player with no resolvable library is not attached at all.
    #[test]
    fn player_without_libraries_is_not_built() {
        let s = scene("[gd_scene format=3]\n\n[node name=\"Main\" type=\"Node2D\"]\n");
        assert!(build_animation_player(&s, &HashMap::new()).is_none());
    }
}
