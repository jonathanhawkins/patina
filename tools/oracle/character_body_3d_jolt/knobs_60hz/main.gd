extends Node3D

# Godot 4.6.1 + Jolt probe behind the CharacterBody3D numbers in
# engine-rs/crates/gdphysics3d/src/character.rs and
# engine-rs/crates/gdscene/tests/character_body_3d_move_and_slide_knobs.rs
# (pat-kiuul). Run:
#   /Applications/Godot.app/Contents/MacOS/Godot --headless --path tools/oracle/character_body_3d_jolt/knobs_60hz --quit-after 1000000
# Physics runs in real time, so it takes a few seconds.

# character_body_3d_move_and_slide_knobs.rs's scene at 60 ticks/s.
var a: CharacterBody3D
var b: CharacterBody3D
var ticks := 0


func make(z: float, slide: bool) -> CharacterBody3D:
	var c := CharacterBody3D.new()
	var cs := CollisionShape3D.new()
	var s := SphereShape3D.new()
	s.radius = 0.5
	cs.shape = s
	c.add_child(cs)
	c.position = Vector3(0, 0, z)
	c.slide_on_ceiling = slide
	add_child(c)
	return c


func _ready() -> void:
	var ceil := StaticBody3D.new()
	var cs := CollisionShape3D.new()
	var bs := BoxShape3D.new()
	bs.size = Vector3(80, 1, 80)
	cs.shape = bs
	ceil.add_child(cs)
	ceil.position = Vector3(0, 3, 0)
	add_child(ceil)
	a = make(0, true)
	b = make(20, false)


func _physics_process(_d: float) -> void:
	ticks += 1
	for c in [a, b]:
		c.velocity = Vector3(6, 10, 0)
		c.move_and_slide()
	if ticks == 120:
		print("slide x=%.6f y=%.6f   stop x=%.6f y=%.6f" % [a.position.x, a.position.y, b.position.x, b.position.y])
		get_tree().quit()
