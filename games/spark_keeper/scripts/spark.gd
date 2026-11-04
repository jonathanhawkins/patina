extends Area2D
## A spark to collect. Bobs in place; when the player touches it, it scores,
## bursts and fades out.

var _t := 0.0
var _collected := false


func _ready() -> void:
	body_entered.connect(_on_body_entered)
	_t = Game.rng.randf() * TAU


func _process(delta: float) -> void:
	_t += delta * 3.0
	$Sprite.position.y = sin(_t) * 2.0


func _on_body_entered(body: Node2D) -> void:
	if _collected or not body.is_in_group("player"):
		return
	_collected = true
	Game.add_score(1)
	body.brighten()
	$Burst.emitting = true
	var tween := create_tween()
	tween.tween_property($Sprite, "scale", Vector2(2.5, 2.5), 0.25)
	tween.parallel().tween_property($Sprite, "modulate:a", 0.0, 0.25)
	await get_tree().create_timer(0.3).timeout
	queue_free()
