extends Node2D

var health = 100
var name_str = "Player"
var velocity = Vector2(0, 0)
var is_alive = true

func _ready():
    print("Ready! Health: " + str(health))

func _process(delta):
    if health <= 0:
        is_alive = false
