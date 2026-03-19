extends Node2D

var speed = 50.0
var direction = 1.0

func _process(delta):
    position.x += speed * direction * delta
    if position.x > 500:
        direction = -1.0
    if position.x < 100:
        direction = 1.0
