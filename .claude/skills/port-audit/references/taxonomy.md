# Port-Audit Subsystem Taxonomy — the stones that must be turned

This file is the coverage floor for every `/port-audit` run. Each finder agent owns ONE
section. The bullets are search directives, not a checklist to copy verbatim — the finder
must expand each bullet against the ground truth (extension_api.json, `upstream/godot/`
source, `upstream/godot/doc/classes/*.xml`) and may add stones, never remove them.

## Scope anchor (what "1:1" means)

The parity bar is: **everything Godot 4.6.1 itself can do (a) in its Web export for
runtime features, and (b) in its desktop editor for editor features.** Patina is a
browser-native engine+editor, so:

- Runtime features that Godot's own web export cannot do (raw UDP/TCP sockets, ENet,
  native OpenXR runtimes, subprocess spawning, native file dialogs) may be classified
  `OUT_OF_SCOPE` **with a citation** (Godot web-limitations doc, `prd/` scope doc, or
  `THIRDPARTY_STRATEGY.md`). Everything Godot's web export CAN do (WebSocket, WebRTC,
  WebXR, fetch-based HTTP, IndexedDB persistence, AudioWorklet, gamepads, TTS,
  clipboard) is in scope — no exceptions.
- Editor features are compared against the full desktop Godot editor
  (`upstream/godot/editor/`), because Patina's browser editor replaces it.
- Anything not covered by an explicit citation is a gap. "We probably don't need it"
  is not a citation.

## Cross-cutting sweeps (owned by the sweep + critic stages, but every finder should flag hits)

- **Unwired-runtime pattern** (THE systemic failure here): real, tested library code that
  nothing in `patina-runner`/`gdscene` MainLoop/SceneTree tick/editor server/served
  EDITOR_HTML ever calls. Module-level tests passing is NOT wiring. Also watch for
  cfg-flag lies (a `editor_backend=="wgpu"`-style branch that can never be true).
- **Singletons**: all 39 (`jq -r '.singletons[].name'`) must exist and be script-reachable.
- **Utility functions**: `jq -r '.utility_functions[].name'` (@GlobalScope) + @GDScript
  built-ins (preload, assert, Color8, print_debug, …) vs the GDScript interpreter's table.
- **Global enums/constants**: `.global_enums` (Key, MouseButton, Error, PropertyHint, …).
- **Default InputMap actions**: all `ui_*` actions and their default events.
- **ProjectSettings surface**: every setting the runtime honors (physics tick, stretch
  mode, audio driver knobs, rendering toggles, autoloads, feature overrides `.web`).
- **Notification constants & order**: ENTER_TREE/READY/PROCESS/EXIT_TREE ordering parity.

---

## core-object — Object model, ClassDB, signals
**Ground truth:** `upstream/godot/core/object/`, api classes Object/RefCounted/WeakRef.
**Patina entry points (hints, verify):** `gdobject` (class_db.rs), `gdcore`.
**Stones:**
- Object: property get/set (+ indexed paths), metadata, script attach/detach, `to_string`,
  notification dispatch, `callv/call_deferred`, MessageQueue flush timing, free vs
  queue_free semantics, `is_instance_valid`, ObjectID + `instance_from_id`.
- ClassDB script API: class list, inheritance queries, can_instantiate/instantiate,
  method/property/signal/constant/enum introspection.
- Signals: connect flags (DEFERRED/ONE_SHOT/PERSIST/REFERENCE_COUNTED), Callable
  bind/unbind, disconnect, signal list introspection, emission order, self-disconnect
  during emission.
- RefCounted lifecycle, WeakRef, reference cycles behavior.
- Property list generation with PROPERTY_USAGE/HINT flags + per-property revert.
- Registration completeness: every `register_*_classes()` table vs api json (the sweep
  diffs names; you verify the registered ones actually construct + expose methods).
**Wired means:** ClassDB dispatch is what GDScript/editor inspector actually route
through; a class registered but not constructible from a .tscn or script is UNWIRED.

## core-variant — Variant, math types, strings, containers
**Ground truth:** `core/variant/`, `core/math/`, api `builtin_classes` (38).
**Patina:** `gdvariant`, `gdcore`.
**Stones:**
- All 38 builtins present with constructors, operators (full operator matrix incl.
  Variant==Variant coercions), methods, conversion rules (`.builtin_classes[].methods`).
- Math semantics: Basis/Transform2D/3D affine ops, Quaternion slerp, Plane, Projection,
  AABB/Rect2 grow/merge/intersects, euler orders, approx-equality epsilons.
- String API (~100 methods): format `%`, case transforms, split/join, uri encode/decode,
  validation, sha/md5, similarity, naturalnocasecmp; StringName interning; NodePath
  parsing incl. `:subname` chains.
- Packed*Array (all 9): COW semantics, methods, bsearch/sort/slice; typed Array[T]
  enforcement; Dictionary (+typed 4.4), sort_custom/map/filter/reduce.
- Callable (bind/unbind/get_object/rpc), Signal as value, var_to_str/str_to_var,
  var_to_bytes(+objects) round-trip, hashing/equality rules across types.
**Wired means:** the GDScript VM and .tscn parser construct/operate these — not just a
Rust type existing.

## gdscript — language, VM, annotations
**Ground truth:** `upstream/godot/modules/gdscript/` (+doc `@GDScript.xml`, `@GlobalScope.xml`).
**Patina:** `gdscript-interop` (+ interpreter location — find it).
**Stones:**
- Grammar: match (constant vs binding patterns, dict/array/guard/multi/rest), lambdas +
  captures, await (signals + coroutines), string formatting, ternary, `in`, `**`,
  compound assigns, bitwise ops, typed declarations + inference, enums, `class_name`,
  inner classes, static vars/funcs + static init, `super` calls, property setget.
- Annotations: all @export variants (range/enum/flags/layers/node_path/file/dir/
  multiline/placeholder/color_no_alpha/group/subgroup/category/custom/tool_button),
  @onready, @tool, @rpc, @warning_ignore, @icon, @static_unload, @abstract (4.5+ if in api).
- Utility functions: full @GlobalScope + @GDScript tables vs interpreter builtins.
- Semantics: error on missing method vs silent, typed-call optimization behavior,
  Object duck calls, signal declaration + emit, preload vs load, tool-mode execution,
  script inheritance (path vs class_name), get_property_list from scripts, first-class
  Callable on methods.
**Wired means:** scripts attached in .tscn actually execute in the runner and editor
Play flow (compile → _ready/_process dispatch → property sync to nodes).

## scene-tree — Node lifecycle, SceneTree, viewports, windows
**Ground truth:** `upstream/godot/scene/main/`.
**Patina:** `gdscene` (lib.rs, main_loop.rs), `patina-runner`.
**Stones:**
- Lifecycle exact order: _enter_tree parent-first, _ready child-first, _exit_tree,
  NOTIFICATION_* constants; process/physics_process + priorities + process modes
  (inherit/pausable/when_paused/always/disabled); pause propagation.
- Groups (+call_group flags), owner semantics, PackedScene instancing (editable
  children, InstancePlaceholder, inherited scenes/diffs), `%unique` names, get_node,
  find_child(pattern), duplicate() flags, reparent/move_child/add_sibling.
- SceneTree: change_scene_to_file/packed deferred semantics, create_timer (+process
  modes), create_tween, quit, node_added/removed/renamed signals, tree_changed,
  auto_accept_quit.
- Viewport tree: Window root (mode/size/content-scale modes+aspect/factor — the web
  canvas stretch pipeline), SubViewport (update modes, clear modes, size_2d_override,
  render target), CanvasLayer (layer, follow_viewport, transform), world_2d/world_3d
  propagation + own_world_3d, Camera2D/3D current management.
- Input propagation stages: _input → gui_input → shortcut_input →
  _unhandled_key_input → _unhandled_input → physics picking; set_input_as_handled;
  Viewport.push_input.
- Timer node, physics interpolation toggles, MainLoop hooks.
**Wired means:** patina-runner's frame loop drives ALL of it: process callbacks,
deferred queue flush, physics step order, draw. This subsystem is where "exists but
never ticked" hides — trace the actual per-frame call sequence.

## nodes-2d — 2D scene node behavior surface
**Ground truth:** `upstream/godot/scene/2d/` + doc XML per class.
**Patina:** `gdscene` (node impls), `gdrender2d`.
**Stones:**
- Sprite2D (region/frame/hframes/vframes/centered/offset/flip), AnimatedSprite2D +
  SpriteFrames (autoplay, speed_scale, frame_progress, signals).
- Camera2D: limits + smoothing, drag margins, zoom, ignore_rotation, process callback.
- TileMapLayer (4.3 node): TileSet atlases, alternative tiles, terrains autotiling,
  per-tile physics/navigation/occlusion/custom-data, animated tiles, Y-sort per layer.
- Line2D (joint/cap modes, width curve, gradient, texture modes), Polygon2D (uv, invert,
  internal vertices, Skeleton2D weights), Path2D/PathFollow2D (progress_ratio, rotates,
  cubic, loop), Curve2D baking.
- Area2D behavior: monitoring/monitorable, enter/exit signals + shape indices, space
  override modes (gravity point/direction, damp priorities), audio bus override.
- RayCast2D/ShapeCast2D semantics (per-physics-frame update, force_raycast_update,
  exclusions, hit_from_inside), TouchScreenButton, CollisionShape2D/Polygon2D
  (disabled, one_way).
- Light2D (point/directional): textures, shadow filters/smooth, blend modes, energy,
  height (normal maps), item cull masks; LightOccluder2D + cull modes; CanvasModulate.
- ParallaxBackground/ParallaxLayer + Parallax2D, Marker2D, RemoteTransform2D,
  VisibleOnScreenNotifier2D/Enabler2D, BackBufferCopy, Bone2D/Skeleton2D rest poses,
  MeshInstance2D/MultiMeshInstance2D, CanvasGroup.
**Wired means:** each node type round-trips: .tscn load → node with typed properties →
per-frame draw commands into the 2D renderer → visible pixels in runner AND editor
viewport; signals fire from the physics/render tick, not just from unit tests.

## rendering-2d — canvas pipeline
**Ground truth:** `servers/rendering/` canvas paths, `scene/main/canvas_item.*`.
**Patina:** `gdrender2d/renderer.rs`, `gdserver2d`.
**Stones:**
- Full CanvasItem draw_* API (~30: line/polyline/arc/circle/rect/texture/
  texture_rect_region/nine-patch/primitive/polygon/string/char/mesh/multimesh/
  set_transform/…), draw ordering, item clipping (clip_children modes), show_behind_parent,
  top_level, self_modulate vs modulate propagation.
- z_index (+z_as_relative) global sort, y_sort_enabled cross-parent rules, per-item
  texture filter/repeat inheritance.
- CanvasItemMaterial (blend modes, light modes, particles anim), CanvasTexture
  (normal/specular), SCREEN_TEXTURE/BackBufferCopy, 2D MSAA, snap-2D-to-pixel,
  canvas SDF (occluders → GPUParticles2D collision).
- 2D lighting pipeline: additive/sub/mix blend, shadows w/ PCF filters, cull masks.
- Viewport canvas_transform vs global_canvas_transform, window stretch integration
  (canvas_items mode, aspect keep/expand, scale factor) — the web-canvas resize path.
- RenderingServer canvas_* API subset used by scenes/scripts.
**Wired means:** scene tree emits draw commands each frame through the canvas pipeline
in BOTH the runner and the editor viewport (and editor Play). A renderer feature with
no node/scene producer reaching it is UNWIRED.

## nodes-3d — 3D scene node behavior surface
**Ground truth:** `upstream/godot/scene/3d/`.
**Patina:** `gdscene`, `gdrender3d`.
**Stones:**
- Node3D transform semantics: local/global, rotation modes + euler orders, top_level,
  visibility propagation, NOTIFICATION_TRANSFORM_CHANGED + set_notify_transform.
- MeshInstance3D (surface overrides, blend shape values, skin/skeleton binding),
  Camera3D (fov/size/frustum offset, near/far, cull_mask, project/unproject,
  keep_aspect, CameraAttributes exposure/DOF, doppler), Lights (directional PSSM
  splits/blend, omni paraboloid/cube shadows, spot angle attenuation; energy,
  indirect/volumetric energies, bake modes, cull masks, distance fade, shadow
  bias/normal-bias/blur, light size soft shadows, projectors, negative, temperature).
- ReflectionProbe (update modes, box projection, interior), Decal (maps, fades),
  FogVolume (shapes), VoxelGI/LightmapGI class surface (bake likely editor-scope),
  OccluderInstance3D.
- Path3D/PathFollow3D + Curve3D (tilt/up vectors, baked lengths), RayCast3D/
  ShapeCast3D/SpringArm3D, Area3D (wind!, reverb bus, point gravity), Label3D
  (billboard, outline, alpha-cut), Sprite3D/AnimatedSprite3D, Marker3D,
  RemoteTransform3D, VisibleOnScreenNotifier3D/Enabler3D, WorldEnvironment.
- CSG family + GridMap (modules — check module scope), MultiMeshInstance3D.
**Wired means:** same round-trip as nodes-2d but through the 3D renderer(s) — software
AND wgpu paths where both exist; transform dirtying actually propagates to draw.

## rendering-3d — materials, environment, shadows, meshes
**Ground truth:** `servers/rendering/`, `scene/resources/` (material/environment/sky/mesh).
**Patina:** `gdrender3d` (renderer.rs, wgpu_pipeline.rs, shadow_map.rs, postprocess.rs,
depth_buffer.rs), `gdserver3d`.
**Stones:**
- StandardMaterial3D/ORMMaterial3D FULL surface: albedo(+vertex color), metallic/
  roughness/specular, emission(+operators), normal, rim, clearcoat, anisotropy, AO,
  heightmap parallax(+deep), subsurface+transmittance, backlight, refraction, detail
  layers, UV1/UV2 transforms + (world-)triplanar, transparency modes (alpha/scissor/
  hash/depth-prepass), blend/cull/depth-draw modes, shading modes, diffuse/specular
  modes, billboard(+particles, keep-scale), grow, fixed-size, point size,
  proximity/distance fade, render_priority, next_pass.
- Environment: bg modes, sky (Procedural/Physical/Panorama materials, rotation,
  ambient-from-sky), ambient+reflected light sources, tonemap (linear/reinhard/filmic/
  ACES/AgX + exposure/white), adjustments (+color correction LUT), glow (levels,
  blend modes incl. softlight/screen/replace/mix, HDR threshold, glow map), fog
  (+aerial perspective, height), volumetric fog (density/albedo/emission/anisotropy/
  GI inject/temporal), SSAO, SSIL, SSR, SDFGI class surface.
- Shadows: directional cascades + blend, atlas quadrants for omni/spot, PCF soft
  filters, caster culling; verify vs Patina shadow_map.rs actually sampled in both
  pipelines.
- Meshes: ArrayMesh all ARRAY_* formats (+custom, compression, 8/16-bit), primitives
  (incl. TextMesh/Ribbon/Tube), ImmediateMesh, LOD (+bias, visibility ranges + fade),
  skinning (Skin binds, blend shapes normalized/relative), MultiMesh (formats, colors,
  custom data, visible count).
- Viewport: 3D scaling modes (bilinear/FSR), TAA/MSAA/SSAA, debanding, HDR-2D,
  render layers, GeometryInstance3D overrides (material override/overlay, transparency,
  custom AABB, extra cull margin, gi mode).
**Wired means:** editor viewport AND runner render with the same feature set; a
postprocess/shadow feature must be reachable from WorldEnvironment/Light nodes parsed
out of a .tscn — not just a struct with tests. Check the wgpu and software paths
separately; a feature in one but not the other is PARTIAL.

## shaders — Godot shading language + ShaderMaterial
**Ground truth:** `servers/rendering/shader_language.cpp`, doc `Shader*.xml`,
shading-language reference in `doc/`.
**Patina:** `gdrender3d/shader.rs` (+ any WGSL transpile), `gdresource` loaders.
**Stones:**
- shader_type spatial/canvas_item/particles/sky/fog; per-type render_mode lists
  (spatial ~40: blend_*, depth_*, cull_*, unshaded, world_vertex_coords,
  skip_vertex_transform, shadows_disabled, ambient_light_disabled, shadow_to_opacity,
  alpha_to_coverage, fog_disabled, …).
- Language: full GLSL-ES-3-subset builtins (texture/textureLod/texelFetch/dFdx/…,
  matrix/packing funcs), structs, functions, const arrays, varyings(+flat), discard,
  preprocessor (#define/#ifdef/#include + ShaderInclude resource).
- Uniforms: every hint_* (source_color, range, enum, filter/repeat variants, default
  white/black/normal, screen_texture/depth_texture/normal_roughness_texture),
  instance uniforms, global uniforms (+ProjectSettings storage + runtime set), groups.
- Per-stage built-ins for each type (vertex/fragment/light; particles start/process +
  emit_subparticle; sky EYEDIR/radiance; fog).
- ShaderMaterial: param get/set, per-instance uniforms; .gdshader load; shader params
  appearing in the editor inspector; error reporting with line numbers.
- VisualShader + VisualShaderNode* class surface (~100 classes — surface-level parity
  + editor graph is editor-ux; class registration counts here).
**Wired means:** a .gdshader in a .tscn drives BOTH pipelines (WGSL path consumed by
wgpu; CPU eval in software). A parser without pipeline consumption is UNWIRED.

## vfx-particles — particles, trails, attractors, collision
**Ground truth:** `scene/2d/gpu_particles_2d.*`, `scene/3d/gpu_particles_3d.*`,
`scene/resources/particle_process_material.*`, CPUParticles*.
**Patina:** search gdscene/gdrender* for particle systems.
**Stones:**
- GPUParticles2D/3D: emitting/amount(+ratio), lifetime, one_shot, preprocess,
  explosiveness, randomness, fixed_fps(+interpolate, fract delta), visibility
  rect/aabb, local_coords, draw order, draw passes (3D), trails (length/sections),
  sub_emitter, seed control, finished signal, emit_particle() manual emission.
- ParticleProcessMaterial FULL: emission shapes (point/sphere/±surface/box/points/
  directed-points/ring), direction/spread/flatness, gravity, initial+angular velocity,
  orbit/radial/tangential accel, damping(+as friction), angle, scale(+curves, split
  xyz), color(+ramps, hue variation), animation offset/speed, turbulence (noise
  strength/scale/speed/influence), attractor interaction, collision modes(+friction/
  bounce), sub-emitter modes, particle flags (align-Y, rotate-Y, disable-Z).
- CPUParticles2D/3D full mirror + convert-from-material.
- GPUParticlesAttractor* (box/sphere/vector-field) + GPUParticlesCollision*
  (box/sphere/heightfield/SDF); 2D collision via canvas SDF.
- Particle trails geometry, custom particle shaders hook (shader_type particles).
**Wired means:** particles visibly simulate + render in runner and editor viewport from
a plain .tscn; "material parses but simulation never steps" is UNWIRED.

## animation — players, trees, tweens, skeletons
**Ground truth:** `scene/animation/`, `scene/resources/animation.*`.
**Patina:** `gdscene/animation.rs` (+ wherever AnimationTree lives).
**Stones:**
- Animation resource: ALL track types (value w/ update modes continuous/discrete/
  capture; position/rotation/scale 3D; blend shape; method; bezier w/ handle modes;
  audio w/ offsets; animation playback), interpolation (nearest/linear/cubic +
  angle-wrapped variants), loop modes (none/linear/pingpong + loop_wrap), track paths
  with `:property` subnames, compression.
- AnimationMixer base: callback modes (process/idle/manual), deferred vs immediate,
  root motion track + accumulators, audio polyphony, capture cache, reset-on-save.
- AnimationPlayer: play/queue/pause/seek(+update), custom blend/speed per play,
  backwards/from_end, section/marker playback, autoplay, signals, speed_scale.
- AnimationTree: state machine (transition modes immediate/sync/at_end, xfade(+curve),
  auto-advance + conditions + expressions, travel pathfinding, nested machines,
  playback control object), blend spaces 1D/2D (points, auto-triangles, sync), blend
  tree nodes (Add2/3, Blend2/3, OneShot w/ fade modes + request enum, TimeScale,
  TimeSeek, Transition, sub-StateMachines), per-connection filters, parameter
  property exposure (get/set via `parameters/...`).
- Tween: tweeners (property w/ from/relative + per-tweener ease/trans, interval,
  callback+delay, method), parallel/chain, loops, pause/process modes, speed scale,
  full ease×transition matrix (incl. spring/back/elastic/bounce), signals, kill
  semantics, Node.create_tween binding + auto-kill with node.
- Skeleton3D: rest/pose/global-pose APIs, Skin binds, motion scale, SkeletonModifier3D
  family (SkeletonIK3D, LookAtModifier3D, SpringBoneSimulator3D 4.4,
  PhysicalBoneSimulator3D), BoneAttachment3D(+external skeleton), BoneMap/
  SkeletonProfileHumanoid retargeting; Skeleton2D/Bone2D + Polygon2D weights +
  SkeletonModification2D stack status in 4.6.
**Wired means:** AnimationPlayer/Tree advance from the SceneTree tick and write node
properties/poses that the renderer consumes (memory: AnimationTree was library-only
once — re-verify every run). Tween must run from create_tween() in a script in a scene.

## physics-2d — server, bodies, queries, joints
**Ground truth:** `servers/physics_2d/`, `scene/2d/physics/`.
**Patina:** `gdphysics2d` (world.rs), `gdserver2d`, gdscene body nodes.
**Stones:**
- Shapes: circle/rectangle/capsule/segment/world-boundary/separation-ray/convex/
  concave; rotation-aware narrowphase for every pair; shape margins.
- RigidBody2D: freeze(+modes), mass/inertia/COM (auto+custom), gravity_scale,
  linear/angular damp (+combine modes), sleeping(+thresholds, can_sleep), contact
  monitor + max_contacts + body_entered w/ shape indices, custom integrator +
  _integrate_forces direct-state (contacts/impulses), forces/impulses/torque
  (central+positioned+constant), CCD modes, physics material
  (friction/bounce/rough/absorbent), pickable input events.
- StaticBody2D constant velocities (conveyors), AnimatableBody2D sync-to-physics.
- CharacterBody2D move_and_slide FULL semantics: floor/wall/ceiling flags, floor
  max angle, snap length, stop-on-slope, constant-speed-on-slope, floor-block-on-wall,
  wall-min-slide-angle, max slides, moving-platform velocity + leave behaviors, safe
  margin, motion modes grounded/floating, slide collision reports; move_and_collide
  (+test_only, recovery_as_collision).
- Area2D overlap timing + space-override combine order + priorities.
- Joints: pin (softness/limits), groove, damped spring; bias, disable-collision.
- Direct space state: intersect_ray/point/shape, cast_motion, collide_shape,
  rest_info (all params: exclude, masks, areas/bodies flags, hit_from_inside).
- One-way collision (+margin, incl. tiles), layers/masks (32), collision priority,
  physics ticks/sec + max steps + jitter fix, 2D interpolation, space params from
  project settings, body_test_motion API, viewport physics picking.
**Wired means:** SceneTree physics step → world step → transforms sync back to nodes →
signals fire. Determinism/stacking sanity vs Godot (behavioral fixtures exist under
`fixtures/`) — but do NOT run cargo; read the fixture tests to see what's covered.

## physics-3d — server, bodies, vehicles, ragdolls
**Ground truth:** `servers/physics_3d/`, `scene/3d/physics/`.
**Patina:** `gdphysics3d/world.rs`, `gdserver3d`.
**Stones:**
- Shapes: box/sphere/capsule/cylinder/convex/trimesh(+backface flag)/heightmap/
  world-boundary/separation-ray; full narrowphase pair matrix (capsule/cylinder/OBB
  rotation-aware — GJK/EPA or SAT); broadphase w/ layer/mask filtering.
- RigidBody3D (2D list + per-axis linear/angular locks, inertia tensor),
  CharacterBody3D move_and_slide 3D semantics, AnimatableBody3D.
- VehicleBody3D/VehicleWheel3D (raycast vehicle: engine force, brake, steering,
  suspension stiffness/travel/rest length/max force, damping compression/relaxation,
  friction slip, roll influence, per-wheel flags).
- SoftBody3D (pinning, stiffness, pressure, drag, damping, disable-collision pairs).
- Joints: Pin/Hinge (limits+motor)/Slider/ConeTwist/Generic6DOF (per-axis limits/
  motors/springs); PhysicalBone3D + ragdoll simulation.
- Area3D: wind source/force/attenuation, point gravity, reverb bus routing.
- Direct space state full API, camera ray picking, CCD, contact reporting, SpringArm3D
  exclusions, physics interpolation 3D.
**Wired means:** same as 2D — stepped by main loop, synced to Node3D transforms,
signals from overlap events; verify Area3D wind/reverb actually influence targets.

## navigation — NavigationServer 2D/3D, agents, baking
**Ground truth:** `servers/navigation*/`, `modules/navigation*/`, `scene/*/navigation*`.
**Patina:** `gdscene/navigation.rs` (+ server locations).
**Stones:**
- NavigationServer2D/3D: maps (cell size/height, edge connection margin, link radius),
  regions (layers, enter/travel costs, transforms), synchronization timing (commit on
  physics frame).
- NavigationMesh/NavigationPolygon: full bake param surface (agent radius/height/
  climb/slope, cell sizes, region sizes, edge lengths, detail sampling, geometry
  parse modes + source group, partition modes), async/on-thread baking, source
  geometry data + obstacle carving.
- NavigationAgent2D/3D: path desired/target distances, path max distance recompute,
  postprocessing (corridorfunnel/edgecentered), path metadata flags, simplify epsilon,
  avoidance (RVO: radius, height, max speed/neighbors, time horizons, layers/mask/
  priority, 3D-vs-layered), velocity_computed flow, target_reached/navigation_finished
  semantics per frame, waypoint/link signals.
- NavigationObstacle2D/3D (dynamic avoidance radius + static carve vertices),
  NavigationLink2D/3D (bidirectional, costs), NavigationRegion nodes (+bake at runtime).
- Path queries (parameters/result objects), closest-point queries, random points,
  AStar2D/3D/AStarGrid2D (weights, disabled points, heuristics, diagonal modes,
  jumping).
**Wired means:** an agent in a .tscn actually paths across a baked/loaded navmesh at
runtime, avoidance velocities feed movement, and debug visualization exists in editor.

## audio — server, buses, effects, streams, spatial players
**Ground truth:** `servers/audio*`, `scene/audio/`, `modules/{vorbis,minimp3,ogg}`.
**Patina:** `gdaudio` (server.rs, bus.rs, decode.rs).
**Stones:**
- AudioServer: bus add/remove/move, send routing, volume/solo/mute/bypass, per-bus
  effect chains (insert/remove/swap/enable), peak meters, playback speed scale,
  mix rate/latency reporting, output device story (web: AudioWorklet backend), input
  (microphone via getUserMedia — web permission flow).
- Every AudioEffect class w/ full params: Amplify, filters (LowPass/HighPass/BandPass/
  BandLimit/Notch/AllPass/LowShelf/HighShelf + EQ6/10/21), Compressor (+sidechain!),
  Limiter/HardLimiter, Delay, Chorus, Phaser, Distortion (5 modes), Reverb,
  SpectrumAnalyzer (+instance FFT queries), Panner, PitchShift, StereoEnhance,
  Record, Capture.
- Streams: WAV (8/16-bit, IMA-ADPCM, QOA, loop modes fwd/pingpong/backward), OggVorbis
  (loop+offset, bpm/beat metadata), MP3, Generator (+playback push_frame), Microphone,
  Randomizer, Playlist, Interactive (clips+transitions), Synchronized, Polyphonic.
- Players: AudioStreamPlayer (bus, autoplay, max_polyphony, pitch_scale, volume,
  play(from)/seek/stream_paused, finished, playback position w/ latency compensation,
  mix_target), 2D (max_distance, attenuation, panning strength, area bus override),
  3D (unit size, max_db, attenuation models incl. distance lowpass filter + cutoffs,
  emission angle cone + outside-cone filter, doppler modes, Area3D reverb/bus
  routing, AudioListener3D vs camera).
- Pause semantics (SceneTree pause + process modes), animation audio tracks +
  polyphony, web autoplay unlock on first gesture.
**Wired means:** the mixer graph runs on the audio thread/worklet pulling streams;
players triggered from scenes are audible with correct bus/effect routing (memory:
audio was once fully built but never wired — re-verify the output backend feed).

## multiplayer — peers, RPC, replication, HTTP/WebSocket/WebRTC
**Ground truth:** `scene/main/multiplayer_*`, `modules/{multiplayer,websocket,webrtc,enet}`,
`core/io/{stream_peer*,packet_peer*,http_client*}`.
**Patina:** `gdplatform/network.rs`, `gdcore/crypto.rs`, grep for rpc/multiplayer.
**Stones:**
- Godot binary Variant codec (encode/decode all types + objects flag) — shared by RPC
  wire, var_to_bytes, binary .scn (cross-check with resources-io finder).
- StreamPeer family (typed get/put, big-endian), StreamPeerBuffer/TCP, PacketPeer
  (+UDP pseudo-connect, broadcast, multicast), TCPServer/UDPServer — native-scope;
  on web cite OUT_OF_SCOPE per web limitations, but the CLASS surface should exist
  with correct errors.
- HTTPClient/HTTPRequest over fetch: methods, headers, chunked, gzip, redirects,
  timeouts, body size caps, download-to-file, TLSOptions story, CORS reality.
- WebSocketPeer (client+server roles — server OOS on web, cite), protocols, extra
  headers, text/binary, buffer sizes, close code/reason; WebSocketMultiplayerPeer.
- WebRTCPeerConnection (+DataChannel: ordered/unordered, retransmits/lifetime,
  negotiated ids, states) + WebRTCMultiplayerPeer (mesh/star); browser RTC glue.
- ENetMultiplayerPeer surface (native-scope; web OOS cite), OfflineMultiplayerPeer.
- MultiplayerAPI/SceneMultiplayer: peer mgmt, unique ids, poll timing in SceneTree,
  per-branch multiplayer (set_multiplayer), auth callback flow, allow_object_decoding,
  refuse_new_connections, server_relay, signals (peer_connected/…, connection_failed).
- RPC: @rpc annotation full config (any_peer/authority, call_local/remote, reliable/
  unreliable(+ordered), channels), rpc()/rpc_id() dispatch, rpc_config dict, authority
  validation errors, node-path caching protocol.
- Replication: MultiplayerSpawner (spawn path, auto list, custom spawn function +
  data, limits), MultiplayerSynchronizer (SceneReplicationConfig property modes
  spawn/sync/watch, intervals, delta, visibility filters + update modes,
  public visibility), authority propagation.
**Wired means:** SceneTree polls the MultiplayerAPI each frame; an @rpc call in a
script on peer A arrives on peer B (loopback/OfflineMultiplayerPeer at minimum, WS
pair ideally); Spawner/Synchronizer react to tree changes at runtime.

## gui-controls — every Control node, layout, focus, drag-drop
**Ground truth:** `scene/gui/` (one file per control), doc XML per class.
**Patina:** grep gdscene/gdrender2d for Control implementations.
**Stones:**
- Control core: anchors/offsets + presets + grow directions, layout modes, size flags
  + stretch ratios, custom_minimum_size, pivot/rotation/scale, clip_contents, focus
  (modes, neighbors, next/prev, tab traversal, grab/release, signals), mouse filters
  (+force_pass_scroll_events, entered/exited semantics), cursor shapes per control,
  tooltips (+custom via _make_custom_tooltip, delay), theme lookup chain + per-instance
  overrides, layout direction/RTL mirroring, auto_translate modes, drag & drop
  (_get_drag_data/_can_drop_data/_drop_data, set_drag_preview, forwarding),
  _gui_input/accept_event, _has_point, minimum-size change propagation + queue_sort.
- Containers: Box/Flow/Grid/Split (offset, collapsed, drag area)/Tab (TabBar surface:
  close policies, rearrange groups, scrolling, metadata)/Scroll (modes, follow focus,
  custom step)/Margin/Center/AspectRatio/Panel/SubViewportContainer (stretch shrink,
  input forwarding), Container base virtuals (_sort_children notification).
- Buttons: BaseButton (toggle, action modes, button masks, groups+allow_unpress,
  shortcuts+feedback, draw-mode states), Button (icon alignment, expand icon, overrun),
  CheckBox/CheckButton, OptionButton (items/ids/metadata/separators, fit_longest),
  MenuButton, LinkButton, TextureButton (per-state textures + click mask),
  ColorPickerButton.
- Ranges: Range base (min/max/step/page, ratio, exp, allow greater/lesser, share),
  H/VSlider (ticks, scrollable), SpinBox (prefix/suffix, custom arrows, update on
  text changed), ProgressBar (fill modes, indeterminate), H/VScrollBar (paging),
  TextureProgressBar (radial fill + nine-patch).
- Text display: Label (autowrap modes, justification flags, overrun+ellipsis, visible
  chars/ratio, uppercase, LabelSettings), RichTextLabel (FULL BBCode: b/i/u/s/code/
  color/bgcolor/fgcolor/font/size/outline/img(+size/region)/url+meta_clicked/indent/
  lists/tables(+cell props)/effects (wave/tornado/shake/fade/rainbow/pulse + custom
  RichTextEffect)/hint/dropcap, push/pop API, threaded parse, scroll-follow, selection,
  visible ratio, fit content).
- Editing: LineEdit (max length, secret, placeholder, clear button, right icon, caret
  API, selection API, context menu, IME, virtual keyboard types, select-on-focus),
  TextEdit (multiple carets!, selection modes, gutters, wrapping, undo/redo, IME,
  placeholder, minimap, drag-drop text, search, line editing signals), CodeEdit
  (completion + hints, breakpoints/bookmarks gutters, folding + regions, auto indent,
  brace matching, delimiters, symbol lookup).
- Item views: ItemList (icon modes, columns, select modes, per-item everything,
  rmb/empty clicks), Tree (columns+titles+expand ratios, TreeItem cell modes string/
  check/range/icon/custom, editable cells, per-cell buttons/icons/colors/tooltips,
  propagate_check, ranges, folding, select modes, drop-section API, edit_selected,
  scroll-to, all signals), PopupMenu (ids/accels/shortcuts/submenus/checkable/
  radio/multistate, separators+labels, hover/autohide behaviors, search, activate
  by event).
- Windows/dialogs: Window (embedded windows, popup semantics + exclusive + transient,
  content scale modes/aspect/factor, min/max size, borderless/transparent,
  close_requested, wrap_controls), AcceptDialog (+custom actions, register_text_enter,
  hide_on_ok), ConfirmationDialog, FileDialog (modes, access scopes, filters+custom,
  options API, show hidden — web: maps to upload/download patterns), PopupPanel,
  Popup.
- Misc: TextureRect (stretch/expand modes, flip), NinePatchRect (axis stretch modes,
  draw center), ColorRect, Separators, Panel, GraphEdit (connections+types+activity,
  zoom/minimap/grid patterns, box select, disconnect requests, arrange, GraphNode
  slots/ports/resize + GraphFrame + GraphElement), ColorPicker (shapes HSV/wheel/VHS/
  OKHSL, modes RGB/HSV/OKHSL+RAW, presets+recent, sampler/screen pick (web limits),
  deferred mode, controls visibility toggles), VideoStreamPlayer (theora or web-video
  strategy — audit approach + cite), MenuBar (+native menu OOS on web).
**Wired means:** controls receive routed input (focus, hover, gui_input), draw through
the canvas pipeline with the real theme, emit signals scripts can connect to — in the
runner AND as the substrate of any in-game UI demo. A Control that renders but never
receives input (or vice versa) is PARTIAL.

## theming-text — Theme, StyleBoxes, fonts, TextServer, i18n
**Ground truth:** `scene/theme/` (default_theme.cpp!), `scene/resources/style_box*`,
`servers/text_server*` (+`modules/text_server_adv`), `core/string/translation*`.
**Patina:** grep for theme/stylebox/font/text server equivalents.
**Stones:**
- Theme resource: item types (color/constant/font/font_size/icon/stylebox), type
  variations + base types, default font/size, merge/clear; ThemeDB fallbacks; project
  custom theme setting; **default theme visual parity** (Godot generates it in
  `default_theme.cpp` — enumerate its styleboxes/colors/margins per control).
- StyleBoxFlat (bg, per-side borders, per-corner radii + detail, expand + content
  margins, shadow, skew, AA), StyleBoxTexture (margins, axis stretch h/v
  stretch/tile/tile-fit, region, modulate), StyleBoxLine, StyleBoxEmpty, custom draw.
- Fonts: FontFile (TTF/OTF/WOFF2, bitmap BMFont, image fonts, MSDF(+range), AA modes
  incl. LCD, hinting, subpixel positioning, oversampling, fixed-size scale modes,
  system fallback, OpenType variations/features, embolden/transform/spacing),
  FontVariation, SystemFont (web constraints — cite), default font stack + fallback
  chain (what does Godot bundle vs Patina?).
- TextServer behavior: shaping (ligatures, clusters, combining marks), BiDi +
  structured text, script itemization, line breaking (mandatory/word/grapheme +
  CJK), justification flags, ellipsis/overrun flags, tab stops, glyph metrics
  (ascent/descent/underline per size), locale-aware upper/lower, TextLine/
  TextParagraph (build/draw/hit-test), RTL UI mirroring end-to-end.
- i18n: TranslationServer (locale matching incl. script/country fallbacks), .po
  (+context, plurals) and .csv translations, tr()/tr_n(+context), auto_translate
  propagation, pseudolocalization (all knobs), per-locale resource remaps,
  translation domains (4.4).
**Wired means:** Label/Button in a default-theme scene renders pixel-plausibly like
Godot (spacing/margins/colors), shaping actually shapes (test with RTL/combining
text), and locale switch re-renders UI text.

## input — events, actions, devices, IME
**Ground truth:** `core/input/` (input.cpp, input_map.cpp, input_event.cpp),
`scene/main/viewport.cpp` (routing).
**Patina:** grep gdscene/gdplatform for input.
**Stones:**
- InputEvent types FULL: Key (keycode vs physical vs key_label, echo, location,
  modifiers + command_or_control), MouseButton (double click, wheel factor),
  MouseMotion (relative, velocity, pen pressure/tilt), JoypadButton/Motion (deadzone
  handling), ScreenTouch/Drag (index, double tap, velocity), Magnify/Pan gestures,
  MIDI (WebMIDI — scope call w/ citation), Action (strength), Shortcut resource
  matching, is_action(+exact) semantics, event accumulation/merging.
- Input singleton: pressed/just_pressed/just_released (+exact), strength, get_axis/
  get_vector (circular deadzone), action press/release injection + parse_input_event
  + flush timing (agile flushing), mouse mode (visible/hidden/captured/confined —
  pointer-lock realities on web), warp, last velocity, keyboard queries (physical),
  joypads (connect signals, names/guids, SDL mapping db on web standard mapping,
  axes/buttons, rumble incl. web vibrationActuator, add_joy_mapping), accelerometer/
  gyro (web devicemotion permission — scope call), custom cursors (+hotspot, css
  cursor limits on web), CursorShape per Control.
- InputMap: project settings actions + per-action deadzone, ALL ui_* defaults,
  runtime add/erase, InputEventShortcut.
- Routing: Window → _input → GUI (focus + mouse target) → shortcut_input →
  _unhandled_key → _unhandled → physics picking; set_input_as_handled, per-viewport
  audio/gui disable flags, CollisionObject pick events, drag threshold, double-click
  time, echo generation on web key repeat, browser preventDefault policy (F5, Ctrl+S,
  space-scroll), touch↔mouse emulation both directions.
**Wired means:** a browser event reaches a script's _input AND a Control's grab-focus
path with correct coordinates under canvas scaling/stretch; captured mouse works in a
runner game (pointer lock), actions respond in editor Play.

## resources-io — loaders, formats, PCK, images, project settings
**Ground truth:** `core/io/`, `scene/resources/resource_format_text.cpp`,
`core/io/resource_format_binary.cpp`, `core/io/file_access_pack.cpp`, `core/io/image*`.
**Patina:** `gdresource` (loader.rs, importers.rs, config_file.rs), `gdcore`.
**Stones:**
- Text .tscn/.tres COMPLETENESS: Object(Class,…) constructors, typed Array[T],
  StringName/NodePath literals, every math literal (Vector4/Quaternion/Plane/AABB/
  Basis/Projection/int vectors), sub/ext resources + uid= refs, node groups/metadata,
  connections (flags/binds/unbinds), editable instances + placeholders — NO silent
  skips of unparseable values.
- Binary .res/.scn format v6 (variant decode all types, string tables, compressed
  RSCC, 64-bit offsets); PackedScene node-path-index bundling + instance diffs.
- PCK V2/V3 (dir flags, per-entry flags+MD5, encrypted index/files, embedded pck,
  file_base, load_resource_pack w/ replace + offset), res://, user://, uid://
  resolution; ResourceUID (base-34, uid_cache.bin, .uid sidecars).
- Import pipeline: .import [remap]/[deps] resolution, .godot/imported/ layout, .ctex
  (CompressedTexture2D: embedded webp/png modes, BasisU, mipmaps), imported scene
  .scn, .oggvorbisstr/.sample equivalents, texture array/3D/cubemap formats.
- ResourceLoader: type hints, cache modes (ignore/reuse/replace), threaded loading
  (request/status/progress/get), exists, dependencies, custom loader registration
  story; ResourceSaver (flags, custom savers); take_over_path/cache behavior.
- FileAccess: modes, compressed (fastlz/deflate/zstd/gzip), encrypted (+pass), endian,
  typed get/store (8..64/float/double/real/string/pascal/csv/var+objects), md5/sha,
  temp files; DirAccess (listing+hidden, recursive mkdir, copy/rename/remove);
  user:// on web → IndexedDB/OPFS + flush semantics.
- ConfigFile (+encrypted), JSON (parse errors w/ line, stringify indent/sort,
  from/to native + full objects), XMLParser, Marshalls.
- Image: decoders png/jpg/webp/tga/bmp/hdr/exr + svg rasterize (themes/fonts use it!),
  encoders png/jpg/webp/exr, ~37 pixel formats + conversions, mipmaps, compress
  (S3TC/ETC2/BPTC/ASTC — web texture policy!) + decompress, resize interpolations,
  blit/blend(+mask), fill(+rect), get/set pixel, premultiply, used_rect; ImageTexture
  (+update), PortableCompressedTexture2D, AtlasTexture (margin/filter_clip),
  Curve/Gradient/Noise textures (FastNoiseLite full surface if module present),
  ViewportTexture (cross-scene rules), CameraTexture (scope call).
- project.godot: full parse (sections, feature overrides `.web`, autoloads (scene vs
  script singletons), input map, layer names, global groups, shader_globals,
  application config (main scene, icon, boot splash), physics/rendering/audio
  settings the runtime honors, custom user settings + ProjectSettings runtime
  get/set/save, globalize/localize_path).
**Wired means:** the runner boots a real exported project (PCK or loose files) through
these loaders; the editor loads/saves the same formats losslessly (round-trip a
complex .tscn and diff).

## platform-os — OS/Time/Engine/DisplayServer, threads, crypto, debug
**Ground truth:** `core/os/`, `core/crypto/`, `servers/display_server*`,
`platform/web/` (Godot's own web platform layer — STUDY IT, it defines scope).
**Patina:** `gdplatform` (lib.rs, export.rs), `gdcore` (crypto.rs), `patina-runner`.
**Stones:**
- OS: env vars, cmdline args (+user args after `--`), feature tags (has_feature: web/
  wasm32/browser names/debug/release), locale, user data/config/cache dirs (virtual on
  web), delay, ticks msec/usec, is_debug_build, get_name "Web", processor count
  (hardwareConcurrency), memory info, alert(), shell_open (window.open), execute/
  process APIs → OOS-on-web citations with correct error returns.
- Time: unix time ↔ datetime dicts/ISO strings (+utc/dst handling), timezone info,
  date/time formatting.
- Engine: max_fps, time_scale, physics ticks/jitter fix, frame counters, version info,
  singleton registration (+script singletons), is_editor_hint.
- DisplayServer (web subset): canvas size/resize policy, fullscreen (+keyboard lock),
  screen DPI/scale (devicePixelRatio), orientation (+lock), cursors (css mapping +
  custom images), clipboard (async API + permission), virtual keyboard (mobile),
  IME composition, TTS (SpeechSynthesis: voices/speak/pause/stop/callbacks — Godot
  web HAS this), window focus/blur events, vsync via rAF.
- Threading: Thread/Mutex/Semaphore/WorkerThreadPool (group tasks, yielding waits) —
  and the web story: SharedArrayBuffer + COOP/COEP (does the editor server send the
  headers? does the runner have a no-threads fallback scheduler?).
- Crypto: Crypto (random bytes, RSA keys, self-signed certs, sign/verify, hmac),
  HashingContext (md5/sha1/sha256 streaming), AESContext (ECB/CBC + streaming),
  X509Certificate/CryptoKey save/load, constant-time compare.
- Expression class (parse/execute w/ inputs + base instance).
- Performance singleton: all built-in monitors (fps, process/physics times, static
  mem, object/node/orphan counts, render draw calls/video mem, physics active
  objects, audio latency) + custom monitors — these power the editor profiler.
- EngineDebugger + remote debug protocol over WebSocket (runner→editor), capture
  registration, profiler hooks.
- JavaScriptBridge: eval, get_interface, create_callback/object, download_buffer,
  force_fs_sync, pwa callbacks — REQUIRED surface for a browser engine.
- GDExtension/plugin story: classify against `THIRDPARTY_STRATEGY.md` (citation
  required either way).
**Wired means:** these singletons are script-callable in the runner (not just Rust
utilities), and the web implementations bind to real browser APIs.

## editor-ux — the full editor feature surface
**Ground truth:** `upstream/godot/editor/` (docks, plugins, gui); the editor-parity
skill's source-driven method (constructor enumeration) applies here at FEATURE level.
**Patina:** `gdeditor` (editor_ui.rs EDITOR_HTML, editor_server.rs routes, inspector.rs,
viewport_3d.rs), existing lanes in `prd/EDITOR_PARITY_BEADS.md` (do NOT read progress
docs as evidence — enumerate from Godot source, then verify Patina serves it).
**Stones (feature presence + wiring; pixel fidelity belongs to /editor-parity):**
- Main screens: 2D/3D/Script/AssetLib switching; multi-scene tabs w/ dirty markers +
  unsaved-changes prompts; distraction-free mode; layouts save/restore.
- Run bar: play/play-scene/play-custom/pause/stop, movie mode; the FULL edit→Play→
  browser-game loop with the edited scene (Patina's core product loop).
- Docks: Scene tree (context menu ops: add/instantiate/change-type/reparent/
  save-branch/editable-children/%unique/copy-path, filter incl. f:/g: syntax,
  multi-select, drag reorder, badges: script/warning/visibility/lock, remote tree
  during play + live edit), FileSystem (tree+list modes, favorites, path bar +
  history, search, context menu incl. move-with-remap/duplicate/reimport/new-*-here,
  drag-to-scene/inspector, import-fail badges), Inspector (per-type property editors
  ~60: ranges/easing/colors/node-paths/resources (quick-load/make-unique/save-as)/
  arrays+dicts (typed, per-element revert)/layers grid/flags/enums/curves/gradients/
  transforms, categories+groups collapse, filter, revert-per-property, pin, keying
  buttons w/ animation integration, doc tooltips), Node dock (signals: connect dialog
  (target picker, method create, binds/unbind, deferred/oneshot flags), connections
  tree, groups manager), Import dock (per-type options + presets + reimport), History
  dock (global undo/redo — and undo/redo depth across ALL editor ops).
- Bottom panels: Output (rich errors w/ links), Debugger (breakpoints, step/continue,
  stack + locals/members/globals inspection, expression evaluator, error trees,
  profiler (frame times, per-function), visual profiler, monitors graphs, video RAM,
  network profiler, remote scene tree), Find in Files (+replace), Audio buses (full
  mixing panel: faders, VU, solo/mute/bypass, effect chains, save/load layouts),
  Animation (track editor: all track types, keyframing + auto-key, bezier editor,
  box select/copy/paste, snap+fps mode, onion skinning, audio waveform preview,
  method param dialogs), AnimationTree (state machine graph w/ live playback pos,
  blend space editors, blend tree graph + filters), Shader Editor (text editor w/
  completion for built-ins, error squiggles per line), TileSet editor (atlas setup,
  per-tile physics/nav/occlusion/custom-data painting, terrains config), TileMap
  editor (paint/line/rect/bucket, terrains painting, patterns, layers, flip/rotate
  hotkeys), Theme editor (preview + pick-from-preview, per-item-type editing,
  import from other themes), SpriteFrames editor, Polygon2D UV editor.
- 2D viewport: rulers + draggable guides, smart snapping (grid/guides/anchors/sides/
  centers/other-nodes + relative + pixel + configure dialog), select/move/rotate/
  scale/list-select tools, IK preview?, skeleton options, frame-selected, zoom widget,
  pan modes, lock/group per node, onion skin (animation), preview stretch scale.
- 3D viewport: nav (orbit/freelook WASD+speed, pan/zoom), gizmos per node type (~30
  plugins: lights/camera/shapes/paths w/ editable handles/particles/navmesh/skeleton),
  transform gizmo modes + local/global + snap dialog + numeric transform dialog,
  view menu (1-4 viewports, ortho/persp, half-res, information/fps overlays, cinematic
  preview, lock rotation), sun/environment preview popup, camera preview PIP, align
  to/from view, selection outline, grid fade, origin lines.
- Dialogs: Create Node (favorites/recent/search/docs), Quick Open (unified 4.4),
  Project Settings (general w/ feature overrides, input map editor!, localization,
  autoload, globals, plugins), Editor Settings (all sections + shortcuts editor!),
  Export (presets, web export options), Attach Script (templates), Rename (batch w/
  regex/counters), Reparent, MeshInstance3D tools (create collision/navmesh/outline),
  Advanced Import Settings (scene tree remap, per-mesh/material/anim options),
  Command Palette, Manage Editor Features, screenshots.
- Script editor: tabs + members outline, find/replace (+in files, regex), goto line/
  symbol, bookmarks/breakpoints, folding, minimap, completion + signature hints +
  hover docs + ctrl-click nav, color picker inline, indent ops, syntax theming,
  script templates, tool-script run (Ctrl+Shift+X).
- Editor systems: undo/redo across every op, EditorSettings persistence, editor
  themes/scale/fonts, command palette, recent files, autosave-on-play, plugin/
  EditorPlugin + EditorImportPlugin surface (scope call w/ citation), asset library
  (scope call), version control hooks (scope call), movie maker, feature profiles.
**Wired means:** the feature works end-to-end in the SERVED editor: the HTML/JS in
EDITOR_HTML calls a real endpoint, the server mutates real engine state, the result
persists to .tscn and shows in the viewport. Server route + test but no client call =
UNWIRED (this exact gap shipped before — test `pub const EDITOR_HTML` directly).

## web-runtime — the wasm/browser target itself
**Ground truth:** `upstream/godot/platform/web/` (Godot's own web platform: js glue,
audio worklet, input, FS), Godot docs "Exporting for the Web" + web limitations.
**Patina:** `patina-runner`, `gdplatform/export.rs`, any wasm/web crates — find them.
**Stones:**
- wasm32 build story: target compiles (cfg-gated native deps: file dialogs, notify,
  threads, native sockets), wasm-bindgen/cdylib entry, clock abstraction, panic hook
  → console + on-screen overlay.
- Boot: canvas acquisition, WebGPU adapter/device async init + WebGL2 fallback path,
  loading progress UI, boot splash, focus-canvas-on-start, error display.
- Main loop: rAF driving process+physics accumulation (fixed timestep), visibility-
  hidden throttling policy, low-processor mode, max fps.
- Canvas/stretch: devicePixelRatio, resize policy (none/project/adaptive), fullscreen,
  content scale pipeline hooked to Window settings.
- Input on web: pointer events + pointer lock, keyboard code/key mapping (physical vs
  unicode), preventDefault policy, touch, gamepads (standard mapping + rumble), wheel
  deltaMode normalization, IME composition.
- Audio: AudioContext + AudioWorklet feed from mixer (SAB ring buffer when isolated,
  message-port fallback), sample-rate adaptation, autoplay-unlock on gesture, latency
  reporting into AudioServer.
- Assets: PCK-over-HTTP fetch (+progress, Range or full), threaded-load emulation
  without threads, user:// → IndexedDB/OPFS (+flush, force_fs_sync), .import remaps
  inside pck, web texture compression policy (ETC2/ASTC vs S3TC dual-pack decision).
- Networking on web: WebSocket, WebRTC glue, fetch-based HTTP, correct OOS errors for
  UDP/TCP/ENet.
- Threads: COOP/COEP cross-origin isolation (editor dev-server sends headers?),
  wasm-threads WorkerThreadPool vs single-thread fallback.
- Export pipeline (gdplatform/export.rs): html shell template (canvas + progress +
  error overlay), head include, PWA (service worker, offline, icons), COOP/COEP
  toggle, thread toggle, VRAM compression choice, custom shell; embedding API surface
  (engine.js-equivalent: start config, preloadFile, onProgress/onExit).
- JavaScriptBridge (cross-check platform-os), OS feature tags ("web", browser names),
  console routing (print→console.log, push_error→console.error + editor capture).
- The editor's own Play-in-browser flow: how a scene edited in Patina's editor becomes
  a running game (served runner + scene) — THE product loop, audit end-to-end.
**Wired means:** a real project boots in a real browser tab. Static analysis: verify
the target exists, entry points are reachable, and each browser API has a binding —
then bead build/run verification as the acceptance test (workers/verifier run builds,
not you).

## xr-peripheral — XR, camera, fringe class surface
**Ground truth:** `servers/xr*`, `modules/webxr/`, `servers/camera*`, api json.
**Patina:** likely absent — verify.
**Stones:**
- WebXR: Godot's web export SUPPORTS WebXR (immersive-vr/ar sessions, reference
  spaces, input sources/select events, framebuffer scaling) — it is IN scope for a
  browser port unless a prd/ doc says otherwise (cite or bead, likely P2/P3).
- XR class surface: XRServer/XRInterface(+Extension), XROrigin3D/XRCamera3D/
  XRController3D/XRNode3D/XRAnchor3D, XRPose/XRPositionalTracker, hand/body/face
  trackers (4.3+).
- OpenXR* (~40 classes) + MobileVRInterface: native-only → OUT_OF_SCOPE with the
  web-limitations citation, but registration-with-correct-error surface is a P3 call.
- CameraServer/CameraFeed (getUserMedia — scope call), MovieWriter (headless — scope
  call), fringe leftovers the sweep assigns here.
**Wired means:** if in scope: an XR session request reaches the browser WebXR API from
a scene with XROrigin3D; otherwise: citations recorded, correct runtime errors.
