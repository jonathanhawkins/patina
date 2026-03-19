extends Node2D
var speed = 200.0
var health = 100
func _ready():
    health = 100
func _process(delta):
    speed = speed + delta
