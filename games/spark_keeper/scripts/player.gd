extends CharacterBody2D
## The spark keeper: a glowing orb that drifts with momentum, pulses to push
## gloom away, and blinks while invulnerable after a hit.

signal pulsed(origin: Vector2, radius: float)

const SPEED := 220.0
const ACCEL := 1400.0
const FRICTION := 900.0
const PULSE_RADIUS := 110.0
const PULSE_COOLDOWN := 2.0
const INVULNERABLE_TIME := 1.5

@export var arena := Rect2(16, 16, 608, 328)

var cooldown := 0.0
var invulnerable := 0.0
var glow := 1.0


func _ready() -> void:
	add_to_group("player")


func _physics_process(delta: float) -> void:
	var dir := Input.get_vector("move_left", "move_right", "move_up", "move_down")
	if dir != Vector2.ZERO:
		velocity = velocity.move_toward(dir * SPEED, ACCEL * delta)
	else:
		velocity = velocity.move_toward(Vector2.ZERO, FRICTION * delta)
	move_and_slide()
	position = position.clamp(arena.position, arena.end)

	cooldown = maxf(cooldown - delta, 0.0)
	if Input.is_action_just_pressed("pulse") and cooldown == 0.0:
		cooldown = PULSE_COOLDOWN
		pulsed.emit(global_position, PULSE_RADIUS)

	if invulnerable > 0.0:
		invulnerable = maxf(invulnerable - delta, 0.0)
		visible = int(invulnerable * 10.0) % 2 == 0
	else:
		visible = true
	glow = move_toward(glow, 1.0, delta)
	queue_redraw()


func _draw() -> void:
	draw_circle(Vector2.ZERO, 14.0 * glow, Color(1.0, 0.85, 0.4, 0.25))
	draw_circle(Vector2.ZERO, 9.0, Color(1.0, 0.95, 0.7))
	draw_circle(Vector2.ZERO, 5.0, Color.WHITE)


func hit() -> bool:
	if invulnerable > 0.0:
		return false
	invulnerable = INVULNERABLE_TIME
	Game.lose_life()
	return true


func brighten() -> void:
	glow = 1.6
