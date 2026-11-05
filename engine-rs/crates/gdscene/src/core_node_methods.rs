//! The core `Node` / `CanvasItem` / `Node3D` / `Viewport` script method
//! surface, plus the generic ClassDB accessor fallback (pat-mgixk.8).
//!
//! Every operation here already had a working, tested implementation on
//! [`SceneTree`] — `move_child`, `reparent`, `get_index`, `find_children`,
//! `get_path_to`, `propagate_call`, `node_groups`, `viewport::get_viewport` —
//! and **none of it was reachable from a script**. The audit measured the gap
//! as method spellings with no quoted literal anywhere in `gdscene/src` or
//! `gdscript-interop/src`: Node 99/121, CanvasItem 71/91, Node2D 22/33,
//! SceneTree 30/47. `$Sprite.hide()` raised `UndefinedFunction`, which aborts
//! the whole handler, so every later statement in `_ready` silently did not run
//! either.
//!
//! Two layers, in the order `call_node_method` consults them:
//!
//! 1. [`call_core_node_method`] — explicit arms for the helpers that are NOT
//!    property accessors (`hide`, `move_child`, `get_viewport`, …). These run
//!    with the rest of the per-subsystem dispatchers, so a class-specific
//!    meaning elsewhere in the table still wins where one exists.
//! 2. [`call_accessor_backed_method`] — the last resort. It resolves `get_X` /
//!    `set_X` / `is_X` against ClassDB's **registered** accessor names and
//!    routes them to the property, so every registered accessor works without
//!    a per-method arm.
//!
//! ⚠️ The fallback matches the recorded `getter`/`setter` strings and NEVER
//! derives a property name from the method name. `PropertyInfo::getter`'s own
//! documentation measures the `get_`/`is_`/`has_` convention at **51.7%** of
//! Godot's 5371 documented members: all six `PhysicsBody3D.axis_lock_*` share
//! one `get_axis_lock`, and `SpriteBase3D.alpha_cut`'s getter is
//! `get_alpha_cut_mode`. A name-derived rule gets those wrong in BOTH
//! directions — inventing a property that does not exist, and missing the one
//! that does.

use gdcore::math::{Rect2, Vector2};
use gdcore::node_path::NodePath;
use gdcore::string_name::StringName;
use gdscript_interop::bindings::SceneAccess;
use gdvariant::Variant;

use crate::node::NodeId;
use crate::scene_tree::SceneTree;

/// Reads an argument as an integer, accepting either Variant number form.
fn arg_int(args: &[Variant], index: usize) -> Option<i64> {
    match args.get(index) {
        Some(Variant::Int(i)) => Some(*i),
        Some(Variant::Float(f)) => Some(*f as i64),
        _ => None,
    }
}

/// Reads an argument as a string, accepting `String`, `StringName` and
/// `NodePath` — the three spellings a script writes a node path in.
fn arg_text(args: &[Variant], index: usize) -> Option<String> {
    match args.get(index) {
        Some(Variant::String(s)) => Some(s.clone()),
        Some(Variant::StringName(s)) => Some(s.as_str().to_owned()),
        Some(Variant::NodePath(p)) => Some(p.to_string()),
        _ => None,
    }
}

/// Reads an argument as a node handle.
fn arg_node(args: &[Variant], index: usize) -> Option<NodeId> {
    match args.get(index) {
        Some(Variant::ObjectId(id)) => Some(NodeId::from_object_id(*id)),
        _ => None,
    }
}

fn node_handle(id: NodeId) -> Variant {
    Variant::ObjectId(id.object_id())
}

/// Godot's `bool` conversion for an optional flag argument: a missing argument
/// takes `default`, and anything present follows Variant truthiness.
fn flag(args: &[Variant], index: usize, default: bool) -> bool {
    match args.get(index) {
        None => default,
        Some(v) => v.is_truthy(),
    }
}

/// The rect a `Viewport` lays its children out in — Godot's
/// `Viewport.get_visible_rect()` / `CanvasItem.get_viewport_rect()`.
///
/// ⚠️ This is the CANVAS size, not the OS window size, once the project turns
/// content scaling on: `RootWindow::visible_size` is `size_override`, so a
/// 640×360 base resolution in a 1920×1080 window reports 640×360. That is the
/// number a Control anchors against, and reporting the window size instead
/// would put every screen-edge anchor off by the stretch factor.
fn visible_rect(tree: &SceneTree) -> Rect2 {
    Rect2::new(Vector2::ZERO, tree.root_window().visible_size())
}

/// Whether `node_id` is a 3D node, which decides which visibility helper
/// answers for it.
///
/// `CanvasItem` and `Node3D` both have `visible` / `show()` / `hide()` /
/// `is_visible_in_tree()` in Godot, and the two have SEPARATE implementations
/// here (`node2d` and `node3d`) because the ancestor walk differs in what it
/// may cross. Asking the class rather than guessing is what keeps a Sprite2D
/// out of the 3D walk and vice versa.
fn is_3d(tree: &SceneTree, node_id: NodeId) -> bool {
    tree.get_node(node_id)
        .map(|n| n.is_class("Node3D"))
        .unwrap_or(false)
}

/// The explicit core-method arms. `None` means "not one of mine", so the
/// caller's table is reached unchanged.
pub fn call_core_node_method(
    tree: &mut SceneTree,
    node_id: NodeId,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    // Bail out before the class lookups below for every name this module does
    // not serve — `call_node_method` runs this for EVERY script method call on
    // EVERY node, so the common case has to be one match.
    if !matches!(
        method,
        "is_visible_in_tree"
            | "get_viewport"
            | "get_window"
            | "get_visible_rect"
            | "get_viewport_rect"
            | "get_index"
            | "move_child"
            | "reparent"
            | "has_node"
            | "get_path"
            | "get_path_to"
            | "get_groups"
            | "is_node_ready"
            | "find_children"
            | "find_parent"
            | "propagate_call"
            | "get_global_transform"
            | "get_global_transform_with_canvas"
    ) {
        return None;
    }
    // A stale handle names nothing; answering `None` lets the caller's own
    // table report the miss rather than inventing a value here.
    tree.get_node(node_id)?;

    match method {
        // -- CanvasItem / Node3D visibility ---------------------------------
        //
        // ⚠️ Node3D ALREADY registers `is_visible_in_tree` in ClassDB
        // (`class_db.rs:1558`) with no dispatch arm anywhere, so on a 3D node
        // it returned `Variant::Nil` SILENTLY rather than raising — the worse
        // of the two failures, because `has_method()` answered true and the
        // call did nothing.
        //
        // ⚠️ `hide` / `show` / `set_visible` / `is_visible` are NOT here: they
        // are served by `call_node_method`'s own table (pat-mgixk.6). This
        // module runs BEFORE that table, so adding them would SHADOW it with a
        // second implementation of the same thing rather than add a missing
        // one. Same for `find_child`, which the table has already.
        "is_visible_in_tree" => Some(Variant::Bool(if is_3d(tree, node_id) {
            crate::node3d::is_visible_in_tree(tree, node_id)
        } else {
            crate::node2d::is_visible_in_tree(tree, node_id)
        })),

        // -- CanvasItem transforms (pat-b4zhq) -------------------------------
        //
        // `get_global_transform_with_canvas` had no arm at all, and a Control's
        // `get_global_transform` answered Nil: the Node2D path serves it
        // through the `global_transform` PROPERTY, which a Control does not
        // have. The 3D platformer's virtual joystick scales its touch area by
        // `get_global_transform_with_canvas().get_scale()`.
        //
        // ⚠️ `get_global_transform` is claimed only for a Control. A Node3D
        // has its own (node3d.rs), and a Node2D keeps the path it already had.
        "get_global_transform" => {
            if !crate::control::node_is_control(tree, node_id) {
                return None;
            }
            Some(Variant::Transform2D(
                crate::canvas_render::canvas_item_global_transform(tree, node_id),
            ))
        }
        "get_global_transform_with_canvas" => {
            if is_3d(tree, node_id) {
                return None;
            }
            Some(Variant::Transform2D(
                crate::canvas_render::global_transform_with_canvas(tree, node_id),
            ))
        }

        // -- Viewport / Window ----------------------------------------------
        "get_viewport" => Some(node_handle(crate::viewport::get_viewport(tree, node_id))),
        "get_window" => Some(node_handle(crate::viewport::get_window(tree, node_id))),
        // `Viewport.get_visible_rect()` and `CanvasItem.get_viewport_rect()`
        // report the same rect in Godot; the difference is only which class you
        // may call it on.
        "get_visible_rect" | "get_viewport_rect" => Some(Variant::Rect2(visible_rect(tree))),

        // -- Node: ordering and parenting -----------------------------------
        //
        // Godot returns -1 for a node with no parent rather than failing.
        "get_index" => Some(Variant::Int(
            tree.get_index(node_id).map(|i| i as i64).unwrap_or(-1),
        )),
        "move_child" => {
            let child = arg_node(args, 0)?;
            let count = tree
                .get_node(node_id)
                .map(|n| n.children().len())
                .unwrap_or(0);
            // Godot: "If `to_index` is negative, the index is counted from the
            // end" (`doc/classes/Node.xml` `move_child`). Clamped rather than
            // wrapped, so `move_child(c, -1)` is the last slot and a wild index
            // lands at an end instead of panicking in the `usize` cast.
            let raw = arg_int(args, 1).unwrap_or(0);
            let to_index = if raw < 0 {
                (count as i64 + raw).max(0) as usize
            } else {
                (raw as usize).min(count.saturating_sub(1))
            };
            // A rejected move is reported in Godot, not silent; there is no
            // error channel on this return, so the Nil is the same answer the
            // successful call gives and the tree is simply unchanged.
            let _ = tree.move_child(node_id, child, to_index);
            Some(Variant::Nil)
        }
        // `reparent(new_parent, keep_global_transform = true)`. The second
        // argument is accepted and ignored: `SceneTree::reparent` preserves the
        // local transform, and a `false` here would have to CLEAR the global
        // transform — inventing that would be a wrong answer rather than a
        // missing one, so it is left to the bead that implements it.
        "reparent" => {
            let new_parent = arg_node(args, 0)?;
            let _ = tree.reparent(node_id, new_parent);
            Some(Variant::Nil)
        }

        // -- Node: paths and groups -----------------------------------------
        "has_node" => {
            let path = arg_text(args, 0)?;
            Some(Variant::Bool(
                tree.get_node_or_null(node_id, &path).is_some(),
            ))
        }
        // Godot types both of these `NodePath`, not `String`.
        "get_path" => Some(Variant::NodePath(NodePath::new(
            &tree.node_path(node_id).unwrap_or_default(),
        ))),
        "get_path_to" => {
            let target = arg_node(args, 0)?;
            Some(Variant::NodePath(NodePath::new(
                &tree.get_path_to(node_id, target).unwrap_or_default(),
            )))
        }
        // Godot's `get_groups()` is `Array[StringName]`.
        "get_groups" => Some(Variant::Array(
            tree.node_groups(node_id)
                .into_iter()
                .map(|g| Variant::StringName(StringName::new(&g)))
                .collect(),
        )),
        "is_node_ready" => Some(Variant::Bool(
            tree.get_node(node_id)
                .map(|n| n.is_ready())
                .unwrap_or(false),
        )),

        // -- Node: search ----------------------------------------------------
        //
        // `find_children(pattern, type = "", recursive = true, owned = true)`.
        // `owned` is accepted and ignored: Patina's nodes carry no `owner`, so
        // filtering on it would drop every match rather than fewer of them.
        "find_children" => {
            let pattern = arg_text(args, 0)?;
            let type_name = arg_text(args, 1).unwrap_or_default();
            let type_filter = if type_name.is_empty() {
                None
            } else {
                Some(type_name.as_str())
            };
            Some(Variant::Array(
                tree.find_children(node_id, &pattern, type_filter, flag(args, 2, true))
                    .into_iter()
                    .map(node_handle)
                    .collect(),
            ))
        }
        "find_parent" => {
            let pattern = arg_text(args, 0)?;
            Some(
                tree.find_parent(node_id, &pattern)
                    .map(node_handle)
                    .unwrap_or(Variant::Nil),
            )
        }

        // `propagate_call(method, args = [], parent_first = false)`.
        //
        // `SceneTree::propagate_call` is a traversal primitive over `&mut Node`,
        // which cannot call back into the tree — its own doc says "the GDScript
        // bridge supplies an `f` that invokes the named script method", and that
        // bridge is this arm. The closure therefore only COLLECTS the order, so
        // the script calls happen afterwards against the whole tree. Reusing the
        // primitive for the order rather than re-walking keeps `parent_first`
        // defined in exactly one place.
        "propagate_call" => {
            let target_method = arg_text(args, 0)?;
            let forwarded = match args.get(1) {
                Some(Variant::Array(items)) => items.to_vec(),
                _ => Vec::new(),
            };
            let parent_first = flag(args, 2, false);
            let mut order: Vec<NodeId> = Vec::new();
            tree.propagate_call(node_id, parent_first, |n| order.push(n.id()));
            for id in order {
                tree.call_script_with_access(id, &target_method, &forwarded);
            }
            Some(Variant::Nil)
        }

        // Unreachable: the guard above admits only the names handled here. An
        // arm rather than a panic, because an audio or input frame is the wrong
        // place to abort over a name.
        _ => None,
    }
}

/// What a method name resolves to in ClassDB's accessor tables.
enum Accessor {
    /// The method is `property`'s registered getter.
    Get { property: String, default: Variant },
    /// The method is `property`'s registered setter.
    Set { property: String },
}

/// Resolves `method` against the REGISTERED getter/setter names of every
/// property `class_name` declares, walking the inheritance chain.
///
/// ⚠️ Setter FIRST. `AudioEffectRecord.format` and friends can have a getter
/// and a setter whose names differ only by prefix, but a few classes register a
/// property whose getter and setter are the SAME recorded string; resolving as
/// a setter when an argument was passed, and as a getter otherwise, is decided
/// by the caller — here the two are kept apart so that choice is possible.
fn resolve_accessor(class_name: &str, method: &str, has_arg: bool) -> Option<Accessor> {
    // Godot's accessors are always `get_`/`is_`/`has_`/`set_`-shaped in
    // practice, so this is a cheap reject for the overwhelming majority of
    // calls before the ClassDB lock is taken — NOT a way of deriving the
    // property name, which is what the module header forbids.
    if !(method.starts_with("get_")
        || method.starts_with("set_")
        || method.starts_with("is_")
        || method.starts_with("has_"))
    {
        return None;
    }
    let properties = gdobject::class_db::get_property_list(class_name);
    // ⚠️ LAST match, not first: `get_property_list` is base-first, and a
    // derived class that redeclares a property appears after its base. Godot
    // reports the most-derived declaration, and `class_db::get_property_getter`
    // takes `next_back()` for exactly this reason.
    if has_arg {
        if let Some(p) = properties.iter().filter(|p| p.setter == method).next_back() {
            return Some(Accessor::Set {
                property: p.name.clone(),
            });
        }
    }
    if let Some(p) = properties.iter().filter(|p| p.getter == method).next_back() {
        return Some(Accessor::Get {
            property: p.name.clone(),
            default: p.default_value.clone(),
        });
    }
    // A setter called with no argument is still that setter; Godot would raise
    // on the arity. Answering it as a no-op write of Nil would silently clear
    // the property, so it is declined and the caller reports an unknown method.
    None
}

/// The property a `Light3D.Param` index names — which is **class-dependent**.
///
/// Transcribed from the `ADD_PROPERTYI(..., "set_param", "get_param", PARAM_*)`
/// bindings in `upstream/godot/scene/3d/light_3d.cpp:385-676`. That is the only
/// place the index↔property pairing is written down: `Light3D.xml` records
/// `getter="get_param"` on all thirteen members without saying which index each
/// one is, so the doc XML cannot answer this and the names cannot be derived.
///
/// ⚠️ Four indices name a DIFFERENT property per light type, which is why this
/// takes `class_name` and not just the index — `PARAM_RANGE` is `omni_range` on
/// an OmniLight3D and `spot_range` on a SpotLight3D.
///
/// ⚠️ `PARAM_SPOT_ATTENUATION` is `spot_angle_attenuation`, NOT
/// `spot_attenuation` — that one is `PARAM_ATTENUATION`. Deriving either from
/// the constant name swaps the two, and both are plain floats on the same node,
/// so nothing downstream would reject the mix-up.
fn light_param_property(class_name: &str, param: i64) -> Option<&'static str> {
    let directional = gdobject::class_db::is_parent_class(class_name, "DirectionalLight3D");
    let spot = gdobject::class_db::is_parent_class(class_name, "SpotLight3D");
    Some(match param {
        0 => "light_energy",
        1 => "light_indirect_energy",
        2 => "light_volumetric_fog_energy",
        3 => "light_specular",
        4 => {
            if spot {
                "spot_range"
            } else {
                "omni_range"
            }
        }
        // Godot binds BOTH spellings to PARAM_SIZE: the editor shows the
        // angular one on a directional light and the metric one elsewhere.
        5 => {
            if directional {
                "light_angular_distance"
            } else {
                "light_size"
            }
        }
        6 => {
            if spot {
                "spot_attenuation"
            } else {
                "omni_attenuation"
            }
        }
        7 => "spot_angle",
        8 => "spot_angle_attenuation",
        9 => "directional_shadow_max_distance",
        10 => "directional_shadow_split_1",
        11 => "directional_shadow_split_2",
        12 => "directional_shadow_split_3",
        13 => "directional_shadow_fade_start",
        14 => "shadow_normal_bias",
        15 => "shadow_bias",
        16 => "directional_shadow_pancake_size",
        17 => "shadow_opacity",
        18 => "shadow_blur",
        19 => "shadow_transmittance_bias",
        20 => {
            if directional {
                "light_intensity_lux"
            } else {
                "light_intensity_lumens"
            }
        }
        // PARAM_MAX (21) is a count, not a parameter, and Godot's own
        // `set_param` would index past the array. Answering `None` reports an
        // unknown method rather than writing somewhere arbitrary.
        _ => return None,
    })
}

/// `Light3D.set_param(index, value)` / `get_param(index)`, routed to the
/// property the index names.
///
/// Writes go through [`SceneAccess::set_node_property`] for the same reason the
/// generic arm does — that is the path the renderer's reader agrees with.
///
/// A missing or out-of-range index answers `None`, so the caller reports an
/// unknown method instead of a silent `Nil`. A one-argument `set_param` lands
/// there too: it has no value to write, and Godot raises on the arity.
/// `MeshInstance3D.set_surface_override_material(i, m)` /
/// `get_surface_override_material(i)` (pat-mgixk.31).
///
/// The property is `surface_material_override/<i>` — the spelling the scene
/// loader writes and `scene_saver.rs:1469` reads back — so a script write lands
/// on exactly the key the render path already consults, with no second model to
/// fall out of sync.
fn call_surface_override_material_method(
    access: &mut dyn SceneAccess,
    node: u64,
    class_name: &str,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    // ⚠️ Gated for the reason the Light3D arm is gated, which that arm's
    // comment records as a REGRESSION it had to fix: an ungated indexed family
    // answers for classes that never bound the name. Only `MeshInstance3D`
    // binds this one (`class_db.rs:1871-1896`).
    //
    // The exact class is accepted WITHOUT a ClassDB lookup: with no classes
    // registered (an embedder, a tool, a test) `is_parent_class` is false even
    // for "MeshInstance3D" itself, so the arm declined for the very class it
    // was written for and the getter raised as undispatched.
    if class_name != "MeshInstance3D"
        && !gdobject::class_db::is_parent_class(class_name, "MeshInstance3D")
    {
        return None;
    }
    let index = arg_int(args, 0)?;
    // ⚠️ A negative index declines rather than formatting
    // `surface_material_override/-1`. That would be a perfectly real property
    // key that nothing ever reads — the write would "succeed" and the material
    // would never appear, which is the worst of the available outcomes.
    if index < 0 {
        return None;
    }
    let property = format!("surface_material_override/{index}");
    if method == "set_surface_override_material" {
        // `?` on the value, so a one-argument call declines and the caller
        // reports an unknown method, as Godot does on an arity mismatch.
        let value = args.get(1)?.clone();
        access.set_node_property(node, &property, value);
        return Some(Variant::Nil);
    }
    // ⚠️ `Nil` IS correct for an unset index here, and deliberately NOT the
    // registered-default fallback the Light3D and particle arms use. Godot's
    // `get_surface_override_material` returns **null** when no override is set
    // (`scene/3d/mesh_instance_3d.cpp`), and `surface_material_override/N` is a
    // dynamic indexed property with no ClassDB entry to take a default from —
    // so there is nothing to fall back to and nothing that should be invented.
    Some(access.get_node_property(node, &property))
}

fn call_light_param_method(
    access: &mut dyn SceneAccess,
    node: u64,
    class_name: &str,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    // ⚠️ Gated, and this gate is a REGRESSION FIX, not caution. `set_param` is
    // overloaded in Godot: Light3D binds it, and so do PinJoint3D,
    // HingeJoint3D, SliderJoint3D and ConeTwistJoint3D — 26 registered
    // properties between them, each with its OWN param enum and its own
    // property names (`params/bias`, `angular_limit/upper`, …). Ungated, this
    // function answered a joint's `set_param(PARAM_BIAS, 0.3)` out of the
    // LIGHT table and wrote `light_energy` onto the joint. Those joints were
    // already served by the reverse lookup below before this arm existed, so
    // leaving it ungated took working classes backwards.
    if !gdobject::class_db::is_parent_class(class_name, "Light3D") {
        return None;
    }
    let property = light_param_property(class_name, arg_int(args, 0)?)?;
    if method == "set_param" {
        let value = args.get(1)?.clone();
        access.set_node_property(node, property, value);
        return Some(Variant::Nil);
    }
    // The REGISTERED default for a parameter the scene never wrote, matching
    // the generic getter arm: a `Nil` reads as "no such property" to the
    // interpreter, which would break `light.get_param(P) + 1` on a fresh node.
    let value = access.get_node_property(node, property);
    Some(if matches!(value, Variant::Nil) {
        gdobject::class_db::get_property_list(class_name)
            .iter()
            .filter(|p| p.name == property)
            .next_back()
            .map(|p| p.default_value.clone())
            .unwrap_or(Variant::Nil)
    } else {
        value
    })
}

/// The property a `PhysicsServer3D.BodyAxis` flag names.
///
/// Transcribed from the `ADD_PROPERTYI(..., "set_axis_lock", "get_axis_lock",
/// PhysicsServer3D::BODY_AXIS_*)` bindings in
/// `upstream/godot/scene/3d/physics/physics_body_3d.cpp:46-51`, paired with the
/// enum in `servers/physics_3d/physics_server_3d.h:493`.
///
/// ⚠️ The flags are **powers of two**, not `0..5`. A positional table would put
/// `BODY_AXIS_LINEAR_Y` (2) in `linear_z`'s slot and shift every axis after it.
/// The six properties are what `physics_server_3d.rs:1709-1717` reads into
/// `body.axis_lock_linear` / `axis_lock_angular` **every tick**, so a write that
/// missed them would leave the body unfrozen.
fn axis_lock_property(axis: i64) -> Option<&'static str> {
    Some(match axis {
        1 => "axis_lock_linear_x",
        2 => "axis_lock_linear_y",
        4 => "axis_lock_linear_z",
        8 => "axis_lock_angular_x",
        16 => "axis_lock_angular_y",
        32 => "axis_lock_angular_z",
        // Not a single BodyAxis value: Godot's `set_axis_lock` takes one axis,
        // never an OR-ed mask, so a combined or unknown flag is an error rather
        // than something to apply partially.
        _ => return None,
    })
}

/// `PhysicsBody3D.set_axis_lock(axis, lock)` / `get_axis_lock(axis)`.
///
/// Gated to PhysicsBody3D and below: Godot binds these on that class, so a
/// plain Node3D answering them would invent API. A missing or unrecognised axis
/// answers `None`, which reports an unknown method rather than freezing the
/// wrong axis or silently doing nothing.
fn call_axis_lock_method(
    access: &mut dyn SceneAccess,
    node: u64,
    class_name: &str,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    if !gdobject::class_db::is_parent_class(class_name, "PhysicsBody3D") {
        return None;
    }
    let property = axis_lock_property(arg_int(args, 0)?)?;
    if method == "set_axis_lock" {
        let locked = match args.get(1)? {
            Variant::Bool(b) => *b,
            // Godot coerces the second argument to bool; an int 0/1 is the
            // common script spelling and must not be read as "no value".
            Variant::Int(i) => *i != 0,
            _ => return None,
        };
        access.set_node_property(node, property, Variant::Bool(locked));
        return Some(Variant::Nil);
    }
    // `false` for an axis never written, matching the engine's own reader:
    // `truthy_prop_3d(tree, nid, "axis_lock_linear_x", false)` defaults the
    // same way, and a `Nil` here would read as "no such property".
    Some(Variant::Bool(matches!(
        access.get_node_property(node, property),
        Variant::Bool(true)
    )))
}

/// The property-name STEM a `CPUParticles3D.Parameter` index names; the caller
/// appends `_min` or `_max`.
///
/// Transcribed from the `ADD_PROPERTYI(..., "set_param_min"/"set_param_max",
/// PARAM_*)` bindings in `upstream/godot/scene/3d/cpu_particles_3d.cpp:1685-1737`.
/// The enum (`cpu_particles_3d.h`) has no initialisers, so it is sequential from
/// zero.
///
/// ⚠️ `PARAM_SCALE` is `scale_amount_*`, NOT `scale_*` — the one stem that is
/// not the lower-cased constant name. All 24 resulting names were checked
/// against the CPUParticles3D registration before this table was written.
fn particle_param_stem(param: i64) -> Option<&'static str> {
    Some(match param {
        0 => "initial_velocity",
        1 => "angular_velocity",
        2 => "orbit_velocity",
        3 => "linear_accel",
        4 => "radial_accel",
        5 => "tangential_accel",
        6 => "damping",
        7 => "angle",
        8 => "scale_amount",
        9 => "hue_variation",
        10 => "anim_speed",
        11 => "anim_offset",
        // PARAM_MAX (12) is a count, not a parameter.
        _ => return None,
    })
}

/// `CPUParticles3D.set_param_min/max(param, value)` and their getters.
///
/// Gated to CPUParticles3D: `CPUParticles2D` and `ParticleProcessMaterial`
/// register the same accessor names over their own property sets, and answering
/// those out of this table would be the same class-confusion bug the Light3D
/// gate above exists to prevent.
///
/// `main_loop.rs:414` reads `initial_velocity_min` (and the `_max` side) into
/// the frame's `ParticleEmitter`, so these writes are observable as simulation
/// behaviour rather than inert node state.
fn call_particle_param_method(
    access: &mut dyn SceneAccess,
    node: u64,
    class_name: &str,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    if !gdobject::class_db::is_parent_class(class_name, "CPUParticles3D") {
        return None;
    }
    let stem = particle_param_stem(arg_int(args, 0)?)?;
    let suffix = if method.ends_with("_min") {
        "min"
    } else {
        "max"
    };
    let property = format!("{stem}_{suffix}");
    if method.starts_with("set_") {
        let value = args.get(1)?.clone();
        access.set_node_property(node, &property, value);
        return Some(Variant::Nil);
    }
    // The registered default for a parameter the scene never wrote, matching
    // the generic getter arm — a `Nil` reads as "no such property" to the
    // interpreter.
    let value = access.get_node_property(node, &property);
    Some(if matches!(value, Variant::Nil) {
        gdobject::class_db::get_property_list(class_name)
            .iter()
            .filter(|p| p.name == property)
            .next_back()
            .map(|p| p.default_value.clone())
            .unwrap_or(Variant::Nil)
    } else {
        value
    })
}

/// The property a joint's `Param` index names, per class.
///
/// Transcribed from the `ADD_PROPERTYI(..., "set_param", "get_param", PARAM_*)`
/// bindings in `upstream/godot/scene/3d/physics/joints/*.cpp`; the enums alias
/// `PhysicsServer3D::{Pin,Hinge,Slider,ConeTwist}JointParam`
/// (`servers/physics_3d/physics_server_3d.h`) and are sequential from zero.
///
/// ⚠️ **The property-name shape differs per class, so this cannot be one
/// table.** PinJoint3D prefixes (`params/bias`) while ConeTwistJoint3D is bare
/// (`bias`, `softness`). HingeJoint3D's limit keys are `angular_limit/upper`
/// but SliderJoint3D's are `angular_limit/upper_angle`. Lower-casing the
/// constant name, or sharing one prefix, is wrong for at least three of the
/// four.
///
/// 🔑 Every index here maps to a key the solver actually reads —
/// `physics_server_3d.rs` pulls all 40 of these through
/// `float_prop_3d`/`truthy_prop_3d` when it syncs the joint. That is why
/// SliderJoint3D's `*_motion/*` and `*_ortho/*` params are included even though
/// ClassDB registers no `PropertyInfo` for them: ClassDB registration governs
/// editor/reflection visibility, **not** whether a write is live. The test for
/// "is this a dead write?" is whether a consumer reads the key, and here one
/// does.
fn joint_param_property(class_name: &str, param: i64) -> Option<&'static str> {
    let is = |c: &str| gdobject::class_db::is_parent_class(class_name, c);
    if is("PinJoint3D") {
        return Some(match param {
            0 => "params/bias",
            1 => "params/damping",
            2 => "params/impulse_clamp",
            _ => return None,
        });
    }
    if is("HingeJoint3D") {
        return Some(match param {
            0 => "params/bias",
            1 => "angular_limit/upper",
            2 => "angular_limit/lower",
            3 => "angular_limit/bias",
            4 => "angular_limit/softness",
            5 => "angular_limit/relaxation",
            6 => "motor/target_velocity",
            7 => "motor/max_impulse",
            _ => return None,
        });
    }
    if is("SliderJoint3D") {
        return Some(match param {
            0 => "linear_limit/upper_distance",
            1 => "linear_limit/lower_distance",
            2 => "linear_limit/softness",
            3 => "linear_limit/restitution",
            4 => "linear_limit/damping",
            5 => "linear_motion/softness",
            6 => "linear_motion/restitution",
            7 => "linear_motion/damping",
            8 => "linear_ortho/softness",
            9 => "linear_ortho/restitution",
            10 => "linear_ortho/damping",
            11 => "angular_limit/upper_angle",
            12 => "angular_limit/lower_angle",
            13 => "angular_limit/softness",
            14 => "angular_limit/restitution",
            15 => "angular_limit/damping",
            16 => "angular_motion/softness",
            17 => "angular_motion/restitution",
            18 => "angular_motion/damping",
            19 => "angular_ortho/softness",
            20 => "angular_ortho/restitution",
            21 => "angular_ortho/damping",
            _ => return None,
        });
    }
    if is("ConeTwistJoint3D") {
        // Bare keys, NOT `params/*`.
        return Some(match param {
            0 => "swing_span",
            1 => "twist_span",
            2 => "bias",
            3 => "softness",
            4 => "relaxation",
            _ => return None,
        });
    }
    None
}

/// `<Joint3D>.set_param(param, value)` / `get_param(param)`.
///
/// Answers `None` for a class with no table and for an out-of-range index
/// (every `PARAM_MAX`), so the caller reports an unknown method rather than
/// writing somewhere arbitrary.
fn call_joint_param_method(
    access: &mut dyn SceneAccess,
    node: u64,
    class_name: &str,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    let property = joint_param_property(class_name, arg_int(args, 0)?)?;
    if method == "set_param" {
        let value = args.get(1)?.clone();
        access.set_node_property(node, property, value);
        return Some(Variant::Nil);
    }
    Some(joint_property_or_default(
        access, node, class_name, property,
    ))
}

/// `HingeJoint3D.set_flag(flag, enabled)` / `get_flag(flag)`.
///
/// Gated to HingeJoint3D: it is the only joint with a `Flag` enum.
fn call_hinge_flag_method(
    access: &mut dyn SceneAccess,
    node: u64,
    class_name: &str,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    if !gdobject::class_db::is_parent_class(class_name, "HingeJoint3D") {
        return None;
    }
    let property = match arg_int(args, 0)? {
        0 => "angular_limit/enable",
        1 => "motor/enable",
        // FLAG_MAX (2) is a count, not a flag.
        _ => return None,
    };
    if method == "set_flag" {
        let enabled = match args.get(1)? {
            Variant::Bool(b) => *b,
            // Godot coerces to bool; an int 0/1 is a common script spelling and
            // must not read as "no value".
            Variant::Int(i) => *i != 0,
            _ => return None,
        };
        access.set_node_property(node, property, Variant::Bool(enabled));
        return Some(Variant::Nil);
    }
    Some(Variant::Bool(matches!(
        access.get_node_property(node, property),
        Variant::Bool(true)
    )))
}

/// A joint property's current value, falling back to the ClassDB default.
///
/// ⚠️ The fallback is `Nil` only when ClassDB registers no `PropertyInfo` for
/// the key — which is the case for SliderJoint3D's twelve `*_motion/*` and
/// `*_ortho/*` params. The solver supplies its own default there
/// (`float_prop_3d(tree, id, key, 1.0)` and friends), so a getter on an
/// unwritten one answering `Nil` would disagree with what the physics step
/// uses. Reported as remaining scope rather than papered over here: the right
/// fix is to register those twelve `PropertyInfo`s with the solver's defaults.
fn joint_property_or_default(
    access: &mut dyn SceneAccess,
    node: u64,
    class_name: &str,
    property: &str,
) -> Variant {
    let value = access.get_node_property(node, property);
    if !matches!(value, Variant::Nil) {
        return value;
    }
    gdobject::class_db::get_property_list(class_name)
        .iter()
        .filter(|p| p.name == property)
        .next_back()
        .map(|p| p.default_value.clone())
        .unwrap_or(Variant::Nil)
}

/// The last-resort arm: a registered property accessor, routed to the property.
///
/// Goes through [`SceneAccess`] rather than the tree directly, and that is
/// load-bearing. `set_node_property` re-routes a Node3D `position`/`rotation`/
/// `scale` write through the canonical local-transform setters, mirrors
/// `metadata/*` into the meta bag, and routes an AnimationTree parameter into
/// the graph. A raw `node.set_property` here would write a key the engine never
/// reads again — the exact shape of silently-dead wiring this bead exists to
/// remove.
///
/// `None` means "no registered accessor by that name", so the caller still
/// reports an unknown method rather than a quiet `Nil`.
pub(crate) fn call_accessor_backed_method(
    access: &mut dyn SceneAccess,
    node: u64,
    class_name: &str,
    method: &str,
    args: &[Variant],
) -> Option<Variant> {
    // `CanvasItem.get_global_mouse_position()` — the accessor already computes
    // it for the 2D input path and no script could reach it.
    if method == "get_global_mouse_position" {
        let (x, y) = access.get_global_mouse_position();
        return Some(Variant::Vector2(Vector2::new(x, y)));
    }

    // pat-mgixk.31: Light3D's parameters are INDEXED accessors — ONE `set_param`
    // keyed by a `Light3D.Param` constant, not one setter per property.
    //
    // ⚠️ This must stay AHEAD of the reverse lookup below, and the reason is a
    // live wrong answer rather than tidiness. `light_energy` registers
    // `set_param` as its setter, so `resolve_accessor` matches it and the
    // `Accessor::Set` arm writes `args[0]` — which for an indexed call is the
    // INDEX. `set_param(PARAM_RANGE, 8.0)` therefore wrote `light_energy = 4`:
    // the right property for exactly one index, and a silent corruption of
    // brightness for the other twenty.
    // ⚠️ `set_param` is overloaded across FIVE classes: Light3D and the four
    // joints, each with its own enum and its own property names. Each table
    // gates on its own class and answers `None` otherwise, so the chain tries
    // them in turn rather than one table guessing for all of them.
    if method == "set_param" || method == "get_param" {
        return call_light_param_method(access, node, class_name, method, args)
            .or_else(|| call_joint_param_method(access, node, class_name, method, args));
    }

    // pat-mgixk.31: HingeJoint3D's SECOND indexed family. Its two BOOL
    // properties travel on `set_flag`/`get_flag`, never `set_param`, so a
    // param-only table leaves `angular_limit/enable` and `motor/enable`
    // unreachable from a script.
    if method == "set_flag" || method == "get_flag" {
        return call_hinge_flag_method(access, node, class_name, method, args);
    }

    // pat-mgixk.31: `PhysicsBody3D.set_axis_lock(axis, lock)` is the same
    // indexed shape, keyed by a `PhysicsServer3D.BodyAxis` flag. All six
    // `axis_lock_*` properties share one `get_axis_lock`, so the reverse lookup
    // below would answer a 0-arg `get_axis_lock()` with whichever property
    // `next_back()` reached — `axis_lock_angular_z`.
    if method == "set_axis_lock" || method == "get_axis_lock" {
        return call_axis_lock_method(access, node, class_name, method, args);
    }

    // pat-mgixk.31: `CPUParticles3D.set_param_min/max(param, value)` — the
    // third indexed family, and the one where each index names a PAIR of
    // properties rather than one.
    if matches!(
        method,
        "set_param_min" | "get_param_min" | "set_param_max" | "get_param_max"
    ) {
        return call_particle_param_method(access, node, class_name, method, args);
    }
    // pat-mgixk.31: `MeshInstance3D.set/get_surface_override_material(i, ..)`
    // — the fourth indexed family, and the one the comment below already names
    // as unservable by the generic table.
    //
    // ⚠️ Until this existed the call was not merely unimplemented, it was
    // ENUMERATED AS IGNORED: `set_surface_override_material` sits in
    // `native_method_gaps::VOID_NOOP_METHODS`, so the interpreter returned
    // `Nil` on purpose and a script assigning a per-surface material saw no
    // error and no effect. (That list is keyed on Godot declaring the method
    // `void`, not on whether Patina implements it, so the entry stays: it is
    // still the right answer if this arm ever declines. `call_node_method`
    // runs BEFORE that fallback — interpreter.rs:16418 vs :16440 — so this arm
    // takes precedence.)
    if matches!(
        method,
        "set_surface_override_material" | "get_surface_override_material"
    ) {
        return call_surface_override_material_method(access, node, class_name, method, args);
    }
    match resolve_accessor(class_name, method, !args.is_empty())? {
        Accessor::Set { property } => {
            // ⚠️ EXACTLY one argument. A registered setter takes one value, so
            // a 2-argument call is an INDEXED accessor this table cannot serve
            // — `set_param(PARAM_X, v)` on a joint, `set_param_min(PARAM_X, v)`
            // on CPUParticles2D, `set_surface_override_material(i, m)`. Writing
            // `args[0]` for those stores the INDEX as the value, which is the
            // worst outcome available: the property is real, the write succeeds,
            // and the number is nonsense. Declining reports an unknown method,
            // which is what Godot does on an arity mismatch and what makes the
            // missing index table visible instead of silent.
            if args.len() != 1 {
                return None;
            }
            access.set_node_property(node, &property, args[0].clone());
            Some(Variant::Nil)
        }
        Accessor::Get { property, default } => {
            // ⚠️ The REGISTERED default, not `Nil`, for a property the scene
            // never wrote. Godot's getter always returns a typed value, and a
            // `Nil` here is worse than wrong: the interpreter's "is this a
            // property?" gate reads a Nil as "no such property", so
            // `node.size += 1` would die mid-frame on an untouched node.
            let value = access.get_node_property(node, &property);
            Some(if matches!(value, Variant::Nil) {
                default
            } else {
                value
            })
        }
    }
}
