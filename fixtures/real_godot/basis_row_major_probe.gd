extends SceneTree

func _initialize():
	call_deferred("_go")

func _go():
	var ps: PackedScene = load("res://player/player.tscn")
	var p = ps.instantiate()
	root.add_child(p)
	var cam: Node3D = p.get_node("Target/Camera3D")
	var t: Transform3D = cam.transform
	print("CAM basis.x=", t.basis.x, " basis.y=", t.basis.y, " basis.z=", t.basis.z)
	print("CAM str=", str(t))
	print("CAM var_to_str=", var_to_str(t))
	print("CAM basis var_to_str=", var_to_str(t.basis))
	print("CAM global=", cam.global_transform)
	print("CAM forward(-z global)=", -cam.global_transform.basis.z)
	print("CAM euler=", t.basis.get_euler(), " rotation=", cam.rotation)

	var b := Basis.from_euler(Vector3(0.3, 0.7, -0.4))
	var tr := Transform3D(b, Vector3(1, 2, 3))
	print("NS basis.x=", b.x, " y=", b.y, " z=", b.z)
	print("NS var_to_str=", var_to_str(tr))
	print("NS basis var_to_str=", var_to_str(b))
	print("NS str=", str(tr))
	print("NS basis str=", str(b))
	var back = str_to_var(var_to_str(tr))
	print("NS roundtrip eq=", back == tr)
	var parsed = str_to_var("Transform3D(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12)")
	print("PARSED x=", parsed.basis.x, " y=", parsed.basis.y, " z=", parsed.basis.z, " o=", parsed.origin)
	var pb = str_to_var("Basis(1, 2, 3, 4, 5, 6, 7, 8, 9)")
	print("PARSEDB x=", pb.x, " y=", pb.y, " z=", pb.z)
	var pp = str_to_var("Projection(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16)")
	print("PARSEDP x=", pp.x, " y=", pp.y, " z=", pp.z, " w=", pp.w)
	print("PARSEDP var_to_str=", var_to_str(pp))
	var pt2 = str_to_var("Transform2D(1, 2, 3, 4, 5, 6)")
	print("PARSED2 x=", pt2.x, " y=", pt2.y, " o=", pt2.origin)

	var par := Node3D.new()
	par.name = "Parent"
	par.transform = tr
	var ch := Node3D.new()
	ch.name = "Child"
	ch.position = Vector3(1, 0.5, -2)
	par.add_child(ch)
	ch.owner = par
	root.add_child(par)
	print("CHILD global_position=", ch.global_position)
	var holder := Node3D.new()
	holder.name = "Holder"
	par.add_child(holder)
	holder.owner = par
	holder.set_meta("xs", [tr, {"k": b}])
	var pk := PackedScene.new()
	pk.pack(par)
	ResourceSaver.save(pk, "res://rotated_probe.tscn")
	ResourceSaver.save(pk, "res://rotated_probe.scn")
	print("SAVED")
	quit()
