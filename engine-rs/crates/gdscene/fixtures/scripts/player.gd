extends Node2D
var speed = 100.0
var moved = false
func _ready():
    self.moved = true
func _process(delta):
    self.speed = self.speed + delta
