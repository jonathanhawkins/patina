//! pat-y2ylj — Patina's random numbers are Godot's random numbers.
//!
//! A frame-exact comparison against real Godot can only line up a game that
//! uses randomness (Dodge the Creeps' mob spawns: `randf()`, two
//! `randf_range`, `pick_random`) when both engines draw the SAME sequence for
//! the same seed. Every expected value below was printed by Godot 4.6.1
//! (`Godot --headless -s probe.gd`, the probe is quoted at the bottom); floats
//! are compared by their IEEE-754 bits, as the probe printed them
//! (`PackedFloat64Array([x]).to_byte_array().decode_u64(0)`, a signed int64).
//!
//! ⚠️ The global functions and `RandomNumberGenerator` use DIFFERENT formulas
//! in Godot (global `randf` is one draw over UINT32_MAX, the instance `randf`
//! is the two-draw clz construction; global `randf_range` is double, the
//! instance one float), so both are pinned separately.
//!
//! The global generator is process-wide, so every global assertion lives in
//! ONE test function: two tests seeding it from parallel threads would
//! interleave their draws.

use gdcore::rng::{self, Rng};

const SEED: u64 = 12345;

fn bits(v: &[i64]) -> Vec<u64> {
    v.iter().map(|&x| x as u64).collect()
}

fn f64_bits(v: impl IntoIterator<Item = f64>) -> Vec<u64> {
    v.into_iter().map(f64::to_bits).collect()
}

/// `randfn` is Box-Muller over `cos`/`ln`/`sqrt` in f64, and those come from
/// the platform's libm: macOS and glibc can round the last bit differently
/// (CI's Linux runner gave ...193 where the macOS-captured Godot value is
/// ...192). Real Godot inherits the same libm on each platform, so the
/// reference itself is only defined to the last ULP — compare within 1 ULP.
/// Every other draw stays bit-exact, so a wrong formula or stream position
/// (which moves the value by far more than one ULP) still fails.
fn assert_within_one_ulp(actual: &[f64], expected_bits: &[i64], what: &str) {
    assert_eq!(actual.len(), expected_bits.len(), "{what}: draw count");
    for (i, (&a, &e)) in actual.iter().zip(expected_bits).enumerate() {
        let e = f64::from_bits(e as u64);
        let ulps = (a.to_bits() as i64)
            .wrapping_sub(e.to_bits() as i64)
            .unsigned_abs();
        assert!(
            a.is_sign_negative() == e.is_sign_negative() && ulps <= 1,
            "{what}: draw {i} is {a:e} ({:#x}), Godot gave {e:e} ({:#x}) — {ulps} ULPs apart",
            a.to_bits(),
            e.to_bits()
        );
    }
}

#[test]
fn random_number_generator_instance_matches_godot_for_seed_12345() {
    let mut r = Rng::with_seed(SEED);
    assert_eq!(r.seed(), SEED);
    assert_eq!(
        r.state(),
        -7751912382566491497i64 as u64,
        "state after `seed = 12345`"
    );

    let randi: Vec<u32> = (0..4).map(|_| r.randi()).collect();
    assert_eq!(randi, [1321476956, 17539747, 3348728241, 2863338820]);

    let mut r = Rng::with_seed(SEED);
    r.randi();
    assert_eq!(r.state(), 2343518260280137770, "state after one draw");

    let mut r = Rng::with_seed(SEED);
    assert_eq!(
        f64_bits((0..4).map(|_| r.randf())),
        bits(&[
            4598212003255812096,
            4604180076135710720,
            4582310135950475264,
            4604860738257813504
        ]),
        "RandomNumberGenerator.randf()"
    );

    let mut r = Rng::with_seed(SEED);
    assert_eq!(
        f64_bits((0..4).map(|_| r.randf_range(-3.5, 7.25))),
        bits(&[
            -4618076180292042752,
            4615439171304030208,
            -4608867059592331264,
            4616729034271752192
        ]),
        "RandomNumberGenerator.randf_range(-3.5, 7.25)"
    );

    let mut r = Rng::with_seed(SEED);
    let ri: Vec<i64> = (0..6).map(|_| r.randi_range(-10, 100)).collect();
    assert_eq!(
        ri,
        [79, 72, 89, 15, 56, 47],
        "RandomNumberGenerator.randi_range(-10, 100)"
    );

    let mut r = Rng::with_seed(SEED);
    assert_eq!(
        f64_bits((0..4).map(|_| r.randfn(1.0, 2.0))),
        bits(&[
            -4619251153421467648,
            4604772473458655232,
            4611754636972392448,
            4605892646563479552
        ]),
        "RandomNumberGenerator.randfn(1.0, 2.0)"
    );
}

#[test]
fn rand_from_seed_matches_godot() {
    let mut s = SEED;
    assert_eq!(rng::rand_from_seed(&mut s), 1321476956);
    assert_eq!(
        s, SEED,
        "Godot writes back the seed it started from: [1321476956, 12345]"
    );
}

#[test]
fn global_random_functions_match_godot_for_seed_12345() {
    rng::pin_seed(None);

    rng::seed(SEED);
    let randi: Vec<u32> = (0..4).map(|_| rng::randi()).collect();
    assert_eq!(
        randi,
        [1321476956, 17539747, 3348728241, 2863338820],
        "randi()"
    );

    rng::seed(SEED);
    assert_eq!(
        f64_bits((0..4).map(|_| rng::randf())),
        bits(&[
            4599214296151359488,
            4571358312406188032,
            4605198011839021056,
            4604180076135710720
        ]),
        "randf() is (float)rand() / (float)UINT32_MAX"
    );

    rng::seed(SEED);
    assert_eq!(
        f64_bits((0..4).map(|_| rng::randf_range(-3.5, 7.25))),
        bits(&[
            -4618076179319626536,
            4611886223412725118,
            4616729034502416664,
            -4622809327214796736
        ]),
        "randf_range(-3.5, 7.25) is the DOUBLE randd() * (to - from) + from"
    );

    rng::seed(SEED);
    let ri: Vec<i64> = (0..6).map(|_| rng::randi_range(-10, 100)).collect();
    assert_eq!(ri, [79, 72, 89, 15, 56, 47], "randi_range(-10, 100)");

    rng::seed(SEED);
    let draws: Vec<f64> = (0..4).map(|_| rng::randfn(1.0, 2.0)).collect();
    assert_within_one_ulp(
        &draws,
        &[
            -4611022827862630423,
            4603723601486594154,
            4612259917416913723,
            -4628335758851322544,
        ],
        "randfn(1.0, 2.0)",
    );

    rng::seed(SEED);
    let picks: Vec<usize> = (0..6).map(|_| rng::pick_index(10)).collect();
    assert_eq!(picks, [6, 7, 1, 0, 6, 9], "[0..9].pick_random()");

    rng::seed(SEED);
    let mut deck: Vec<i32> = (0..10).collect();
    rng::shuffle(&mut deck);
    assert_eq!(deck, [3, 2, 5, 8, 4, 0, 9, 1, 7, 6], "[0..9].shuffle()");

    // Mixed calls share ONE stream: each function consumes exactly Godot's
    // number of draws (randf 1, randi_range 1, randf_range 3).
    rng::seed(SEED);
    assert_eq!(rng::randi(), 1321476956);
    assert_eq!(rng::randf().to_bits(), 4571358312406188032);
    assert_eq!(rng::randi_range(1, 6), 4);
    assert_eq!(rng::randf_range(0.0, 1.0).to_bits(), 4602858048925967050);
    assert_eq!(rng::randi(), 4179236141);

    rng::seed(42);
    let s42: Vec<u32> = (0..3).map(|_| rng::randi()).collect();
    assert_eq!(
        s42,
        [492690617, 1919685028, 3561993920],
        "seed(42) — the agent play tool's seed"
    );

    // Pinned seeding: the start, the k-th `randomize()` and the j-th
    // RandomNumberGenerator are all derived from the base, by the rule the
    // Godot capture (scripts/lib/godot-reference-capture.gd) applies.
    rng::pin_seed(Some(SEED));
    assert_eq!(
        rng::randi(),
        1321476956,
        "pinning seeds the global generator with the base"
    );
    assert_eq!(
        rng::randomize(),
        SEED + 1,
        "the 1st randomize() seeds base + 1"
    );
    let after_randomize = rng::randi();
    rng::seed(SEED + 1);
    assert_eq!(rng::randi(), after_randomize);
    assert_eq!(
        rng::randomize(),
        SEED + 2,
        "the 2nd randomize() seeds base + 2"
    );
    let a = Rng::new_for_script();
    assert_eq!(a.seed(), SEED + rng::PINNED_INSTANCE_OFFSET + 1);
    let mut b = Rng::new_for_script();
    assert_eq!(b.seed(), SEED + rng::PINNED_INSTANCE_OFFSET + 2);
    assert_eq!(
        b.randomize(),
        SEED + rng::PINNED_INSTANCE_OFFSET + 3,
        "an instance randomize() shares the instance counter"
    );

    // Unpinned again: randomize() and new instances come from entropy.
    rng::pin_seed(None);
    let c = Rng::new_for_script();
    let d = Rng::new_for_script();
    assert_ne!(
        c.seed(),
        d.seed(),
        "unpinned RandomNumberGenerator.new() is randomized"
    );
    assert_ne!(rng::randomize(), SEED + 3);
}

// The Godot probe (abridged; every list is 4 or 6 draws after `seed(12345)`
// or `r.seed = 12345`):
//
//     func b(x: float) -> int:
//         return PackedFloat64Array([x]).to_byte_array().decode_u64(0)
//     seed(12345); for i in 4: g.append(randi())
//     seed(12345); for i in 4: g.append(b(randf()))
//     seed(12345); for i in 4: g.append(b(randf_range(-3.5, 7.25)))
//     seed(12345); for i in 6: g.append(randi_range(-10, 100))
//     seed(12345); for i in 4: g.append(b(randfn(1.0, 2.0)))
//     seed(12345); for i in 6: g.append([0,1,2,3,4,5,6,7,8,9].pick_random())
//     seed(12345); sh.shuffle()
//     seed(12345); [randi(), b(randf()), randi_range(1, 6), b(randf_range(0.0, 1.0)), randi()]
//     var r := RandomNumberGenerator.new(); r.seed = 12345; ...the same on r...
//     rand_from_seed(12345); seed(42); [randi(), randi(), randi()]
