extends Node2D

var speed = 200.0
var can_shoot = true
var shoot_cooldown = 0.0

func _process(delta):
    if Input.is_action_pressed("ui_left"):
        position.x -= speed * delta
    if Input.is_action_pressed("ui_right"):
        position.x += speed * delta
    if Input.is_action_pressed("ui_up"):
        position.y -= speed * delta
    if Input.is_action_pressed("ui_down"):
        position.y += speed * delta
    position.x = clamp(position.x, 0, 640)
    position.y = clamp(position.y, 0, 480)
    if shoot_cooldown > 0:
        shoot_cooldown -= delta
    if Input.is_action_pressed("shoot") and shoot_cooldown <= 0:
        shoot_cooldown = 0.3
        print("SHOOT!")
