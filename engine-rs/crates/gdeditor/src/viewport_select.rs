//! Click-selection in the editor viewport.
//!
//! A click picks the topmost selectable node under the cursor and selects it;
//! a Shift/Ctrl-click toggles a node in or out of the current selection; and a
//! plain click on empty space clears the selection. Mirrors Godot's viewport
//! selection behaviour.

use gdscene::node::NodeId;
use std::collections::{HashMap, HashSet};

/// A screen-space rectangle (in viewport pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    /// Whether the rectangle contains the point (half-open on the far edges).
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.width && py >= self.y && py < self.y + self.height
    }
}

/// A selectable node in the viewport, with its screen rect and draw order.
#[derive(Debug, Clone, Copy)]
pub struct Selectable {
    /// The node this item represents.
    pub node: NodeId,
    /// The node's screen-space bounds.
    pub rect: Rect,
    /// Draw / z-order; higher is drawn on top and wins the topmost pick.
    pub z_order: i32,
}

/// The modifier held during a viewport click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickModifier {
    /// Plain click — replaces the selection.
    None,
    /// Shift or Ctrl click — toggles the hit node in/out of the selection.
    Toggle,
}

/// Returns the topmost selectable whose rect contains the point: the one with
/// the highest `z_order`, ties broken by later position in `items` (drawn
/// last). Returns `None` if no item is under the point.
pub fn topmost_at(items: &[Selectable], px: f32, py: f32) -> Option<NodeId> {
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.rect.contains(px, py))
        .max_by(|(ai, a), (bi, b)| a.z_order.cmp(&b.z_order).then(ai.cmp(bi)))
        .map(|(_, item)| item.node)
}

/// Returns all selectables under the point, ordered topmost-first (descending
/// `z_order`; ties broken so the later-drawn item comes first) — the stacking
/// order the overlap cycle walks through.
pub fn nodes_at(items: &[Selectable], px: f32, py: f32) -> Vec<NodeId> {
    let mut hits: Vec<(usize, &Selectable)> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.rect.contains(px, py))
        .collect();
    hits.sort_by(|(ai, a), (bi, b)| b.z_order.cmp(&a.z_order).then(bi.cmp(ai)));
    hits.into_iter().map(|(_, item)| item.node).collect()
}

/// The viewport's current node selection.
#[derive(Debug, Default, Clone)]
pub struct ViewportSelection {
    selected: Vec<NodeId>,
    /// Tracks the in-progress overlap cycle: `(px, py, index_in_stack)` of the
    /// last `click_cycle` so a repeated click at the same point advances.
    last_cycle: Option<(f32, f32, usize)>,
}

impl ViewportSelection {
    /// Creates an empty selection.
    pub fn new() -> Self {
        Self::default()
    }

    /// The currently selected nodes, in selection order.
    pub fn selected(&self) -> &[NodeId] {
        &self.selected
    }

    /// Whether `node` is currently selected.
    pub fn is_selected(&self, node: NodeId) -> bool {
        self.selected.contains(&node)
    }

    /// Whether nothing is selected.
    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }

    /// Handles a viewport click at `(px, py)` against `items`, updating the
    /// selection:
    /// - plain click on a node selects only that node;
    /// - Shift/Ctrl click toggles the hit node in/out of the selection;
    /// - plain click on empty space clears the selection;
    /// - Shift/Ctrl click on empty space leaves the selection unchanged.
    ///
    /// Returns the node that was hit (the topmost under the cursor), if any.
    pub fn click(
        &mut self,
        items: &[Selectable],
        px: f32,
        py: f32,
        modifier: ClickModifier,
    ) -> Option<NodeId> {
        let hit = topmost_at(items, px, py);
        match (hit, modifier) {
            (Some(node), ClickModifier::None) => {
                self.selected.clear();
                self.selected.push(node);
            }
            (Some(node), ClickModifier::Toggle) => {
                if let Some(pos) = self.selected.iter().position(|n| *n == node) {
                    self.selected.remove(pos);
                } else {
                    self.selected.push(node);
                }
            }
            (None, ClickModifier::None) => {
                self.selected.clear();
            }
            (None, ClickModifier::Toggle) => {}
        }
        self.last_cycle = None;
        hit
    }

    /// Cycles the selection through nodes stacked at `(px, py)`. The first click
    /// at a point selects the topmost node; each repeated click at the same
    /// point advances to the next node down in z-order, wrapping from the
    /// bottom back to the top. A click at a different point (or empty space)
    /// restarts the cycle. Returns the newly selected node, if any.
    pub fn click_cycle(&mut self, items: &[Selectable], px: f32, py: f32) -> Option<NodeId> {
        let stack = nodes_at(items, px, py);
        if stack.is_empty() {
            self.selected.clear();
            self.last_cycle = None;
            return None;
        }
        let index = match self.last_cycle {
            Some((lx, ly, last)) if lx == px && ly == py => (last + 1) % stack.len(),
            _ => 0,
        };
        let node = stack[index];
        self.selected.clear();
        self.selected.push(node);
        self.last_cycle = Some((px, py, index));
        Some(node)
    }

    /// Handles a viewport click honoring Godot's locked/grouped selection rules
    /// and replaces the selection with the resolved node:
    /// - nodes in `locked` are not selectable (skipped during the topmost pick);
    /// - if the picked node is a member of a group (`group_roots` maps a node to
    ///   its group root), the group root is selected instead of the member.
    ///
    /// Returns the node that ends up selected, or `None` if the click hits no
    /// selectable node (which clears the selection).
    pub fn click_with_rules(
        &mut self,
        items: &[Selectable],
        px: f32,
        py: f32,
        locked: &HashSet<NodeId>,
        group_roots: &HashMap<NodeId, NodeId>,
    ) -> Option<NodeId> {
        let hit = items
            .iter()
            .enumerate()
            .filter(|(_, item)| !locked.contains(&item.node) && item.rect.contains(px, py))
            .max_by(|(ai, a), (bi, b)| a.z_order.cmp(&b.z_order).then(ai.cmp(bi)))
            .map(|(_, item)| item.node);

        self.last_cycle = None;
        match hit {
            Some(node) => {
                let resolved = *group_roots.get(&node).unwrap_or(&node);
                self.selected.clear();
                self.selected.push(resolved);
                Some(resolved)
            }
            None => {
                self.selected.clear();
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect {
            x,
            y,
            width: w,
            height: h,
        }
    }

    /// Acceptance (pat-wreth): clicking selects the topmost selectable node at
    /// that point; Shift/Ctrl-click toggles a node in/out of the selection; and
    /// clicking empty space clears it.
    #[test]
    fn viewport_select_click() {
        let a = NodeId::next();
        let b = NodeId::next();
        let c = NodeId::next();

        // `a` and `b` overlap around (15,15); `b` is on top (higher z_order).
        // `c` sits apart.
        let items = vec![
            Selectable {
                node: a,
                rect: rect(0.0, 0.0, 30.0, 30.0),
                z_order: 0,
            },
            Selectable {
                node: b,
                rect: rect(10.0, 10.0, 30.0, 30.0),
                z_order: 1,
            },
            Selectable {
                node: c,
                rect: rect(100.0, 100.0, 10.0, 10.0),
                z_order: 0,
            },
        ];

        let mut sel = ViewportSelection::new();

        // Click in the overlap selects the topmost node (b).
        assert_eq!(
            sel.click(&items, 15.0, 15.0, ClickModifier::None),
            Some(b),
            "topmost node under the cursor is selected"
        );
        assert_eq!(sel.selected(), &[b]);

        // A plain click elsewhere replaces the selection.
        sel.click(&items, 105.0, 105.0, ClickModifier::None);
        assert_eq!(sel.selected(), &[c]);

        // Shift/Ctrl click toggles a node into the selection without clearing.
        sel.click(&items, 15.0, 15.0, ClickModifier::Toggle);
        assert!(
            sel.is_selected(c) && sel.is_selected(b),
            "toggle-click adds to the selection"
        );

        // Toggle-clicking the same node again removes it.
        sel.click(&items, 15.0, 15.0, ClickModifier::Toggle);
        assert!(!sel.is_selected(b), "toggle-click removes an already-selected node");
        assert!(sel.is_selected(c));

        // Clicking empty space clears the selection.
        assert_eq!(sel.click(&items, 500.0, 500.0, ClickModifier::None), None);
        assert!(
            sel.is_empty(),
            "a plain click on empty space clears the selection"
        );
    }

    #[test]
    fn toggle_click_empty_space_keeps_selection() {
        let a = NodeId::next();
        let items = vec![Selectable {
            node: a,
            rect: rect(0.0, 0.0, 10.0, 10.0),
            z_order: 0,
        }];
        let mut sel = ViewportSelection::new();
        sel.click(&items, 5.0, 5.0, ClickModifier::None);
        assert_eq!(sel.selected(), &[a]);

        // Toggle-click on empty space must not clear the selection.
        assert_eq!(sel.click(&items, 50.0, 50.0, ClickModifier::Toggle), None);
        assert_eq!(sel.selected(), &[a]);
    }

    /// Acceptance (pat-8ryxl): clicking repeatedly at a point with stacked
    /// nodes cycles the selection through each overlapping node in z-order.
    #[test]
    fn viewport_select_overlap_cycle() {
        let a = NodeId::next(); // bottom (z 0)
        let b = NodeId::next(); // middle (z 1)
        let c = NodeId::next(); // top    (z 2)
        let items = vec![
            Selectable {
                node: a,
                rect: rect(0.0, 0.0, 10.0, 10.0),
                z_order: 0,
            },
            Selectable {
                node: b,
                rect: rect(0.0, 0.0, 10.0, 10.0),
                z_order: 1,
            },
            Selectable {
                node: c,
                rect: rect(0.0, 0.0, 10.0, 10.0),
                z_order: 2,
            },
        ];

        let mut sel = ViewportSelection::new();

        // Repeated clicks walk top -> middle -> bottom, then wrap to the top.
        assert_eq!(sel.click_cycle(&items, 5.0, 5.0), Some(c));
        assert_eq!(sel.selected(), &[c]);
        assert_eq!(sel.click_cycle(&items, 5.0, 5.0), Some(b));
        assert_eq!(sel.click_cycle(&items, 5.0, 5.0), Some(a));
        assert_eq!(
            sel.click_cycle(&items, 5.0, 5.0),
            Some(c),
            "cycling past the bottom wraps back to the top"
        );

        // Clicking a different point restarts the cycle at the topmost there.
        let d = NodeId::next();
        let items2 = vec![
            Selectable {
                node: c,
                rect: rect(0.0, 0.0, 10.0, 10.0),
                z_order: 2,
            },
            Selectable {
                node: d,
                rect: rect(100.0, 100.0, 10.0, 10.0),
                z_order: 0,
            },
        ];
        assert_eq!(
            sel.click_cycle(&items2, 105.0, 105.0),
            Some(d),
            "a click at a new point restarts the cycle at its topmost node"
        );

        // Clicking empty space clears the selection.
        assert_eq!(sel.click_cycle(&items2, 500.0, 500.0), None);
        assert!(sel.is_empty());
    }

    /// Acceptance (pat-bs6ux): a locked node is not selectable by viewport
    /// click, and clicking a child of a grouped node selects the group root.
    #[test]
    fn viewport_locked_grouped_selection() {
        let normal = NodeId::next();
        let locked_node = NodeId::next();
        let group_root = NodeId::next();
        let child = NodeId::next();

        let items = vec![
            Selectable {
                node: normal,
                rect: rect(0.0, 0.0, 10.0, 10.0),
                z_order: 0,
            },
            Selectable {
                node: locked_node,
                rect: rect(20.0, 0.0, 10.0, 10.0),
                z_order: 0,
            },
            Selectable {
                node: child,
                rect: rect(40.0, 0.0, 10.0, 10.0),
                z_order: 0,
            },
        ];

        let mut locked = HashSet::new();
        locked.insert(locked_node);
        let mut group_roots = HashMap::new();
        group_roots.insert(child, group_root);

        let mut sel = ViewportSelection::new();

        // A normal node is selectable.
        assert_eq!(
            sel.click_with_rules(&items, 5.0, 5.0, &locked, &group_roots),
            Some(normal)
        );
        assert_eq!(sel.selected(), &[normal]);

        // A locked node is not selectable: the click hits nothing there.
        assert_eq!(
            sel.click_with_rules(&items, 25.0, 5.0, &locked, &group_roots),
            None
        );
        assert!(sel.is_empty(), "a locked node is not selectable by click");

        // Clicking a child of a grouped node selects the group root.
        assert_eq!(
            sel.click_with_rules(&items, 45.0, 5.0, &locked, &group_roots),
            Some(group_root)
        );
        assert_eq!(
            sel.selected(),
            &[group_root],
            "clicking a grouped child selects the group root"
        );
    }
}
