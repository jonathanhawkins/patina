extends Node2D

# Every value the test reads back. Declared defaults are deliberately NOT the
# values written, so an assertion cannot pass on a variable the script failed to
# assign.
var wrote = false
var r32 = 0
var rline = ""
var rdouble = 0.0
var rvar_keys = 0
var rvar_a0 = 0
var rvar_a1 = 0.0
var at_eof = false
var buf = null
var missing_is_null = false
var open_err = -1
var ok_err = -1

func _ready():
    var f = FileAccess.open("user://save.dat", FileAccess.WRITE)
    f.store_32(7)
    f.store_line("hi")
    f.store_var({"a": [1, 2.5]})
    f.store_double(1.5)
    f.close()
    self.wrote = true

    # Reopened for READ, so the cursor starts at 0 and every value must come
    # back in the order it was written.
    var g = FileAccess.open("user://save.dat", FileAccess.READ)
    self.r32 = g.get_32()
    self.rline = g.get_line()
    var v = g.get_var()
    self.rvar_keys = v.keys().size()
    self.rvar_a0 = v["a"][0]
    self.rvar_a1 = v["a"][1]
    self.rdouble = g.get_double()
    # Everything written has now been read, so the cursor is at the end.
    self.at_eof = g.eof_reached()
    g.close()

    # A separate open, because get_buffer must start from a known cursor.
    var h = FileAccess.open("user://save.dat", FileAccess.READ)
    self.buf = h.get_buffer(4)
    h.close()
    # An open that SUCCEEDED must leave the open error at OK, or a stale error
    # from an earlier failure would look like this open had failed.
    self.ok_err = FileAccess.get_open_error()

    # Godot returns null and records the reason; it does not raise.
    var missing = FileAccess.open("user://missing.dat", FileAccess.READ)
    self.missing_is_null = missing == null
    self.open_err = FileAccess.get_open_error()
