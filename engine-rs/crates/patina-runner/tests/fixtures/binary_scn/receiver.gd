extends Node2D

var got_bind = -1
var fired = 0

func on_poked(bind_value):
	self.fired = self.fired + 1
	self.got_bind = bind_value
