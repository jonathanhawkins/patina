extends Node2D

var ready_ran = false
var plain_name = ""

func _ready():
    self.ready_ran = true
    self.plain_name = get_node("Plain").name
