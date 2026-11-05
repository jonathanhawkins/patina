## Injected as an autoload by scripts/godot-reference.mjs (pat-xggzs) into a
## TEMP COPY of a game — never into games/. Replays a key timeline at fixed
## physics frames, records watched properties every frame, saves PNG frames.
##
## Frame numbering matches Patina's MCP `play` tool exactly:
## - frame 0 is the state before anything steps (after every `_ready`);
## - frame f is the state after f main-loop iterations (each runs the
##   project's physics ticks for one frame — TWO at the 3D platformer's
##   physics_ticks_per_second=120 — then `_process`, then the render);
## - an input {start, end} is pressed before frame `start` steps and released
##   after frame `end` steps.
## Frame f (f >= 1) is recorded at the very END of iteration f: from a
## deferred call queued by this node's `_process`, which runs LAST (highest
## process priority), so every script, `move_and_slide`, the physics step,
## every `_process` and the deferred calls they queued have run. Frame 0 is
## recorded in the first physics tick of iteration 1 (lowest physics
## priority). Pictures are still taken in the first physics tick of the NEXT
## iteration, which is when frame f's render is the last drawn one.
##
## 🔑 Inputs for frame f+1 are parsed right after frame f is recorded and NOT
## flushed: the OS loop flushes buffered events BETWEEN iterations
## (`os_macos.mm`: `flush_buffered_events` then `Main::iteration`), which is
## where a real key press lands. Input marks a press as
## `pressed_physics_frame = physics_frames + 1` ("the earliest we can react
## to it is the next physics tick", `input.cpp`). Pressing and flushing INSIDE
## the first tick, as this used to, made `pressed` true at once but
## `is_action_just_pressed` fire one tick LATE — the 2D Platformer's jump
## moved one tick instead of two in its first frame, a trace no real player
## (and no Patina run) produces. Recording before the inputs go in also keeps
## an `_input` handler of frame f+1 (Dodge the Creeps' Enter starting the
## game) out of frame f's record.
##
## ⚠️ Inputs starting on frame 1 cannot be parsed between frames (nothing of
## ours runs before the first iteration but `_ready`, which is before the main
## scene's own `_ready`), so they keep the old in-tick press: for them alone
## `is_action_just_pressed` in `_physics_process` is one tick late.
##
## 🔑 Held keys survive a FOCUS LOSS (pat-kiuul). macOS Godot calls
## `Input::release_pressed_events()` whenever the window resigns key status or
## moves (`godot_window_delegate.mm`: `windowDidResignKey`, `windowDidMove`),
## releasing every pressed key and action. The capture window sits off-screen
## while other Godot windows (another agent's capture, an editor) open and
## take the focus, so a held W was dropped mid-timeline: the platformer's
## player stopped dead at f96 in one capture and walked on in the next, and the
## gate judged Patina against whichever it got. Every key this script holds is
## re-pressed the moment a focus-out arrives, and checked again before each
## iteration's physics, so the game sees the timeline and nothing else.
##
## 🔑 Random numbers are PINNED (pat-y2ylj). Godot seeds its global RNG from
## the clock at startup (`SceneTree::SceneTree` calls `Math::randomize()`), and
## again in every `randomize()` and every `RandomNumberGenerator.new()`, so a
## game that uses randomness (Dodge the Creeps' mob spawns) recorded a
## different reference every run — the parity gate flagged a regression on a
## frame Patina drew pixel-identically. Here:
## - `_init` (the FIRST autoload, so before every other autoload's field
##   initializers and the main scene) calls `seed(cfg.seed)`;
## - godot-reference.mjs rewrites the temp copy's scripts so the k-th bare
##   `randomize()` becomes `pinned_randomize()` (seed + k), and every
##   `RandomNumberGenerator.new()` / `rng.randomize()` takes
##   `next_instance_seed()` (seed + 1000000 + j).
## Patina pins the same three by the same rule (`gdcore::rng::pin_seed`, the
## MCP `play` tool's `seed`), and its generator is a bit-exact port of
## Godot's PCG32, so both engines draw the same numbers.
extends Node

const PINNED_INSTANCE_OFFSET := 1000000

static var pin_base := 42
static var pin_global_reseeds := 0
static var pin_instance_reseeds := 0

var cfg: Dictionary = {}
var out_dir := ""
var last_iteration := -1
var samples: Array = []
var shots: Dictionary = {}
var written := false
var finished := false
## Keys the timeline currently holds, by name -> their press event.
var held: Dictionary = {}
## How many times a held key had to be pressed again after the OS dropped it.
var reasserted := 0


## The k-th bare `randomize()` of a pinned run (the rewrite in
## godot-reference.mjs calls this instead): seeds the global RNG `base + k`.
static func pinned_randomize() -> void:
	pin_global_reseeds += 1
	seed(pin_base + pin_global_reseeds)


## The seed of the j-th RandomNumberGenerator reseed (construction or its own
## `randomize()`): `base + 1000000 + j`.
static func next_instance_seed() -> int:
	pin_instance_reseeds += 1
	return pin_base + PINNED_INSTANCE_OFFSET + pin_instance_reseeds


## `RandomNumberGenerator.new()` in a pinned run.
static func pinned_rng() -> RandomNumberGenerator:
	var r := RandomNumberGenerator.new()
	r.seed = next_instance_seed()
	return r


func _init() -> void:
	# Read the config here, not in `_ready`: the seed has to be in before any
	# other autoload is even instantiated (their field initializers run then).
	var path := ""
	var args := OS.get_cmdline_user_args()
	for i in range(args.size()):
		if args[i] == "--godot-reference-config" and i + 1 < args.size():
			path = args[i + 1]
	var f := FileAccess.open(path, FileAccess.READ)
	if f == null:
		return
	cfg = JSON.parse_string(f.get_as_text())
	pin_base = int(cfg.get("seed", 42))
	pin_global_reseeds = 0
	pin_instance_reseeds = 0
	seed(pin_base)


func _ready() -> void:
	process_mode = Node.PROCESS_MODE_ALWAYS
	process_physics_priority = -2147483000
	process_priority = 2147483000
	if cfg.is_empty():
		printerr("godot-reference: cannot read the config (--godot-reference-config)")
		get_tree().quit(3)
		return
	out_dir = cfg["out_dir"]
	for s in cfg.get("shots", []):
		shots[int(s)] = true
	DirAccess.make_dir_recursive_absolute(out_dir.path_join("frames"))
	print("godot-reference: ", RenderingServer.get_current_rendering_method(), " on ",
		RenderingServer.get_current_rendering_driver_name(), ", window ", get_viewport().size,
		", ", Engine.physics_ticks_per_second, " physics ticks/s")


## Presses again every held key the OS released behind our back (see the
## header). `flush` applies them now; otherwise they ride the OS loop's flush.
func _reassert_held(flush: bool) -> void:
	var any := false
	for name in held:
		var ev: InputEventKey = held[name]
		if not Input.is_physical_key_pressed(ev.physical_keycode):
			Input.parse_input_event(ev)
			reasserted += 1
			any = true
	if any and flush:
		Input.flush_buffered_events()


func _notification(what: int) -> void:
	if what == NOTIFICATION_WM_WINDOW_FOCUS_OUT or what == NOTIFICATION_APPLICATION_FOCUS_OUT:
		_reassert_held(false)


func _physics_process(_delta: float) -> void:
	if finished:
		return
	_reassert_held(true)
	# During iteration k's physics ticks Engine.get_process_frames() is k-1:
	# the number of COMPLETED frames. Only the first tick of an iteration acts.
	var frame := Engine.get_process_frames()
	if frame == last_iteration:
		return
	last_iteration = frame
	if frame == 0:
		_record(0)
		_inject(0)
		Input.flush_buffered_events()
	if shots.has(frame) and frame > 0:
		_shoot(frame)
	if frame >= int(cfg["frames"]):
		finished = true
		_write()
		get_tree().quit(0)
		return


func _process(_delta: float) -> void:
	if finished:
		return
	# This iteration produces frame `get_process_frames() + 1` (the counter is
	# bumped at the END of `Main::iteration`).
	_end_of_frame.call_deferred(Engine.get_process_frames() + 1)


## Records frame `done`, then queues the inputs for frame `done + 1`; the OS
## loop flushes them before the next iteration — exactly between frames.
func _end_of_frame(done: int) -> void:
	if finished:
		return
	_record(done)
	_inject(done)


## Queues the key changes due between frame `done` and frame `done + 1`:
## releases of inputs held through `done`, presses of inputs starting at
## `done + 1`. Left buffered on purpose (see the header).
func _inject(done: int) -> void:
	for inp in cfg["inputs"]:
		if int(inp["end"]) == done:
			_key(inp, false)
	for inp in cfg["inputs"]:
		if int(inp["start"]) == done + 1:
			_key(inp, true)


func _key(inp: Dictionary, down: bool) -> void:
	var name: String = inp["key"]
	var code := OS.find_keycode_from_string(name)
	if code == KEY_NONE:
		printerr("godot-reference: unknown key ", name)
		return
	var ev := InputEventKey.new()
	ev.keycode = code
	ev.physical_keycode = code
	ev.key_label = code
	ev.pressed = down
	if down and name.length() == 1:
		ev.unicode = name.to_lower().unicode_at(0)
	elif down and name == "Space":
		ev.unicode = 32
	if down:
		held[name] = ev
	else:
		held.erase(name)
	Input.parse_input_event(ev)


func _record(frame: int) -> void:
	var values := {}
	var errors := {}
	for w in cfg["watches"]:
		var node := get_node_or_null(NodePath(w["path"]))
		if node == null:
			errors[w["id"]] = "no node at " + str(w["path"])
			values[w["id"]] = null
			continue
		values[w["id"]] = _plain(node.get(w["prop"]))
	var sample := {
		"frame": frame,
		"values": values,
		"physics_frames": Engine.get_physics_frames(),
		"process_frames": Engine.get_process_frames(),
	}
	if not errors.is_empty():
		sample["errors"] = errors
	samples.append(sample)


func _plain(v: Variant) -> Variant:
	match typeof(v):
		TYPE_VECTOR2, TYPE_VECTOR2I:
			return [v.x, v.y]
		TYPE_VECTOR3, TYPE_VECTOR3I:
			return [v.x, v.y, v.z]
		TYPE_VECTOR4, TYPE_VECTOR4I, TYPE_QUATERNION:
			return [v.x, v.y, v.z, v.w]
		TYPE_COLOR:
			return [v.r, v.g, v.b, v.a]
		TYPE_BASIS:
			return [v.x.x, v.x.y, v.x.z, v.y.x, v.y.y, v.y.z, v.z.x, v.z.y, v.z.z]
		TYPE_TRANSFORM3D:
			var b: Basis = v.basis
			return [b.x.x, b.x.y, b.x.z, b.y.x, b.y.y, b.y.z, b.z.x, b.z.y, b.z.z, v.origin.x, v.origin.y, v.origin.z]
		TYPE_TRANSFORM2D:
			return [v.x.x, v.x.y, v.y.x, v.y.y, v.origin.x, v.origin.y]
		TYPE_BOOL:
			return 1 if v else 0
		TYPE_INT, TYPE_FLOAT:
			return v
		TYPE_NIL:
			return null
	return str(v)


func _shoot(frame: int) -> void:
	# The texture holds the last DRAWN frame: the render that ended iteration
	# `frame`, i.e. the state after `frame` steps.
	var img := get_viewport().get_texture().get_image()
	if img == null:
		printerr("godot-reference: no image for frame ", frame)
		return
	img.save_png(out_dir.path_join("frames").path_join("f%05d.png" % frame))


func _write() -> void:
	if written:
		return
	written = true
	var data := {
		"engine": "godot",
		"version": Engine.get_version_info()["string"],
		"rendering_method": RenderingServer.get_current_rendering_method(),
		"rendering_driver": RenderingServer.get_current_rendering_driver_name(),
		"window": [get_viewport().size.x, get_viewport().size.y],
		"physics_ticks_per_second": Engine.physics_ticks_per_second,
		"physics_interpolation": ProjectSettings.get_setting("physics/common/physics_interpolation", false),
		"seed": pin_base,
		"pinned_reseeds": {"global": pin_global_reseeds, "instance": pin_instance_reseeds},
		"frames": samples,
		"quit_early": not finished,
		"held_keys_reasserted": reasserted,
	}
	var f := FileAccess.open(out_dir.path_join("trace.json"), FileAccess.WRITE)
	f.store_string(JSON.stringify(data, "\t"))
	f.close()


func _exit_tree() -> void:
	# The game quit itself (or crashed out of the tree) before the timeline
	# ended: keep what was recorded.
	_write()
