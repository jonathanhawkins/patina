//! Multiplayer scene replication bridge (pat-u3l8c.80).
//!
//! gdplatform's `MultiplayerSpawner`/`MultiplayerSynchronizer` operate at the
//! network-packet level (they encode/send/observe spawn and sync packets).
//! This module bridges their **decoded** packets to a real [`SceneTree`]:
//! it instantiates spawned `PackedScene`s and applies synchronized property
//! values to the live nodes, tracking a `net_id -> NodeId` mapping so later
//! sync updates target the node the spawn created.

use std::collections::HashMap;

use gdplatform::{SpawnPacket, SyncPacket};
use gdvariant::Variant;

use crate::node::NodeId;
use crate::packed_scene::{add_packed_scene_to_tree, PackedScene};
use crate::scene_tree::SceneTree;

/// Bridges multiplayer spawn/sync packets to a [`SceneTree`].
#[derive(Debug, Default)]
pub struct MultiplayerReplicator {
    /// `net_id -> instantiated node id` for every spawn this replicator applied.
    spawned: HashMap<u32, NodeId>,
}

impl MultiplayerReplicator {
    /// Creates an empty replicator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Instantiates the scene named by a decoded [`SpawnPacket`] under `parent`
    /// and records its `net_id -> NodeId` mapping. `resolve` maps a scene path
    /// to its `.tscn` source (the caller controls resource loading). Returns the
    /// new node id, or `None` if the scene cannot be resolved or parsed.
    pub fn apply_spawn<F>(
        &mut self,
        tree: &mut SceneTree,
        parent: NodeId,
        packet: &SpawnPacket,
        resolve: F,
    ) -> Option<NodeId>
    where
        F: FnOnce(&str) -> Option<String>,
    {
        let source = resolve(&packet.scene_path)?;
        let scene = PackedScene::from_tscn(&source).ok()?;
        let node_id = add_packed_scene_to_tree(tree, parent, &scene).ok()?;
        self.spawned.insert(packet.net_id, node_id);
        Some(node_id)
    }

    /// The node instantiated for `net_id`, if this replicator spawned it.
    pub fn node_for(&self, net_id: u32) -> Option<NodeId> {
        self.spawned.get(&net_id).copied()
    }

    /// Number of tracked spawned instances.
    pub fn spawned_count(&self) -> usize {
        self.spawned.len()
    }

    /// Drops the mapping for `net_id`, returning the node it referred to
    /// (pat-s3rrv.17).
    ///
    /// The caller removes the node from the tree; this only forgets it, so a
    /// later SYNC frame addressed to a despawned id lands on nothing instead of
    /// resurrecting a stale `NodeId` that the tree may have reused.
    pub fn forget(&mut self, net_id: u32) -> Option<NodeId> {
        self.spawned.remove(&net_id)
    }

    /// Applies a decoded [`SyncPacket`] by writing each replicated property onto
    /// the node mapped to the packet's `net_id`. Returns `true` if that node was
    /// found and updated, `false` if the `net_id` is unknown or the node is gone.
    pub fn apply_sync(&mut self, tree: &mut SceneTree, packet: &SyncPacket) -> bool {
        let Some(&node_id) = self.spawned.get(&packet.net_id) else {
            return false;
        };
        let Some(node) = tree.get_node_mut(node_id) else {
            return false;
        };
        for (name, value) in &packet.properties {
            // The packet already carries real Variants (pat-s3rrv.17), so the
            // value is written through untouched. This used to run a
            // `decode_property_value` guess over a stringified payload, which
            // could only recover `Vector2` from an `"x,y"` spelling and handed
            // back `Variant::String` for everything else — so a replicated
            // `Vector3`, `Color` or `bool` landed on the node as a string and
            // silently mismatched the property it was written to.
            node.set_property(name, value.clone());
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const PLAYER_TSCN: &str = "[gd_scene format=3]\n\n[node name=\"Player\" type=\"Node2D\"]\n";

    fn unique_net_id() -> u32 {
        static N: AtomicUsize = AtomicUsize::new(1);
        N.fetch_add(1, Ordering::Relaxed) as u32
    }

    fn spawn_packet(net_id: u32) -> SpawnPacket {
        SpawnPacket {
            scene_path: "res://player.tscn".to_string(),
            data: String::new(),
            net_id,
            scene_index: 0,
        }
    }

    #[test]
    fn apply_spawn_instantiates_scene_under_parent() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut repl = MultiplayerReplicator::new();
        let net_id = unique_net_id();

        let node = repl
            .apply_spawn(&mut tree, root, &spawn_packet(net_id), |_| {
                Some(PLAYER_TSCN.to_string())
            })
            .expect("scene instantiated on spawn");
        assert_eq!(repl.node_for(net_id), Some(node));
        assert_eq!(repl.spawned_count(), 1);
        assert_eq!(tree.get_node(node).unwrap().name(), "Player");
    }

    #[test]
    fn apply_sync_updates_real_node_position() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut repl = MultiplayerReplicator::new();
        let net_id = unique_net_id();
        let node = repl
            .apply_spawn(&mut tree, root, &spawn_packet(net_id), |_| {
                Some(PLAYER_TSCN.to_string())
            })
            .unwrap();

        let sync = SyncPacket {
            net_id,
            properties: vec![(
                "position".to_string(),
                Variant::Vector2(gdcore::math::Vector2::new(10.0, 20.0)),
            )],
        };
        assert!(repl.apply_sync(&mut tree, &sync), "node found and updated");
        assert_eq!(
            tree.get_node(node).unwrap().get_property("position"),
            Variant::Vector2(gdcore::math::Vector2::new(10.0, 20.0))
        );
    }

    #[test]
    fn apply_sync_unknown_net_id_is_ignored() {
        let mut tree = SceneTree::new();
        let mut repl = MultiplayerReplicator::new();
        let sync = SyncPacket {
            net_id: 999,
            properties: vec![(
                "position".to_string(),
                Variant::Vector2(gdcore::math::Vector2::new(1.0, 2.0)),
            )],
        };
        assert!(
            !repl.apply_sync(&mut tree, &sync),
            "unknown net_id is a no-op"
        );
    }

    #[test]
    fn two_peer_spawn_and_sync_round_trip() {
        use gdplatform::{
            decode_spawn_packet, decode_sync_packet, encode_spawn_packet, encode_sync_packet,
            MultiplayerSpawner, MultiplayerSynchronizer,
        };

        // Host (peer 1): register a spawnable scene and spawn one instance.
        let mut host_spawner = MultiplayerSpawner::new(0);
        host_spawner.add_spawnable_scene("res://player.tscn");
        let req = host_spawner.spawn("skin=blue").expect("host spawns");

        // Wire: encode the spawn, decode it on the peer.
        let spawn = decode_spawn_packet(&encode_spawn_packet(&req)).expect("decode spawn");

        // Peer (peer 2): instantiate the spawned scene into its own tree.
        let mut peer_tree = SceneTree::new();
        let peer_root = peer_tree.root_id();
        let mut peer_repl = MultiplayerReplicator::new();
        let node = peer_repl
            .apply_spawn(&mut peer_tree, peer_root, &spawn, |_| {
                Some(PLAYER_TSCN.to_string())
            })
            .expect("peer instantiates spawned scene");
        assert_eq!(peer_tree.get_node(node).unwrap().name(), "Player");

        // Host authority sets a synced property; wire it to the peer.
        let mut host_sync = MultiplayerSynchronizer::new(0);
        host_sync.set_net_id(req.net_id);
        host_sync.set_property_value(
            "position",
            Variant::Vector2(gdcore::math::Vector2::new(10.0, 20.0)),
        );
        // ⚠️ The property that proves the payload is a real Variant. The old
        // string codec had exactly one spelling — `"x,y"` → `Vector2` — so a
        // `Vector3` could not survive it: it would arrive as
        // `Variant::String("...")` and mismatch the property. If the transport
        // ever regresses to strings, this assertion fails while the `position`
        // one above still passes.
        host_sync.set_property_value(
            "velocity",
            Variant::Vector3(gdcore::math::Vector3::new(1.0, 2.0, 3.0)),
        );
        host_sync.set_property_value("alive", Variant::Bool(true));
        let sync = decode_sync_packet(&encode_sync_packet(
            host_sync.get_net_id(),
            &host_sync.property_snapshot(),
        ))
        .expect("decode sync");

        // Peer applies the sync to the real spawned node.
        assert!(peer_repl.apply_sync(&mut peer_tree, &sync));
        assert_eq!(
            peer_tree.get_node(node).unwrap().get_property("position"),
            Variant::Vector2(gdcore::math::Vector2::new(10.0, 20.0))
        );
        assert_eq!(
            peer_tree.get_node(node).unwrap().get_property("velocity"),
            Variant::Vector3(gdcore::math::Vector3::new(1.0, 2.0, 3.0)),
            "a Vector3 must survive the wire — the string codec could not \
             represent one, so this is what proves the payload is a Variant"
        );
        assert_eq!(
            peer_tree.get_node(node).unwrap().get_property("alive"),
            Variant::Bool(true),
            "a bool must arrive as a bool, not as the string \"true\""
        );
    }
}
