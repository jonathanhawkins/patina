#!/usr/bin/env bash
# Beads for remaining 1:1 port work not already covered by open pat-p0s82 P1/P2.
set -euo pipefail
cd "$(dirname "$0")/.."

EPIC=$(br create --silent -t epic -p 1 -l "port-1to1,parity-audit" \
  --title "Godot 4.6.1 1:1 port completion (scorecard post-P0 2026-07-11)" \
  --description "Follow-up epic after P0 close. Scorecard: prd/PORT_1TO1_SCORECARD_2026-07-11.md (~55% typical 2D game readiness, ~30% strict 1:1). Existing open P1/P2 under pat-p0s82 remain primary queue for GI/cascades/GUI/nav/import-pixels/etc. This epic tracks ADDITIONAL gaps found in re-audit.")
echo "EPIC=$EPIC"

create() {
  local prio="$1" labels="$2" title="$3" desc="$4"
  local id
  id=$(br create --silent -t feature -p "$prio" -l "$labels" --parent "$EPIC" \
    --title "$title" --description "$desc")
  echo "$id  $title"
}

# Integration gaps for recently landed P0 (not full re-do)
create 0 "port-1to1,render2d,p0" \
  "2D: wire Light2D into SoftwareRenderer frame path (not post-pass only)" \
  "EVIDENCE: gdrender2d/src/light2d.rs has light_factor_at/apply_lights_to_framebuffer but renderer.rs has zero light2d references — scene PointLight2D does not auto-shade frames. ACCEPT: collect lights/occluders from scene or server; apply in SoftwareRenderer each frame; regression test lit sprite darker far from light."

create 0 "port-1to1,import,p0" \
  "Import: decode PNG pixels into Texture2D (header-only today)" \
  "EVIDENCE: gdresource/src/importers.rs import_image still says only header parsed. OPEN bead pat-p0s82.19 overlaps — if that bead is the owner, close THIS as dup after confirm. ACCEPT: PNG file -> RGBA buffer usable by gdrender2d; fixture test non-black pixels."

create 0 "port-1to1,gdscript,p0" \
  "GDScript: SceneTree.create_timer + await timer.timeout Godot idiom" \
  "EVIDENCE: await create_timer(t) free builtin exists; no get_tree().create_timer().timeout / SceneTreeTimer node. ACCEPT: SceneTreeTimer resource; get_tree() method; await my_timer.timeout suspends; test with process frames."

create 1 "port-1to1,multiplayer,p1" \
  "Multiplayer: document wire format + optional Godot ENet interop plan" \
  "EVIDENCE: Patina UDP uses PEN1 framing; PRPC binary — not modules/enet or scene_multiplayer wire. ACCEPT: docs/MULTIPLAYER_WIRE.md; decision: Patina-only vs Godot interop; if interop, spike bead; if not, COMPAT_MATRIX says Claimed/Partial not Measured vs Godot peers."

create 1 "port-1to1,audio,p1" \
  "Audio: ClassDB AudioEffect* resources + bus layout from project.godot" \
  "EVIDENCE: effects exist in Rust but no AudioEffectAmplify ClassDB/resource; project bus layout may not load. ACCEPT: register effect classes; load bus layout from project settings if present; scene can add effect to bus."

create 1 "port-1to1,core,p1" \
  "Core: real AES (or documented crypto scope) — AES stub returns error" \
  "EVIDENCE: gdcore/src/crypto.rs AES stub Err. ACCEPT: rustcrypto AES-GCM or document deferred + tests for SHA only as measured."

create 1 "port-1to1,physics,p1" \
  "Physics2D: capsule + segment raycast (explicitly unimplemented)" \
  "EVIDENCE: gdphysics2d/src/world.rs segment and capsule raycasts not yet implemented; collision3d capsule pairs missing. ACCEPT: raycast hits capsule/segment; unit tests vs analytic geometry."

create 1 "port-1to1,text,p1" \
  "Text: TextServer-lite for Label shaping (kerning/metrics beyond 5x7 bitmap)" \
  "EVIDENCE: no TextServer; font path is bitmap/rect. ACCEPT: load TTF metrics via existing strategy; Label draws with advance widths; basic Latin test."

create 1 "port-1to1,render3d,p1" \
  "3D: capture Godot oracle goldens for multi_light + physics_3d scenes" \
  "EVIDENCE: COMPAT_MATRIX defers 3D oracle; tooling exists. ACCEPT: upstream oracle outputs for listed 3D fixtures; parity test gate documented."

create 1 "port-1to1,animation,p1" \
  "Animation: AnimationMixer root motion + method tracks" \
  "EVIDENCE: AnimationPlayer/Tree exist; root motion / method tracks incomplete. ACCEPT: root motion delta applied to Node3D; method track invokes script method at key."

create 1 "port-1to1,scene,p1" \
  "Scene: RemoteTransform2D/3D + VisibilityNotifier enter/exit signals" \
  "EVIDENCE: common Godot nodes often missing from ClassDB list. ACCEPT: ClassDB + behavior tests for remote transform copy and visibility camera AABB."

create 1 "port-1to1,scene,p1" \
  "Scene: Label3D + Sprite3D billboard modes in render path" \
  "EVIDENCE: Sprite3D server types exist; Label3D weak. ACCEPT: Label3D renders glyphs in 3D view or software path; billboard flag rotates to camera."

create 1 "port-1to1,platform,p1" \
  "Platform: Windows desktop runtime smoke (window + input + audio)" \
  "EVIDENCE: Windows claimed deferred. ACCEPT: CI or documented manual smoke; or explicit Phase 7 defer with COMPAT_MATRIX update."

create 2 "port-1to1,scripting,p2" \
  "Scripting: GDExtension host slice (load .gdextension library)" \
  "EVIDENCE: apps/godot lab; engine-rs not full host. ACCEPT: load stub extension; call one method; or document out of scope."

create 2 "port-1to1,scripting,p2" \
  "Scripting: C# / mono — classify in THIRDPARTY_STRATEGY (no silent claim)" \
  "EVIDENCE: no mono in engine-rs. ACCEPT: THIRDPARTY_STRATEGY entry Deferred/Replace; no ClassDB C# without implementation."

create 2 "port-1to1,export,p2" \
  "Export: Godot-compatible export presets vs Patina packaging clarity" \
  "EVIDENCE: export.rs has template manifests; not Godot export-template parity. ACCEPT: migration-guide + COMPAT_MATRIX state Export as Partial with listed limits."

create 2 "port-1to1,theme,p2" \
  "GUI: Theme / StyleBoxFlat application on Control draw" \
  "EVIDENCE: theme types partial in gdscene; full theme cascade incomplete. ACCEPT: theme override colors/fonts on Button/Label; draw uses theme."

create 2 "port-1to1,input,p2" \
  "Input: joypad axes + action strength (Input.get_action_strength)" \
  "EVIDENCE: keyboard/mouse strong; joypad depth unclear. ACCEPT: map axes to actions; get_action_strength returns 0..1; test."

create 2 "port-1to1,particles,p2" \
  "Particles: GPUParticles2D/3D collision + trail (if claiming GPU particles)" \
  "EVIDENCE: CPU sim exists; GPU name may overclaim. ACCEPT: document CPU-sim; or add collision bounce test."

create 2 "port-1to1,docs,p0" \
  "Docs: refresh COMPAT_MATRIX + AUDIO_MILESTONE after P0 (stale 'no audio output')" \
  "EVIDENCE: COMPAT_MATRIX Audio row still says no real output; contradicts cpal output.rs. ACCEPT: update Audio/Multiplayer/2D light rows; link scorecard; do not claim 1:1 complete."

create 2 "port-1to1,docs,p1" \
  "Docs: maintain PORT_1TO1_SCORECARD after each major subsystem close" \
  "ACCEPT: process note in AGENTS or scorecard; bump date/scores when P1 beads close."

# Cross-link: ensure open pat-p0s82 queue is labeled as primary remaining 1:1 work
create 1 "port-1to1,meta,p1" \
  "Meta: map pat-p0s82 open children into Now/Next in BEAD_EXECUTION_MAP" \
  "EVIDENCE: 14 P1 + 9 P2 open under pat-p0s82. ACCEPT: BEAD_EXECUTION_MAP section lists claim order: PNG/font, GUI core, cascades, VoxelGI, SubViewport, Path3D, nav RVO, bus sends, then P2."

echo "---"
br count
echo "Open P0:"
br list --status open 2>&1 | rg '\[● P0\]' || true
echo "New epic children:"
br list --status open 2>&1 | rg "port-1to1|1:1" | head -40
