# Port Audit — 2026-10-30 (oracle: Godot 4.6.1-stable)
Epic: pat-mgixk · Findings: 453 verified (1 refuted, 1 unverified) · Beads: 402 created (400 from audit drafts + 2 hand-written for overturned out-of-scope findings), 0 already tracked

Run: workflow wf_87a6d367-059. 24 finders + 3 sweep chunks (138 unregistered classes) + critic with 6 supplemental areas. 16 agents first failed on an account usage limit and were re-run via resume; none dropped.

## Per-subsystem
| Subsystem | Parity-OK | MISSING | STUB | UNWIRED | PARTIAL | P0s | Coverage confession |
|---|---|---|---|---|---|---|---|
| core-object | 13 | 3 | 0 | 2 | 8 | 3 | Covered: Object method surface vs extension_api.json (all 46 Object methods hunted by exact-name grep across engine-rs/crates), ClassDB script API arms (presence only), signal connect/disconnect/emit paths through interp |
| core-variant | 16 | 3 | 0 | 0 | 12 | 2 | Covered: the GDScript runtime dispatch for builtin methods (call_method_on arms + variant_math), a scripted name-level diff of all 38 builtin_classes' methods and the full non-equality operator table against the dispatch |
| gdscript | 13 | 5 | 1 | 1 | 7 | 5 | What I covered: the tokenizer (tokenizer.rs, read in full), the parser (statement/annotation/expression/primary paths), run_class/instantiate_class/onready/super/method lookup in interpreter.rs, the runner and SceneTree  |
| scene-tree | 15 | 3 | 1 | 4 | 9 | 7 | This was a read-only static audit; no tests were run. I covered: - Node lifecycle code: lifecycle.rs and scene_tree.rs add_child, reparent and the deletion paths. - The MainLoop::step per-frame sequence. - Process and pa |
| nodes-2d | 13 | 4 | 0 | 2 | 9 | 1 | Covered: the 2D draw path (render_kind_for and the Play/runtime wiring), Sprite2D, AnimatedSprite2D with SpriteFrames loading, Camera2D, TileMapLayer rendering and script API, Line2D, Polygon2D, Path2D/PathFollow2D/Curve |
| rendering-2d | 18 | 5 | 0 | 4 | 7 | 5 | Covered: the CanvasItem draw_* API (all 33 draw methods in extension_api.json, checked against the scripting.rs dispatch and the viewport_2d_render command converter), draw ordering (z, z_as_relative, y-sort, show_behind |
| nodes-3d | 15 | 2 | 2 | 8 | 9 | 8 | Covered: Node3D transform/visibility/top_level/notifications/script surface; Camera3D (render path, script API, environment/attributes, doppler); Light3D property reading vs consumption; MeshInstance3D mesh/material reso |
| rendering-3d | 14 | 3 | 1 | 6 | 4 | 6 | Read-only audit of gdrender3d (renderer, wgpu_pipeline, shadow_map, postprocess, camera_attributes), gdserver3d (material, environment, mesh, lod, material_math), gdscene render_server_3d.rs, patina-runner render3d/main, |
| shaders | 14 | 2 | 0 | 4 | 10 | 0 | Covered: the shader_type list, render_mode tables (spatial; canvas_item only checked for any consumer), AST language features and the builtin function table against shader_language.cpp:2419, uniform types/hints/global/in |
| vfx-particles | 10 | 4 | 1 | 5 | 5 | 4 | Covered: GPUParticles2D/3D and CPUParticles2D/3D runtime paths in gdscene (particle.rs, particle3d.rs, MainLoop::process_particles, packed_scene ParticleProcessMaterial flattener), 2D renderers in gdeditor, ClassDB regis |
| animation | 18 | 4 | 4 | 2 | 9 | 3 | I read the whole taxonomy section. Godot ground truth came from extension_api.json (the AnimationMixer, AnimationPlayer and Animation enums, plus the subclass trees of SkeletonModifier3D, AnimationNode and Tweener) and f |
| physics-2d | 22 | 1 | 1 | 1 | 10 | 2 | Covered: the main_loop physics tick; how PhysicsServer registers and syncs bodies; character movement; the integrate_forces dispatch; direct space state arms; PhysicsServer2D arm coverage (name-level grep against extensi |
| physics-3d | 19 | 1 | 0 | 2 | 9 | 1 | Covered: the gdphysics3d crate structure, the gdscene PhysicsServer3D bridge (body/shape/joint/area/vehicle/softbody sync), the MainLoop physics tick ordering, CharacterBody3D script dispatch, RigidBody3D force dispatch  |
| navigation | 14 | 3 | 0 | 2 | 7 | 1 | Covered: the taxonomy navigation floor (servers/maps, regions, NavigationPolygon/NavigationMesh bake params, agents incl. avoidance and signal lifecycle, obstacles, links, path queries, AStar2D/3D/AStarGrid2D) against th |
| audio | 19 | 3 | 3 | 6 | 3 | 2 | Covered: gdaudio crate structure (server/mixer/bus/effects factory/stream/import/output/web_output/sample_pool), gdscene AudioServerBridge end to end, MainLoop audio phase and Area bus overrides, patina-runner boot wirin |
| multiplayer | 19 | 2 | 0 | 2 | 8 | 1 | What I did: read the taxonomy section, sliced extension_api for 40 networking classes, and compared ClassDB registration counts and interpreter/scripting dispatch arm lists against the Godot method lists for MultiplayerA |
| gui-controls | 16 | 8 | 0 | 2 | 11 | 1 | Covered: the Control input pipeline (scene_tree.rs push_input, gui_input_stage, gui_key_stage), hit-testing, the focus ring, button semantics, container layout, the draw arms in control_render.rs, script method dispatch  |
| theming-text | 14 | 5 | 0 | 3 | 6 | 2 | Covered: theme resolution/wiring into the only Control render path (editor Play via build_control_render_items), per-node overrides, default theme constants vs default_theme.cpp (spot-checked Panel/Button/Label/focus, no |
| input | 20 | 0 | 1 | 3 | 9 | 2 | What I covered: the taxonomy 'input' floor (event types, Input singleton, InputMap, routing, devices, cursor, IME). I checked it against extension_api.json (Input and InputMap method lists, the Key enum count), upstream  |
| resources-io | 16 | 6 | 0 | 4 | 3 | 4 | Covered: - Text variant parser completeness (loader.rs, read in full for value arms) and the packed_scene silent-skip behaviour. - The binary RSRC loader and its callers. - PCK reader header and flags. - The runner boot  |
| platform-os | 16 | 2 | 0 | 3 | 8 | 2 | Covered: OS (method-by-method diff vs extension_api.json plus reading the web-relevant arms and gdplatform/src/os.rs), Time (full diff and TZ path), Engine (diff, plus wiring of max_fps, time_scale, physics steps and jit |
| editor-ux | 15 | 4 | 5 | 3 | 12 | 4 | Method: I diffed every server route in editor_server.rs (317) against every /api/ call in EDITOR_HTML (272) to find UNWIRED routes. I grepped the client for toast/log-only handlers and for 'not implemented'. I then deep- |
| web-runtime | 18 | 4 | 0 | 4 | 3 | 3 | This was static analysis only. I did not build or run the wasm target or open a browser, so every 'works' item in parity_ok is code-reading evidence, not a boot. I read all of patina-web (lib, engine, browser_runtime, ca |
| xr-peripheral | 9 | 3 | 0 | 2 | 3 | 0 | Covered: taxonomy xr-peripheral stones. Godot inventory comes from extension_api.json (classes matching ^XR/OpenXR/WebXR/MobileVR/CameraServer/CameraFeed/MovieWriter) and the upstream modules webxr/mobile_vr/openxr/camer |
| sweep-0 | 7 | 4 | 0 | 0 | 6 | 0 | All 46 classes checked. Method: grep -rn '"<Class>"' and grep -rlw <Class> over engine-rs/crates/*.rs, plus snake_case/keyword greps (atlas_texture, gradient_texture, 'struct Gradient', 'struct Curve', chorus, phaser, pa |
| sweep-1 | 4 | 2 | 0 | 1 | 4 | 0 | Checked all 46 names. Patterns used: grep -c '"<Name>"' on engine-rs/crates/gdobject/src/class_db.rs, and grep -rnF '"<Name>"' --include='*.rs' across engine-rs/crates, excluding tests. I also checked the other register_ |
| sweep-2 | 0 | 5 | 0 | 1 | 2 | 0 | All 46 input classes searched (exact quoted name in class_db.rs, -w word grep across engine-rs/**/*.rs, string-literal grep in crates/, plus snake_case/module greps for gzip/theora/upnp/openxr/text_layout/bone_map/ik_sol |
| project-settings-surface | 11 | 4 | 1 | 4 | 4 | 3 | Covered: the Rust ProjectSettings store and its 61-key default table vs the 513 distinct upstream GLOBAL_DEF keys (computed from core/scene/servers/main); every engine-side reader of the keys the critic named (stretch, e |
| virtual-callback-dispatch | 11 | 10 | 0 | 1 | 1 | 2 | Method: quoted-name sweeps of every virtual name in non-test engine-rs/crates (call_script_with_access / call_method / call_user_func call-site inventory), plus per-class virtual list from extension_api.json for ~60 clas |
| signal-emission-sweep | 21 | 6 | 0 | 1 | 5 | 1 | I built the inventory of all 489 extension_api signals and checked each signal NAME against string literals in non-test runtime code in every engine crate. I excluded class_db.rs and #[cfg(test)] modules, and also scanne |
| classdb-registered-but-nil-sweep | 8 | 1 | 1 | 9 | 2 | 3 | Method: a static, read-only sweep with no cargo run. I stripped comments, then took every ClassRegistration::new("<literal>") and MethodInfo::new("<literal>") in gdobject/src/class_db.rs and editor_plugin_classes.rs, exc |
| import-pipeline-and-gltf | 12 | 1 | 0 | 3 | 6 | 1 | Covered: runtime glTF load/instantiate paths, GLTFDocument/GLTFState dispatch surface vs extension_api, glTF parser feature presence (keyword grep, not full semantic review), editor scene reimport writer and options, tex |
| property-defaults-and-default-theme | 15 | 2 | 0 | 0 | 10 | 0 | I ran a scripted, inheritance- and override-aware diff of every PropertyInfo::new default in gdobject/src/class_db.rs (outside cfg(test)) against every doc/classes/*.xml and modules/*/doc_classes/*.xml member default. 10 |

## P0 list
- pat-mgixk.1 [PARTIAL] Animation: load AnimationPlayer libraries from `libraries/<name>` and ExtResource libraries
- pat-mgixk.2 [PARTIAL] AnimationPlayer: Godot-exact play() restart, queue-clear and signal semantics
- pat-mgixk.3 [PARTIAL] Skeleton3D: adopt Godot 4 absolute bone-pose semantics and scene pose keys
- pat-mgixk.4 [UNWIRED] Web audio: feed the MainLoop's AudioServer into the AudioWorklet instead of a private demo server
- pat-mgixk.5 [PARTIAL] AudioStreamPlayer: restart on play(), reload on stream change, accept Resource and inline WAV streams
- pat-mgixk.6 [UNWIRED] GDScript: dispatch Node.get_child/get_node_or_null/remove_child/is_inside_tree/set_process family + Object.notification
- pat-mgixk.7 [STUB] ClassDB: make registered-but-unimplemented native calls fail loudly + add registration-vs-dispatch sweep test
- pat-mgixk.8 [MISSING] GDScript: generic property-accessor dispatch + core Node/CanvasItem/Node2D/SceneTree methods
- pat-mgixk.9 [UNWIRED] Signals: dispatch connections to native methods and scriptless targets (+1 related)
- pat-mgixk.10 [UNWIRED] MessageQueue: deferred calls run native methods, bound args and lambdas
- pat-mgixk.11 [PARTIAL] GDScript: complete the Variant arithmetic operator matrix (vectors, Color, Array/Packed concat, transforms x scalar) (+1 related)
- pat-mgixk.12 [PARTIAL] Variant: String==StringName equality and Godot StringLike key comparison for Dictionary/Array (+1 related)
- pat-mgixk.13 [STUB] Editor: back the Node-dock signal connect/disconnect with real SceneTree connections
- pat-mgixk.14 [UNWIRED] Debugger: pause the GDScript interpreter at breakpoints and drive real step/stack/locals (+1 related)
- pat-mgixk.15 [STUB] Animation editor: write every clip/track/key edit through to the AnimationPlayer with undo (+1 related)
- pat-mgixk.16 [PARTIAL] Input Map editor: event-capture dialog, typed events, deadzone, and persist to project.godot [input] (+1 related)
- pat-mgixk.17 [MISSING] GDScript: lexer support for ! && ||, hex/bin/exp numbers, triple/raw strings, bracket line-joining
- pat-mgixk.18 [MISSING] GDScript: parse := inference, one-line suites, ; separators, multi-line enums, typed for vars, inner-class extends
- pat-mgixk.19 [PARTIAL] GDScript: typed vars default to the type's zero value and convert/enforce on assign
- pat-mgixk.20 [UNWIRED] GDScript: resolve @onready initializers with scene access so $Child works
- pat-mgixk.21 [MISSING] GDScript: load base scripts and resolve inherited methods, vars and super calls
- pat-mgixk.22 [MISSING] Scripting: send ClassDB accessor methods and show/hide to node properties
- pat-mgixk.23 [UNWIRED] glTF: instantiate .glb/.gltf as a real PackedScene (load, preload, sub-scene, generate_scene) (+1 related)
- pat-mgixk.24 [UNWIRED] Input: editor Play loads the project's [input] actions instead of a hard-coded map
- pat-mgixk.25 [UNWIRED] Input: map pointer events through the stretch transform and the game-view scale
- pat-mgixk.26 [PARTIAL] Multiplayer: rpc() broadcasts to all peers, server relays, and receivers enforce @rpc mode
- pat-mgixk.27 [UNWIRED] Navigation: feed avoidance the parent body's global position instead of the agent's local position
- pat-mgixk.28 [UNWIRED] Runtime2D: move the scene-to-canvas builder out of gdeditor and drive it from patina-web and patina-runner
- pat-mgixk.29 [UNWIRED] Node3D: route every transform write (tween, animation, root motion, transform/basis/quaternion) through the canonical local transform (+1 related)
- pat-mgixk.30 [UNWIRED] Node3D: dispatch global_*/basis/quaternion properties and the full Node3D method table to node3d.rs
- pat-mgixk.31 [UNWIRED] Scripting: generic ClassDB accessor bridge so set_X/get_X/is_X calls on 3D nodes read and write their properties
- pat-mgixk.32 [PARTIAL] Render3D: honour is_visible_in_tree and top_level for every collected 3D node and light
- pat-mgixk.33 [PARTIAL] Render3D Play: draw nothing for non-visual Node3D classes (drop Box markers from game frames)
- pat-mgixk.34 [UNWIRED] Sprite3D render: let AnimatedSprite3D through the sprite quad path (+1 related)
- pat-mgixk.35 [UNWIRED] Camera3D: dispatch projection/unprojection and set_perspective/orthogonal/frustum to camera3d.rs
- pat-mgixk.36 [PARTIAL] Play: composite the 2D canvas (CanvasLayer/Control HUD) over the 3D frame instead of choosing one renderer
- pat-mgixk.37 [PARTIAL] CharacterBody2D: run move_and_slide synchronously inside the script call
- pat-mgixk.38 [PARTIAL] RigidBody2D: call _integrate_forces whenever overridden; custom_integrator skips default integration
- pat-mgixk.39 [PARTIAL] Physics3D: honour Godot shape defaults, WorldBoundary plane and CollisionShape3D rotation
- pat-mgixk.40 [PARTIAL] OS/Time: route script clocks and randomize() through wasm-safe time sources
- pat-mgixk.41 [UNWIRED] JavaScriptBridge: pump JS callbacks each frame and define the PWA/fs-sync shell hooks
- pat-mgixk.42 [UNWIRED] ProjectSettings: seed script singleton with Godot registered defaults and widen the default table
- pat-mgixk.43 [STUB] Editor: back the Project Settings dialog with project.godot via gdcore::ProjectSettings
- pat-mgixk.44 [UNWIRED] Editor: apply the project's ProjectSettings when Play builds the run MainLoop
- pat-mgixk.45 [UNWIRED] Render2D: route editor Play through one runtime canvas entry point (groups, normal maps, noise, stretch, quality)
- pat-mgixk.46 [UNWIRED] Render2D: move the canvas render list into a runtime crate and emit 2D frames from patina-runner (+1 related)
- pat-mgixk.47 [PARTIAL] Editor2D: render the edited scene through the runtime canvas list under the editor grid and gizmos
- pat-mgixk.48 [UNWIRED] Render2D: retarget the 2D golden/parity gates at the shipping canvas path and retire SoftwareRenderer::render_frame
- pat-mgixk.49 [UNWIRED] Runtime: render the 3D scene every frame in patina-runner and patina-web
- pat-mgixk.50 [UNWIRED] Render3D: route the editor/Play 3D item list through one scene-sync shared with RenderServer3DAdapter
- pat-mgixk.51 [UNWIRED] Render3D: drop non-visual Node3D marker cubes from the Play view (+1 related)
- pat-mgixk.52 [PARTIAL] Render3D GPU: sample albedo/normal/ORM/emission textures and run SSAO + probe reflections in the wgpu pipeline
- pat-mgixk.53 [STUB] Materials3D: load ExtResource .tres materials for material_override, overlay and surface overrides
- pat-mgixk.54 [UNWIRED] FileAccess: bind the full typed read/write surface to GDScript (+1 related)
- pat-mgixk.55 [UNWIRED] ResourceUID: populate the registry at boot and resolve uid:// main_scene/autoloads/load()
- pat-mgixk.56 [MISSING] Export boot (1/3): read project.binary, follow .remap redirects, pack-aware .import resolution (+2 related)
- pat-mgixk.57 [UNWIRED] SceneTree: gate MainLoop _process/_physics_process on set_process* and physics priority, and expose toggles to GDScript
- pat-mgixk.58 [UNWIRED] Node: route process_mode/process_priority/process_physics_priority property writes into the Node fields
- pat-mgixk.59 [MISSING] GDScript: make get_tree()/get_viewport()/get_window() return real objects with SceneTree/Viewport/Window property, method and signal dispatch
- pat-mgixk.60 [UNWIRED] Node: dispatch the hierarchy/query API from GDScript to the existing SceneTree implementations (+1 related)
- pat-mgixk.61 [PARTIAL] PackedScene: instantiate() returns a detached, script-attached subtree; enter/ready fire once on add_child
- pat-mgixk.62 [PARTIAL] SceneTree: scene change replaces only current_scene (keep autoloads), defers all variants, re-attaches scripts, emits scene_changed
- pat-mgixk.63 [PARTIAL] SceneTree: deferred calls and group calls fall back to native node methods (call_deferred("add_child"), call_group(..,"queue_free"))
- pat-mgixk.64 [MISSING] Node: emit ready/tree_exiting/tree_exited/child_*_tree/child_order_changed/renamed from the lifecycle paths
- pat-mgixk.65 [UNWIRED] Theme: wire Control.theme, ancestor themes and gui/theme/custom into the Play render path
- pat-mgixk.66 [UNWIRED] Fonts: draw default UI text with the built-in TTF face in Play render, matching measured widths
- pat-mgixk.67 [UNWIRED] Particles2D: read Vector3 material gravity/direction + scale/color ramps in the 2D emitter
- pat-mgixk.68 [UNWIRED] ParticleProcessMaterial: flatten the full parameter set so sub-emitter/collision/attractor work from Godot scenes
- pat-mgixk.69 [UNWIRED] Particles2D: honor collision_mode from the material and collide in canvas space
- pat-mgixk.70 [UNWIRED] Particles3D: render draw_pass meshes per live particle and spawn from the emitter transform
- pat-mgixk.71 [UNWIRED] Resource: call setup_local_to_scene from PackedScene instancing so local_to_scene resources duplicate per instance
- pat-mgixk.72 [PARTIAL] GDScript: for-loop over Dictionary/String/Packed arrays/Vector2i and custom _iter_init objects
- pat-mgixk.73 [UNWIRED] Web runtime: boot the exported project's main scene from the mounted pck in startEngine
- pat-mgixk.74 [MISSING] Web runtime: render the running SceneTree to the canvas every frame with project stretch settings
- pat-mgixk.75 [UNWIRED] Web audio: drain the MainLoop's AudioServer into the worklet instead of a private mixer

## Proposed OUT_OF_SCOPE (needs human ratification)
- **ENetMultiplayerPeer / ENetConnection / ENetPacketPeer** (multiplayer) — citation: upstream/godot/platform/web/net_socket_web.h:50-59 - the Godot web export NetSocket returns ERR_UNAVAILABLE for open/bind/listen/connect/send/recv, so ENet (UDP) cannot work in Godot's own web export; port-audit taxonomy Scope anchor lists ENet as OOS-with-citation.
- **Raw TCP/UDP sockets (StreamPeerTCP, TCPServer, PacketPeerUDP, UDPServer, PacketPeerDTLS, DTLSServer, StreamPeerTLS) and WebSocket server role on web** (multiplayer) — citation: upstream/godot/platform/web/net_socket_web.h:50-59 (all socket ops ERR_UNAVAILABLE in Godot web export) and modules/websocket/emws_peer.cpp:117,139 (WebSocket server/accept_stream ERR_UNAVAILABLE on web).
- **SystemFont (OS font lookup) on web** (theming-text) — citation: upstream/godot/doc/classes/SystemFont.xml:11 — '[b]Note:[/b] This class is implemented on iOS, Linux, macOS and Windows, on other platforms it will fallback to default theme font.' (Web is not in the list.)
- **Accelerometer/gyroscope/magnetometer/gravity + set_joy_light on web** (input) — citation: upstream/godot/platform/web (Godot 4.6.1 web platform has no devicemotion/accelerometer/gyroscope or joy-light implementation; grep over platform/web/js/libs and display_server_web.cpp returns 0 hits), so Godot's Web export cannot do this either
- **GDExtension native library loading (manifests parsed, nothing dlopen'd)** (platform-os) — citation: THIRDPARTY_STRATEGY.md:237-268 'GDExtension native plugins (pat-0q2kx.252) — Classification: Deferred (bucket 2) ... No library is dlopen'd and no native entry point is called.'
- **OpenXR* classes (~70) and OpenXRInterface** (xr-peripheral) — citation: upstream/godot/modules/openxr/config.py:2-3 - `if platform in ("linuxbsd", "windows", "android", "macos")` else 'Not supported on these platforms' (web excluded); taxonomy Scope anchor: native OpenXR runtimes out of scope for the web runtime
- **ENet low-level classes (ENetConnection, ENetPacketPeer)** (sweep-0) — citation: .claude/skills/port-audit/references/taxonomy.md:14 scope anchor explicitly lists 'raw UDP/TCP sockets, ENet' as web-export-impossible and OUT_OF_SCOPE-eligible; THIRDPARTY_STRATEGY.md:163 'enet | Reliable UDP | MIT | Replace with enet-rs or defer'.
- **OpenXR runtime family (interface, action maps, composition layers, hand/trackers, render models, extension wrappers, haptics)** (sweep-1) — citation: upstream/godot/modules/openxr/config.py:2-6: `if platform in ("linuxbsd", "windows", "android", "macos"): return not env["disable_xr"] else: # Not supported on these platforms. return False`. Godot 4.6.1's web export does not compile the OpenXR module, so under the taxonomy scope anchor ("Runtime features that Godot's own web export cannot do (... native OpenXR runtimes ...) may be classified OUT_OF_SCOPE") none of these classes exist in a Godot web build. WebXR (WebXRInterface) is the in-scope XR path and belongs to xr-peripheral.
- **OpenXR spatial-entity / visibility-mask classes (Godot 4.6 OpenXR module)** (sweep-2) — citation: upstream/godot/modules/openxr/config.py:1-6 (OpenXR not built for the web platform in Godot 4.6.1); prd/THIRDPARTY_STRATEGY.md:81 (`thirdparty/openxr` | XR | Defer; not in v1 scope); taxonomy scope anchor lists native OpenXR runtimes as OUT_OF_SCOPE-eligible.
- **UPNP / UPNPDevice (port mapping via miniupnpc)** (sweep-2) — citation: upstream/godot/modules/upnp/register_types.cpp:35-51 and modules/upnp/SCsub:13 - Godot 4.6.1's web export compiles out the UPNP implementation (raw UDP/SSDP discovery is impossible in browsers); taxonomy scope anchor lists raw UDP/TCP sockets as OUT_OF_SCOPE-eligible.

## Refuted
- PhysicalBone3D ragdoll joint types (cone/hinge/slider/6DOF) (physics-3d): The finder relied on a stale doc comment (physics_server_3d.rs:1012-1015). The code below it maps joint_type at physics_server_3d.rs:1102-1155: 0 -> no joint, 2 -> ConeTwistJoint3D with joint_constraints/swing_span, twist_span, bias, softness and relaxation (degrees converted to radians), 3 -> HingeJoint3D with angular_limit_enabled/lower/upper, 4 -> SliderJoint3D with linear_limit_lower/upper, 5 

## Findings without a bead spec
- Signed int64 overflow panics instead of wrapping (core-variant): The code is as described: binary_add/sub/mul use plain `a + b`/`a - b`/`a * b` on i64 (interpreter.rs:5296,5316,5335), Neg uses `-i` (:3638), and engine-rs/Cargo.toml [profile.dev] (:118-119) sets only debug, so overflow checks stay on in dev/test. But release builds wrap, which matches Godot, so th
- SSR, SSIL, SDFGI, volumetric fog (Environment) (rendering-3d): The Godot citations check out: Environment.xml:218/287/306/362 say Forward+ only, display_server_web.cpp:997-1001 offers only opengl3, and detect.py:81 sets vulkan False. The scoping still fails, because no Patina scope declaration limits it to web or the Compatibility renderer. AGENTS.md and THIRDP
- shader_type fog (FogMaterial/FogVolume shaders) (shaders): The gap is real, but the reason given for scoping it out does not hold up. Absence confirmed: ShaderType3D has only Spatial and Sky (engine-rs/crates/gdserver3d/src/shader.rs:23-28), and ShaderType in gdserver2d/src/shader.rs:17-26 has Spatial, CanvasItem, Particles and Sky. A recursive grep of engi
  - SSR/volumetric fog → hand-written pat-mgixk.401; fog shaders → pat-mgixk.402; int64 overflow folded into the float-division bead per its verifier.

## Dropped/degraded
- No subsystem dropped. 16 agents (2 supplemental finders, 3 supplemental verify groups) hit a usage limit on the first pass and completed on resume.
- 1 finding remains unverified (P1 cap applied, marked in its bead).
- Merge rule applied: identical normalized group_key within a subsystem (46 merged beads). Merging on a shared primary Godot class was NOT applied, because it would have collapsed unrelated findings (e.g. everything citing Node/Object) into oversized beads.

## Critic notes
- **project-settings-surface**: No finder swept ProjectSettings as a modality. platform-os checked only max_fps and the physics knobs, and resources-io checked only main_scene and overrides. Godot 4.6.1 documents 932 ProjectSettings members (grep -c '<member ' in ProjectSettings.xml = 932). A crate-wide grep of engine-rs/crates for quoted keys under rendering/, display/, application/, physics/, input_devices/, gui/, audio/, layer_names/, internationalization/ and navigation/ (pattern '"(rendering|display|application|physics|in
- **virtual-callback-dispatch**: Finders checked method dispatch, meaning script calling engine. Nobody checked the reverse direction: engine calling the script's virtual overrides. extension_api.json has 1413 is_virtual methods. Grepping quoted virtual names in non-test engine-rs/crates found 0 hits for "_validate_property" and "_property_can_revert", and only 1 hit each for "_can_drop_data", "_drop_data", "_get_drag_data", "_make_custom_tooltip" and "_has_point". So drag-and-drop, custom tooltips, custom hit shapes, property 
- **signal-emission-sweep**: Signal emit sites were checked only for gui classes (gui-controls) and for a few AnimationMixer signals; animation confessed that mixer_applied, mixer_updated, caches_cleared and animation_list_changed were not checked. extension_api declares 489 class signals. No finder diffed each declared signal against an emit site in Patina, or checked emission order and argument shape. Examples: tree_entered/tree_exiting/tree_exited ordering, child_order_changed, visibility_changed, resized, item_rect_chan
- **classdb-registered-but-nil-sweep**: core-object explicitly confessed that the 'registered but returns Nil' sweep is undone across all 871 registered classes: every registered class should construct, and every registered method should have a non-Nil dispatch arm. Name-presence greps from the other finders cannot catch it, because a method that is in ClassDB but has no arm makes has_method() return true while the call returns Nil silently. Project memory records this failure mode recurring (is_class returned Nil for every object). T
- **import-pipeline-and-gltf**: resources-io audited only the resolution of .import/.ctex artifacts. Nobody audited the importers: texture import options (compress/mode, mipmaps, the normal_map and process/* flags), scene import (glTF/GLB, OBJ, FBX, .blend, plus import-script and subresource options), audio import options (WAV loop/trim/force mono, Ogg/MP3 loop_offset), font import, or the runtime glTF API. extension_api has 19 GLTF* classes, including GLTFDocument.append_from_file/append_from_buffer/generate_scene, which are 
- **property-defaults-and-default-theme**: Several finders confessed they did not compare property defaults: vfx compared 99 of the 113 ParticleProcessMaterial properties by name only, core-object skipped the PROPERTY_USAGE/HINT flags, and theming-text spot-checked about 4 of the roughly 800 default_theme.cpp items. No finder diffed per-class property default values against the doc XML across all classes. A wrong default changes every scene that relies on it, because .tscn files omit properties left at their default. Examples include Cam
- notes: "I picked six cross-cutting modalities that no per-subsystem finder ran end to end, ranked by expected yield. I checked them with these read-only commands:\n- jq over extension_api.json: 1413 virtual methods, 489 signals, 19 GLTF classes.\n- grep -c over ProjectSettings.xml: 932 members, against 108 distinct setting keys found in Patina.\n- Quoted-name greps for the virtual callbacks across non-test engine-rs/crates.\nSome candidates were already partly covered by a finder and I left them out: notification order (scene-tree), utility functions (gdscript), the singletons table (platform-os and sweep-1), and the editor scene-saver round-trip (confessed by resources-io, but lower priority than the six above). If there is room for a 7th finder, the next one should be lossless round-trip of a c
