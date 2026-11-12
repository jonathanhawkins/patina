# ClassDB inventory (pat-p0s82.29)

Generated scorecard companion. Status: full / partial / absent for high-use runtime nodes.

| Godot class | Patina | Notes |
|-------------|--------|-------|
| Node/Node2D/Node3D | full | |
| Sprite2D / MeshInstance3D | full/partial | |
| CharacterBody2D/3D | full | |
| PointLight2D / DirectionalLight2D | partial | ClassDB + shade module + renderer lights |
| Path3D / PathFollow3D | partial | Curve3D runtime + ClassDB |
| GridMap | partial | runtime + ClassDB |
| NavigationAgent2D/3D | partial | A* yes, RVO partial |
| NavigationLink2D/3D | partial | ClassDB + path_via_link |
| RichTextLabel / ScrollContainer / TabContainer | partial | ClassDB registered |
| VoxelGI / LightmapGI | partial | spatial bake samples |
| SoftBody3D | partial | stepped in PhysicsWorld3D |
| XROrigin3D | partial | ClassDB registered w/ defaults (pat-p0s82.29); full XR session handling is a separate bead |
| WebRTCMultiplayerPeer | partial | ClassDB registered w/ core methods (pat-p0s82.29); native WebRTC transport is pat-p0s82.22 |
| VideoStreamPlayer | partial | ClassDB registered w/ defaults (pat-p0s82.29); video decode/playback is pat-p0s82.28 |
| ENetMultiplayerPeer | partial | UDP not stock ENet |

See `class_db.rs` for the live registry (~130+ classes after expansion).
