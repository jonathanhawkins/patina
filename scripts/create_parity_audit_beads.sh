#!/usr/bin/env bash
# Create beads from 2026-07-11 code-level Patina vs Godot parity audit.
set -euo pipefail
cd "$(dirname "$0")/.."

EPIC=$(br create --silent -t epic -p 1 -l "parity-audit,port-completion" \
  --title "Full Godot 4.6.1 runtime port parity (code audit 2026-07-11)" \
  --description "Code-level audit of engine-rs against upstream/godot 4.6.1-stable. Verified stubs: NullAudioOutput-only audio; ENet=TCP wrapper; VoxelGI volume-wide color stub; import_gltf NotFound; no 2D light render; await is synchronous; ~109 ClassDB vs ~912 Godot classes. Close when P0/P1 children prove observable Godot-compatible behavior, not ClassDB names alone. See prd/PARITY_CODE_AUDIT_2026-07-11.md.")
echo "EPIC=$EPIC"

create() {
  local prio="$1" labels="$2" title="$3" desc="$4"
  local id
  id=$(br create --silent -t feature -p "$prio" -l "$labels" --parent "$EPIC" \
    --title "$title" --description "$desc")
  echo "$id  $title"
}

# P0
create 0 "parity-audit,audio,p0" \
  "Audio: real platform output backend (CoreAudio/WASAPI/ALSA or cpal)" \
  "EVIDENCE: gdaudio/src/sample.rs only NullAudioOutput; AUDIO_MILESTONE deferred real output; Godot drivers/coreaudio|wasapi|alsa. GAP: silent games. ACCEPT: real backend from AudioServer mix; macOS+Linux or one primary; NullAudioOutput for CI; test play WAV -> N frames."

create 0 "parity-audit,audio,p0" \
  "Audio: bus effect chain (reverb, EQ, compress, etc.)" \
  "EVIDENCE: bus.rs/mixer.rs volume/mute/solo only; Godot servers/audio/effects has 17+ AudioEffect*. ACCEPT: ordered effect inserts; Amplify+Reverb+EQ process buffers; ClassDB/resource types; deterministic unit tests."

create 0 "parity-audit,multiplayer,p0" \
  "Multiplayer: real ENet UDP peer (not TCP alias)" \
  "EVIDENCE: gdplatform/src/enet_multiplayer.rs thin TcpMultiplayerPeer wrapper; Unreliable is metadata-only. Godot modules/enet real UDP. ACCEPT: native ENet/UDP; TransferMode affects delivery; multi-client poll; localhost UDP integration test."

create 0 "parity-audit,multiplayer,p0" \
  "Multiplayer: SceneMultiplayer RPC + replication protocol parity" \
  "EVIDENCE: MultiplayerAPI::rpc uses format!(rpc:method:argc) string; Spawner/Sync bounded slices; no SceneReplicationConfig/visibility/auth. Godot scene_multiplayer + scene_rpc/replication interfaces. ACCEPT: Variant-compatible framing or documented wire; sync intervals/root_path; spawnable scenes+spawn_function; two-tree sync test."

create 0 "parity-audit,import,p0" \
  "Import: implement glTF 2.0 mesh/scene load (import_gltf is stub)" \
  "EVIDENCE: gdresource/src/lib.rs import_gltf returns NotFound stub. Godot modules/gltf. ACCEPT: glTF/GLB -> mesh hierarchy+materials+transforms; fixture .glb; unit tests; document unsupported extensions."

create 0 "parity-audit,render2d,p0" \
  "2D: Light2D / PointLight2D / DirectionalLight2D + LightOccluder2D render path" \
  "EVIDENCE: class defaults only in patina-runner; no Light2D path in gdrender2d/gdserver2d. Godot scene/2d/light_2d + light_occluder_2d. ACCEPT: Point+Directional affect canvas; occluder shadows (or partial+docs); light_mask; golden/analytic test."

create 0 "parity-audit,gdscript,p0" \
  "GDScript: real await/coroutine suspension (currently sync warning)" \
  "EVIDENCE: interpreter warns await evaluated synchronously; parser has await. ACCEPT: await timer/signal suspends across frames; concurrent coroutines; regression tests."

# P1
create 1 "parity-audit,lighting,p1" \
  "Lighting: directional cascade/PSSM shadow splits" \
  "EVIDENCE: shadow_map.rs single directional map; no PARAM_SHADOW_SPLIT_*. ACCEPT: 2-4 cascades; Godot split params; near+far casters both shadowed."

create 1 "parity-audit,lighting,p1" \
  "GI: real VoxelGI / SDFGI contribution (not volume-wide color stub)" \
  "EVIDENCE: gdserver3d/src/gi.rs stub; bake stores single baked_indirect Color; no SDFGI in gdrender3d. ACCEPT: spatial field sampled per position; interior mode; two positions different irradiance test."

create 1 "parity-audit,lighting,p1" \
  "GI: LightmapGI bake + sample path" \
  "EVIDENCE: LightmapGI config stub in gi.rs. ACCEPT: bake lightmap UVs; runtime sample; occluder darkens opposite side fixture."

create 1 "parity-audit,lighting,p1" \
  "Postprocess: SSAO and/or SSR screen-space effects" \
  "EVIDENCE: postprocess.rs glow+tonemap only. Godot ss_effects. ACCEPT: SSAO on depth; optional SSR; env toggles; geometry test."

create 1 "parity-audit,audio,p1" \
  "Audio: bus send routing topology" \
  "EVIDENCE: flat bus list no sends. Godot bus send graph. ACCEPT: A->B send volume; solo/mute with sends; routing matrix tests."

create 1 "parity-audit,audio,p1" \
  "Audio: MP3 + AudioStreamGenerator / polyphonic playback" \
  "EVIDENCE: decode WAV+OGG only. Godot mp3 module + generator/polyphonic. ACCEPT: MP3 decode; generator PCM push; poly voice limit; tests."

create 1 "parity-audit,navigation,p1" \
  "Navigation: RVO avoidance + NavigationLink2D/3D" \
  "EVIDENCE: avoidance_enabled bool only; NavigationLink editor counter only. ACCEPT: agents avoid; links connect disjoint nav; tests."

create 1 "parity-audit,scene,p1" \
  "Scene: Path3D / PathFollow3D / Curve3D (and Curve2D runtime)" \
  "EVIDENCE: Path2D ClassDB only; no Path3D/Curve3D runtime. ACCEPT: sample_baked/closest; PathFollow rotation modes; ClassDB props; unit tests."

create 1 "parity-audit,scene,p1" \
  "Scene: GridMap runtime (mesh library + cell map)" \
  "EVIDENCE: GridMap only plugin_manager name. Godot modules/gridmap. ACCEPT: MeshLibrary set_cell_item; tscn load; tests."

create 1 "parity-audit,viewport,p1" \
  "SubViewport: render full 2D/3D child scene into ViewportTexture" \
  "EVIDENCE: sub_viewport.rs software ColorRect fill only. ACCEPT: Camera3D+mesh pixels; UpdateMode; ViewportTexture sample; integration test."

create 1 "parity-audit,physics,p1" \
  "Physics3D: SoftBody3D world-step integration + capsule raycast" \
  "EVIDENCE: softbody in gdphysics2d not gdphysics3d step; capsule raycast missing/sphere approx. ACCEPT: SoftBody step; capsule cast_motion; deterministic tests."

create 1 "parity-audit,import,p1" \
  "Import: real PNG/JPG pixel decode + font load (not header stubs)" \
  "EVIDENCE: importers.rs PNG header-only no pixels; font stub. ACCEPT: PNG/JPEG RGBA for renderer; TTF for Label; fixture tests."

create 1 "parity-audit,gui,p1" \
  "GUI: expand Control set to core Godot UI nodes" \
  "EVIDENCE: ClassDB has Button/Label/LineEdit/Panel/TextEdit/HBox/VBox only vs 50+ Godot gui. ACCEPT: RichTextLabel bbcode subset, TextureRect, ProgressBar, CheckBox, OptionButton, Scroll/Margin/Grid/Tab containers; layout tests."

create 1 "parity-audit,shader,p1" \
  "Shaders: Godot shader language to runnable WGSL/SPIR-V for custom materials" \
  "EVIDENCE: tokenizers exist; not full Godot SL->GPU. ACCEPT: spatial+canvas_item subset; ShaderMaterial render test; unsupported list."

# P2
create 2 "parity-audit,multiplayer,p2" \
  "Multiplayer: WebRTCMultiplayerPeer" \
  "EVIDENCE: trait mentions WebRTC; no impl. Godot modules/webrtc. ACCEPT: offer/answer; data channel packets; mock signaling test."

create 2 "parity-audit,xr,p2" \
  "XR: XRServer / OpenXR session stub to headset pose" \
  "EVIDENCE: zero XR/OpenXR in engine-rs. ACCEPT: XRServer+XROrigin3D/Camera/Controller ClassDB; mock pose backend for CI."

create 2 "parity-audit,import,p2" \
  "Import: FBX via ufbx or documented thirdparty strategy" \
  "EVIDENCE: no FBX; Godot modules/fbx. ACCEPT: THIRDPARTY_STRATEGY entry; implement or clear defer; if implement fixture+test."

create 2 "parity-audit,lighting,p2" \
  "Lighting: light projectors + soft shadow filter quality" \
  "EVIDENCE: no projector textures; hard shadow compare only. ACCEPT: projector modulate; PCF soft option; visual tests."

create 2 "parity-audit,render3d,p2" \
  "Rendering: Decal + FogVolume actual contribution" \
  "EVIDENCE: ClassDB only; little render path. ACCEPT: decal projects albedo; fog volume density; tests."

create 2 "parity-audit,physics,p2" \
  "Physics: Jolt backend option (Godot 4.6 path)" \
  "EVIDENCE: Godot jolt_physics module; Patina custom only. ACCEPT: THIRDPARTY_STRATEGY; feature flag; rigid/character tests."

create 2 "parity-audit,media,p2" \
  "Media: VideoStreamPlayer / video decode path" \
  "EVIDENCE: no VideoStream in engine-rs. ACCEPT: play fixture frames to texture OR explicit API stubs+docs if deferred."

create 2 "parity-audit,classdb,p2" \
  "ClassDB: inventory missing high-use runtime classes + defaults" \
  "EVIDENCE: ~109 vs ~912 classes. ACCEPT: inventory table full/partial/absent; register P0/P1 nodes; extend classdb_parity_test."

create 2 "parity-audit,docs,p2" \
  "Docs: reconcile COMPAT_MATRIX with code-audit reality" \
  "EVIDENCE: matrix overstates Measured vs stubs (audio claimed, GI/multiplayer gaps). ACCEPT: update rows; link epic; no full-port claim until P0 closed."

echo "---"
br count
echo "Open:"
br list --status open 2>/dev/null | head -100
