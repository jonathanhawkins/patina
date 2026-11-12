# Port Audit — 2026-07-12 (oracle: Godot 4.6.1.stable.official)

**Epic:** `pat-nzgdu` · **Method:** fresh-eyes, source-derived checklist (extension_api 4.6.1.stable.official + `upstream/godot/`), adversarially verified. Read-only static analysis; bead acceptance tests are run later by the verifier lane.

**Findings:** 231 subsystem (0 refuted, 15 adjusted) + 254 sweep + 29 supplemental. **Beads:** 227 created, 72 already-tracked (skipped), 0 create-errors.

**Created by priority:** P0=24 · P1=72 · P2=73 · P3=58


## Per-subsystem

| Subsystem | Parity-OK | MISSING | STUB | UNWIRED | PARTIAL | P0 | Coverage confession (abbrev) |
|---|--:|--:|--:|--:|--:|--:|---|
| core-object | 11 | 1 | 1 | 1 | 3 | 0 | (1) Callable bind/unbind/get_object/get_bound_arguments correctness (core-variant overlap) — only confirmed connect ignores flags; (2) the complete @G |
| core-variant | 10 | 4 | 0 | 0 | 8 | 0 | (1) exact numerical/epsilon parity of the math ops that DO exist vs Godot fixtures; (2) the full StringName (110 methods) and NodePath (11 methods) me |
| gdscript | 15 | 3 | 0 | 1 | 5 | 2 | exhaustive per-method audit of the ~100 String/Array/Dictionary methods and full match-pattern runtime semantics (spot-checked only); type_checker.rs  |
| scene-tree | 14 | 8 | 0 | 2 | 0 | 1 | Turned every scene-tree taxonomy stone against Godot 4.6.1 ground truth (extension_api SceneTree/Node/Viewport/Window/SubViewport/CanvasLayer/Timer me |
| nodes-2d | 5 | 5 | 1 | 7 | 1 | 1 | (1) Physics semantics of Area2D/StaticBody2D/RigidBody2D/CharacterBody2D/CollisionShape2D/CollisionPolygon2D and 2D joints — deferred to the physics-2 |
| rendering-2d | 6 | 4 | 0 | 3 | 0 | 1 | (1) the wgpu 2D backend (gdrender2d/wgpu_backend.rs) — confirmed unwired (only exported from lib.rs) but I did not enumerate its batch/shader feature  |
| nodes-3d | 10 | 1 | 0 | 7 | 3 | 1 | SCOPE: I audited the 3D scene-node surface (Node3D transform semantics; MeshInstance3D/MultiMeshInstance3D; Camera3D; Light3D family; ReflectionProbe/ |
| rendering-3d | 10 | 6 | 0 | 0 | 7 | 1 | Covered the full taxonomy floor for rendering-3d: StandardMaterial3D/ORMMaterial3D surface + the scene-to-viewport material bridge, Environment (tonem |
| shaders | 4 | 3 | 1 | 1 | 2 | 2 | Read the full 3D shader compiler (gdserver3d/src/shader.rs, 1499 lines), the software fragment bridge (gdrender3d/src/shader.rs), the wgpu pipeline sh |
| vfx-particles | 0 | 4 | 0 | 2 | 2 | 2 | STRONG on the core question (wiring) and class inventory; every claim is backed by grep/file:line. Confirmed: all 3 CPU particle simulators (gdscene/s |
| animation | 5 | 2 | 0 | 3 | 3 | 2 | Fully audited (read end-to-end): animation.rs, animation_tree.rs, tween.rs, skeleton3d.rs, plus the wiring surfaces — SceneTree side-tables/process_an |
| physics-2d | 10 | 6 | 0 | 2 | 2 | 1 | WHAT I COVERED: Read the full gdphysics2d 2D core (shape.rs, collision.rs narrowphase, body.rs integrator, world.rs step, character.rs, area2d.rs, joi |
| physics-3d | 6 | 2 | 1 | 5 | 3 | 1 | Fully covered every physics-3d taxonomy stone by reading all gdphysics3d modules (world/body/collision/character/area3d/joint/query/shape), the gdscen |
| navigation | 0 | 3 | 1 | 1 | 1 | 1 | Covered all navigation taxonomy stones against Godot 4.6.1 ground truth (extension_api.json: 24 nav/AStar classes, 137/154 server methods, agent prope |
| audio | 8 | 3 | 2 | 2 | 2 | 3 | Fully audited: gdaudio (server/bus/mixer/stream/player/spatial/effects/decode/wav/import/output/sample), the gdscene AudioServerBridge + MainLoop wiri |
| multiplayer | 5 | 1 | 1 | 6 | 0 | 3 | Turned every stone in the taxonomy 'multiplayer' section and classified the whole subsystem. Verified ground truth against extension_api.json (38 netw |
| gui-controls | 5 | 6 | 1 | 2 | 4 | 3 | Audited all ~60 Control-subtree classes + the Window/Popup/dialog subtree (from extension_api.json inheritance walk) against Patina ClassDB registrati |
| theming-text | 2 | 4 | 3 | 1 | 0 | 1 | Fully audited (ground truth = extension_api.json + upstream/godot C++): Theme/ThemeDB (gdscene/src/theme.rs), StyleBox family, Font/FontFile + .ttf im |
| input | 5 | 5 | 0 | 2 | 2 | 2 | the native winit/PlatformBackend event completeness in display.rs/window.rs (only spot-checked — it converts Key+Mouse only, no gamepad/touch WindowEv |
| resources-io | 10 | 5 | 0 | 4 | 7 | 2 | (ran out of budget, would refine on a second pass): (1) glTF import feature completeness vs Godot (gltf_import.rs, 37KB — only confirmed it's wired, n |
| platform-os | 7 | 3 | 1 | 3 | 3 | 1 | Turned every platform-os stone against the extension_api.json inventory and upstream/godot: OS/Time/Engine/DisplayServer/JavaScriptBridge/Performance/ |
| editor-ux | 24 | 5 | 2 | 5 | 2 | 1 | METHOD: Cross-referenced all 238 server /api routes (editor_server.rs) against the served client's api()-helper call sites (single helper at editor_ui |
| web-runtime | 0 | 4 | 1 | 1 | 1 | 2 | Covered the full web-runtime stone list by static analysis against Godot 4.6.1 (platform/web C++/JS + extension_api singletons). Headline: Patina has  |
| xr-peripheral | 0 | 4 | 0 | 0 | 0 | 0 | Fully covered (READ-ONLY, no cargo/br/edits run): the entire xr-peripheral surface — XR (XRServer + 16 XR* classes/nodes/trackers), WebXRInterface (al |

## P0 beads created (critical: broken core path or false-done behind green tests)

- `pat-nzgdu.1` [audio] Audio: feed the mixer output to a real backend in patina-runner (web AudioContext/Worklet + desktop attach)
- `pat-nzgdu.2` [audio] Audio: dispatch AudioStreamPlayer play/stop/seek/is_playing method calls from GDScript to the bridge
- `pat-nzgdu.3` [autoload-global-singletons-runtime] Autoload: instantiate project [autoload] singletons under /root at boot (runner + editor Play)
- `pat-nzgdu.4` [editor-ux] Editor: wire scene-tree Change-Type/Make-Local/Editable-Children/%Unique/Lock/Warnings into served context menu
- `pat-nzgdu.5` [gdscript] GDScript: parse annotation arguments and carry @export hint metadata into ExportInfo
- `pat-nzgdu.6` [gdscript] GDScript: drive coroutine/await resumption from the SceneTree tick and bind awaited values
- `pat-nzgdu.7` [globalscope-enum-constants-in-gdscript] GDScript: resolve @GlobalScope enum constants (KEY_*/OK/FAILED/ERR_*/MOUSE_BUTTON_*/PROPERTY_HINT_*/alignment/TYPE_*) in the VM
- `pat-nzgdu.8` [image-import-format-breadth-webp-svg-compressed] Image import: decode WebP source + WebP-compressed .ctex (Godot default web/2D texture)
- `pat-nzgdu.9` [input] Input singleton: add get_axis / is_action_just_released / get_action_strength + capture just_released in the snapshot + circular-deadzone get_vector/get_axis
- `pat-nzgdu.10` [navigation] Navigation: wire NavigationAgent2D/3D nodes to per-frame pathfinding + signals
- `pat-nzgdu.11` [nodes-2d] Nodes2D: wire a SceneTree->CanvasItem bridge so 2D nodes emit draw commands into the served viewport
- `pat-nzgdu.12` [nodes-3d] Nodes3D: drive RayCast3D/ShapeCast3D/SpringArm3D every physics frame and expose to GDScript
- `pat-nzgdu.13` [notification-virtual-delivery-to-gdscript] Notifications: deliver Object::notification -> _notification(int) virtual to attached GDScripts
- `pat-nzgdu.14` [physics-2d] Physics2D: wire RigidBody2D force/impulse/torque script API + angular dynamics
- `pat-nzgdu.15` [project-settings-runtime-honoring-and-script-api] ProjectSettings: honor physics tick rate + 2D/3D gravity from project.godot at boot
- `pat-nzgdu.16` [rendering-2d] Rendering2D: wire SceneTree -> canvas pipeline with global transforms and z/y-sort draw order
- `pat-nzgdu.17` [resources-io] Resources/Boot: boot patina-runner from project.godot (main scene, autoloads, input map, ProjectSettings, res:// root)
- `pat-nzgdu.18` [resources-io] Editor/Resources: lossless .tscn save — regenerate ext_resource/sub_resource tables, uid+load_steps header, and full Variant serialization
- `pat-nzgdu.19` [scene-tree] Runtime: drive the MainLoop continuously during editor Play
- `pat-nzgdu.20` [shaders] Shaders: real GDShader fragment/vertex compiler + execution (not magic-uniform pattern-matching)
- `pat-nzgdu.21` [shaders] Shaders: load .gdshader/ShaderMaterial resources, register in ClassDB, and wire material_override to the renderer
- `pat-nzgdu.22` [vfx-particles] Particles2D: wire GPUParticles2D/CPUParticles2D into the frame loop and 2D renderer
- `pat-nzgdu.23` [web-runtime] Editor: drive the running scene in real time on Play (continuous MainLoop tick)
- `pat-nzgdu.24` [web-runtime] Web export: generate real HTML shell + engine loader + PWA/COOP-COEP config (stop faking success)

**P0-level gaps already tracked (skipped, 11):** re-confirmed present against current tree — query matched existing beads:
- [animation] Animation: wire AnimationPlayer from .tscn load and GDScript play() to node-property output  (query `AnimationPlayer` → 4 hit)
- [animation] Tween: bind create_tween() into GDScript with node auto-kill, process/pause modes and signals  (query `PropertyTweener` → 1 hit)
- [globalscope-utility-functions-completeness-diff] GDScript globals: wire var_to_bytes/bytes_to_var to gdvariant codec + add var_to_str/str_to_var  (query `var_to_bytes` → 1 hit)
- [gui-controls] GUI: tick Control/Container layout from SceneTree + editor render loop  (query `Panel` → 3 hit)
- [input] Input: event-routing pipeline — dispatch _input/_shortcut_input/_unhandled_key_input/_unhandled  (query `InputEventKey` → 2 hit)
- [multiplayer] Multiplayer: wire MultiplayerAPI into SceneTree per-frame poll + connection signals + Offline d  (query `MultiplayerAPI` → 1 hit)
- [multiplayer] Multiplayer: make MultiplayerSpawner/Synchronizer tree-wired nodes + SceneReplicationConfig rep  (query `MultiplayerSpawner` → 1 hit)
- [physics-3d] Physics3D: wire CharacterBody3D.move_and_slide through MainLoop + implement full move_and_slide  (query `CharacterBody3D` → 1 hit)
- [platform-os] Engine: wire is_editor_hint + get_frames_per_second to runtime state and add frame counters, ti  (query `Engine` → 8 hit)
- [rendering-3d] Materials3D: render StandardMaterial3D metallic/roughness/emission + PBR texture maps end-to-en  (query `StandardMaterial3D` → 2 hit)
- [theming-text] Theming-text: wire Label/Button text through font+theme to visible pixels in runner and editor   (query `Label` → 7 hit)

## Proposed OUT_OF_SCOPE — needs human ratification (13)

No beads filed. Ratify or override each; anything without a valid citation is a real gap.

- **OpenXR native runtime (63 OpenXR* classes) + MobileVRInterface** [xr-peripheral] — classes: OpenXRInterface, OpenXRActionMap, OpenXRHand, OpenXRCompositionLayer, OpenXRSpatialEntityTracker, MobileVRInterface — citation: prd/THIRDPARTY_STRATEGY.md:81 — `| thirdparty/openxr | XR | Defer; not in v1 scope | — |`; reinforced by Godot 4.6 web e
- **ENet native transport (ENetConnection/ENetPacketPeer)** [multiplayer/enet-native-transport] — classes: ENetConnection, ENetPacketPeer — citation: Taxonomy scope anchor: 'Runtime features that Godot's own web export cannot do (raw UDP/TCP sockets, ENet ...) may be cl
- **GDExtension native extension loading (GDExtension/GDExtensionManager)** [platform-os/gdextension] — classes: GDExtension, GDExtensionManager — citation: Taxonomy scope anchor: web export 'cannot do (... native OpenXR runtimes, subprocess spawning, native file dialogs)'-cla
- **MobileVRInterface (native mobile split-screen VR)** [xr-peripheral/mobile-vr-interface] — classes: MobileVRInterface — citation: Taxonomy scope anchor references/taxonomy.md:759-760 (xr-peripheral): "OpenXR* (~40 classes) + MobileVRInterface: native
- **NativeMenu (native OS menu bar) singleton** [platform-os/native-menu] — classes: NativeMenu — citation: Taxonomy scope anchor references/taxonomy.md:494 (gui-controls): "MenuBar (+native menu OOS on web)" and scope anchor li
- **OpenXR native XR class family** [xr-peripheral/openxr-family] — classes: OpenXRAction, OpenXRActionBindingModifier, OpenXRActionMap, OpenXRActionSet, OpenXRAnalogThresholdModifier, OpenXRAnchorTracker — citation: prd/THIRDPARTY_STRATEGY.md:81 — '| thirdparty/openxr | XR | Defer; not in v1 scope | — |'. Reinforced by taxonomy.md:15 
- **OpenXR spatial-entity / anchor / marker / plane-tracking family** [xr-peripheral/openxr-spatial-entity] — classes: OpenXRSpatialComponentBounded3DList, OpenXRSpatialComponentData, OpenXRSpatialComponentMarkerList, OpenXRSpatialComponentMesh2DList, OpenXRSpatialComponentMesh3DList, OpenXRSpatialComponentParentList — citation: prd/THIRDPARTY_STRATEGY.md: '| `thirdparty/ni` | XR | Defer; not in v1 scope | — |' plus the port-audit scope anchor (ta
- **RenderingDevice pipeline-state description structs (RD*)** [rendering-3d/rendering-device-pipeline-structs] — classes: RDAttachmentFormat, RDFramebufferPass, RDPipelineColorBlendState, RDPipelineColorBlendStateAttachment, RDPipelineDepthStencilState, RDPipelineMultisampleState — citation: .claude/skills/port-audit/references/taxonomy.md scope anchor (lines 8-19): runtime parity bar = 'everything Godot 4.6.1
- **SocketServer (abstract native socket server base)** [multiplayer/socket-server] — classes: SocketServer — citation: port-audit taxonomy.md scope anchor: 'Runtime features that Godot's own web export cannot do (raw UDP/TCP sockets, ENet,
- **StatusIndicator — OS system-tray / status-bar icon** [platform-os/status-indicator] — classes: StatusIndicator — citation: Taxonomy scope anchor (references/taxonomy.md L14-16): runtime features Godot's own web export cannot do (native file di
- **TCPServer — raw TCP listening socket server** [multiplayer/tcp-server] — classes: TCPServer — citation: Taxonomy multiplayer stone (references/taxonomy.md ~L414-416): 'StreamPeer family... TCPServer/UDPServer — native-scope;
- **Native/extension StreamPeers — StreamPeerSocket, StreamPeerUDS, StreamPeerTLS, S** [multiplayer/streampeer-native] — classes: StreamPeerSocket, StreamPeerUDS, StreamPeerTLS, StreamPeerExtension — citation: Taxonomy scope anchor (references/taxonomy.md L14-16) + multiplayer stone (~L414): raw sockets are native-scope and OUT_
- **Native raw-socket / UPnP networking (UDPServer, UDSServer, UPNP, UPNPDevice)** [multiplayer/native-socket-oos] — classes: UDPServer, UDSServer, UPNP, UPNPDevice — citation: port-audit taxonomy Scope anchor (references/taxonomy.md lines 14-16): 'Runtime features that Godot's own web export can

## Dropped / degraded coverage

- **Sweep chunks 3, 6, 16 failed** (StructuredOutput schema retry cap, not usage limit) — ~150 unregistered Godot classes were not triaged by the sweep. These are dominated by editor-internal classes (`Editor*`, export platforms), `Input*`/`Joint*`/`JSON`/`GLTF*` (index 6), and `VisualShaderNode*` particle-emitter nodes (index 16). Behavioral finders for those subsystems (input, physics, resources-io, shaders, editor-ux) still ran and cover the important gaps; the untriaged remainder is mostly exotic/editor-internal. **Recommend a scoped re-sweep** of those classes.
- No bead create-errors.
- All 24 subsystem finders + verifiers completed (no dropped subsystems). Oracle was the live Godot 4.6.1 API dump (not degraded).

## Critic notes (completeness)

- **globalscope-enum-constants-in-gdscript** — No finder audited whether the GDScript VM resolves @GlobalScope named constants (KEY_SPACE, OK/FAILED, MOUSE_BUTTON_LEFT, PROPERTY_HINT_RANGE, HORIZONTAL_ALIGNMENT_CENTER, TYPE_VECTOR2, SIDE_LEFT, JOY (→ supplemental pass run)
- **autoload-global-singletons-runtime** — Autoloads (global singleton nodes like Global.gd / GameManager) are the single most common cross-scene state mechanism in real Godot projects, yet NO finder in the 24-subsystem list mentions 'autoload (→ supplemental pass run)
- **project-settings-runtime-honoring-and-script-api** — ProjectSettings as a runtime behavioral surface + script API is confessed-skipped by three finders (physics-2d: 'did not trace a boot call site reading default_gravity / physics_ticks_per_second'; pla (→ supplemental pass run)
- **globalscope-utility-functions-completeness-diff** — A complete diff of the 114 @GlobalScope utility_functions was never produced: gdscript verified the dispatch MECHANISM plus common math fns, core-object explicitly DEFERRED 'the complete @GlobalScope  (→ supplemental pass run)
- **image-import-format-breadth-webp-svg-compressed** — Texture/image decode breadth is far below Godot's web-export set and, critically, misses the DEFAULT web texture format -- resources-io confessed not verifying importer completeness and this depth was (→ supplemental pass run)
- **notification-virtual-delivery-to-gdscript** — The `_notification(what)` virtual is never delivered to GDScript, a distinct modality from scene-tree's lifecycle-SIGNAL findings and from gdscript's `_ready`/`_process` dispatch. The engine routes no (→ supplemental pass run)
