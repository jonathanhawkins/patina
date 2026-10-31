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
    match resolve_accessor(class_name, method, !args.is_empty())? {
        Accessor::Set { property } => {
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
