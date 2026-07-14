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
| XROrigin3D | absent | |
| WebRTCMultiplayerPeer | absent | |
| VideoStreamPlayer | absent | |
| ENetMultiplayerPeer | partial | UDP not stock ENet |

See `class_db.rs` for the live registry (~130+ classes after expansion).
