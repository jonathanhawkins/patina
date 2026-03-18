//! Property-based tests for the Patina Engine.
//!
//! Uses a simple PRNG to generate random Variant values and verify
//! serialization invariants across many iterations.

use gdcore::math::{Color, Rect2, Vector2, Vector3};
use gdcore::node_path::NodePath;
use gdcore::string_name::StringName;
use gdvariant::serialize::{from_json, to_json};
use gdvariant::variant::{Variant, VariantType};

/// Simple xorshift64 PRNG — no external deps needed.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn next_f32(&mut self) -> f32 {
        (self.next_u64() % 10000) as f32 / 100.0 - 50.0
    }

    fn next_bool(&mut self) -> bool {
        self.next_u64() % 2 == 0
    }
}

/// Generates a random Variant of a randomly chosen type.
fn random_variant(rng: &mut Rng) -> Variant {
    // We limit to the types that roundtrip correctly through JSON.
    // Transform2D is excluded because it doesn't have from_json support.
    match rng.next_u64() % 14 {
        0 => Variant::Nil,
        1 => Variant::Bool(rng.next_bool()),
        2 => Variant::Int(rng.next_u64() as i64 % 100_000 - 50_000),
        3 => Variant::Float(rng.next_f32() as f64),
        4 => {
            let len = (rng.next_u64() % 20) as usize;
            let s: String = (0..len).map(|_| (b'a' + (rng.next_u64() % 26) as u8) as char).collect();
            Variant::String(s)
        }
        5 => Variant::Vector2(Vector2::new(rng.next_f32(), rng.next_f32())),
        6 => Variant::Vector3(Vector3::new(rng.next_f32(), rng.next_f32(), rng.next_f32())),
        7 => Variant::Color(Color::new(
            rng.next_f32().abs() / 50.0,
            rng.next_f32().abs() / 50.0,
            rng.next_f32().abs() / 50.0,
            rng.next_f32().abs() / 50.0,
        )),
        8 => Variant::Rect2(Rect2::new(
            Vector2::new(rng.next_f32(), rng.next_f32()),
            Vector2::new(rng.next_f32(), rng.next_f32()),
        )),
        9 => {
            let len = (rng.next_u64() % 5) as usize;
            let items: Vec<Variant> = (0..len)
                .map(|_| {
                    // Limit nesting depth: only generate scalar types inside collections.
                    match rng.next_u64() % 4 {
                        0 => Variant::Int(rng.next_u64() as i64 % 1000),
                        1 => Variant::Bool(rng.next_bool()),
                        2 => Variant::Float(rng.next_f32() as f64),
                        _ => Variant::String("inner".into()),
                    }
                })
                .collect();
            Variant::Array(items)
        }
        10 => {
            let s = format!("sn_{}", rng.next_u64() % 1000);
            Variant::StringName(StringName::new(&s))
        }
        11 => {
            let s = format!("/root/node_{}", rng.next_u64() % 1000);
            Variant::NodePath(NodePath::new(&s))
        }
        12 => Variant::ObjectId(gdcore::id::ObjectId::from_raw(rng.next_u64() % 10000)),
        _ => {
            let q = gdcore::math3d::Quaternion::new(
                rng.next_f32(),
                rng.next_f32(),
                rng.next_f32(),
                rng.next_f32(),
            );
            Variant::Quaternion(q)
        }
    }
}

// ---------------------------------------------------------------------------
// Property: JSON roundtrip preserves value
// ---------------------------------------------------------------------------

#[test]
fn property_json_roundtrip_100_iterations() {
    let mut rng = Rng::new(0xDEAD_BEEF);

    for i in 0..100 {
        let original = random_variant(&mut rng);
        let json = to_json(&original);
        let restored = from_json(&json);

        assert!(
            restored.is_some(),
            "Iteration {i}: from_json returned None for {original:?}, json={json}"
        );
        assert_eq!(
            original,
            restored.unwrap(),
            "Iteration {i}: roundtrip mismatch for {original:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Property: Serialization is idempotent (serialize twice → same JSON)
// ---------------------------------------------------------------------------

#[test]
fn property_serialize_idempotent_100_iterations() {
    let mut rng = Rng::new(0xCAFE_BABE);

    for i in 0..100 {
        let v = random_variant(&mut rng);
        let json1 = to_json(&v);
        let json2 = to_json(&v);

        assert_eq!(
            json1, json2,
            "Iteration {i}: double-serialize produced different JSON for {v:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Property: variant_type() is preserved after roundtrip
// ---------------------------------------------------------------------------

#[test]
fn property_type_tag_preserved_100_iterations() {
    let mut rng = Rng::new(0xBAAD_F00D);

    for i in 0..100 {
        let original = random_variant(&mut rng);
        let original_type = original.variant_type();

        let json = to_json(&original);
        let restored = from_json(&json).expect(&format!("Iteration {i}: from_json returned None"));

        assert_eq!(
            original_type,
            restored.variant_type(),
            "Iteration {i}: variant_type changed after roundtrip: {original_type:?} -> {:?}",
            restored.variant_type()
        );
    }
}

// ---------------------------------------------------------------------------
// Property: every VariantType can be generated and roundtripped
// ---------------------------------------------------------------------------

#[test]
fn property_all_types_representable() {
    let mut rng = Rng::new(42);

    // Generate enough variants to statistically cover all types.
    let mut seen = std::collections::HashSet::new();
    for _ in 0..1000 {
        let v = random_variant(&mut rng);
        seen.insert(v.variant_type());
    }

    // We generate 14 types in random_variant.
    let expected_types = vec![
        VariantType::Nil,
        VariantType::Bool,
        VariantType::Int,
        VariantType::Float,
        VariantType::String,
        VariantType::Vector2,
        VariantType::Vector3,
        VariantType::Color,
        VariantType::Rect2,
        VariantType::Array,
        VariantType::StringName,
        VariantType::NodePath,
        VariantType::ObjectId,
        VariantType::Quaternion,
    ];

    for ty in &expected_types {
        assert!(seen.contains(ty), "Type {:?} was never generated", ty);
    }
}

// ---------------------------------------------------------------------------
// Property: from_json(to_json(v)) roundtrip through string form
// ---------------------------------------------------------------------------

#[test]
fn property_json_string_roundtrip() {
    let mut rng = Rng::new(0x1234_5678);

    for i in 0..50 {
        let v = random_variant(&mut rng);
        let json_value = to_json(&v);
        // Serialize to string and back to Value.
        let json_string = serde_json::to_string(&json_value).unwrap();
        let parsed_value: serde_json::Value = serde_json::from_str(&json_string).unwrap();
        let restored = from_json(&parsed_value);

        assert!(
            restored.is_some(),
            "Iteration {i}: string roundtrip failed for {v:?}"
        );
        assert_eq!(v, restored.unwrap(), "Iteration {i}: string roundtrip mismatch");
    }
}
