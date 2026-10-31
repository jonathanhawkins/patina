//! The `RenderingServer` canvas-item store behind Godot's `canvas_*` script API
//! (pat-0q2kx.213).
//!
//! Godot ground truth: `servers/rendering_server.cpp`. A script can bypass nodes
//! and draw straight into the canvas — `canvas_item_create()` hands back an RID,
//! `canvas_item_set_parent(item, node.get_canvas_item())` attaches it under a
//! node, and the `canvas_item_add_*` family appends draw commands that render at
//! the parent's global transform.
//!
//! This lives in `gdscript-interop` rather than a rendering crate on purpose:
//! the interpreter must **write** it and the viewport rasterizer must **read**
//! it, and `gdeditor` already depends on this crate while the reverse is not
//! (and must not be) true.
//!
//! # Why a process-global
//!
//! It mirrors the existing headless singleton state in [`crate::interpreter`]
//! (`ENGINE_SINGLETONS` and friends): Godot's RenderingServer *is* a process
//! singleton, and a script reaches it with no handle to any renderer. Tests that
//! assert on canvas contents must therefore call [`reset`] first — see its docs.

use gdcore::math::{Color, Rect2, Transform2D, Vector2};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// One draw command appended to a canvas item by the `canvas_item_add_*` family.
///
/// Deliberately a small closed set: these are the commands the rasterizer can
/// actually honour today. An unsupported `canvas_item_add_*` call is rejected at
/// the dispatch layer rather than stored and silently never drawn.
#[derive(Debug, Clone, PartialEq)]
pub enum CanvasCommand {
    /// `canvas_item_add_rect(item, rect, color)`.
    Rect {
        /// The rectangle, in the canvas item's local space.
        rect: Rect2,
        /// Fill color.
        color: Color,
    },
    /// `canvas_item_add_circle(item, pos, radius, color)`.
    Circle {
        /// Center, in the canvas item's local space.
        center: Vector2,
        /// Radius in pixels.
        radius: f32,
        /// Fill color.
        color: Color,
    },
    /// `canvas_item_add_line(item, from, to, color, width)`.
    Line {
        /// Start point, local space.
        from: Vector2,
        /// End point, local space.
        to: Vector2,
        /// Stroke width in pixels.
        width: f32,
        /// Stroke color.
        color: Color,
    },
    /// `canvas_item_add_polygon(item, points, colors)`.
    Polygon {
        /// Vertices, local space.
        points: Vec<Vector2>,
        /// Fill color (Godot takes a per-vertex array; the first entry wins
        /// here, since the rasterizer fills a polygon with one color).
        color: Color,
    },
}

/// The mutable state of one canvas item.
#[derive(Debug, Clone)]
pub struct CanvasItemState {
    /// The RID this item is parented to — another canvas item, or a node's
    /// canvas item from [`node_canvas_item`]. `0` means unparented, which in
    /// Godot means "attached to no canvas" and therefore invisible.
    pub parent: u64,
    /// `canvas_item_set_visible`. Godot's canvas items start visible.
    pub visible: bool,
    /// `canvas_item_set_modulate`, multiplied into every command's color.
    pub modulate: Color,
    /// `canvas_item_set_z_index`, added to the parent node's z when sorting.
    pub z_index: i32,
    /// `canvas_item_set_transform`, applied under the parent's global transform.
    pub transform: Transform2D,
    /// The appended draw commands, in call order.
    pub commands: Vec<CanvasCommand>,
}

impl Default for CanvasItemState {
    fn default() -> Self {
        Self {
            parent: 0,
            visible: true,
            modulate: Color::new(1.0, 1.0, 1.0, 1.0),
            z_index: 0,
            transform: Transform2D::IDENTITY,
            commands: Vec::new(),
        }
    }
}

#[derive(Default)]
struct CanvasStore {
    items: HashMap<u64, CanvasItemState>,
    /// Node raw id -> the canvas-item RID standing for that node, so
    /// `get_canvas_item()` is stable across calls the way Godot's is.
    node_items: HashMap<u64, u64>,
}

fn store() -> &'static Mutex<CanvasStore> {
    static STORE: OnceLock<Mutex<CanvasStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(CanvasStore::default()))
}

/// RIDs start at 1 so that `0` keeps Godot's "invalid RID" meaning. Script
/// items and node canvas items draw from the same counter, so the two spaces
/// cannot collide.
static NEXT_RID: AtomicU64 = AtomicU64::new(1);

fn lock() -> std::sync::MutexGuard<'static, CanvasStore> {
    match store().lock() {
        Ok(g) => g,
        // A panic in one script must not poison the canvas for the whole
        // process; the store is plain data with no invariant to protect.
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// `RenderingServer.canvas_item_create()` — a fresh, visible, empty item.
pub fn canvas_item_create() -> u64 {
    let rid = NEXT_RID.fetch_add(1, Ordering::Relaxed);
    lock().items.insert(rid, CanvasItemState::default());
    rid
}

/// The canvas-item RID for a scene node, allocated on first ask and remembered
/// (`CanvasItem.get_canvas_item()`).
///
/// The returned RID has no state of its own — it is an attachment point that
/// script items parent themselves to, and the *node* supplies the transform when
/// the frame is built.
pub fn node_canvas_item(node_raw: u64) -> u64 {
    let mut s = lock();
    if let Some(&rid) = s.node_items.get(&node_raw) {
        return rid;
    }
    let rid = NEXT_RID.fetch_add(1, Ordering::Relaxed);
    s.node_items.insert(node_raw, rid);
    rid
}

/// The node a canvas-item RID stands for, if it came from [`node_canvas_item`].
pub fn node_for_canvas_item(rid: u64) -> Option<u64> {
    lock()
        .node_items
        .iter()
        .find(|(_, &v)| v == rid)
        .map(|(&k, _)| k)
}

/// `canvas_item_set_parent(item, parent)`.
pub fn canvas_item_set_parent(item: u64, parent: u64) -> bool {
    with_item(item, |it| it.parent = parent)
}

/// `canvas_item_set_visible(item, visible)`.
pub fn canvas_item_set_visible(item: u64, visible: bool) -> bool {
    with_item(item, |it| it.visible = visible)
}

/// `canvas_item_set_transform(item, transform)`.
pub fn canvas_item_set_transform(item: u64, transform: Transform2D) -> bool {
    with_item(item, |it| it.transform = transform)
}

/// `canvas_item_set_modulate(item, color)`.
pub fn canvas_item_set_modulate(item: u64, color: Color) -> bool {
    with_item(item, |it| it.modulate = color)
}

/// `canvas_item_set_z_index(item, z)`.
pub fn canvas_item_set_z_index(item: u64, z: i32) -> bool {
    with_item(item, |it| it.z_index = z)
}

/// `canvas_item_clear(item)` — drops the draw commands, keeping the item and its
/// parent/transform/visibility.
pub fn canvas_item_clear(item: u64) -> bool {
    with_item(item, |it| it.commands.clear())
}

/// `free_rid(item)` — forgets the item entirely.
pub fn free_canvas_item(item: u64) -> bool {
    lock().items.remove(&item).is_some()
}

/// Appends a command (`canvas_item_add_*`). Returns `false` for an unknown RID.
pub fn canvas_item_add(item: u64, command: CanvasCommand) -> bool {
    with_item(item, |it| it.commands.push(command))
}

fn with_item(item: u64, f: impl FnOnce(&mut CanvasItemState)) -> bool {
    match lock().items.get_mut(&item) {
        Some(it) => {
            f(it);
            true
        }
        None => false,
    }
}

/// Whether the item exists.
pub fn canvas_item_exists(item: u64) -> bool {
    lock().items.contains_key(&item)
}

/// A snapshot of the item, for tests and for the rasterizer.
pub fn canvas_item_state(item: u64) -> Option<CanvasItemState> {
    lock().items.get(&item).cloned()
}

/// Every **visible, non-empty** script canvas item attached to `node_raw`'s
/// canvas item, in RID order so the draw order is deterministic.
///
/// This is the rasterizer's entry point. Items whose parent RID is not this
/// node's are skipped, as are invisible ones — which is what makes
/// `canvas_item_set_visible(ci, false)` take effect on the next frame.
pub fn items_for_node(node_raw: u64) -> Vec<CanvasItemState> {
    let s = lock();
    let Some(&parent_rid) = s.node_items.get(&node_raw) else {
        // The node was never asked for its canvas item, so nothing can be
        // parented to it. Cheap early out for the common case.
        return Vec::new();
    };
    let mut found: Vec<(u64, CanvasItemState)> = s
        .items
        .iter()
        .filter(|(_, it)| it.parent == parent_rid && it.visible && !it.commands.is_empty())
        .map(|(&rid, it)| (rid, it.clone()))
        .collect();
    found.sort_by_key(|(rid, _)| *rid);
    found.into_iter().map(|(_, it)| it).collect()
}

/// Whether any node has been handed a canvas item yet.
///
/// The rasterizer checks this once per frame to skip the whole merge when no
/// script has touched the RenderingServer — the overwhelmingly common case.
pub fn has_any_node_items() -> bool {
    !lock().node_items.is_empty()
}

/// Clears all canvas state.
///
/// **Tests must call this first.** The store is a process global, so items left
/// by another test in the same binary would otherwise leak in and a test could
/// pass on a neighbour's rectangles.
pub fn reset() {
    let mut s = lock();
    s.items.clear();
    s.node_items.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Resets the process-global canvas store AND serializes this test against
    /// every other test in this module. Bind it for the whole test body:
    /// `let _guard = fresh();`.
    ///
    /// ⚠️ Each test used to open with a bare `reset()`, which is only safe
    /// when every test owns its own process — true under `cargo nextest run`,
    /// FALSE under `cargo test` and when this lib test binary is run directly,
    /// where libtest runs these tests as THREADS IN ONE PROCESS. One test's
    /// `reset()` then deleted another's canvas items mid-assertion, so
    /// `create_parent_add_and_clear` failed only sometimes: six identical runs
    /// of the lib binary scored 751 to 754 passing, with the failing set
    /// moving between this module and `editor_commands` (pat-21oab).
    #[must_use = "bind the guard for the whole test (`let _guard = fresh();`); \
                  dropping it immediately serializes nothing"]
    fn fresh() -> std::sync::MutexGuard<'static, ()> {
        static TEST_LOCK: Mutex<()> = Mutex::new(());
        // Poison-tolerant: one panicking test must not cascade into every
        // other test in the module failing with `PoisonError`.
        let guard = TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reset();
        guard
    }

    #[test]
    fn create_parent_add_and_clear() {
        let _guard = fresh();
        let node_rid = node_canvas_item(42);
        assert_eq!(
            node_canvas_item(42),
            node_rid,
            "get_canvas_item must be stable for a node"
        );

        let ci = canvas_item_create();
        assert_ne!(ci, node_rid, "RIDs must not collide across the two spaces");
        assert!(canvas_item_set_parent(ci, node_rid));
        assert!(canvas_item_add(
            ci,
            CanvasCommand::Rect {
                rect: Rect2 {
                    position: Vector2::ZERO,
                    size: Vector2::new(20.0, 20.0),
                },
                color: Color::new(1.0, 0.0, 0.0, 1.0),
            }
        ));

        assert_eq!(items_for_node(42).len(), 1, "the item reaches its node");

        // Invisible items are withheld from the frame...
        assert!(canvas_item_set_visible(ci, false));
        assert!(items_for_node(42).is_empty());
        assert!(canvas_item_set_visible(ci, true));

        // ...and clearing drops the commands while keeping the item.
        assert!(canvas_item_clear(ci));
        assert!(canvas_item_exists(ci));
        assert!(
            items_for_node(42).is_empty(),
            "a command-less item contributes nothing"
        );
    }

    #[test]
    fn unknown_rid_is_rejected_rather_than_created() {
        let _guard = fresh();
        assert!(!canvas_item_set_parent(9_999, 1));
        assert!(!canvas_item_clear(9_999));
        assert!(!canvas_item_exists(9_999));
    }

    #[test]
    fn unparented_item_reaches_no_node() {
        let _guard = fresh();
        let ci = canvas_item_create();
        canvas_item_add(
            ci,
            CanvasCommand::Circle {
                center: Vector2::ZERO,
                radius: 4.0,
                color: Color::new(0.0, 1.0, 0.0, 1.0),
            },
        );
        let node_rid = node_canvas_item(7);
        assert!(
            items_for_node(7).is_empty(),
            "an item with no parent must not draw under an unrelated node"
        );
        canvas_item_set_parent(ci, node_rid);
        assert_eq!(items_for_node(7).len(), 1);
    }

    /// ⚠️ Concurrency cover for the process-global canvas store, required by
    /// `CLAUDE.md` for server-side state. `NEXT_RID` is a single `AtomicU64`
    /// handing ids to whichever thread asks, so the invariant worth pinning is
    /// that two threads creating items at the same moment never receive the
    /// same RID and never leak an item onto each other's node — a non-atomic
    /// fetch-then-store would collide here and nowhere else in this module.
    #[test]
    fn concurrent_item_creation_hands_out_distinct_rids_per_node() {
        let _guard = fresh();
        const THREADS: u64 = 8;
        let handles: Vec<_> = (0..THREADS)
            .map(|n| {
                std::thread::spawn(move || {
                    let node = node_canvas_item(1_000 + n);
                    let ci = canvas_item_create();
                    assert!(canvas_item_set_parent(ci, node));
                    assert!(canvas_item_add(
                        ci,
                        CanvasCommand::Rect {
                            rect: Rect2 {
                                position: Vector2::ZERO,
                                size: Vector2::new(20.0, 20.0),
                            },
                            color: Color::new(1.0, 0.0, 0.0, 1.0),
                        }
                    ));
                    ci
                })
            })
            .collect();
        let rids: Vec<u64> = handles
            .into_iter()
            .map(|h| h.join().expect("canvas worker thread panicked"))
            .collect();

        let mut unique = rids.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            unique.len(),
            rids.len(),
            "two threads were handed the same canvas RID: {rids:?}"
        );
        for n in 0..THREADS {
            assert_eq!(
                items_for_node(1_000 + n).len(),
                1,
                "node {n} must see exactly its own item"
            );
        }
    }
}
