extends Node2D
## The visible shockwave of a pulse: an expanding ring that fades out.

var _radius := 0.0
var _max_radius := 0.0
var _alpha := 0.0


func show_at(origin: Vector2, radius: float) -> void:
	position = origin
	_max_radius = radius
	_radius = 8.0
	_alpha = 1.0


func _process(delta: float) -> void:
	if _alpha <= 0.0:
		return
	_radius = move_toward(_radius, _max_radius, _max_radius * 4.0 * delta)
	_alpha = maxf(_alpha - delta * 2.5, 0.0)
	queue_redraw()


func _draw() -> void:
	if _alpha > 0.0:
		draw_arc(Vector2.ZERO, _radius, 0.0, TAU, 48, Color(1.0, 0.9, 0.5, _alpha), 3.0)
