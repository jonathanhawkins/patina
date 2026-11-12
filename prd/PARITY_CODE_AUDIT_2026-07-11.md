# Patina vs Godot 4.6.1 — Code-Level Parity Audit

**Date**: 2026-07-11  
**Oracle**: `upstream/godot` (version.py: 4.6.1 stable)  
**Patina**: `engine-rs/` crates  
**Method**: Source comparison (not docs-only). ClassDB registration and unit-test volume do **not** imply Godot behavioral completeness.

**Epic**: Full Godot 4.6.1 runtime port parity (code audit 2026-07-11) — label `parity-audit`

---

## Executive answer

**No — Patina cannot honestly claim a fully completed Godot 4.6.1 runtime port today.**

Strengths are real: core object/scene model, large GDScript interop surface, 2D/3D vertical slices with goldens, physics worlds, glow/tonemap, multi-light GPU shadows (single cascade), TCP multiplayer scaffolding, editor shell.

Critical production subsystems remain **stubs, thin wrappers, or header-only**:

| Area | Patina reality | Godot reality |
|------|----------------|---------------|
| Audio output | `NullAudioOutput` only | CoreAudio / WASAPI / ALSA / Pulse |
| Audio effects | None | 17+ `AudioEffect*` |
| ENet multiplayer | TCP alias (`enet_multiplayer.rs`) | Real ENet UDP + channels |
| Scene multiplayer | String RPC + bounded spawner/sync | Full scene_rpc + replication interfaces |
| glTF | `import_gltf` returns NotFound | Full `modules/gltf` |
| VoxelGI / SDFGI | Volume-wide color bake stub | Voxel cone tracing + SDFGI |
| Cascade shadows | Single directional map | PSSM splits |
| SSAO/SSR | Not in postprocess | `ss_effects` |
| 2D lights | Class defaults only | Canvas lights + occluders |
| Navigation avoidance | Bool flag | RVO |
| NavigationLink | Editor counter only | Full links |
| Path3D / Curve3D | Missing | Full path sampling |
| GridMap | Plugin name only | Full module |
| XR | Absent | OpenXR / XRServer |
| GUI | ~7 controls | 50+ `scene/gui` types |
| `await` | Sync warning | Real coroutines |
| ClassDB | ~109 classes | ~912 class docs |

---

## Subsystem findings

### Audio (`gdaudio` ≈ 3k LOC)

**Present**: Bus volume/mute/solo, mixer, playback state machine, WAV + OGG decode (`lewton`/`hound`), spatial attenuation math for AudioStreamPlayer3D, AudioServer mix-to-buffer, import metadata.

**Missing vs Godot** (`servers/audio`, `drivers/*audio*`, `modules/mp3|vorbis`):

1. Real device output (only `NullAudioOutput`)
2. Entire effect stack (`servers/audio/effects/*`)
3. Bus send routing
4. MP3, generator, polyphonic, microphone, interactive music
5. Driver latency / device enumeration

**Docs**: `AUDIO_MILESTONE.md` still lists real output as deferred; COMPAT_MATRIX correctly marks Audio **Claimed**.

### Multiplayer (`gdplatform` network + peers)

**Present**: `NetworkPeer` trait, TCP multiplayer peer, PacketPeerUDP, StreamPeerTCP, WebSocketPeer, HTTPClient/Request, MultiplayerAPI with register_rpc, MultiplayerSpawner/Synchronizer **bounded slices**, ClassDB names including ENetMultiplayerPeer.

**Missing vs Godot** (`modules/multiplayer`, `modules/enet`, `modules/webrtc`):

1. Real ENet (explicitly deferred in file header — TCP always reliable)
2. Godot SceneMultiplayer binary protocol (Patina RPC is `rpc:method:argc` text)
3. SceneReplicationConfig, visibility filters, delta sync, auth handshake
4. WebRTCMultiplayerPeer
5. Mesh peer / bandwidth limits

### Lighting & environment (`gdserver3d` + `gdrender3d`)

**Present**: Directional / Omni / Spot light types; soft of hard shadow maps for directional + spot + omni cubemaps; multi-light GPU packing; environment tonemap (incl. AgX fields); glow bloom CPU postprocess; sky; reflection probe **sampling helpers**; light/GI **property containers**.

**Missing / stub**:

1. **VoxelGI / LightmapGI**: documented stubs; bake collapses to one `baked_indirect` color (`gi.rs`)
2. **SDFGI**: absent
3. **Directional cascades / PSSM**: single map
4. **SSAO / SSIL / SSR**: postprocess is glow + tonemap only
5. **Light projectors**, soft shadow quality modes
6. **Decal / FogVolume** render contribution largely absent despite ClassDB

### Import (`gdresource`)

1. `import_gltf` — hard stub error
2. PNG importer — **header only**, no pixel decode
3. Font importer — stub-level
4. No FBX (Godot `modules/fbx`)

This alone blocks “drop a Godot project and run it” for almost any 3D title.

### 2D lighting

Class defaults for Light2D / PointLight2D / DirectionalLight2D; **no** lighting path under `gdrender2d` / `gdserver2d`. Godot canvas lights + occluders are a major 2D feature.

### Navigation

A* 2D/3D, regions, agents, obstacle **storage**, nav mesh bake helpers.  
`avoidance_enabled` does not implement RVO. NavigationLink is not a runtime path edge.

### Paths / GridMap / XR

- Path2D ClassDB only; no Path3D/PathFollow3D/Curve3D evaluation
- GridMap: editor plugin string only
- XR: zero engine code

### Physics

Substantial 2D/3D worlds, joints, character, vehicle/softbody **types**. Gaps: SoftBody not clearly integrated in gdphysics3d step; capsule raycast incomplete; no Jolt (Godot 4.6 first-class).

### GDScript

Large interop surface. **`await` is not a real coroutine** (synchronous + warning). Blocks modern Godot scripts.

### GUI

Tiny Control subset vs full `scene/gui`. UI-heavy games and editor-adjacent tools will fail scene load / layout.

### Shaders

Tokenizers and partial compilers exist; not a full Godot Shader Language → WGSL/SPIR-V pipeline for arbitrary project shaders.

### ClassDB scale

~109 registered runtime classes vs ~912 Godot class documentation files. Many Godot classes are editor-only; still, high-use runtime nodes are absent (links, lights 2D, GridMap, XR, audio effects, rich GUI).

---

## What *is* strong (do not understate)

- Scene tree lifecycle, signals, resources (.tres/.tscn), runner, oracle property tests for fixture corpus
- 2D render/physics vertical slice with goldens
- 3D software + wgpu paths with real multi-light shadows (recent work)
- AnimationPlayer / AnimationTree partial graph
- Particles/CSG server-side simulation (quality varies; CSG booleans simplified)
- Editor browser shell large surface (separate from runtime port completeness)

---

## Bead policy from this audit

- **P0**: blocks “real Godot project” claims (audio out, ENet, multiplayer protocol, glTF, 2D lights, await)
- **P1**: major visual/gameplay parity (cascades, GI, SSAO, nav avoidance/links, paths, GridMap, SubViewport full render, softbody, real image import, GUI core, shaders)
- **P2**: WebRTC, XR, FBX, projectors, decals/fog, Jolt, video, ClassDB inventory, doc reconciliation

Do **not** close these beads with ClassDB registration alone. Acceptance requires observable behavior + tests.

---

## Recommended honest public statement

> Patina is a Rust-native Godot-compatible runtime with a measured 2D/3D core slice and a large editor shell. Full Godot 4.6.1 subsystem parity is **not** complete: audio output, real ENet multiplayer, glTF import, GI, 2D lighting, and several gameplay systems remain partial or stubbed. See epic `parity-audit` and this document.

---

## Files checked (non-exhaustive)

| Path | Role |
|------|------|
| `engine-rs/crates/gdaudio/**` | Audio stub/output |
| `engine-rs/crates/gdplatform/src/enet_multiplayer.rs` | ENet=TCP |
| `engine-rs/crates/gdplatform/src/network.rs` | RPC/spawner/sync |
| `engine-rs/crates/gdserver3d/src/gi.rs` | GI stubs |
| `engine-rs/crates/gdrender3d/src/{shadow_map,postprocess,wgpu_pipeline}.rs` | Shadows/post |
| `engine-rs/crates/gdresource/src/{lib,importers}.rs` | glTF/PNG stubs |
| `engine-rs/crates/gdscene/src/{navigation,sub_viewport}.rs` | Nav / SubViewport |
| `engine-rs/crates/gdobject/src/class_db.rs` | Class registry |
| `upstream/godot/servers/**` | Oracle servers |
| `upstream/godot/modules/{multiplayer,enet,gltf,navigation_*,jolt_physics,mp3,webrtc,openxr,gridmap}` | Oracle modules |
| `upstream/godot/scene/{2d,3d,gui,audio,animation}` | Oracle scene |
