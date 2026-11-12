extends Area2D
## Gloom drifts toward the player and costs a life on contact. A pulse shoves
## it away; it recovers its course over time.

@export var speed := 55.0

var knockback := Vector2.ZERO


func _ready() -> void:
	body_entered.connect(_on_body_entered)


func _physics_process(delta: float) -> void:
	var player := get_tree().get_first_node_in_group("player") as Node2D
	if player:
		var to_player := (player.global_position - global_position).normalized()
		position += (to_player * speed + knockback) * delta
	knockback = knockback.move_toward(Vector2.ZERO, 400.0 * delta)
	rotation += delta * 1.5


func push_from(origin: Vector2, radius: float) -> void:
	var away := global_position - origin
	if away.length() <= radius:
		knockback = away.normalized() * 420.0


func _on_body_entered(body: Node2D) -> void:
	if body.is_in_group("player") and body.hit():
		knockback = (global_position - body.global_position).normalized() * 300.0
