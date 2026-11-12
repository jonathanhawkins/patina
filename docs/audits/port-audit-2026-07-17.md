# Port Audit — 2026-07-17 (oracle: Godot Engine v4.6.1.stable.official)

Epic: **pat-0q2kx** · Findings: 524 verified (0 refuted, 0 unverified; 8 adjusted) · Beads: **264 created, 33 already tracked** · OUT_OF_SCOPE: 13 (need ratification)


Oracle: fresh `--dump-extension-api` (Godot Engine v4.6.1.stable.official, 1023 classes) + upstream/godot C++ + doc XML. Finders authored by Fable 5; **every finding adversarially re-verified by Opus 4.8** (verify + completeness critic + 6 supplemental finders all ran on Opus). 0 of 349 finder findings were refuted.


Priority split (created beads): P0=51, P1=103, P2=82, P3=28


## Per-subsystem

| Subsystem | Parity-OK | MISSING | STUB | UNWIRED | PARTIAL | P0s | Coverage confession |
|---|--:|--:|--:|--:|--:|--:|---|
| core-object | 14 | 3 | 0 | 3 | 1 | 2 | Covered every taxonomy stone for core-object: Object method surface (all 49 api methods hunted individually with recorded 0-hit greps for… |
| core-variant | 14 | 3 | 0 | 2 | 8 | 0 | Covered every taxonomy stone for core-variant with file:line evidence: the 38-builtin presence matrix (9 types missing entirely), constru… |
| gdscript | 20 | 6 | 0 | 3 | 7 | 2 | Covered: full taxonomy floor for gdscript — grammar (match/lambda/await/setget/static/super/inner classes/operators/literals), all 37 @GD… |
| scene-tree | 17 | 4 | 0 | 2 | 3 | 2 | Covered every taxonomy bullet for scene-tree: lifecycle order + notifications (found wrong constant values), process modes/priorities/pau… |
| nodes-2d | 11 | 2 | 3 | 5 | 6 | 2 | Audited every taxonomy bullet for nodes-2d against extension_api.json + upstream scene/2d, tracing the two production 2D pixel paths (edi… |
| rendering-2d | 10 | 4 | 0 | 3 | 1 | 2 | Covered all six taxonomy stones for rendering-2d with call-path tracing: the full CanvasItem draw_*/script API, ordering/inheritance sema… |
| nodes-3d | 10 | 0 | 1 | 10 | 2 | 2 | I traced all three 3D render paths (test-only RenderServer3DAdapter in gdscene/render_server_3d.rs; editor viewport software+wgpu via gde… |
| rendering-3d | 14 | 3 | 0 | 3 | 3 | 4 | Covered with file:line evidence: the full wired 3D render path (editor viewport + Play in examples/editor.rs), the unwired RenderServer3D… |
| shaders | 8 | 6 | 1 | 2 | 2 | 1 | Covered all six taxonomy stones with static call-path tracing: shader types + render_mode lists (diffed against shader_types.cpp:230-336)… |
| vfx-particles | 10 | 4 | 1 | 2 | 0 | 1 | Covered every taxonomy bullet for vfx-particles: node surface (GPUParticles2D/3D, CPUParticles2D/3D) diffed against extension_api.json pr… |
| animation | 14 | 4 | 0 | 6 | 4 | 4 | Covered every taxonomy stone for animation: Animation resource track types/interpolation/loop modes, AnimationMixer base surface, Animati… |
| physics-2d | 13 | 1 | 0 | 6 | 6 | 4 | Read in full: gdphysics2d world.rs/shape.rs/body.rs/collision.rs/area2d.rs (plus character.rs core loop, joint.rs outline, ccd.rs/tilemap… |
| physics-3d | 15 | 4 | 0 | 4 | 3 | 3 | Covered every taxonomy stone for physics-3d: shapes/narrowphase matrix (read collision.rs dispatch in full), broadphase, RigidBody3D surf… |
| navigation | 9 | 2 | 0 | 3 | 4 | 2 | Covered: all 24 navigation classes + 5 singletons sliced from extension_api.json (methods/properties/signals); full read of gdscene/src/n… |
| audio | 10 | 2 | 2 | 4 | 2 | 2 | Covered: the full gdaudio crate (all 18 modules read or skimmed with the 8 core ones read line-by-line), AudioServerBridge and its MainLo… |
| multiplayer | 3 | 2 | 1 | 3 | 2 | 0 | Covered every bullet of the multiplayer taxonomy section: Variant codec, StreamPeer/PacketPeer family, HTTP, WebSocket, WebRTC, ENet/Offl… |
| gui-controls | 14 | 7 | 0 | 4 | 7 | 2 | Covered: all 62 runtime Control classes + 7 Window/dialog classes from extension_api.json traced through gdscene (control.rs 2881 lines, … |
| theming-text | 6 | 1 | 0 | 3 | 4 | 1 | Covered fully: Theme/ThemeDB resource surface + default theme vs default_theme.cpp (item counts verified: 794 items/~60 types vs 24/6); a… |
| input | 10 | 4 | 1 | 4 | 4 | 5 | Covered every taxonomy stone for 'input': all 21 API classes inventoried from extension_api.json and hunted in Patina (registration table… |
| resources-io | 16 | 5 | 1 | 3 | 5 | 1 | Covered against the taxonomy floor: text .tscn/.tres parse+save fidelity, binary .res/.scn, PCK, import pipeline/.ctex, ResourceLoader/Sa… |
| platform-os | 9 | 4 | 2 | 3 | 4 | 1 | Covered: all taxonomy bullets for platform-os — OS/Time/Engine surfaces method-by-method against extension_api.json (83/21/41 methods), D… |
| editor-ux | 23 | 6 | 3 | 5 | 11 | 6 | Verified end-to-end wiring (served EDITOR_HTML fetch -> editor_server route -> engine state) for every taxonomy stone group: run bar, doc… |
| web-runtime | 7 | 1 | 0 | 0 | 0 | 0 | Audited against upstream/godot/platform/web (web_main.cpp, os_web.cpp, net_socket_web.h, audio_driver_web.cpp, javascript_bridge_singleto… |
| xr-peripheral | 7 | 2 | 0 | 1 | 2 | 0 | Covered: full XR class inventory from extension_api.json (92 classes XR*/OpenXR*/WebXR/MobileVR/Camera*/MovieWriter) diffed against Class… |
| class-inheritance-parent-chain-fidelity |  | 0 | 0 | 0 | 6 | 1 |  |
| globalscope-enum-constant-value-fidelity |  | 0 | 0 | 0 | 1 | 0 |  |
| project-settings-defaults-sweep |  | 0 | 0 | 0 | 2 | 0 |  |
| registered-class-property-default-fidelity |  | 0 | 0 | 1 | 3 | 0 |  |
| utility-function-argument-and-return-semantics |  | 2 | 0 | 0 | 2 | 0 |  |
| variant-builtin-type-method-semantics |  | 2 | 0 | 1 | 6 | 1 |  |

## P0 list (51 created)

- **pat-0q2kx.8** [core-object] GDScript: synchronous cross-node script method calls + call/callv/has_method on object refs + Callable target routing
- **pat-0q2kx.9** [core-object] Variant/GDScript: first-class Signal type with connect/emit/disconnect + Godot 4 connect(Callable) signature
- **pat-0q2kx.16** [gdscript] GDScript: generic Object method dispatch — script methods on other nodes + native node method fallback
- **pat-0q2kx.17** [gdscript] GDScript: apply .tscn exported-property overrides to script instances via setters
- **pat-0q2kx.46** [scene-tree] Editor Play: dispatch full Godot lifecycle (_enter_tree parent-first, _ready child-first) via LifecycleManager
- **pat-0q2kx.47** [scene-tree] Scripting: wire SceneTree control surface (change_scene/quit/pause/groups/create_tween) into the GDScript bridge
- **pat-0q2kx.27** [nodes-2d] Camera2D: wire runtime scroll into the 2D canvas (zoom, offset, limits, smoothing, drag)
- **pat-0q2kx.28** [nodes-2d] Physics2D nodes: resolve CollisionShape2D SubResource shapes + CollisionPolygon2D, disabled, one_way, multi-shape
- **pat-0q2kx.39** [rendering-2d] Rendering2D: wire canvas_order semantics (modulate propagation, z_as_relative, y_sort, clip_children) into the live Play
- **pat-0q2kx.40** [rendering-2d] Rendering2D: implement Viewport canvas_transform and feed it from the active Camera2D each Play frame
- **pat-0q2kx.29** [nodes-3d] Node3D: unify position/rotation/scale/transform into one canonical local-transform state
- **pat-0q2kx.30** [nodes-3d] Play/game view: render running 3D scenes from the scene's current Camera3D with a game-mode render list
- **pat-0q2kx.41** [rendering-3d] Render3D: sync Omni/Spot lights and shadow_enabled from the scene into editor+play rendering
- **pat-0q2kx.42** [rendering-3d] Render3D: wire StandardMaterial3D resources (params + textures) from .tscn into the editor/play render path
- **pat-0q2kx.43** [rendering-3d] Render3D: honor primitive mesh size/subdivision properties and render real ArrayMesh/glTF/OBJ geometry
- **pat-0q2kx.44** [rendering-3d] Play mode: render 3D game frames from the scene's active Camera3D without editor overlays; honor camera_override
- **pat-0q2kx.48** [shaders] Shaders2D: wire CanvasItem ShaderMaterial from .tscn into the 2D render path
- **pat-0q2kx.51** [vfx-particles] VFX-2D: render live particles in the editor Play canvas + edit viewport (texture quads, global-space local_coords)
- **pat-0q2kx.1** [animation] Animation: parse Animation/AnimationLibrary from .tscn and attach AnimationPlayer at scene load with autoplay
- **pat-0q2kx.2** [animation] AnimationTree: SceneTree store + MainLoop tick + build tree_root from .tscn AnimationNode* sub_resources
- **pat-0q2kx.3** [animation] Skeleton3D: run modifier pass + bone attachments each frame and feed live bone poses into render-path skinning
- **pat-0q2kx.4** [animation] Tween: expose create_tween()+tweeners to GDScript and dispatch callbacks/methods/finished from the frame tick
- **pat-0q2kx.31** [physics-2d] Physics2D: register/sync bodies and areas with global transforms (compose parent chain, write back through inverse-paren
- **pat-0q2kx.32** [physics-2d] RayCast2D: publish _hit_* results from a globally-transformed, fully-filtered cast (mask, exclude_parent, hit_from_insid
- **pat-0q2kx.33** [physics-2d] GDScript: dispatch CharacterBody2D move_and_slide/move_and_collide + floor-state query methods to the physics bridge
- **pat-0q2kx.34** [physics-2d] Physics2D: resolve .tscn CollisionShape2D/CollisionPolygon2D sub-resources into real world shapes (offset, disabled, one
- **pat-0q2kx.35** [physics-3d] Area3D: register scene nodes into PhysicsWorld3D, emit overlap signals, wire space overrides
- **pat-0q2kx.36** [physics-3d] CharacterBody3D: wire move_and_slide into the physics tick and script surface (3D mirror of 2D flow)
- **pat-0q2kx.37** [physics-3d] RigidBody3D: 3D script force/impulse/velocity API + per-tick property sync + contact_monitor signals
- **pat-0q2kx.25** [navigation] Navigation: NavigationAgent2D/3D Godot agent contract — parent-position source, script method dispatch, full signal set
- **pat-0q2kx.26** [navigation] Navigation: feed runtime navmesh from NavigationRegion2D/3D navigation_polygon/navigation_mesh resources
- **pat-0q2kx.5** [audio] Audio: wire seek/playback_position/stream_paused/pitch_scale through AudioServerBridge + pause audio on SceneTree pause
- **pat-0q2kx.6** [audio] Audio: resolve .tscn ExtResource/SubResource audio streams to playable buffers in AudioServerBridge
- **pat-0q2kx.18** [gui-controls] GUI input: wire Control mouse/key routing stage into SceneTree::push_input
- **pat-0q2kx.19** [gui-controls] GUI render: drive control_render + sort_container_tree from the Play-view 2D frame
- **pat-0q2kx.49** [theming-text] Theming: wire Theme/StyleBox/control_render pipeline into the editor Play and 2D viewport render paths
- **pat-0q2kx.20** [input] Input: typed InputEvent objects in _input + register all InputEvent/Shortcut classes
- **pat-0q2kx.21** [input] Input: GUI input stage — focus system, _gui_input dispatch, mouse picking in push_input
- **pat-0q2kx.22** [input] Input: load project.godot [input] into editor Play and install parsed map in runner
- **pat-0q2kx.23** [input] Input: wire editor-Play mouse events end-to-end (push_event + snapshot + wheel)
- **pat-0q2kx.24** [input] Input: route Input singleton mutators from scripts to the engine InputState
- **pat-0q2kx.45** [resources-io] Resources: route runtime + editor loads through ResourceFormatLoader (import remap, ctex/webp/svg/jpg, real load() resul
- **pat-0q2kx.38** [platform-os] DisplayServer: backend-trait wiring for clipboard/TTS/window metrics + honest has_feature
- **pat-0q2kx.10** [editor-ux] Debugger: breakpoint gutter toggle (click + F9) wired to /api/script/breakpoint/toggle
- **pat-0q2kx.11** [editor-ux] Editor: implement /api/export/presets CRUD routes and wire Export dialog to a real web export
- **pat-0q2kx.12** [editor-ux] Editor: undo steps for viewport transforms, structural ops, signals, and animation edits
- **pat-0q2kx.13** [editor-ux] Inspector: functional resource picker — New (typed), Quick Load, drag-drop assign, Make Unique/Save As
- **pat-0q2kx.14** [editor-ux] ScriptEditor: wire completion popup + Ctrl+Click goto-definition to the live endpoints
- **pat-0q2kx.15** [editor-ux] Viewport2D: route drags through the active tool (rotate/scale/axis/multi) to the existing endpoints
- **pat-0q2kx.7** [class-inheritance-parent-chain-fidelity] ClassDB: register Resource as a RefCounted subclass (fix Resource<-Object chain)
- **pat-0q2kx.50** [variant-builtin-type-method-semantics] Variant: wire Rect2/Transform2D/Transform3D/Quaternion/Basis/AABB/Plane into the GDScript VM (constructors, members, met

## Proposed OUT_OF_SCOPE — needs human ratification (13)

No beads filed. Each carries a scope citation; ratify or reject.

- **ENet protocol family: ENetConnection, ENetPacketPeer, true ENet-compatible UDP transport** (ENetConnection, ENetPacketPeer, ENetMultiplayerPeer) — cite: .claude/skills/port-audit/references/taxonomy.md lines 14-19 (Scope anchor: 'Runtime features that Godot's own web export cannot do (raw UDP/TCP sockets, ENet, ...) may be classified OUT_OF_SCOPE') and lines 419-423 ('ENetMultiplayerPeer surface (native-scope; web OOS cite)'); THIRDPARTY_STRATEGY.md:163 ('enet | Reliable UDP | MIT | Replace with enet-rs or defer')
- **Magnify/Pan trackpad gesture event generation at runtime** (InputEventMagnifyGesture, InputEventPanGesture) — cite: Taxonomy scope anchor ('Runtime features that Godot's own web export cannot do ... may be classified OUT_OF_SCOPE'); ground truth: upstream/godot/platform/web/display_server_web.cpp and platform/web/js/libs/library_godot_input.js contain no magnify/pan gesture synthesis.
- **Raw TCP/UDP sockets, TCPServer/UDPServer listening, and ENet transport running inside the browser tab** (StreamPeerTCP, PacketPeerUDP, TCPServer, UDPServer, ENetMultiplayerPeer) — cite: upstream/godot/platform/web/net_socket_web.h (ERR_UNAVAILABLE for every raw-socket op) + port-audit taxonomy scope anchor: 'Runtime features that Godot's own web export cannot do (raw UDP/TCP sockets, ENet, ...) may be classified OUT_OF_SCOPE' (.claude/skills/port-audit/references/taxonomy.md lines 14-19)
- **OpenXR native runtime + ~60 OpenXR* classes (incl. action maps, composition layers, spatial entities, editor UIs)** (OpenXRInterface, OpenXRActionMap, OpenXRCompositionLayer, OpenXRHand) — cite: upstream/godot/modules/openxr/config.py (native platforms only — not built for web); prd/THIRDPARTY_STRATEGY.md:81 'thirdparty/openxr | XR | Defer; not in v1 scope'
- **MovieWriter offline movie capture (--write-movie pipeline, PNG/WAV + AVI writers)** (MovieWriter) — cite: upstream/godot/main/main.cpp:3663-3667 — MovieWriter runs only under the --write-movie CLI flag, which upstream/godot/platform/web/ never sets (grep write_movie platform/web = 0); headless/desktop-only feature per Godot's own web behavior
- **DTLSServer (server-side DTLS socket)** (DTLSServer) — cite: taxonomy.md Scope anchor: 'Runtime features that Godot's own web export cannot do (raw UDP/TCP sockets, ENet, ...) may be classified OUT_OF_SCOPE with a citation'; taxonomy.md multiplayer section: 'TCPServer/UDPServer — native-scope; on web cite OUT_OF_SCOPE per web limitations'. DTLSServer is a server-side DTLS socket over UDPServer, same server-socket class as TCPServer/UDPServer.
- **Native mobile/embedded editor export platforms (Android/iOS/AppleEmbedded/VisionOS)** (EditorExportPlatformAndroid, EditorExportPlatformIOS, EditorExportPlatformAppleEmbedded, EditorExportPlatformVisionOS) — cite: prd/PHASE7_PLATFORM_PARITY_AUDIT.md:419 declares 'PackageExecutor::validate_platform() checks the target against DESKTOP_TARGETS (linux, macos, windows, web only)' — Android/iOS/visionOS/AppleEmbedded are explicitly outside the supported target slice; combined with taxonomy.md Scope anchor (browser-native engine; runtime bounded by web export). Mobile/embedded cross-compilation requires native SDKs (Android NDK, Xcode) unavailable to the engine's export path.
- **ENet networking (ENetConnection / ENetPacketPeer)** (ENetConnection, ENetPacketPeer) — cite: THIRDPARTY_STRATEGY.md:163 (enet → 'Replace with enet-rs or defer'); .claude/skills/port-audit/references/taxonomy.md:14-16 + multiplayer stone 'ENetMultiplayerPeer surface (native-scope; web OOS cite)'. Browsers expose no raw UDP, so ENet cannot run in the web target (matches Godot's own web-export limitation).
- **OpenXR class family (native VR/AR runtime bindings)** (OpenXRAction, OpenXRActionBindingModifier, OpenXRActionMap, OpenXRActionSet, OpenXRAnalogThresholdModifier, OpenXRAnchorTracker, OpenXRAndroidThreadSettingsExtension, OpenXRAPIExtension, OpenXRBindingModifier, OpenXRBindingModifierEditor, OpenXRCompositionLayer, OpenXRCompositionLayerCylinder, OpenXRCompositionLayerEquirect, OpenXRCompositionLayerQuad, OpenXRDpadBindingModifier, OpenXRExtensionWrapper, OpenXRExtensionWrapperExtension, OpenXRFrameSynthesisExtension, OpenXRFutureExtension, OpenXRFutureResult, OpenXRHand, OpenXRHapticBase, OpenXRHapticVibration, OpenXRInteractionProfile, OpenXRInteractionProfileEditor) — cite: upstream/godot/modules/openxr/config.py can_build(): returns True only for ('linuxbsd','windows','android','macos'), else False with comment '# Not supported on these platforms.' — OpenXR is not compiled for the web/wasm export. ALSO prd/THIRDPARTY_STRATEGY.md:81 '| `thirdparty/openxr` | XR | Defer; not in v1 scope | — |'. Godot's in-scope browser XR path is WebXR (WebXRInterface), already implemented in gdplatform/src/xr.rs:47.
- **OpenXR native XR class surface (interfaces, interaction-profile bindings, render models, spatial entities/anchors/plane+marker tracking)** (OpenXRInteractionProfileEditorBase, OpenXRInteractionProfileMetadata, OpenXRInterface, OpenXRIPBinding, OpenXRIPBindingModifier, OpenXRMarkerTracker, OpenXRPlaneTracker, OpenXRRenderModel, OpenXRRenderModelExtension, OpenXRRenderModelManager, OpenXRSpatialAnchorCapability, OpenXRSpatialCapabilityConfigurationAnchor, OpenXRSpatialCapabilityConfigurationAprilTag, OpenXRSpatialCapabilityConfigurationAruco, OpenXRSpatialCapabilityConfigurationBaseHeader, OpenXRSpatialCapabilityConfigurationMicroQrCode, OpenXRSpatialCapabilityConfigurationPlaneTracking, OpenXRSpatialCapabilityConfigurationQrCode, OpenXRSpatialComponentAnchorList, OpenXRSpatialComponentBounded2DList, OpenXRSpatialComponentBounded3DList, OpenXRSpatialComponentData, OpenXRSpatialComponentMarkerList, OpenXRSpatialComponentMesh2DList, OpenXRSpatialComponentMesh3DList, OpenXRSpatialComponentParentList, OpenXRSpatialComponentPersistenceList, OpenXRSpatialComponentPlaneAlignmentList, OpenXRSpatialComponentPlaneSemanticLabelList, OpenXRSpatialComponentPolygon2DList, OpenXRSpatialContextPersistenceConfig, OpenXRSpatialEntityExtension, OpenXRSpatialEntityTracker, OpenXRSpatialMarkerTrackingCapability, OpenXRSpatialPlaneTrackingCapability, OpenXRSpatialQueryResultData, OpenXRStructureBase, OpenXRVisibilityMask) — cite: .claude/skills/port-audit/references/taxonomy.md xr-peripheral section, scope anchor: "OpenXR* (~40 classes) + MobileVRInterface: native-only -> OUT_OF_SCOPE with the web-limitations citation, but registration-with-correct-error surface is a P3 call." Reinforced by the same section's WebXR stone ("Godot's web export SUPPORTS WebXR ... it is IN scope for a browser port") which establishes WebXR — not OpenXR — as the in-scope XR path. Overall scope anchor (lines 12-19): runtime features Godot's own web export cannot do are OUT_OF_SCOPE with citation.
- **Native raw-socket StreamPeer/Server family** (StreamPeerSocket, StreamPeerUDS, StreamPeerTLS, StreamPeerExtension, SocketServer) — cite: .claude/skills/port-audit/references/taxonomy.md:14-19 scope anchor names 'raw UDP/TCP sockets' among features 'Godot's own web export cannot do' and thus OUT_OF_SCOPE with a citation; corroborated by StreamPeerUDS.xml (UNIX-only) and the multiplayer stone (taxonomy.md:414-416) 'TCPServer/UDPServer — native-scope; on web cite OUT_OF_SCOPE ... but the CLASS surface should exist with correct errors'.
- **StatusIndicator (native tray icon)** (StatusIndicator) — cite: .claude/skills/port-audit/references/taxonomy.md:14-19 scope anchor: features 'Godot's own web export cannot do ... native file dialogs' are OUT_OF_SCOPE with a citation; StatusIndicator.xml explicitly limits itself to macOS/Windows (no web/tray in the browser).
- **Native listener sockets + UPnP (TCPServer, UDPServer, UDSServer, UPNP, UPNPDevice)** (TCPServer, UDPServer, UDSServer, UPNP, UPNPDevice) — cite: .claude/skills/port-audit/references/taxonomy.md scope anchor (lines 13-15): 'Runtime features that Godot's own web export cannot do (raw UDP/TCP sockets, ENet, ...) may be classified OUT_OF_SCOPE with a citation'; and multiplayer section: 'TCPServer/UDPServer — native-scope; on web cite OUT_OF_SCOPE per web limitations, but the CLASS surface should exist with correct errors.' Recommend human ratification; a P3 registration-with-correct-errors stub is the only in-scope remnant.

## Dropped / degraded

- Subsystem finders: **none dropped** (all 24 completed + verified).
- Sweep: **11 of 15 chunks triaged**; 4 chunks (indices 5, 8, 13, 14) hit the StructuredOutput retry cap (5) — NOT a spend error. Untriaged class families: IK/JavaClass/Lightmap/MainLoop/MultiMesh/NativeMenu (chunk 5); Physics*Server/State/Query* + Parallax + PacketPeer + PackedScene (chunk 8); VisualShaderNode Transform/UInt/Vec* + WebRTC/WebSocket + Window/World2D/3D + XMLParser (chunk 13); XR* trackers/interfaces (chunk 14). **All four families were behaviorally covered by their subsystem finders** (physics-2d/3d, nodes-2d, multiplayer, shaders, web-runtime, xr-peripheral), so the gap is registration-sweep backstop only. A scoped re-sweep (`/port-audit` with those chunks split smaller) can close it.
- Oracle: full (live Godot 4.6.1 binary dump; no fallback).
- Interruptions: run hit the monthly spend cap twice mid-flight (Fable finders, then a resume); recovered by resuming the same run id — cached finders replayed free, verify+sweep+critic ran on Opus.

## Critic notes

Completeness critic flagged these blind spots; each was filled by an Opus supplemental finder (findings folded into the beads above):
- **registered-class-property-default-fidelity** — The 12 sweeps only verified class-NAME presence in ClassDB; no finder ran a systematic per-class property-table + default_value diff, and this is Patina's documented failure mode (MEMORY: registered_classes_may_be_wrong_not_just_absent, enum_zero_is_not_the_node_default). Confessed as spot-check-only by core-object, nodes-2d ('spot-checked Sprite2D only'), nodes-3d, shaders. Evidence: engine-rs/crates/gdobject/src/class_db.rs has 480 ClassRegistration::new but only 755 .property() calls total (~1.5/class); Sprite2D/AnimatedSprite2D/Camera2D register ZERO properties (class_db.rs:2899-2905), so .new() yields objects with no texture/centered/hframes/offset/flip_h etc. and reflection returns absent. Defaults are hand-maintained in TWO independent tables that can diverge from Godot: class_db.rs PropertyInfo.default_value (runtime .new() seeding, class_db.rs:572) and gdeditor/src/inspector_schema_generated.rs (1344 lines, editor inspector). Wrong/missing defaults silently corrupt both runtime construction and the inspector.
- **project-settings-defaults-sweep** — No finder owned ProjectSettings as a modality; scene-tree/platform-os/physics/audio each touched a few isolated keys but nobody swept the default table. Godot registers 120+ core GLOBAL_DEF/GLOBAL_DEF_RST defaults (project_settings.cpp) exposed as 932 ProjectSettings.xml members driving physics/common/physics_ticks_per_second, physics/2d|3d/default_gravity, rendering, audio mix rate, display/window sizing, and the default ui_* InputMap. Patina's engine-rs/crates/gdcore/src/project_settings.rs (806 lines) is a bare key/value bag that knows only ~23 keys — several of which are test fixtures (custom/foo, section/key, nonexistent/key) — and seeds none of Godot's documented defaults, so GLOBAL_GET of an omitted key returns absent instead of the spec default. Sub-example: input_map.cpp defines 72 default ui_* actions; gdplatform/src/input.rs:502-539 seeds only ~10 (ui_accept/cancel/focus_next/left/up...), so ui_text_* and dialog/filedialog navigation are absent.
- **globalscope-enum-constant-value-fidelity** — The gdscript finder verified singleton/utility-fn presence but EXPLICITLY deferred the 482 @GlobalScope constant/enum VALUE diff 'to the sweep', flagging Error (~17/49) and PropertyHint (~11/40) as thin; the 12 sweeps only diffed class registrations, so nobody ran the enum-value check — a modality that fell through the cracks. Evidence: engine-rs/crates/gdscript-interop/src/global_constants.rs (665 lines) references only ~9 ERR_ of Godot's 49 Error codes and ~11 of 45 PropertyHint values. Absent or mis-valued constants silently mis-evaluate scripts: `if load(x) != OK`, `if err == ERR_FILE_NOT_FOUND`, `event.keycode == KEY_ESCAPE` (Key has 193 values), PROPERTY_USAGE_* flag math, JoyButton/MouseButton comparisons.
- **class-inheritance-parent-chain-fidelity** — The sweeps checked whether a class name is registered, never whether its parent chain matches Godot; wrong parents were only caught incidentally. Confirmed live defects: engine-rs/crates/gdobject/src/class_db.rs:3292-3298 registers AudioEffectAmplify/Reverb/EQ with .parent("Resource") but Godot inherits AudioEffect (jq confirms AudioEffectReverb.inherits=="AudioEffect"), and the AudioEffect base is unregistered (grep count 0); sweep-0/sweep-12 also note AnimationPlayer/Tree skip the AnimationMixer base and VisibleOnScreenNotifier3D parents Node3D vs VisualInstance3D. A mechanical .parent() vs .inherits diff across all 480 registrations was never run. Wrong parents break is_class()/is_instance_of, get_class_list inheritance queries, virtual/notification dispatch up the chain, and inherited-property resolution.
- **variant-builtin-type-method-semantics** — core-variant and gdscript both did NAME-presence diffs of the ~500 builtin-type methods (String/Array/Dictionary/Packed*/Vector*/etc.) via jq+comm but explicitly skipped BEHAVIORAL parity: core-variant confesses 'behavioral spot-checks of the 66 present String methods (presence-verified only — rsplit maxsplit etc. unaudited)', the full % format-specifier matrix, hash() value parity, and member-WRITE paths (v.x=1, position.x+=). These methods execute in nearly every gameplay line; a present-but-wrong method (String.format/%, Array.sort_custom ordering, Dictionary.merge overwrite flag, PackedByteArray decode endianness) passes the registration sweep yet breaks any imported Godot project. Tests passing on the module is not evidence of Godot-trace parity.
- **utility-function-argument-and-return-semantics** — gdscript verified PRESENCE of ~99/114 @GlobalScope utility functions but explicitly not 'argument-order/edge-case semantics of the ~99 present utility functions'. No finder behaviorally validated the numeric/serialization helpers that projects rely on: snapped, remap/range_lerp, ease, wrapf/wrapi, pingpong, lerp_angle, is_equal_approx tolerance, str_to_var/var_to_str round-trip, type_convert, bytes_to_var/var_to_bytes. A present-but-wrong utility function (e.g. off-by-one wrapi, wrong ease curve, non-round-tripping var_to_str) silently corrupts gameplay math and save/load without failing the presence sweep.

## Already tracked (33 skipped, deduped by distinctive query)

- P0 [core-object] classdb-runtime-wiring → pat-p0s82,pat-nzgdu.196
- P0 [core-variant] container-reference-semantics → pat-nzgdu.196
- P0 [gdscript] gdscript-onready-access → pat-u3l8c,pat-nzgdu.113
- P0 [multiplayer] mp-rpc-script-wiring → pat-nzgdu.113
- P0 [core-variant] variant-equality → pat-nzgdu.78
- P0 [core-variant] variant-missing-types → pat-nzgdu.78
- P0 [web-runtime] web-wasm-runtime → pat-u3l8c.111,pat-u3l8c,pat-nzgdu.113
- P1 [animation] animation-resource-tracks → pat-u3l8c.111,pat-u3l8c
- P1 [resources-io] file-access-parity → pat-nzgdu.149
- P1 [gdscript] gdscript-callable-semantics → pat-u3l8c,pat-nzgdu.113
- P1 [gdscript] gdscript-exports → pat-u3l8c,pat-nzgdu.113
- P1 [gdscript] gdscript-inheritance → pat-u3l8c,pat-nzgdu.113
- P1 [gdscript] gdscript-lexer → pat-nzgdu.78
- P1 [variant-builtin-type-method-semantics] math-int-4d-vectors → pat-nzgdu.78
- P1 [variant-builtin-type-method-semantics] math-vector-methods → pat-nzgdu.78
- P1 [nodes-3d] mesh-instance-3d → pat-nzgdu.76
- P1 [multiplayer] net-http → pat-u3l8c.78
- P1 [core-variant] operator-matrix → pat-nzgdu.78
- P1 [platform-os] platform-os-jsbridge → pat-u3l8c.115
- P1 [rendering-3d] r3d-viewport → pat-nzgdu.76
- P1 [resources-io] resource-loader-singleton → pat-nzgdu.149
- P1 [core-variant] string-api → pat-nzgdu.78
- P1 [variant-builtin-type-method-semantics] string-sprintf → pat-u3l8c.113,pat-nzgdu.87,pat-nzgdu.78,pat-u3l8c.114
- P1 [core-variant] tscn-dict-keys → pat-nzgdu.196
- P2 [variant-builtin-type-method-semantics] color-methods → pat-p0s82,pat-nzgdu.196
- P2 [gdscript] gdscript-statics → pat-u3l8c,pat-nzgdu.113
- P2 [gdscript] gdscript-tool-mode → pat-u3l8c,pat-nzgdu.113
- P2 [gdscript] gdscript-typing → pat-u3l8c,pat-nzgdu.113
- P2 [gui-controls] gui-textdisplay → pat-nzgdu.196
- P2 [multiplayer] net-webrtc → pat-nzgdu.113
- P2 [variant-builtin-type-method-semantics] string-split-params → pat-u3l8c.113,pat-nzgdu.87,pat-nzgdu.78,pat-u3l8c.114
- P2 [core-variant] typed-containers → pat-nzgdu.196
- P3 [variant-builtin-type-method-semantics] string-format-method → pat-u3l8c.113,pat-nzgdu.87,pat-nzgdu.78,pat-u3l8c.114
