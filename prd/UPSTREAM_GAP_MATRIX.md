# Upstream Gap Matrix

Structured source-backed planner input for gaps found by comparing Patina against
the pinned `upstream/godot` 4.6.1 oracle checkout and the audited Patina
implementation. This is intentionally narrow and machine-readable so the planner
can create concrete beads without inferring from prose.

## Source-Backed Planner Gaps

### `collisionpolygon3d-node`
- Title: Implement CollisionPolygon3D scene node and shape baking path
- Status: missing
- Priority: P4
- Labels: phase6, 3d, physics
- Acceptance: ./scripts/rust_task.sh nextest run --test physics3d_trace_comparison_test -- collision_polygon3d

Evidence:
- Upstream Godot exposes `CollisionPolygon3D` in `scene/3d/physics/collision_polygon_3d.*`.
- [prd/PHASE6_3D_PARITY_AUDIT.md](/Users/bone/dev/games/patina/prd/PHASE6_3D_PARITY_AUDIT.md:313) classifies `CollisionPolygon3D` as missing.
- Patina’s measured 3D collision coverage currently focuses on primitive shape families, not polygon-backed collision nodes.

### `conetwistjoint3d-runtime`
- Title: Implement ConeTwistJoint3D runtime constraint integration
- Status: missing
- Priority: P4
- Labels: phase6, 3d, physics
- Acceptance: ./scripts/rust_task.sh nextest run --test physics3d_trace_comparison_test -- conetwist_joint3d

Evidence:
- Upstream Godot exposes `ConeTwistJoint3D` in `scene/3d/physics/joints/cone_twist_joint_3d.*`.
- [prd/PHASE6_3D_PARITY_AUDIT.md](/Users/bone/dev/games/patina/prd/PHASE6_3D_PARITY_AUDIT.md:324) classifies `ConeTwistJoint3D` as missing.
- Current joint coverage in [joint.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdphysics3d/src/joint.rs:492) is still bounded and defers part of the full 3D solver surface.

### `generic6dof-basis-runtime`
- Title: Implement Generic6DOFJoint3D basis-rotated runtime constraint behavior
- Status: partial
- Priority: P4
- Labels: phase6, 3d, physics
- Acceptance: ./scripts/rust_task.sh nextest run --test physics3d_trace_comparison_test -- generic6dof_joint3d

Evidence:
- Upstream Godot exposes `Generic6DOFJoint3D` in `scene/3d/physics/joints/generic_6dof_joint_3d.*`.
- [joint.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdphysics3d/src/joint.rs:492) documents the current V1 limitation: world-aligned axes only, with full basis rotation deferred.
- This is a source-level partial implementation gap even if the data model exists.

### `lightmapgi-runtime`
- Title: Implement LightmapGI resource loading and baked sampling path
- Status: missing
- Priority: P4
- Labels: phase6, 3d, render
- Acceptance: ./scripts/rust_task.sh nextest run --test render_3d_parity_test -- lightmap_gi

Evidence:
- Upstream Godot exposes `LightmapGI` in `scene/3d/lightmap_gi.*`.
- [prd/PHASE6_3D_PARITY_AUDIT.md](/Users/bone/dev/games/patina/prd/PHASE6_3D_PARITY_AUDIT.md:301) classifies `LightmapGI` as missing.
- [gi.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdserver3d/src/gi.rs:1) only provides configuration stubs for GI nodes and explicitly keeps full baking out of scope.

### `navigationagent3d-runtime`
- Title: Implement NavigationAgent3D runtime pathfinding integration
- Status: missing
- Priority: P4
- Labels: phase6, 3d, navigation
- Acceptance: ./scripts/rust_task.sh nextest run --test physics3d_trace_comparison_test -- navigation_agent3d

Evidence:
- Upstream Godot exposes `NavigationAgent3D` in `scene/3d/navigation_agent_3d.*`.
- [prd/PHASE6_3D_PARITY_AUDIT.md](/Users/bone/dev/games/patina/prd/PHASE6_3D_PARITY_AUDIT.md:289) classifies `NavigationRegion3D` as deferred and [prd/PHASE6_3D_PARITY_AUDIT.md](/Users/bone/dev/games/patina/prd/PHASE6_3D_PARITY_AUDIT.md:290) classifies `NavigationAgent3D` as missing.
- The current runtime does not provide node-level pathfinding parity for this family.

## 2026-04-20 Upstream Review Additions

The following entries were added from the 2026-04-20 Opus 4.7 upstream review against `godotengine/godot` 4.6.1. Each names a concrete missing or partial subsystem in Patina.

### `animation-player`
- Title: Implement AnimationPlayer node with keyframe track playback
- Status: missing
- Priority: P1
- Labels: phase6, animation, scene
- Acceptance: ./scripts/rust_task.sh nextest run --test animation_parity_test -- animation_player

Evidence:
- Upstream Godot exposes `AnimationPlayer` in `scene/animation/animation_player.*`.
- No `gdanimation` crate or animation module exists in `engine-rs/crates/` as of 2026-04-20.
- Scene lifecycle traces exist but no AnimationPlayer/AnimationTree runtime is implemented — game-blocking for animated content.

### `tween-node`
- Title: Implement Tween property interpolation with easing curves and chained sequences
- Status: missing
- Priority: P1
- Labels: phase6, animation, scene
- Acceptance: ./scripts/rust_task.sh nextest run --test animation_parity_test -- tween_property

Evidence:
- Upstream Godot exposes `Tween` in `scene/animation/tween.*`.
- No tween implementation is present in Patina's scene or animation code paths.

### `animation-tree`
- Title: Implement AnimationTree with blend nodes, state machine, and blend spaces
- Status: missing
- Priority: P2
- Labels: phase6, animation, scene
- Acceptance: ./scripts/rust_task.sh nextest run --test animation_parity_test -- animation_tree_blend

Evidence:
- Upstream Godot exposes `AnimationTree` in `scene/animation/animation_tree.*` and the `AnimationNode` hierarchy in `scene/resources/animation_node_*`.
- Patina has no blend-tree or animation state-machine implementation.

### `gltf-import`
- Title: Implement glTF 2.0 scene import producing Patina scene tree
- Status: missing
- Priority: P1
- Labels: phase6, import, resource
- Acceptance: ./scripts/rust_task.sh nextest run --test import_parity_test -- gltf_scene

Evidence:
- Upstream Godot ships the glTF importer in `modules/gltf/`.
- `gdresource` handles Patina's native `.tres`/`.res` loading but has no glTF backend.
- Editor import registry UI in `gdeditor` exists but lacks a glTF converter — game-blocking for 3D asset pipelines.

### `image-import`
- Title: Implement PNG, JPEG, WebP image decode producing Image/Texture2D
- Status: missing
- Priority: P1
- Labels: phase6, import, resource
- Acceptance: ./scripts/rust_task.sh nextest run --test import_parity_test -- image_decode

Evidence:
- Upstream Godot provides image decoders in `modules/{svg,webp}` and `drivers/png/`.
- Patina has no image decode path; texture resources cannot be populated from external files.

### `audio-import`
- Title: Implement WAV and OGG Vorbis audio decode producing AudioStream resources
- Status: missing
- Priority: P2
- Labels: phase6, import, audio
- Acceptance: ./scripts/rust_task.sh nextest run --test import_parity_test -- audio_decode

Evidence:
- Upstream Godot decodes audio via `modules/vorbis/` and the core `AudioStreamWAV` class.
- `gdaudio` has bus routing and WAV decode stubs (88 tests) but no full importer producing resource objects.

### `font-import`
- Title: Implement TTF/OTF font import producing FontFile resource
- Status: missing
- Priority: P2
- Labels: phase6, import, ui
- Acceptance: ./scripts/rust_task.sh nextest run --test import_parity_test -- font_import

Evidence:
- Upstream Godot imports fonts via `modules/freetype/` producing `FontFile`.
- Patina has no font import path and no FontFile resource.

### `tilemap-node`
- Title: Implement TileMap and TileSet runtime nodes with layer rendering and physics
- Status: missing
- Priority: P1
- Labels: phase6, 2d, scene
- Acceptance: ./scripts/rust_task.sh nextest run --test tilemap_parity_test -- tilemap_layer_render

Evidence:
- Upstream Godot exposes `TileMap` in `scene/2d/tile_map.*` and `TileSet` in `scene/resources/2d/tile_set.*`.
- `gdeditor` has tilemap tooling but no scene-tree TileMap runtime — 2D games cannot be built.

### `cpu-particles2d`
- Title: Implement CPUParticles2D emission and simulation for 2D scenes
- Status: missing
- Priority: P3
- Labels: phase6, 2d, render
- Acceptance: ./scripts/rust_task.sh nextest run --test render_2d_parity_test -- cpu_particles2d

Evidence:
- Upstream Godot exposes `CPUParticles2D` in `scene/2d/cpu_particles_2d.*`.
- Lane A covers 3D GPU particles; no 2D particle node exists in `gdscene` or `gdrender2d`.

### `theme-resource`
- Title: Implement Theme resource loading and style property lookup
- Status: missing
- Priority: P2
- Labels: phase6, ui
- Acceptance: ./scripts/rust_task.sh nextest run --test ui_parity_test -- theme_property_lookup

Evidence:
- Upstream Godot exposes `Theme` in `scene/resources/theme.*` with `StyleBox`, color/font/constant lookup.
- Patina has no Theme resource or style lookup path; Control nodes have no visual theming.

### `control-layout`
- Title: Implement Control anchor/margin/size-flags layout solver
- Status: missing
- Priority: P2
- Labels: phase6, ui
- Acceptance: ./scripts/rust_task.sh nextest run --test ui_parity_test -- control_anchor_layout

Evidence:
- Upstream Godot implements Control layout in `scene/gui/control.*`.
- Patina's `gdscene` has Node hierarchy but no layout constraint solver matching Godot's anchor/margin rules.

### `container-nodes`
- Title: Implement VBoxContainer, HBoxContainer, GridContainer layout arrangement
- Status: missing
- Priority: P2
- Labels: phase6, ui
- Acceptance: ./scripts/rust_task.sh nextest run --test ui_parity_test -- container_arrangement

Evidence:
- Upstream Godot exposes container layouts in `scene/gui/{box_container,grid_container}.*`.
- No container-node runtime in Patina's scene crate.

### `gdscript-export-vars`
- Title: Serialize @export-annotated GDScript properties through TSCN save/load
- Status: partial
- Priority: P2
- Labels: phase6, scripting
- Acceptance: ./scripts/rust_task.sh nextest run --test gdscript_parity_test -- export_var_round_trip

Evidence:
- Upstream Godot handles `@export` in `modules/gdscript/gdscript_parser.cpp` and emits them into scene/resource saves.
- `gdscript-interop` parses `@onready` and signal/func dispatch (464 tests) but does not yet emit `@export` properties into the oracle — known gap in the 4.6.1 repin.

### `gdscript-type-checker`
- Title: Implement GDScript static type checking (class_name, typed args, return types)
- Status: missing
- Priority: P3
- Labels: phase6, scripting
- Acceptance: ./scripts/rust_task.sh nextest run --test gdscript_parity_test -- type_check_errors

Evidence:
- Upstream Godot type-checks in `modules/gdscript/gdscript_analyzer.cpp`.
- Patina's interop crate has no analyzer pass; typed signatures parse but are not validated against ClassDB.

### `httpclient-node`
- Title: Implement HTTPClient and HTTPRequest nodes for HTTP(S) access
- Status: missing
- Priority: P3
- Labels: phase6, networking
- Acceptance: ./scripts/rust_task.sh nextest run --test networking_parity_test -- http_request

Evidence:
- Upstream Godot exposes `HTTPClient` and `HTTPRequest` in `core/io/` and `scene/main/http_request.*`.
- No networking crate or HTTP path exists in Patina (distinct from the deferred ENet multiplayer stack).

### `websocket-peer`
- Title: Implement WebSocketPeer for bidirectional WS connections
- Status: missing
- Priority: P3
- Labels: phase6, networking
- Acceptance: ./scripts/rust_task.sh nextest run --test networking_parity_test -- websocket_peer

Evidence:
- Upstream Godot provides `WebSocketPeer` in `modules/websocket/`.
- No WebSocket implementation in Patina.

### `gpu-render-backbone`
- Title: Promote gdserver2d/gdserver3d software rasterisation to a wgpu (Vulkan/Metal/GL) pipeline
- Status: partial
- Priority: P3
- Labels: phase6, render, infrastructure
- Acceptance: ./scripts/rust_task.sh nextest run --test render_3d_parity_test -- gpu_backend_smoke

Evidence:
- Upstream Godot renders via `drivers/vulkan/` and `drivers/gles3/`.
- Patina's render crates are software-only today; individual GPU beads (multimesh-gpu, particle3d-gpu) exist but no backbone GPU pipeline bead.
