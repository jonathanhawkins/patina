extends Node3D

# Godot 4.6.1 + Jolt probe behind the CharacterBody3D numbers in
# engine-rs/crates/gdphysics3d/src/character.rs and
# engine-rs/crates/gdscene/tests/character_body_3d_move_and_slide_knobs.rs
# (pat-kiuul). Run:
#   /Applications/Godot.app/Contents/MacOS/Godot --headless --path tools/oracle/character_body_3d_jolt/delta_one --quit-after 1000000
# Physics runs in real time, so it takes a few seconds.

# Synthetic CharacterBody3D fixtures, measured on Godot 4.6.1 + Jolt with
# physics_ticks_per_second = 1 so move_and_slide() moves by exactly `velocity`.

var step := 0
var bodies: Array[Node] = []


func box(pos: Vector3, half: Vector3, vel := Vector3.ZERO) -> StaticBody3D:
	var b := StaticBody3D.new()
	var cs := CollisionShape3D.new()
	var s := BoxShape3D.new()
	s.size = half * 2.0
	cs.shape = s
	b.add_child(cs)
	b.position = pos
	b.constant_linear_velocity = vel
	add_child(b)
	bodies.append(b)
	return b


func sphere_char(pos: Vector3, r := 1.0) -> CharacterBody3D:
	var c := CharacterBody3D.new()
	var cs := CollisionShape3D.new()
	var s := SphereShape3D.new()
	s.radius = r
	cs.shape = s
	c.add_child(cs)
	c.position = pos
	add_child(c)
	bodies.append(c)
	return c


func capsule_char(pos: Vector3) -> CharacterBody3D:
	var c := CharacterBody3D.new()
	var cs := CollisionShape3D.new()
	var s := CapsuleShape3D.new()
	s.radius = 0.4
	s.height = 2.1
	cs.shape = s
	cs.position = Vector3(0, 1.05, 0)
	c.add_child(cs)
	c.position = pos
	add_child(c)
	bodies.append(c)
	return c


func clear() -> void:
	for b in bodies:
		remove_child(b)
		b.free()
	bodies.clear()


func p(label: String, v) -> void:
	if v is Vector3:
		print("%s = (%.9f, %.9f, %.9f)" % [label, v.x, v.y, v.z])
	else:
		print("%s = %s" % [label, str(v)])


func _physics_process(_delta: float) -> void:
	step += 1
	match step:
		1:
			# A: falling body above the floor is not snapped; grounded one is.
			box(Vector3(0, -1, 0), Vector3(100, 1, 100))
			var c := sphere_char(Vector3(0, 1.07, 0))
			c.velocity = Vector3(0, -0.02, 0)
			c.move_and_slide()
			p("A falling pos", c.position)
			p("A falling on_floor", c.is_on_floor())
			# Ground it, then teleport 0.07 up and move -0.02 (gap 0.05).
			var g := sphere_char(Vector3(5, 1.0005, 0))
			g.velocity = Vector3(0, -0.01, 0)
			g.move_and_slide()
			p("A ground first pos", g.position)
			p("A ground first on_floor", g.is_on_floor())
			g.position = Vector3(5, 1.07, 0)
			g.velocity = Vector3(0, -0.02, 0)
			g.move_and_slide()
			p("A ground snap pos", g.position)
			p("A ground snap on_floor", g.is_on_floor())
			# Same with a 0.04 gap.
			g.position = Vector3(5, 1.06, 0)
			g.velocity = Vector3(0, -0.02, 0)
			g.move_and_slide()
			p("A ground snap2 pos", g.position)
			p("A ground snap2 on_floor", g.is_on_floor())
		2:
			clear()
			# B: jump into a flat ceiling, slide_on_ceiling true / false.
			box(Vector3(0, 1, 0), Vector3(100, 1, 100))
			var s := sphere_char(Vector3(0, -2, 0))
			s.velocity = Vector3(3, 2.5, 0)
			s.move_and_slide()
			p("B slide pos", s.position)
			p("B slide vel", s.velocity)
			p("B slide on_ceiling", s.is_on_ceiling())
			var t := sphere_char(Vector3(0, -2, 10))
			t.slide_on_ceiling = false
			t.velocity = Vector3(3, 2.5, 0)
			t.move_and_slide()
			p("B stop pos", t.position)
			p("B stop vel", t.velocity)
			p("B stop on_ceiling", t.is_on_ceiling())
		3:
			clear()
			# C: a moving floor carries a resting body, from the 2nd call on.
			box(Vector3(0, -1, 0), Vector3(100, 1, 100), Vector3(2, 0, 0))
			var c := sphere_char(Vector3(0, 1, 0))
			c.velocity = Vector3.ZERO
			c.move_and_slide()
			p("C first pos", c.position)
			p("C first on_floor", c.is_on_floor())
			p("C first platform_velocity", c.get_platform_velocity())
			c.velocity = Vector3.ZERO
			c.move_and_slide()
			p("C second pos", c.position)
			p("C second platform_velocity", c.get_platform_velocity())
		4:
			clear()
			# D: platform_on_leave ADD_UPWARD_VELOCITY off an up-and-sideways
			# and a down-and-sideways platform.
			box(Vector3(0, -1, 0), Vector3(10, 1, 10), Vector3(2, 3, 0))
			box(Vector3(50, -1, 0), Vector3(10, 1, 10), Vector3(2, -3, 0))
			var u := sphere_char(Vector3(0, 1, 0))
			u.platform_on_leave = CharacterBody3D.PLATFORM_ON_LEAVE_ADD_UPWARD_VELOCITY
			u.velocity = Vector3.ZERO
			u.move_and_slide()
			u.velocity = Vector3(0, 50, 0)
			u.move_and_slide()
			p("D up vel", u.velocity)
			var d := sphere_char(Vector3(50, 1, 0))
			d.platform_on_leave = CharacterBody3D.PLATFORM_ON_LEAVE_ADD_UPWARD_VELOCITY
			d.velocity = Vector3.ZERO
			d.move_and_slide()
			d.velocity = Vector3(0, 50, 0)
			d.move_and_slide()
			p("D down vel", d.velocity)
		5:
			clear()
			# E: the platformer's capsule walking on a 2 m box floor (top -4),
			# 120 Hz numbers fed at delta 1: motion per call as at 120 Hz.
			box(Vector3(0, -5, 0), Vector3(1000, 1, 1000))
			var c := capsule_char(Vector3(0, -3.998966, 0))
			for i in 40:
				c.velocity = Vector3(6.0666667 / 120.0, -22.0 / 120.0 / 120.0, 0)
				c.move_and_slide()
			p("E walk pos", c.position)
			p("E walk vel", c.velocity)
			p("E walk on_floor", c.is_on_floor())
		6:
			get_tree().quit()
