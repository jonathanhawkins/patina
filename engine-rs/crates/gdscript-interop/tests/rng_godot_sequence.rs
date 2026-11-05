//! pat-y2ylj — a GDScript `seed(n)` draws Godot's own numbers.
//!
//! The parity gate compares Dodge the Creeps against real Godot frame by
//! frame; its mobs spawn at `randf()` along the path, turned by
//! `randf_range(-PI / 4, PI / 4)`, sped by `randf_range(150.0, 250.0)` and
//! skinned by `pick_random()`. Those only line up when the script builtins draw
//! Godot's sequence. They used to run off a private counter-LCG, so
//! `seed(12345); randi()` was 2 instead of Godot's 1321476956.
//!
//! Expected values were printed by Godot 4.6.1 (`Godot --headless -s
//! probe.gd`; see engine-rs/crates/gdcore/tests/rng_matches_godot_pcg.rs for
//! the probe). Floats are compared by bits.
//!
//! The global RNG is process-wide, so the tests here serialize on one lock.

use gdscript_interop::interpreter::{pin_random_seed, Interpreter};
use gdvariant::Variant;

#[must_use = "bind it for the whole test: `let _guard = serial();`"]
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn run_list(src: &str) -> Vec<Variant> {
    let mut interp = Interpreter::new();
    match interp
        .run(src)
        .unwrap_or_else(|e| panic!("script failed: {e:?}\n--- source ---\n{src}"))
        .return_value
        .expect("script returned a value")
    {
        Variant::Array(a) => a.to_vec(),
        other => panic!("expected an Array, got {other:?}"),
    }
}

fn ints(v: &[Variant]) -> Vec<i64> {
    v.iter()
        .map(|x| match x {
            Variant::Int(i) => *i,
            other => panic!("expected Int, got {other:?}"),
        })
        .collect()
}

fn float_bits(v: &[Variant]) -> Vec<i64> {
    v.iter()
        .map(|x| match x {
            Variant::Float(f) => f.to_bits() as i64,
            other => panic!("expected Float, got {other:?}"),
        })
        .collect()
}

#[test]
fn global_builtins_draw_godots_sequence_for_seed_12345() {
    let _guard = serial();
    pin_random_seed(None);

    let v = run_list("seed(12345)\nreturn [randi(), randi(), randi(), randi()]\n");
    assert_eq!(ints(&v), [1321476956, 17539747, 3348728241, 2863338820], "randi()");

    let v = run_list("seed(12345)\nreturn [randf(), randf(), randf(), randf()]\n");
    assert_eq!(
        float_bits(&v),
        [4599214296151359488, 4571358312406188032, 4605198011839021056, 4604180076135710720],
        "randf()"
    );

    let v = run_list(
        "seed(12345)\nreturn [randf_range(-3.5, 7.25), randf_range(-3.5, 7.25), randf_range(-3.5, 7.25), randf_range(-3.5, 7.25)]\n",
    );
    assert_eq!(
        float_bits(&v),
        [-4618076179319626536, 4611886223412725118, 4616729034502416664, -4622809327214796736],
        "randf_range(-3.5, 7.25)"
    );

    let v = run_list(
        "seed(12345)\nvar out = []\nfor i in 6:\n\tout.append(randi_range(-10, 100))\nreturn out\n",
    );
    assert_eq!(ints(&v), [79, 72, 89, 15, 56, 47], "randi_range(-10, 100)");

    let v = run_list(
        "seed(12345)\nreturn [randfn(1.0, 2.0), randfn(1.0, 2.0), randfn(1.0, 2.0), randfn(1.0, 2.0)]\n",
    );
    assert_eq!(
        float_bits(&v),
        [-4611022827862630423, 4603723601486594154, 4612259917416913723, -4628335758851322544],
        "randfn(1.0, 2.0)"
    );

    let v = run_list(
        "seed(12345)\nvar arr = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]\nvar out = []\nfor i in 6:\n\tout.append(arr.pick_random())\nreturn out\n",
    );
    assert_eq!(ints(&v), [6, 7, 1, 0, 6, 9], "pick_random()");

    let v = run_list("seed(12345)\nvar sh = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]\nsh.shuffle()\nreturn sh\n");
    assert_eq!(ints(&v), [3, 2, 5, 8, 4, 0, 9, 1, 7, 6], "shuffle()");
}

#[test]
fn rand_from_seed_is_godots() {
    let _guard = serial();
    let mut interp = Interpreter::new();
    let v = interp
        .run("return rand_from_seed(12345)\n")
        .expect("runs")
        .return_value
        .expect("value");
    assert_eq!(v, Variant::PackedInt64Array(vec![1321476956, 12345]));
}

#[test]
fn random_number_generator_draws_godots_sequence_for_seed_12345() {
    let _guard = serial();
    let v = run_list(
        "var r = RandomNumberGenerator.new()\nr.seed = 12345\nvar a = [r.randi(), r.randi_range(-10, 100)]\n\
         r.seed = 12345\nreturn a + [r.randf(), r.randf_range(-3.5, 7.25), r.randfn(1.0, 2.0)]\n",
    );
    assert_eq!(ints(&v[..2]), [1321476956, 72]);
    // randf (2 draws) then randf_range (2) then randfn (4) from one stream.
    let mut probe = gdcore::rng::Rng::with_seed(12345);
    let expect = [
        probe.randf().to_bits() as i64,
        probe.randf_range(-3.5, 7.25).to_bits() as i64,
        probe.randfn(1.0, 2.0).to_bits() as i64,
    ];
    assert_eq!(float_bits(&v[2..]), expect);
    assert_eq!(expect[0], 4598212003255812096, "Godot's first r.randf() for seed 12345");
}

/// A pinned run (the agent `play` tool, the Godot reference capture) derives
/// EVERY reseed from the base — the start, each `randomize()` and each
/// `RandomNumberGenerator.new()` — by the rule the capture applies to Godot.
#[test]
fn a_pinned_seed_makes_randomize_and_new_generators_reproducible() {
    let _guard = serial();
    let src = "randomize()\nvar a = randi()\nvar r = RandomNumberGenerator.new()\nvar s = r.seed\n\
               r.randomize()\nreturn [a, s, r.seed, randi()]\n";
    pin_random_seed(Some(42));
    let first = ints(&run_list(src));
    pin_random_seed(Some(42));
    let second = ints(&run_list(src));
    pin_random_seed(None);
    assert_eq!(first, second, "a pinned run replays exactly");
    assert_eq!(first[1], 42 + 1_000_000 + 1, "the 1st RandomNumberGenerator.new() seeds base + 1000001");
    assert_eq!(first[2], 42 + 1_000_000 + 2, "its randomize() takes the next instance seed");
    let mut after_randomize = gdcore::rng::Rng::with_seed(43);
    assert_eq!(first[0], after_randomize.randi() as i64, "the 1st randomize() seeds base + 1");

    // Unpinned, Godot's RandomNumberGenerator.new() is randomized.
    let seeds = ints(&run_list(
        "var a = RandomNumberGenerator.new()\nvar b = RandomNumberGenerator.new()\nreturn [a.seed, b.seed]\n",
    ));
    assert_ne!(seeds[0], seeds[1], "two unpinned generators must not share a seed");
}
