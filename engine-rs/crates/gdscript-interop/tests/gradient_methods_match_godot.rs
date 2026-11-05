//! pat-nyn3w: `Gradient`'s method surface on a script-built `Gradient.new()`.
//!
//! There was no Gradient method at all (`sample` raised
//! `UndefinedFunction("Resource.sample")`), and `Gradient` had no registered
//! property, so `Gradient.new()` was an empty bag rather than Godot's default
//! black-to-white gradient.
//!
//! Every expected value is real Godot 4.6.1's (`--headless --script`, the same
//! statements, printed with `var_to_str`). Note Godot's raw-order storage:
//! `add_point` APPENDS (offsets read `(0, 1, 0.5)`), and only sampling sorts.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

const SRC: &str = r#"extends Node

func run() -> Array:
	var out := []
	var g := Gradient.new()
	out.append(g.sample(0.25))
	out.append(g.get_point_count())
	out.append(g.offsets)
	g.add_point(0.5, Color(1, 0, 0, 1))
	out.append(g.offsets)
	out.append(g.sample(0.25))
	out.append(g.sample(0.75))
	g.interpolation_mode = Gradient.GRADIENT_INTERPOLATE_CUBIC
	out.append(g.sample(0.3))
	out.append(g.sample(0.8))
	g.interpolation_mode = Gradient.GRADIENT_INTERPOLATE_CONSTANT
	out.append(g.sample(0.3))
	out.append(g.sample(0.8))
	g.interpolation_mode = Gradient.GRADIENT_INTERPOLATE_LINEAR
	g.interpolation_color_space = Gradient.GRADIENT_COLOR_SPACE_LINEAR_SRGB
	out.append(g.sample(0.3))
	g.interpolation_color_space = Gradient.GRADIENT_COLOR_SPACE_OKLAB
	out.append(g.sample(0.3))
	g.set_color(0, Color(0, 0, 1, 1))
	g.remove_point(1)
	out.append(g.colors)
	out.append(g.get_offset(1))
	return out
"#;

fn run() -> Vec<Variant> {
    gdobject::class_db::register_all_classes();
    let mut interp = Interpreter::new();
    let class_def = interp.run_class(SRC).expect("script parses");
    let mut inst = interp
        .instantiate_class(&class_def)
        .expect("class instantiates");
    match interp
        .call_instance_method(&mut inst, "run", &[])
        .unwrap_or_else(|e| panic!("run() failed: {e:?}"))
    {
        Variant::Array(a) => a.to_vec(),
        other => panic!("expected an Array, got {other:?}"),
    }
}

fn assert_color(v: &Variant, want: [f32; 4], what: &str) {
    let Variant::Color(c) = v else {
        panic!("{what}: expected a Color, got {v:?}");
    };
    let got = [c.r, c.g, c.b, c.a];
    for i in 0..4 {
        assert!(
            (got[i] - want[i]).abs() < 1e-5,
            "{what}: got {got:?}, Godot 4.6.1 gives {want:?}"
        );
    }
}

#[test]
fn gradient_methods_match_godot() {
    let r = run();
    assert_color(&r[0], [0.25, 0.25, 0.25, 1.0], "default gradient at 0.25");
    assert_eq!(r[1], Variant::Int(2), "default point count");
    assert_eq!(r[2], Variant::PackedFloat32Array(vec![0.0, 1.0]));
    assert_eq!(
        r[3],
        Variant::PackedFloat32Array(vec![0.0, 1.0, 0.5]),
        "add_point appends"
    );
    assert_color(&r[4], [0.5, 0.0, 0.0, 1.0], "linear 0.25");
    assert_color(&r[5], [1.0, 0.5, 0.5, 1.0], "linear 0.75");
    assert_color(&r[6], [0.624, -0.072, -0.072, 1.0], "cubic 0.3");
    assert_color(&r[7], [1.048, 0.624, 0.624, 1.0], "cubic 0.8");
    assert_color(&r[8], [0.0, 0.0, 0.0, 1.0], "constant 0.3");
    assert_color(&r[9], [1.0, 0.0, 0.0, 1.0], "constant 0.8");
    assert_color(&r[10], [0.7977378, 0.0, 0.0, 1.0], "linear-sRGB 0.3");
    assert_color(&r[11], [0.5021107, 0.0, 0.0, 1.0], "Oklab 0.3");
    assert_eq!(
        r[12],
        Variant::PackedColorArray(vec![
            gdcore::math::Color::new(0.0, 0.0, 1.0, 1.0),
            gdcore::math::Color::new(1.0, 1.0, 1.0, 1.0),
        ]),
        "set_color(0) then remove_point(1), on the order the first sample sorted"
    );
    assert_eq!(r[13], Variant::Float(1.0), "get_offset(1)");
}
