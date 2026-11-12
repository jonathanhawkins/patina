# Upstream Gap Matrix

Structured source-backed planner input for gaps found by comparing Patina against
the pinned `upstream/godot` 4.6.1 oracle checkout and the audited Patina
implementation. This is intentionally narrow and machine-readable so the planner
can create concrete beads without inferring from prose.

> **Scan 2026-07-11 (post zero-open):** New open bead set from code+docs scan (see planner_log).
> Highlights: `call_deferred` stub, GDScript docs contradiction, NavigationAgent ClassDB,
> MultiplayerSpawner/Synchronizer, PacketPeerUDP/StreamPeerTCP, Spot shadows, SubViewport
> (prior close incomplete), Vehicle/SoftBody ClassDB, GPU shadow atlas + equirect sky,
> editor visual-fidelity remaining chrome, ENet peer slice, FS exit-criteria tick.
> Deferred only: `pat-ism62` (wgpu default editor renderer).
>
> **Reconciled 2026-07-11:** HTTP/WebSocket closed; FileAccess/DirAccess, RayCast3D/ShapeCast3D,
> AudioStreamPlayer ClassDB, GDScript singletons, TcpMultiplayerPeer closed. Remaining partial:
> `gpu-render-backbone` (editor wgpu default still deferred as pat-ism62).
>
> **2026-07-10 source review:** HTTP/WebSocket beads closed 2026-06-29. New gap beads
> created from code+upstream compare: `pat-nwt9y` (RayCast3D/ShapeCast3D — closed),
> `pat-e7t1n` (FileAccess/DirAccess — closed), open `pat-0uov2` (AudioStreamPlayer ClassDB),
> `pat-0m7oj` (GDScript singletons), `pat-fg8dk` (real multiplayer peer), `pat-273bn` (docs).

## Source-Backed Planner Gaps

### `collisionpolygon3d-node`
- Title: Implement CollisionPolygon3D scene node and shape baking path
- Status: implemented (reconciled 2026-06-12 — pat-k63cu et al. closed; shape baking in gdphysics3d)
- Priority: P4
- Labels: phase6, 3d, physics
- Acceptance: ./scripts/rust_task.sh nextest run --test physics3d_trace_comparison_test -- collision_polygon3d

Evidence:
- Upstream Godot exposes `CollisionPolygon3D` in `scene/3d/physics/collision_polygon_3d.*`.
- [prd/PHASE6_3D_PARITY_AUDIT.md](/Users/bone/dev/games/patina/prd/PHASE6_3D_PARITY_AUDIT.md:313) classifies `CollisionPolygon3D` as missing.
- Patina’s measured 3D collision coverage currently focuses on primitive shape families, not polygon-backed collision nodes.

### `conetwistjoint3d-runtime`
- Title: Implement ConeTwistJoint3D runtime constraint integration
- Status: implemented (reconciled 2026-06-12 — pat-maqfj/pat-mjs42 closed)
- Priority: P4
- Labels: phase6, 3d, physics
- Acceptance: ./scripts/rust_task.sh nextest run --test physics3d_trace_comparison_test -- conetwist_joint3d

Evidence:
- Upstream Godot exposes `ConeTwistJoint3D` in `scene/3d/physics/joints/cone_twist_joint_3d.*`.
- [prd/PHASE6_3D_PARITY_AUDIT.md](/Users/bone/dev/games/patina/prd/PHASE6_3D_PARITY_AUDIT.md:324) classifies `ConeTwistJoint3D` as missing.
- Current joint coverage in [joint.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdphysics3d/src/joint.rs:492) is still bounded and defers part of the full 3D solver surface.

### `generic6dof-basis-runtime`
- Title: Implement Generic6DOFJoint3D basis-rotated runtime constraint behavior
- Status: implemented (reconciled 2026-06-12 — pat-mjs42/pat-wewu2 closed; basis-rotated constraints landed)
- Priority: P4
- Labels: phase6, 3d, physics
- Acceptance: ./scripts/rust_task.sh nextest run --test physics3d_trace_comparison_test -- generic6dof_joint3d

Evidence:
- Upstream Godot exposes `Generic6DOFJoint3D` in `scene/3d/physics/joints/generic_6dof_joint_3d.*`.
- [joint.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdphysics3d/src/joint.rs:492) documents the current V1 limitation: world-aligned axes only, with full basis rotation deferred.
- This is a source-level partial implementation gap even if the data model exists.

### `lightmapgi-runtime`
- Title: Implement LightmapGI resource loading and baked sampling path
- Status: implemented (reconciled 2026-06-12 — pat-oincb/pat-1c0st closed; gdserver3d/src/gi.rs)
- Priority: P4
- Labels: phase6, 3d, render
- Acceptance: ./scripts/rust_task.sh nextest run --test render_3d_parity_test -- lightmap_gi

Evidence:
- Upstream Godot exposes `LightmapGI` in `scene/3d/lightmap_gi.*`.
- [prd/PHASE6_3D_PARITY_AUDIT.md](/Users/bone/dev/games/patina/prd/PHASE6_3D_PARITY_AUDIT.md:301) classifies `LightmapGI` as missing.
- [gi.rs](/Users/bone/dev/games/patina/engine-rs/crates/gdserver3d/src/gi.rs:1) only provides configuration stubs for GI nodes and explicitly keeps full baking out of scope.

### `navigationagent3d-runtime`
- Title: Implement NavigationAgent3D runtime pathfinding integration
- Status: implemented (reconciled 2026-06-12 — pat-lrh01/pat-mdjrx closed; gdserver3d/src/navigation.rs)
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
- Status: implemented (reconciled 2026-06-12 — gdscene/src/animation.rs)
- Priority: P1
- Labels: phase6, animation, scene
- Acceptance: ./scripts/rust_task.sh nextest run --test animation_parity_test -- animation_player

Evidence:
- Upstream Godot exposes `AnimationPlayer` in `scene/animation/animation_player.*`.
- No `gdanimation` crate or animation module exists in `engine-rs/crates/` as of 2026-04-20.
- Scene lifecycle traces exist but no AnimationPlayer/AnimationTree runtime is implemented — game-blocking for animated content.

### `tween-node`
- Title: Implement Tween property interpolation with easing curves and chained sequences
- Status: implemented (reconciled 2026-06-12 — gdscene/src/tween.rs)
- Priority: P1
- Labels: phase6, animation, scene
- Acceptance: ./scripts/rust_task.sh nextest run --test animation_parity_test -- tween_property

Evidence:
- Upstream Godot exposes `Tween` in `scene/animation/tween.*`.
- No tween implementation is present in Patina's scene or animation code paths.

### `animation-tree`
- Title: Implement AnimationTree with blend nodes, state machine, and blend spaces
- Status: implemented (reconciled 2026-06-12 — gdscene/src/animation_tree.rs)
- Priority: P2
- Labels: phase6, animation, scene
- Acceptance: ./scripts/rust_task.sh nextest run --test animation_parity_test -- animation_tree_blend

Evidence:
- Upstream Godot exposes `AnimationTree` in `scene/animation/animation_tree.*` and the `AnimationNode` hierarchy in `scene/resources/animation_node_*`.
- Patina has no blend-tree or animation state-machine implementation.

### `gltf-import`
- Title: Implement glTF 2.0 scene import producing Patina scene tree
- Status: implemented (reconciled 2026-06-12 — pat-epu6j/pat-rib6j/pat-z42y8 closed; gdresource)
- Priority: P1
- Labels: phase6, import, resource
- Acceptance: ./scripts/rust_task.sh nextest run --test import_parity_test -- gltf_scene

Evidence:
- Upstream Godot ships the glTF importer in `modules/gltf/`.
- `gdresource` handles Patina's native `.tres`/`.res` loading but has no glTF backend.
- Editor import registry UI in `gdeditor` exists but lacks a glTF converter — game-blocking for 3D asset pipelines.

### `image-import`
- Title: Implement PNG, JPEG, WebP image decode producing Image/Texture2D
- Status: implemented (reconciled 2026-06-12 — gdresource/src/texture_import.rs)
- Priority: P1
- Labels: phase6, import, resource
- Acceptance: ./scripts/rust_task.sh nextest run --test import_parity_test -- image_decode

Evidence:
- Upstream Godot provides image decoders in `modules/{svg,webp}` and `drivers/png/`.
- Patina has no image decode path; texture resources cannot be populated from external files.

### `audio-import`
- Title: Implement WAV and OGG Vorbis audio decode producing AudioStream resources
- Status: implemented (reconciled 2026-06-12 — gdresource/src/audio_import.rs)
- Priority: P2
- Labels: phase6, import, audio
- Acceptance: ./scripts/rust_task.sh nextest run --test import_parity_test -- audio_decode

Evidence:
- Upstream Godot decodes audio via `modules/vorbis/` and the core `AudioStreamWAV` class.
- `gdaudio` has bus routing and WAV decode stubs (88 tests) but no full importer producing resource objects.

### `font-import`
- Title: Implement TTF/OTF font import producing FontFile resource
- Status: implemented (reconciled 2026-06-12 — gdresource/src/importers.rs + gdrender2d/src/font.rs)
- Priority: P2
- Labels: phase6, import, ui
- Acceptance: ./scripts/rust_task.sh nextest run --test import_parity_test -- font_import

Evidence:
- Upstream Godot imports fonts via `modules/freetype/` producing `FontFile`.
- Patina has no font import path and no FontFile resource.

### `tilemap-node`
- Title: Implement TileMap and TileSet runtime nodes with layer rendering and physics
- Status: implemented (reconciled 2026-06-12 — gdscene/src/tilemap.rs + gdphysics2d tilemap_collision.rs)
- Priority: P1
- Labels: phase6, 2d, scene
- Acceptance: ./scripts/rust_task.sh nextest run --test tilemap_parity_test -- tilemap_layer_render

Evidence:
- Upstream Godot exposes `TileMap` in `scene/2d/tile_map.*` and `TileSet` in `scene/resources/2d/tile_set.*`.
- `gdeditor` has tilemap tooling but no scene-tree TileMap runtime — 2D games cannot be built.

### `cpu-particles2d`
- Title: Implement CPUParticles2D emission and simulation for 2D scenes
- Status: implemented (reconciled 2026-06-12 — pat-estfm/pat-jajqq closed; registered in class_db)
- Priority: P3
- Labels: phase6, 2d, render
- Acceptance: ./scripts/rust_task.sh nextest run --test render_2d_parity_test -- cpu_particles2d

Evidence:
- Upstream Godot exposes `CPUParticles2D` in `scene/2d/cpu_particles_2d.*`.
- Lane A covers 3D GPU particles; no 2D particle node exists in `gdscene` or `gdrender2d`.

### `theme-resource`
- Title: Implement Theme resource loading and style property lookup
- Status: implemented (reconciled 2026-06-12 — gdscene/src/theme.rs)
- Priority: P2
- Labels: phase6, ui
- Acceptance: ./scripts/rust_task.sh nextest run --test ui_parity_test -- theme_property_lookup

Evidence:
- Upstream Godot exposes `Theme` in `scene/resources/theme.*` with `StyleBox`, color/font/constant lookup.
- Patina has no Theme resource or style lookup path; Control nodes have no visual theming.

### `control-layout`
- Title: Implement Control anchor/margin/size-flags layout solver
- Status: implemented (reconciled 2026-06-12 — anchors/offsets in gdscene/src/control.rs)
- Priority: P2
- Labels: phase6, ui
- Acceptance: ./scripts/rust_task.sh nextest run --test ui_parity_test -- control_anchor_layout

Evidence:
- Upstream Godot implements Control layout in `scene/gui/control.*`.
- Patina's `gdscene` has Node hierarchy but no layout constraint solver matching Godot's anchor/margin rules.

### `container-nodes`
- Title: Implement VBoxContainer, HBoxContainer, GridContainer layout arrangement
- Status: implemented (reconciled 2026-06-12 — pat-nri4p/pat-sk37j/pat-grdja closed; gdscene/src/control.rs)
- Priority: P2
- Labels: phase6, ui
- Acceptance: ./scripts/rust_task.sh nextest run --test ui_parity_test -- container_arrangement

Evidence:
- Upstream Godot exposes container layouts in `scene/gui/{box_container,grid_container}.*`.
- No container-node runtime in Patina's scene crate.

### `gdscript-export-vars`
- Title: Serialize @export-annotated GDScript properties through TSCN save/load
- Status: implemented (reconciled 2026-06-12 — export_var_round_trip in tests/gdscript_parity_test.rs)
- Priority: P2
- Labels: phase6, scripting
- Acceptance: ./scripts/rust_task.sh nextest run --test gdscript_parity_test -- export_var_round_trip

Evidence:
- Upstream Godot handles `@export` in `modules/gdscript/gdscript_parser.cpp` and emits them into scene/resource saves.
- `gdscript-interop` parses `@onready` and signal/func dispatch (464 tests) but does not yet emit `@export` properties into the oracle — known gap in the 4.6.1 repin.

### `gdscript-type-checker`
- Title: Implement GDScript static type checking (class_name, typed args, return types)
- Status: implemented (reconciled 2026-06-12 — gdscript-interop/src/type_checker.rs)
- Priority: P3
- Labels: phase6, scripting
- Acceptance: ./scripts/rust_task.sh nextest run --test gdscript_parity_test -- type_check_errors

Evidence:
- Upstream Godot type-checks in `modules/gdscript/gdscript_analyzer.cpp`.
- Patina's interop crate has no analyzer pass; typed signatures parse but are not validated against ClassDB.

### `httpclient-node`
- Title: Implement HTTPClient and HTTPRequest nodes for HTTP(S) access
- Status: implemented (closed pat-7xflw) (created 2026-06-12)
- Priority: P3
- Labels: phase6, networking
- Acceptance: ./scripts/rust_task.sh nextest run --test networking_parity_test -- http_request

Evidence:
- Upstream Godot exposes `HTTPClient` and `HTTPRequest` in `core/io/` and `scene/main/http_request.*`.
- **Landed:** Patina implements `HTTPClient`/`HTTPRequest` and related networking paths (closed with pat-7xflw). Remaining gap is full ENet multiplayer RPC surface, not base HTTP.

### `websocket-peer`
- Title: Implement WebSocketPeer for bidirectional WS connections
- Status: implemented (closed pat-p2nes) (created 2026-06-12)
- Priority: P3
- Labels: phase6, networking
- Acceptance: ./scripts/rust_task.sh nextest run --test networking_parity_test -- websocket_peer

Evidence:
- Upstream Godot provides `WebSocketPeer` in `modules/websocket/`.
- **Landed:** Patina implements `WebSocketPeer` (closed pat-p2nes).

### `gpu-render-backbone`
- Title: Promote gdserver2d/gdserver3d software rasterisation to a wgpu (Vulkan/Metal/GL) pipeline
- Status: partial
- Priority: P3
- Labels: phase6, render, infrastructure
- Acceptance: ./scripts/rust_task.sh nextest run --test render_3d_parity_test -- gpu_backend_smoke

Evidence:
- Upstream Godot renders via `drivers/vulkan/` and `drivers/gles3/`.
- Patina's render crates are software-only today; individual GPU beads (multimesh-gpu, particle3d-gpu) exist but no backbone GPU pipeline bead.

## 2026-07-17 Scope Decisions

Gaps deliberately **not** closed, with the citation that settles them. A scope
decision recorded here is a commitment: do not re-open it as a MISSING gap
without new evidence.

### `godot-instance-embedding`
- Title: GodotInstance (libgodot native host embedding)
- Status: **OUT_OF_SCOPE (browser target)** — decided 2026-07-17, pat-nzgdu.203
- Priority: n/a (scope call, not work)
- Labels: platform-os, scope-decision, web
- Acceptance: `./scripts/rust_task.sh nextest run -p gdobject --test godot_instance_scope_stub_returns_unsupported_error_without_panic`

Decision: **register the class, implement no runtime.** `ClassDB.class_exists("GodotInstance")`
is **true** and the class is **abstract**, exactly as upstream — so introspection
and defensive scripts see what they see in Godot, and no host-embedding runtime
is built for the browser.

Evidence (from the pinned `upstream/godot` 4.6.1 oracle):
- `GodotInstance` is a host-embedding handle: one per process, created by the C
  API `libgodot_create_godot_instance`, and driven by `start()` / `iteration()`
  (`core/extension/godot_instance.h:36-57`). The *host application* owns the loop.
- **Upstream itself ships no web implementation.** The entry point exists only in
  `platform/linuxbsd/libgodot_linuxbsd.cpp` and `platform/windows/libgodot_windows.cpp`
  — there is no `platform/web/libgodot_*`. Godot does not embed itself in a page,
  because in the browser the *page* is the host. There is no browser analog to port.
- Upstream registers it **abstract**: `GDREGISTER_ABSTRACT_CLASS(GodotInstance)`
  (`core/register_core_types.cpp:290`), matching `is_instantiable=false` in
  `extension_api.json`. A script therefore **cannot construct one in Godot either** —
  the C API does. Patina mirrors that flag, which is a *stronger* guarantee than a
  stub that errors on `start()`: there is no legitimate way to obtain an instance
  to call `start()` on.

Why not "leave it unregistered": `class_exists` would then disagree with upstream,
and a defensive script probing for the class would take the wrong branch. Registering
the honest, runtime-free surface keeps introspection truthful and costs nothing.

### `script-extension-gdextension-hooks`
- Title: ScriptExtension / ScriptLanguageExtension (GDExtension script hooks)
- Status: **surface-only, runtime OUT_OF_SCOPE (wasm target)** — decided 2026-07-17, pat-nzgdu.204
- Priority: n/a (scope call, not work)
- Labels: platform-os, scope-decision, web, gdextension
- Acceptance: `./scripts/rust_task.sh nextest run -p gdobject --test script_extension_surface_test`

Decision: **register the full class surface, implement no virtuals.** These are the
hooks a *native GDExtension plugin* implements to add a scripting language. Loading
native plugins is not feasible on a wasm target, so nothing here has a runtime.

Evidence (from the pinned `upstream/godot` 4.6.1 oracle):
- `ScriptExtension` (37 virtuals) and `ScriptLanguageExtension` (60 virtuals) declare
  **zero non-virtual methods** between them: they are pure override surfaces, not
  APIs the engine offers. Every entry is a hook a plugin fills in.
- Both are **instantiable** upstream, unlike their abstract parents `Script` and
  `ScriptLanguage`. Patina mirrors that exactly.

⚠️ **An instance being inert is Godot's behaviour, not a Patina limitation.** In
Godot, constructing a `ScriptExtension` without a GDExtension providing its virtuals
also yields an object that does nothing. So this port does **not** make construction
raise "GDExtension not supported": that would *diverge* from upstream and break
introspection parity, which is the whole point of registering the surface. What is
unsupported on wasm is the **native plugin loader**, not the class.

Note `Script` and `ScriptLanguage` themselves were missing too, and were registered
alongside — a child over an absent parent has a silently severed chain.
