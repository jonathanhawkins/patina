# Fresh-Eyes Parity Audit — 2026-07-11

**Method**: 15 parallel read-only auditors compared `engine-rs/crates/*` against the vendored
Godot 4.6.1 source (`upstream/godot`), reading function bodies — not symbol names, ClassDB
registrations, or documentation. All prior docs/reports were deliberately ignored.

**Tracker**: epic `pat-u3l8c` — 117 beads (41 P0 / 65 P1 / 11 P2), deduped against epics
`pat-p0s82` and `pat-z35r2`, with blocking dependencies.

---

## The systemic finding: real code, never wired

`MainLoop::step` (gdscene/src/main_loop.rs:143) drives exactly **one** behavior server: the 2D
`PhysicsServer`. Everything else is standalone, unit-tested library code with **zero runtime
call sites**:

| Subsystem | Library state | Runtime state |
|---|---|---|
| 3D physics (`gdphysics3d`, `physics_server_3d.rs`) | tested | never stepped — RigidBody3D never falls |
| Audio (`gdaudio`) | real cpal backend, DSP tested | **no crate even depends on gdaudio** |
| Navigation (`navigation.rs`, 1214 lines) | A* tested | never referenced by main_loop/runner |
| Particles (`particle.rs`, `particle3d.rs`) | sims tested | `step()` called only in tests |
| AnimationTree (`animation_tree.rs`) | blend math tested | never applied to nodes |
| GUI layout (`resolve_control_layout`, `arrange_container`) | math correct | called only from tests |
| PathFollow (`path3d.rs`) | sampling tested | never moves a node |

**Consequence for methodology**: "N unit tests passing" is not parity evidence. A subsystem
counts as working only when an app-level test exercises it through `MainLoop::step` /
`patina-runner` on a `.tscn` scene. When auditing any claim, grep for call sites from
`main_loop.rs` / `scene_tree.rs` first.

---

## Per-subsystem verdicts

### Browser / web platform — ~0%, structurally absent (stated project goal)
- No wasm target anywhere: zero `wasm-bindgen`/`web-sys` deps, no `cdylib`, no wasm entry.
- `gdplatform/src/web.rs` is a self-described simulation stub (JsBridge → HashMap).
- What the browser sees today is **BMP frames streamed from a native server to an `<img>` tag**
  (`frame_server.rs`); game logic runs natively on the host.
- Native-only blockers everywhere: cpal audio, `pollster::block_on` around wgpu (forbidden on
  wasm), native winit, `std::net`, `std::time::Instant`, filesystem-only asset loading.
- Beads: `pat-u3l8c.109–.115` (wasm crate, canvas wgpu surface, rAF+input, AudioWorklet,
  fetch VFS, export pipeline, JS bridge).

### GDScript — solid procedural core, P0 grammar holes
Works: control flow, funcs/lambdas, await/coroutines (real suspension), Godot-3 signals,
classes/inner classes, `$Node` access, ~30 builtins.
Broken/missing: `const` (parse error), compound assigns beyond `+= -=`, `assert`, `is`, `as`,
match arms bind instead of comparing constants, `preload()` returns a fake, method surface
~10–15% of Godot (String 10/150, Array 10/40), Packed*Array types absent, indexed assignment
only on bare variables, deterministic RNG seeded 42. Beads: `.7–.22`.

### 2D rendering — editor-mockup maturity
Scene nodes never emit draw commands (the icon renderer is what Play shows); rotated sprites
rasterize as axis-aligned AABBs; alpha ignored in the main path; TileMap renders as a 2px dot;
WgpuRenderer2D hardcodes the software backend. Beads: `.23–.31`.

### 3D rendering — no textures, no skinning
Material texture slots exist but are **never sampled in either path** (every textured mesh is
flat-colored); no skeletal skinning reaches the renderer; no transparency modes; Godot-default
dual-paraboloid omni shadows produce no shadow; GPU path lacks fog and omni shadows entirely;
SSAO is implemented but unwired; GI (VoxelGI/SDFGI/LightmapGI) is stub-level.
Solid: PBR math, dir/spot hard shadows, sky materials, AgX/filmic/ACES tonemap, glow.
Beads: `.32–.42`, plus `pat-p0s82.8/.10/.25/.26`.

### Shaders — custom .gdshader is non-functional end to end
Not a loadable resource; ShaderMaterial never parsed from scenes; shader bodies discarded
(only ~6 magic uniform names); generated WGSL has zero consumers; vertex stage always standard;
`TIME` hardcoded 0.0; the 2D canvas_item interpreter exists but is never called by any render
path. Beads: `.43–.45` + `pat-p0s82.21`.

### 2D physics — toy engine missing platformer basics
**No gravity at all**; narrowphase ignores rotation (rect-rect is AABB); only 3 shape pairs
(capsule/segment collide as `None`); no floor snap; no moving platforms; `move_and_slide`
velocity-semantics bug collapses speed to ~v·dt per frame; no space-state queries; RayCast2D
unwired; O(n²) broadphase. Solid: layer/mask filtering, Area2D body signals, contact tracking.
Beads: `.46–.53`.

### 3D physics — API shells over ~3 collision pairs
Never stepped at runtime (see systemic finding); capsule and world-boundary have **no**
collision; boxes collide as world-axis AABBs; layers/masks ignored in the pair loop; joints
are inert data; convex/trimesh shapes load with empty geometry; `remove_body` is a no-op.
Beads: `.54–.60` + `pat-p0s82.18/.27`.

### Audio — orphaned island
Correct-in-isolation DSP (3/27 effects), WAV/OGG decode, real cpal output — but no crate
depends on `gdaudio`, nothing pumps `mix()`, the player nodes are ClassDB metadata, spatial
math results are never consumed, and there is no resampling (wrong pitch for non-44.1k).
Beads: `.2`, `.104–.108` + `pat-p0s82.12/.13`, `pat-z35r2.5`.

### Animation — good math, broken pipeline
Tracks only animate the AnimationPlayer's own node (node_path never read); no 3D transform
track types; no quaternion slerp; AnimationTree/blend spaces dead at runtime; no skinning
consumer; no glTF animation import; no player signals/autoplay/seek.
Solid: keyframe/easing/tween math, state-machine crossfade math. Beads: `.94–.103`.

### Navigation — node shells over centroid-hop A*
AStar2D/AStar3D/AStarGrid2D **do not exist**; paths hop polygon centroids (no funnel);
single-region search only; obstacle on path ⇒ empty result; no server RID API; no agent
signals; bake is a slope filter (no voxelization); no 2D bake at all. Beads: `.61–.66` +
`pat-p0s82.14`.

### GUI — no runtime GUI system
0 of ~55 controls functional (19 registered: 10 data-only, 9 shells); no render-from-tree, no
input dispatch, no control signals, no StyleBox classes, no container sort at runtime; text is
a hardcoded 5×7 ASCII bitmap — no TTF parsing, shaping, or wrapping. Beads: `.67–.74` +
`pat-p0s82.20`, `pat-z35r2.17`.

### Networking — good plumbing, zero Godot compat, nothing secure
Real TCP/UDP/HTTP-plaintext/WebSocket-client/SHA-256/AES-CBC. But: multiplayer is not wired
into SceneTree or GDScript (`@rpc` absent); ENet framing and RPC bytes are proprietary (a
Patina client cannot talk to a Godot server); Variant wire encoding covers 5 types; **no TLS
anywhere** (no HTTPS, no wss); Spawner/Synchronizer are inert data structs. Beads: `.75–.82`
+ `pat-p0s82.22`.

### Resources — text formats good; real Godot exports unloadable
Solid: `.tscn`/`.tres` text subset, project.godot, PNG/JPEG/WAV/OGG, glTF meshes+PBR factors.
P0 wall: binary `.res`/`.scn` loader is a stub; PCK reader layout mismatches Godot (desyncs on
every real pack); no `.ctex` reader or `.import` remap; `Object(...)` values silently dropped;
UID codec uses the wrong algorithm. glTF lacks skins/animations/textures (+ matrix-rotation
bug). Beads: `.83–.93`.

### Core — strong object model, script surface gaps
Solid: Variant common types, Callable model, signal backend, Object/ClassDB (Rust-side),
input state machine, thread backends. Gaps: Packed arrays (P0), int-vector/RID variants,
Signal-as-value, `connect` flags, RNG class, and many working backends (RegEx, ProjectSettings,
Thread, ClassDB) never exposed to scripts. Beads: `.11`, `.16–.22`.

---

## What is genuinely solid (keep building on it)

Node lifecycle (groups/owner/process modes/notification order), 2D physics wiring skeleton +
deterministic stepping, Camera3D projection/ray math, tween/keyframe/easing math, sky +
tonemap (incl. AgX) + glow pyramid, `.tscn`/`.tres` text parsing, oracle capture pipeline vs
Godot 4.6.1, HTTP/WS wire implementations, Callable/Signal/Object backend, editor tooling.

## Recommended attack order

1. **Wiring** (`.1–.3`): step PhysicsServer3D, wire audio, Timer node — turns dead code live.
2. **2D game viability** (`.23/.24/.46–.48`): node→canvas rendering, gravity, character basics.
3. **GDScript P0s** (`.7–.11`): const/assert/is/match/math/packed arrays.
4. **Real-project loading** (`.76/.83–.87`): binary codec, PCK, .ctex, UID.
5. **Browser pillar** (`.109` then `.110–.113`): wasm target, then render/input/audio/VFS.
