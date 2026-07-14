# Patina ↔ Godot 4.6.1 — 1:1 Port Scorecard (post-P0)

**Date**: 2026-07-11  
**Oracle**: `upstream/godot` 4.6.1-stable  
**Method**: Code review of `engine-rs/` vs Godot `servers/`, `scene/`, `modules/`  
**ClassDB**: ~113 registered runtime classes vs ~912 Godot class docs  

## Bottom line

| Question | Answer |
|----------|--------|
| Can we claim a **completed 1:1 Godot port**? | **No** |
| Is Patina a **real, useful Godot-compatible runtime slice**? | **Yes** — stronger than mid-project stubs |
| Rough completion for “ship a typical 2D Godot game” | **~55–65%** of what that needs |
| Rough completion for “drop any Godot 4.6 project and run” | **~25–35%** |
| Rough completion for “editor + all platforms + C# + XR” | **~15–20%** |

P0 work closed this week moved several “hard blockers” from **stub → usable slice**. That is necessary but far from sufficient for 1:1.

---

## How to read the scores

Each major system is scored **0–5** against **behavioral 1:1 with Godot 4.6.1**, not lines of code:

| Score | Meaning |
|------:|---------|
| 0 | Absent |
| 1 | Name/ClassDB only or pure stub |
| 2 | Partial API, wrong or missing behavior for real projects |
| 3 | Vertical slice works; major Godot features missing |
| 4 | Broad parity for common projects; edge cases / quality gaps |
| 5 | Honest 1:1 for the subsystem’s public contract |

---

## System-by-system

### 1. Core object model (`gdobject`, `gdvariant`, `gdcore`) — **4 / 5**

**Have**: Variant types, Object/RefCounted, signals, ClassDB registration, properties, notifications, IDs, math, file access, project settings surface.

**Gaps**: AES crypto stub; not every Godot core singleton; callables/lambdas depth; property hint/usage full metadata.

**1:1 blocker?** Low for most games.

---

### 2. Scene tree & lifecycle (`gdscene`) — **3.5 / 5**

**Have**: Node hierarchy, enter/ready/process, instancing, PackedScene text path, SubViewport buffer (partial), call_deferred, large unit/integration surface.

**Gaps**: SubViewport does not re-render full 2D/3D pipelines every frame like Godot; many node types ClassDB-only; Path3D/GridMap/NavigationLink missing.

**1:1 blocker?** Medium — scene load fails or no-ops for common nodes.

---

### 3. GDScript (`gdscript-interop`) — **3 / 5**

**Have**: Large tokenizer/parser/interpreter; builtins; class scripts; Input hooks; **real await coroutines** (timer/signal/frames) post-P0.

**Gaps**: `await get_tree().create_timer().timeout` Godot idiom incomplete (have free `create_timer` awaitable, not SceneTreeTimer); typed GDScript / analyzer; full match/lambda/preload fidelity; C#/GDExtension not a replacement.

**1:1 blocker?** High for modern scripts until SceneTree timer + more stdlib match Godot.

---

### 4. Resources & import (`gdresource`) — **2.5 / 5**

**Have**: `.tres`/`.tscn` loaders, cache, UID, **glTF/GLB mesh import** (post-P0), audio import metadata, PCK partial.

**Gaps**: **PNG still header-only** (no pixels in importers.rs); glTF no skins/animation/textures full; **FBX absent**; font import thin; binary `.res` depth.

**1:1 blocker?** **Critical** — most projects need textures + models.

---

### 5. Audio (`gdaudio`) — **2.5 / 5** (was ~1)

**Have (post-P0)**: WAV/OGG decode, buses, mixer, spatial math, **cpal platform output**, **Amplify/Reverb/EQ chain**, capture/null sinks.

**Gaps**: Only 3 of ~20 Godot effects; no bus **sends**; no MP3/generator/polyphonic/mic; no ClassDB `AudioEffect*` resources; no latency/device enum parity; COMPAT_MATRIX still stale (“no real output”).

**1:1 blocker?** Medium for SFX-heavy games; high for music production graphs.

---

### 6. Multiplayer / networking (`gdplatform`) — **2.5 / 5** (was ~1.5)

**Have (post-P0)**: **Real UDP** peer with reliable/unreliable/ordered; binary **PRPC** framing; spawner/sync slices; TCP/WebSocket/HTTP/UDP packet peers.

**Gaps**: Not stock **ENet** wire (Godot clients won’t interop); not full SceneMultiplayer protocol; no WebRTC; no auth/visibility/delta filters full; mesh topology incomplete.

**1:1 blocker?** High if target is “multiplayer with Godot peers”; medium for Patina↔Patina LAN.

---

### 7. 2D rendering (`gdrender2d` / `gdserver2d`) — **3 / 5**

**Have**: Software canvas, sprites, draw cmds, goldens, fonts (bitmap), **2D light module** (point/directional + occluders) post-P0.

**Gaps**: Lights **not wired into SoftwareRenderer main loop** (library + tests, not automatic scene lights); no full canvas light shader parity; limited materials/shaders.

**1:1 blocker?** Medium for lit 2D games until lights are in the frame path.

---

### 8. 3D rendering (`gdrender3d` / `gdserver3d`) — **2.5 / 5**

**Have**: Meshes, lights, single-cascade-style shadows (directional/spot/omni maps), sky, glow/tonemap, reflection probe sampling helpers, environment props, CSG/particles **server-side** slices.

**Gaps**: **VoxelGI/LightmapGI stubs** (volume color); no SDFGI; no PSSM cascades; no SSAO/SSR; decals/fog volumes weak; custom shader→GPU incomplete; no clustered forward fidelity.

**1:1 blocker?** **High** for any modern 3D look.

---

### 9. Physics 2D (`gdphysics2d`) — **3.5 / 5**

**Have**: Bodies, shapes, character, joints, areas, deterministic goldens, tilemap collision.

**Gaps**: Capsule/segment raycast incomplete (explicit comment); not every joint/param.

**1:1 blocker?** Low–medium for platformers; medium for complex queries.

---

### 10. Physics 3D (`gdphysics3d` + softbody in gdphysics2d) — **2.5 / 5**

**Have**: Rigid/static/character, shapes, joints, areas, world step; Vehicle/SoftBody **types** registered.

**Gaps**: SoftBody not fully in 3D server step; capsule approx; **no Jolt** (Godot 4.6 path); continuous collision / sleeping fidelity unknown.

**1:1 blocker?** High for softbody/vehicle/Jolt projects.

---

### 11. Navigation — **2 / 5**

**Have**: Nav mesh data, A*, agents, regions, bake helpers, ClassDB agents.

**Gaps**: `avoidance_enabled` flag without RVO; no NavigationLink path edges; not full NavigationServer API.

**1:1 blocker?** High for AI pathing games.

---

### 12. Animation — **2.5 / 5**

**Have**: AnimationPlayer, AnimationTree partial (blend spaces, state machine, oneshot), Skeleton3D.

**Gaps**: Full AnimationMixer, root motion, method tracks, blend tree node completeness, retargeting.

**1:1 blocker?** Medium–high for character animation.

---

### 13. GUI / Control — **1.5 / 5**

**Have**: Button, Label, LineEdit, Panel, TextEdit, HBox/VBox, ColorRect, containers subset.

**Gaps**: No RichTextLabel, Tree, ItemList, TabContainer, ScrollContainer, GraphEdit, FileDialog, etc. (~58 Godot gui headers vs handful).

**1:1 blocker?** **Critical** for UI-heavy games and tools.

---

### 14. Platform / export / OS — **2.5 / 5**

**Have**: Desktop windowing (winit path), input maps, timers, export config tests, multi-OS metadata.

**Gaps**: Windows/mobile/web not parity; export **templates** ≠ Godot export; signing partial.

**1:1 blocker?** High for shipping non-dev platforms.

---

### 15. XR — **0 / 5**

Absent (XRServer / OpenXR).

---

### 16. Text / fonts — **1.5 / 5**

Bitmap/simple path; no TextServer advanced shaping (BiDi, complex scripts).

---

### 17. Editor (`gdeditor`) — **2 / 5** (separate product)

Large browser editor shell; not Godot editor 1:1. Feature gate was lifted for maintenance/features but runtime still limits what the editor can honestly edit/run.

---

### 18. C# / GDExtension plugins — **0.5 / 5**

No mono; GDExtension lab may exist under `apps/godot` but not full host parity in engine-rs.

---

## Weighted “how close” picture

Assuming goal = **run common Godot 4.x games (2D first, some 3D)**:

| Layer | Weight | Score | Weighted |
|-------|-------:|------:|---------:|
| Core + scene | 15% | 3.7 | 0.56 |
| GDScript | 15% | 3.0 | 0.45 |
| Import assets | 15% | 2.5 | 0.38 |
| 2D render+physics | 15% | 3.2 | 0.48 |
| 3D render+physics | 15% | 2.5 | 0.38 |
| Audio | 5% | 2.5 | 0.13 |
| Multiplayer | 5% | 2.5 | 0.13 |
| GUI | 10% | 1.5 | 0.15 |
| Nav/anim/misc | 5% | 2.2 | 0.11 |
| **Total** | 100% | | **~2.8 / 5 ≈ 55%** |

For **strict 1:1 including 3D look + UI + multiplayer interop + platforms**: closer to **~30%**.

---

## What P0 changed (honesty)

| Before | After |
|--------|--------|
| glTF NotFound stub | Mesh/material/node import works for basic GLB |
| Silent NullAudio only | cpal device path + capture tests |
| await = sync warning | Real suspend/resume |
| ENet = TCP alias | Real UDP + transfer modes |
| RPC = `rpc:name:argc` text | Binary PRPC + spawn/sync pipeline |
| 2D lights ClassDB only | Shade + occluder module (+ ClassDB) |
| No bus FX | Amplify/Reverb/EQ chain |

None of that equals “port complete.”

---

## Already-tracked beads (do not re-create)

Epic **pat-p0s82** still has open **P1/P2** children for cascades, GI, SSAO, bus sends, MP3, nav, Path3D, GridMap, SubViewport, SoftBody, PNG/font, GUI, shaders, WebRTC, XR, FBX, projectors, Decal/Fog, Jolt, video, ClassDB inventory, docs matrix.

See `br list --status open` for live IDs.

---

## Recommended claim language

> Patina is a Rust-native Godot-compatible runtime with a **measured core + 2D/3D vertical slice**, recently upgraded platform audio, glTF mesh import, await, UDP multiplayer, and 2D lighting math. It is **not** a finished 1:1 port of Godot 4.6.1: asset import depth, GUI, GI/advanced lighting, navigation avoidance, and cross-engine multiplayer interop remain major gaps.
