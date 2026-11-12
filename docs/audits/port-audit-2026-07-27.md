# Port Audit — 2026-07-27 (oracle: Godot 4.6.1-stable)

Epic: **pat-s3rrv** · Findings: **417 verified** (3 refuted, 12 unverified) + 111 sweep-triaged = 543 raw · Beads: **263 created**, 0 already tracked

Run: 2 passes over one workflow (`wf_e7f38751-0b9`). Pass 1 hit the session limit during the supplemental round; pass 2 resumed from cache and completed 104/104 agents with 0 errors. Synthesis completed 2026-07-28.

Oracle built fresh this run: `--dump-extension-api` from the installed Godot 4.6.1-stable (1023 classes) diffed against 638 `ClassRegistration::new()` literals across all crates (627 real Godot classes) → 396 unregistered names swept in 8 chunks.

## Per-subsystem

`*` = supplemental pass added by the completeness critic.

| Subsystem | Parity-OK | MISSING | STUB | UNWIRED | PARTIAL | P0s | Coverage confession |
|---|--:|--:|--:|--:|--:|--:|---|
| core-object | 16 | 2 | 0 | 3 | 6 | 3 | Covered in depth: the Object script surface (all 48 api methods checked by name across every crate), the ClassDB singleton dispatch table (all 30 api methods), notification constant VALUES v |
| core-variant | 15 | 3 | 0 | 1 | 9 | 3 | Covered: the 38 builtin_classes inventory from extension_api.json diffed method-by-method against Patina's actual GDScript-reachable dispatch (engine-rs/crates/gdscript-interop/src/interpret |
| gdscript | 19 | 6 | 0 | 2 | 6 | 2 | Covered the taxonomy's gdscript stones by reading upstream/godot/modules/gdscript (tokenizer, parser annotation registration, utility function table, parse_get_node) and the extension_api du |
| scene-tree | 21 | 1 | 2 | 3 | 6 | 4 | Covered in depth: the taxonomy's scene-tree stones for lifecycle ordering + notifications, process/physics priorities and process modes, pause propagation, groups and call_group flags, node  |
| nodes-2d | 26 | 3 | 1 | 5 | 5 | 2 | Covered end-to-end: Node2D/CanvasItem transform + visibility + ordering, Sprite2D, AnimatedSprite2D/SpriteFrames, Camera2D, Light2D/LightOccluder2D/CanvasModulate, Line2D, Polygon2D, Path2D/ |
| rendering-2d | 16 | 2 | 2 | 3 | 6 | 2 | Covered: the CanvasItem draw_* script surface (all 32 Godot methods diffed against gdscene/src/scripting.rs), the DrawCommand model, both live 2D render paths (gdeditor viewport_2d_render fo |
| nodes-3d | 18 | 4 | 1 | 4 | 5 | 2 | Covered end-to-end: Node3D transform/visibility/top_level semantics, the script-reachability of the Node3D+Camera3D API, the actual .tscn -> render-list -> pixels path used by the shipped ed |
| rendering-3d | 13 | 7 | 0 | 7 | 3 | 4 | Covered in depth: the StandardMaterial3D/BaseMaterial3D property surface (diffed all ~135 api-json properties against gdserver3d/src/material.rs and the .tscn parser), the Environment surfac |
| shaders | 16 | 2 | 1 | 5 | 7 | 2 | Covered: the full Patina shader stack read end to end — gdserver2d/src/shader.rs (2214 lines), gdserver3d/src/shader.rs (2823), gdresource/src/{shader_tokenizer,shader_ast,shader_preprocesso |
| vfx-particles | 13 | 2 | 1 | 4 | 3 | 2 | Covered fully: the 2D/3D CPU simulators (gdscene/src/particle.rs, particle3d.rs), the frame-loop wiring (gdscene/src/main_loop.rs process_particles + its single call site at :2990), all thre |
| animation | 24 | 3 | 2 | 4 | 5 | 1 | Covered from ground truth (extension_api.json class/method/property/enum slices + upstream/godot C++ and doc XML): the Animation resource track/interpolation/loop/update-mode surface, Animat |
| physics-2d | 23 | 5 | 1 | 5 | 5 | 2 | Covered end-to-end: the shape set and narrowphase pair matrix; the world/solver step and its wiring into MainLoop; RigidBody2D's property surface vs extension_api.json (all 23); StaticBody2D |
| physics-3d | 22 | 3 | 0 | 3 | 7 | 0 | Covered end-to-end: the gdphysics3d crate (world/body/collision/shape/query/area3d/joint/character/softbody/obb_sat, 11.4k lines read or grepped in full), the gdscene 3D bridge (physics_serv |
| navigation | 16 | 3 | 0 | 4 | 1 | 2 | Covered end-to-end: the taxonomy's navigation stones for agents (2D+3D), obstacles/avoidance, links, regions, tile navigation, path smoothing, the A* family, baking, path-query objects, edit |
| audio | 21 | 3 | 1 | 7 | 4 | 2 | Covered: the full gdaudio crate (server/mixer/bus/effects/stream/spatial/decode/wav/import/output/web_output/player/sample_pool/ima_adpcm), the gdscene AudioServerBridge and its MainLoop cal |
| multiplayer | 15 | 4 | 1 | 5 | 5 | 3 | Covered end-to-end: the RPC send/receive path (network.rs + scene_tree.rs + scripting.rs + interpreter.rs), @rpc annotation config, path-id cache, MultiplayerAPI/SceneMultiplayer surface and |
| gui-controls | 23 | 4 | 2 | 6 | 5 | 2 | Covered: the full Control core surface (layout, anchors, size flags, focus, mouse filters, hit test, cursor, theme overrides), every container class in the api dump, the BaseButton family, t |
| theming-text | 18 | 3 | 0 | 3 | 4 | 2 | Covered: the full Theme/ThemeDB data model and its (missing) wiring into the only production Control draw path; StyleBox family field-by-field against scene/resources/style_box*.cpp; Godot's |
| input | 19 | 5 | 2 | 5 | 2 | 2 | Covered the taxonomy's input floor end-to-end from ground truth: the 17 InputEvent classes + Input/InputMap/Shortcut method+property+signal surface sliced out of extension_api.json, the Key/ |
| resources-io | 31 | 5 | 0 | 1 | 6 | 3 | Covered with direct source evidence: text .tscn/.tres value grammar and the UNPARSED_BUILTINS gap; binary .res/.scn container v6 (header, tables, variant decode) and the missing `_bundled` S |
| platform-os | 14 | 3 | 1 | 3 | 3 | 1 | Covered in depth: the OS / Time / Engine / DisplayServer / EngineDebugger / JavaScriptBridge / Performance / ProjectSettings singleton dispatch tables in gdscript-interop's interpreter (diff |
| editor-ux | 28 | 3 | 5 | 6 | 8 | 1 | Method: I enumerated Godot 4.6.1's editor feature surface from upstream/godot/editor/ (113 *_plugin.cpp files, docks/, settings/, import/, run/, shader/, animation/, audio/, debugger/) and t |
| web-runtime | 22 | 7 | 2 | 2 | 3 | 4 | HEADLINE: Patina has no web runtime. `patina-web` is the sole wasm32 root and links only gdcore/gdvariant/gdaudio/gdrender3d (crates/patina-web/Cargo.toml:16-51) — no SceneTree, no .tscn loa |
| xr-peripheral | 15 | 5 | 1 | 4 | 2 | 1 | Covered in full: the taxonomy's four xr-peripheral bullets. I sliced extension_api.json for every XR*/OpenXR*/WebXR*/MobileVR*/Camera*/MovieWriter class (method, property, signal and enum li |
| class-scoped-enums-and-constants* | 8 | 1 | 0 | 2 | 4 | 2 | CONFIRMED MECHANISM + SIZED IT. I traced `Ident.MEMBER` end to end (interpreter.rs:3222 -> try_static_member:14111 -> the `_` arm at :14157 -> variant_math.rs:427 + global_constants.rs:1170/ |
| registered-class-member-depth* | 9 | 0 | 0 | 0 | 17 | 0 | METHOD: parsed all 680 `ClassRegistration::new("X")` literal blocks in engine-rs/crates/gdobject/src/class_db.rs + editor_plugin_classes.rs (638 distinct names, matching the audit's patina_r |
| project-settings-key-surface* | 16 | 6 | 0 | 6 | 4 | 6 | Covered: the full 932-member Godot surface extracted from upstream/godot/doc/classes/ProjectSettings.xml and bucketed by family/type; a whole-workspace scan for setting-shaped string literal |
| engine-virtual-method-table* | 12 | 14 | 0 | 7 | 5 | 1 | METHOD. Ground truth built from `jq '[.classes[]/. as $c/(.methods//[])[]/select(.is_virtual)/{class:$c.name,method:.name}]' extension_api.json` = 1413 (class, virtual) pairs / 106 classes / |
| asset-import-pipeline* | 15 | 7 | 2 | 4 | 2 | 4 | Covered: the `.import` sidecar model (parse/resolve/absence of a writer), the `.godot/imported/` artifact layer (`.ctex` formats, missing layered/VRAM/Basis paths), the ResourceFormatLoader  |
| renderingserver-renderingdevice-script-api* | 12 | 7 | 1 | 0 | 2 | 1 | WHAT I COVERED: full method-by-method diff of RenderingServer (526 methods, 83 enums/482 enum values, 18 plain constants, 2 signals, 1 property) and RenderingDevice (118 methods, 36 enums/54 |
| singleton-dispatch-table* | 21 | 5 | 1 | 2 | 2 | 1 | COVERED: all 39 entries of `jq -r '.singletons[].name' extension_api.json` were individually grepped against engine-rs/crates/gdscript-interop/src/interpreter.rs, engine-rs/crates/gdobject/s |
| sweep (8 chunks) | 55 | 52 | 2 | 25 | 27 | 1 | 396 unregistered Godot classes triaged; self-verifying (no separate verify stage) |

## P0 list

- **pat-s3rrv.1** [animation] Animation: expose AnimationNodeStateMachinePlayback so scripts can travel/start/stop a state machine
- **pat-s3rrv.2** [asset-import-pipeline] Import dock: persist import options to .import sidecars and run a real reimport
- **pat-s3rrv.3** [asset-import-pipeline] Scene import: real glTF→PackedScene importer with node suffixes, materials and collision
- **pat-s3rrv.4** [asset-import-pipeline] GDScript load(): route preload/load through the format loaders and .import remaps
- **pat-s3rrv.5** [audio] Audio: resolve res:// stream paths against the project root in the runner boot
- **pat-s3rrv.6** [audio] Audio: expose the AudioServer singleton to GDScript (buses, effects, peaks, speed scale)
- **pat-s3rrv.7** [core-object] Core: populate the full ClassDB at runner boot (and expose one register_all_classes entry point)
- **pat-s3rrv.8** [core-variant] Variant: make Array and Dictionary reference types (shared storage) like Godot
- **pat-s3rrv.9** [core-variant] Variant: complete the Vector2/Vector3/Vector2i/Vector3i method + constant surface in GDScript
- **pat-s3rrv.10** [editor-ux] Editor3D: wire per-node gizmo handles end-to-end (shape/light/camera) through the served viewport
- **pat-s3rrv.11** [gdscript] GDScript: resolve script-declared named enums at runtime (Enum.MEMBER, enum as type, cross-instance access)
- **pat-s3rrv.12** [gui-controls] GUI: implement BaseButton toggle/action-mode/button-mask/group semantics and press mouse capture
- **pat-s3rrv.13** [gui-controls] GUI: Godot-accurate Control hit-testing (visibility, clip_contents, top_level, per-class mouse_filter defaults, local event coords)
- **pat-s3rrv.14** [input] Input: real InputEvent object family delivered to _input/_gui_input/_unhandled_input
- **pat-s3rrv.15** [input] Web input: forward real browser key modifiers, physical code, echo and unicode into the running game
- **pat-s3rrv.16** [multiplayer] Multiplayer: make MultiplayerPeer script-constructible and assignable via multiplayer.multiplayer_peer
- **pat-s3rrv.17** [multiplayer] Multiplayer: drive MultiplayerSpawner/MultiplayerSynchronizer as real scene nodes from the SceneTree tick
- **pat-s3rrv.18** [multiplayer] SceneMultiplayer: complete the script surface (auth callback, relay, root_path, packet caps) and bind multiplayer.<signal>.connect
- **pat-s3rrv.19** [navigation] Navigation: bind AStar2D/AStar3D/AStarGrid2D to GDScript and complete their method surface
- **pat-s3rrv.20** [navigation] Navigation: NavigationAgent2D/3D method surface + stop moving the agent node
- **pat-s3rrv.21** [nodes-2d] Light2D: load occluders from the OccluderPolygon2D sub-resource and light from global transforms
- **pat-s3rrv.22** [nodes-2d] Canvas2D: collapse the dead gdscene::scene_bridge onto the shipped viewport_2d_render path
- **pat-s3rrv.23** [nodes-3d] Nodes3D: render real ArrayMesh/glTF meshes, skinning and blend shapes in the editor/Play 3D path
- **pat-s3rrv.24** [nodes-3d] Nodes3D: dispatch the Node3D/Camera3D method + global-transform property API to GDScript
- **pat-s3rrv.25** [physics-2d] Physics2D: register/unregister bodies and areas as nodes enter and exit the tree
- **pat-s3rrv.26** [physics-2d] Physics2D: expose PhysicsDirectSpaceState2D queries to GDScript via get_world_2d().direct_space_state
- **pat-s3rrv.27** [platform-os] Debugger: attach EngineDebugger to the editor Play session and drain its messages/breaks over a WebSocket transport
- **pat-s3rrv.28** [rendering-2d] Rendering2D: carry material, blend mode and texture filter/repeat into the Play rasterizer
- **pat-s3rrv.29** [rendering-3d] Rendering3D: implement GeometryInstance3D settings, decals, fog volumes and occluders in the wgpu RenderingServer3D impl
- **pat-s3rrv.30** [rendering-3d] Materials3D: wire transparency, blend, cull and depth-draw modes from .tscn into both render pipelines
- **pat-s3rrv.31** [rendering-3d] Rendering3D: resolve glTF and ArrayMesh mesh resources into the editor viewport and Play frame
- **pat-s3rrv.32** [rendering-3d] Rendering3D: drive a per-frame 3D scene render from patina-runner and the patina-web canvas
- **pat-s3rrv.33** [renderingserver-renderingdevice-script-api] Rendering: make global_shader_parameter_* speak the GlobalShaderParameterType enum
- **pat-s3rrv.34** [resources-io] Resources-IO: route every res:// read through the mounted-pack VFS and boot the runner from a .pck
- **pat-s3rrv.35** [scene-tree] SceneTree: advance SceneTreeTimers once per frame and honor create_timer's process flags
- **pat-s3rrv.36** [shaders] Shaders: route both render paths through the real GDShader AST evaluator
- **pat-s3rrv.37** [theming-text] Text: measure Control text with the real font (route layout through TextServerLite, drop the 5x7 monospace model)
- **pat-s3rrv.38** [theming-text] Theming: route Control draw through the real Theme chain (node theme, ancestors, project theme, ThemeDB)
- **pat-s3rrv.39** [vfx-particles] Particles: apply the ParticleProcessMaterial sub-resource onto GPU/CPU emitters
- **pat-s3rrv.40** [web-runtime] Web: drive MainLoop from requestAnimationFrame through FrameDriver in the wasm entry
- **pat-s3rrv.41** [web-runtime] Web: bind browser pointer/keyboard/wheel/touch/gamepad events into the engine InputState
- **pat-s3rrv.42** [xr-peripheral] GDScript: resolve ClassDB integer/enum constants through Class.CONSTANT member access

## Proposed OUT_OF_SCOPE (needs human ratification — no beads created)

- **[platform-os]** GDExtension native library loading (dlopen of .gdextension libraries)
  - citation: THIRDPARTY_STRATEGY.md:237-265 — "Patina recognises `.gdextension` manifests and executes nothing from them … No library is `dlopen`'d … Reopen when loading real native plugins enters milestone scope."
- **[renderingserver-renderingdevice-script-api]** RenderingDevice device implementation (118 methods: compute pipelines, SPIR-V compilation, buffers/textures/framebuffers/draw lists) and the RD* family's runtime effect
  - citation: port-audit taxonomy Scope anchor ("Runtime features that Godot's own web export cannot do ... may be classified OUT_OF_SCOPE with a citation (Godot web-limitations doc)") + upstream/godot/platform/web/detect.py:81 (`"vulkan": False`) and :250 (opengl3-only) + upstream/godot/doc/classes/RenderingDevice.xml ("not available ... when using the Compatibility rendering method") + upstream/godot/doc/classes/RenderingServer.xml get_rendering_device ("always returns null")
- **[singleton-dispatch-table]** JavaClassWrapper singleton (2 methods: wrap, get_exception)
  - citation: Port-audit scope anchor: 'Runtime features that Godot's own web export cannot do ... may be classified OUT_OF_SCOPE.' Ground truth for the exclusion is upstream/godot/platform/android/java_class_wrapper.cpp (Android platform layer) plus its absence from core/register_core_types.cpp:357-382 and servers/register_server_types.cpp — JavaClassWrapper is not part of Godot 4.6.1's web singleton set, so it is not part of the 1:1 bar for a browser-native port.
- **[sweep-1]** ENetConnection / ENetPacketPeer
  - citation: .claude/skills/port-audit/references/taxonomy.md:14-16 — "Runtime features that Godot's own web export cannot do (raw UDP/TCP sockets, ENet, native OpenXR runtimes, subprocess spawning, native file dialogs) may be classified OUT_OF_SCOPE with a citation"; THIRDPARTY_STRATEGY.md:163 — "| enet | Reliable UDP | MIT | Replace with enet-rs or defer |".
- **[sweep-3]** OpenXR class family (38 assigned of Godot's 63 OpenXR* classes)
  - citation: upstream/godot/modules/openxr/config.py:1-6 (`can_build` returns False for every platform outside linuxbsd/windows/android/macos — "Not supported on these platforms"), so all 63 OpenXR* classes are compiled out of Godot 4.6.1's own Web export; plus .claude/skills/port-audit/references/taxonomy.md:13-16 (native OpenXR runtimes are an explicit OUT_OF_SCOPE category with citation) and taxonomy.md:759-760 (xr-peripheral stone: "OpenXR* (~40 classes) + MobileVRInterface: native-only → OUT_OF_SCOPE with the web-limitations citation").
- **[sweep-4]** OpenXR spatial-entity / marker / plane-tracking class family (25 classes)
  - citation: upstream/godot/modules/openxr/config.py:1-7 — `can_build()` returns True only for linuxbsd/windows/android/macos and False for every other platform, so Godot's own web export ships none of these classes (corroborated by `ls upstream/godot/doc/classes | grep -c OpenXR` = 0; they live only in modules/openxr/doc_classes/). The taxonomy scope anchor, /Users/bone/dev/games/patina/.claude/skills/port-audit/references/taxonomy.md:10-19, puts runtime features Godot's web export cannot do — explicitly naming "native OpenXR runtimes" — out of scope with a citation, and :759-760 repeats the call for the OpenXR* family specifically. WebXR (modules/webxr/config.py, buildable wherever opengl3 is enabled) remains the in-scope browser XR path and is NOT covered by this exclusion.
- **[sweep-4]** GDExtension virtual base classes for custom physics servers and packet peers
  - citation: /Users/bone/dev/games/patina/THIRDPARTY_STRATEGY.md:237-256 — "GDExtension native plugins (pat-0q2kx.252). Classification: Deferred (bucket 2 — wrap via FFI when scheduled). Metadata recognition ships now; execution does not... No library is `dlopen`'d and no native entry point is called." The same section (:265-270) records that the wasm target "has no `dlopen` at all" and that reopening requires a dynamic-loader path plus the GDExtension C ABI. Since every method on these seven classes is `virtual required` and can only be satisfied by a loaded native subclass, they are unreachable for as long as that classification stands. Reopen this finding together with the GDExtension execution bead; a registration-only surface (so `ClassDB.class_exists` and class docs answer correctly) would be a P3 follow-up at most.
- **[sweep-6]** UPNP / UPNPDevice port forwarding
  - citation: upstream/godot/platform/web/net_socket_web.h:50-73 — Godot's own web platform stubs every NetSocket operation (`sendto`, `recvfrom`, `join_multicast_group`, `bind`, `listen`, `connect_to_host`) to return ERR_UNAVAILABLE, so the SSDP UDP-multicast discovery that `upstream/godot/modules/upnp/upnp.cpp` (miniupnpc) performs cannot run in Godot's web export either. Permitted by the port-audit taxonomy scope anchor, .claude/skills/port-audit/references/taxonomy.md:13-16 ('raw UDP/TCP sockets ... may be classified OUT_OF_SCOPE with a citation').

Additionally, these findings were classified as gaps but their finder drafted no bead (no acceptance criteria). Per the skill's hard rule they were **not** beaded rather than given fabricated criteria; all are the same web-export scoping question as above:

- [multiplayer] P2 ENetMultiplayerPeer / ENetConnection / ENetPacketPeer native reliable-UDP transport in the browser
- [multiplayer] P3 UPNP / UPNPDevice automatic port forwarding
- [xr-peripheral] P3 CameraServer real device enumeration / getUserMedia camera driver
- [xr-peripheral] P3 MobileVRInterface (sensor-driven cardboard-style stereo)
- [xr-peripheral] P3 OpenXR* class family (68 classes: OpenXRInterface, OpenXRActionMap, OpenXRCompositionLayer*, OpenXRSpatial*, OpenXRHand, OpenXRRenderModel*, …)

## Dropped / degraded

- **Dropped subsystems: none.** All 24 primary subsystems completed with full adversarial verification in pass 1.
- Pass 1 lost the `registered-class-member-*` supplemental finder and 26 verifiers to the session limit. Pass 2 re-ran them; the member sweep returned 18 findings, all verified. The hole is closed.
- Pass 2's critic chose different supplemental keys than pass 1. Four of pass 1's five areas have pass-2 equivalents (verified). `singleton-dispatch-table` had none, so its 11 pass-1 findings were **carried forward and marked unverified** rather than dropped.
- Oracle: full fidelity, no fallback. `Godot Engine v4.6.1.stable.official`.
- Unverified findings surviving into beads: 2 (capped at P1, flagged in the bead body).

## Caveats on confidence

- Refutation rate is low: 3 REFUTED vs 349 CONFIRMED across 420 verified findings. The 68 ADJUSTED verdicts show verifiers did push back on status/priority, but a single CONFIRMED vote should be read as *probable*, not proven.
- Sweep findings (103) were self-triaged by design — no independent verifier saw them.

## Critic notes

Verification method: I re-derived every number from ground truth rather than from finder claims. Sweep-coverage check: unregistered.txt has 396 names, sweep_chunks.json has 8 chunks totaling 396 — proving the sweep covered ONLY unregistered classes and never touched the 638 registered ones (this is the root of missed_area #2 and why #5's GLTF family was invisible). ClassDB shell count came from parsing `ClassRegistration::new("X")` blocks in class_db.rs and testing each for .property()/.method()/.signal() presence: 675 literal blocks, 216 name-only. Enum/constant reachability was traced from the call site (interpreter.rs:3224) through try_static_member (:14111) to variant_math::static_member (:427) — no ClassDB fallback exists on that path. ProjectSettings counts: 932 `<member>` in doc/classes/ProjectSettings.xml vs 76 distinct keys in gdcore/src/project_settings.rs. Virtual-method probes were per-name rg with tests and gdeditor/src/inspector_schema_generated.rs excluded (that generated file is not registration and not implementation — three sweep agents independently flagged it as a false-positive trap and I honored that). Read-only throughout: Read/rg/ls/python-json only; no cargo, no writes, no br, no COMPAT_MATRIX/docs-audits/prd-progress/.beads.

Candidates I evaluated and deliberately REJECTED rather than pad the list: (a) numeric precision f32-vs-double, confessed by core-variant — Patina's Variant::Float is already f64 (gdvariant/src/variant.rs:218) and Godot's default build uses real_t=float for Vector2/3, so the premise is largely wrong; (b) singleton coverage — genuinely covered, sweep-1 and sweep-4 both quantified it (18 of 39 script-reachable, 8 of 39 in ClassDB); (c) utility functions and global enum constants — gdscript machine-diffed all 114 and all 512; (d) default-theme item VALUES — theming-text did the full name census and the value diff is explicitly /editor-parity's job, so it is a deferred-with-owner, not an orphan; (e) notification codes and lifecycle ordering — core-object diffed values against extension_api and scene-tree traced the ordering.

Supplemental areas the critic identified as missed by the main sweep:

- **class-scoped-enums-and-constants** — No finder ran the class-enum modality end to end. core-object confessed it explicitly ("per-class ENUM coverage vs .classes[].enums — individual enum gaps are unquantified"); xr-peripheral spotted the MECHANISM but said it "only spot-checked XR names" and asked someone to size the blast radius; nobody did. I verified the mechanism and the size, and it is a P0-class runtime break, not a long tail. 
- **registered-class-member-depth** — STRUCTURAL BLIND SPOT IN THE AUDIT'S OWN INSTRUMENT. The 8 sweep chunks covered exactly the 396 names in unregistered.txt (8 x ~50 = 396). The 638 names in patina_registered.txt were NEVER individually examined by anyone — registration was treated as the pass condition. But registration is only rung (a): parsing engine-rs/crates/gdobject/src/class_db.rs I count 675 literal `ClassRegistration::new(
- **project-settings-key-surface** — A whole modality nobody ran, and three finders explicitly deferred it to each other into a void. platform-os: "ProjectSettings' full setting surface and which settings the runtime actually honors — I only confirmed the singleton dispatches and left the setting-by-setting audit to resources-io". resources-io covered the project.godot PARSER and the ProjectSettings SINGLETON but never the key surfac
- **engine-virtual-method-table** — The 'extend an engine class from GDScript' modality — the entire reason ClassDB and script attachment exist — was never audited as a surface. gdscript owned the LANGUAGE, core-object owned Object's 48 api methods, scene-tree owned the Node LIFECYCLE virtuals; nobody owned the class virtual table. Godot exposes 1413 virtual methods over 106 classes (1144 unique `_`-prefixed names). Patina dispatche
- **asset-import-pipeline** — The whole import modality is unowned, and it is INVISIBLE to the sweep by construction: the sweep only ran over the 396 unregistered classes, and all 19 GLTF* classes are registered (class_db.rs:11071 GLTFDocument, :11331 GLTFState, :11434 FBXDocument), so no sweep chunk ever touched them; the 17 ResourceImporter* classes were touched only by sweep-5, which dropped them as parity_ok while flagging
- **renderingserver-renderingdevice-script-api** — The single largest class surface in the API had no owner. RenderingServer is 526 methods + 83 enums; RenderingDevice is 118 methods + 36 enums; plus the RD* resource family. Three finders each deferred it: rendering-3d ("The RenderingServer script API subset (rs.mesh_create / instance_set_base / etc.) — I audited the internal Rust trait, not the script-facing singleton"), rendering-2d (audited "th

