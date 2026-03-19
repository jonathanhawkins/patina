extends Node2D

var speed = 100.0

func _process(delta):
    self.speed = self.speed + delta
