//! Type conversion rules between Variant types.
//!
//! Implements Godot's implicit and explicit conversion semantics.
//! Conversions that would lose data return an error rather than silently
//! truncating.

use crate::variant::{Variant, VariantType};
use thiserror::Error;

/// Error returned when a variant cannot be converted to the requested type.
#[derive(Debug, Error)]
#[error("cannot convert {from} to {to}")]
pub struct ConversionError {
    /// The source variant type.
    pub from: VariantType,
    /// The target type that was requested.
    pub to: VariantType,
}

impl ConversionError {
    fn new(from: VariantType, to: VariantType) -> Self {
        Self { from, to }
    }
}

// ---------------------------------------------------------------------------
// From impls — ergonomic construction
// ---------------------------------------------------------------------------

impl From<bool> for Variant {
    fn from(v: bool) -> Self { Self::Bool(v) }
}

impl From<i64> for Variant {
    fn from(v: i64) -> Self { Self::Int(v) }
}

impl From<i32> for Variant {
    fn from(v: i32) -> Self { Self::Int(v as i64) }
}

impl From<f64> for Variant {
    fn from(v: f64) -> Self { Self::Float(v) }
}

impl From<f32> for Variant {
    fn from(v: f32) -> Self { Self::Float(v as f64) }
}

impl From<String> for Variant {
    fn from(v: String) -> Self { Self::String(v) }
}

impl From<&str> for Variant {
    fn from(v: &str) -> Self { Self::String(v.to_owned()) }
}

impl From<gdcore::math::Vector2> for Variant {
    fn from(v: gdcore::math::Vector2) -> Self { Self::Vector2(v) }
}

impl From<gdcore::math::Vector3> for Variant {
    fn from(v: gdcore::math::Vector3) -> Self { Self::Vector3(v) }
}

impl From<gdcore::math::Color> for Variant {
    fn from(v: gdcore::math::Color) -> Self { Self::Color(v) }
}

impl From<gdcore::math3d::Basis> for Variant {
    fn from(v: gdcore::math3d::Basis) -> Self { Self::Basis(v) }
}

impl From<gdcore::math3d::Transform3D> for Variant {
    fn from(v: gdcore::math3d::Transform3D) -> Self { Self::Transform3D(v) }
}

impl From<gdcore::math3d::Quaternion> for Variant {
    fn from(v: gdcore::math3d::Quaternion) -> Self { Self::Quaternion(v) }
}

impl From<gdcore::math3d::Aabb> for Variant {
    fn from(v: gdcore::math3d::Aabb) -> Self { Self::Aabb(v) }
}

impl From<gdcore::math3d::Plane> for Variant {
    fn from(v: gdcore::math3d::Plane) -> Self { Self::Plane(v) }
}

impl From<gdcore::StringName> for Variant {
    fn from(v: gdcore::StringName) -> Self { Self::StringName(v) }
}

impl From<gdcore::NodePath> for Variant {
    fn from(v: gdcore::NodePath) -> Self { Self::NodePath(v) }
}

impl From<Vec<Variant>> for Variant {
    fn from(v: Vec<Variant>) -> Self { Self::Array(v) }
}

// ---------------------------------------------------------------------------
// TryFrom impls — extracting typed values
// ---------------------------------------------------------------------------

impl TryFrom<Variant> for bool {
    type Error = ConversionError;
    fn try_from(v: Variant) -> Result<Self, Self::Error> {
        match v {
            Variant::Bool(b) => Ok(b),
            Variant::Int(i) => Ok(i != 0),
            other => Err(ConversionError::new(other.variant_type(), VariantType::Bool)),
        }
    }
}

impl TryFrom<Variant> for i64 {
    type Error = ConversionError;
    fn try_from(v: Variant) -> Result<Self, Self::Error> {
        match v {
            Variant::Int(i) => Ok(i),
            Variant::Float(f) => Ok(f as i64),
            Variant::Bool(b) => Ok(if b { 1 } else { 0 }),
            other => Err(ConversionError::new(other.variant_type(), VariantType::Int)),
        }
    }
}

impl TryFrom<Variant> for f64 {
    type Error = ConversionError;
    fn try_from(v: Variant) -> Result<Self, Self::Error> {
        match v {
            Variant::Float(f) => Ok(f),
            Variant::Int(i) => Ok(i as f64),
            Variant::Bool(b) => Ok(if b { 1.0 } else { 0.0 }),
            other => Err(ConversionError::new(other.variant_type(), VariantType::Float)),
        }
    }
}

impl TryFrom<Variant> for String {
    type Error = ConversionError;
    fn try_from(v: Variant) -> Result<Self, Self::Error> {
        match v {
            Variant::String(s) => Ok(s),
            // Godot allows converting most types to string via str().
            other => Ok(other.to_string()),
        }
    }
}

impl TryFrom<Variant> for gdcore::StringName {
    type Error = ConversionError;
    fn try_from(v: Variant) -> Result<Self, Self::Error> {
        match v {
            Variant::StringName(sn) => Ok(sn),
            Variant::String(s) => Ok(gdcore::StringName::new(&s)),
            other => Err(ConversionError::new(other.variant_type(), VariantType::StringName)),
        }
    }
}

impl TryFrom<Variant> for gdcore::NodePath {
    type Error = ConversionError;
    fn try_from(v: Variant) -> Result<Self, Self::Error> {
        match v {
            Variant::NodePath(np) => Ok(np),
            Variant::String(s) => Ok(gdcore::NodePath::new(&s)),
            other => Err(ConversionError::new(other.variant_type(), VariantType::NodePath)),
        }
    }
}

impl TryFrom<Variant> for gdcore::math::Vector2 {
    type Error = ConversionError;
    fn try_from(v: Variant) -> Result<Self, Self::Error> {
        match v {
            Variant::Vector2(vec) => Ok(vec),
            other => Err(ConversionError::new(other.variant_type(), VariantType::Vector2)),
        }
    }
}

impl TryFrom<Variant> for gdcore::math::Vector3 {
    type Error = ConversionError;
    fn try_from(v: Variant) -> Result<Self, Self::Error> {
        match v {
            Variant::Vector3(vec) => Ok(vec),
            other => Err(ConversionError::new(other.variant_type(), VariantType::Vector3)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Vector2;

    #[test]
    fn from_primitives() {
        assert_eq!(Variant::from(true), Variant::Bool(true));
        assert_eq!(Variant::from(42_i64), Variant::Int(42));
        assert_eq!(Variant::from(1.5_f64), Variant::Float(1.5));
        assert_eq!(Variant::from("hello"), Variant::String("hello".into()));
    }

    #[test]
    fn try_from_bool() {
        assert_eq!(bool::try_from(Variant::Bool(true)).unwrap(), true);
        assert_eq!(bool::try_from(Variant::Int(0)).unwrap(), false);
        assert!(bool::try_from(Variant::String("x".into())).is_err());
    }

    #[test]
    fn try_from_int() {
        assert_eq!(i64::try_from(Variant::Int(7)).unwrap(), 7);
        assert_eq!(i64::try_from(Variant::Float(3.9)).unwrap(), 3);
        assert_eq!(i64::try_from(Variant::Bool(true)).unwrap(), 1);
    }

    #[test]
    fn try_from_float() {
        assert_eq!(f64::try_from(Variant::Float(1.5)).unwrap(), 1.5);
        assert_eq!(f64::try_from(Variant::Int(3)).unwrap(), 3.0);
    }

    #[test]
    fn try_from_string_converts_anything() {
        let s = String::try_from(Variant::Int(42)).unwrap();
        assert_eq!(s, "42");
    }

    #[test]
    fn try_from_vector2() {
        let v = Vector2::new(1.0, 2.0);
        assert_eq!(Vector2::try_from(Variant::Vector2(v)).unwrap(), v);
        assert!(Vector2::try_from(Variant::Int(0)).is_err());
    }

    #[test]
    fn from_string_name() {
        let sn = gdcore::StringName::new("test");
        assert_eq!(Variant::from(sn), Variant::StringName(sn));
    }

    #[test]
    fn try_from_string_name() {
        let sn = gdcore::StringName::new("hello");
        let v = Variant::StringName(sn);
        let extracted = gdcore::StringName::try_from(v).unwrap();
        assert_eq!(extracted, sn);
    }

    #[test]
    fn try_from_string_name_from_string() {
        let v = Variant::String("coerced".into());
        let sn = gdcore::StringName::try_from(v).unwrap();
        assert_eq!(sn.as_str(), "coerced");
    }

    #[test]
    fn from_node_path() {
        let np = gdcore::NodePath::new("/root/Player");
        assert_eq!(
            Variant::from(np.clone()),
            Variant::NodePath(gdcore::NodePath::new("/root/Player")),
        );
    }

    #[test]
    fn try_from_node_path() {
        let np = gdcore::NodePath::new("/root/Player");
        let v = Variant::NodePath(np.clone());
        let extracted = gdcore::NodePath::try_from(v).unwrap();
        assert_eq!(extracted, np);
    }

    // -- Error cases for every invalid TryFrom ------------------------------

    #[test]
    fn try_from_bool_error_on_float() {
        assert!(bool::try_from(Variant::Float(1.0)).is_err());
    }

    #[test]
    fn try_from_bool_error_on_nil() {
        assert!(bool::try_from(Variant::Nil).is_err());
    }

    #[test]
    fn try_from_bool_error_on_array() {
        assert!(bool::try_from(Variant::Array(vec![])).is_err());
    }

    #[test]
    fn try_from_int_error_on_string() {
        assert!(i64::try_from(Variant::String("42".into())).is_err());
    }

    #[test]
    fn try_from_int_error_on_nil() {
        assert!(i64::try_from(Variant::Nil).is_err());
    }

    #[test]
    fn try_from_int_error_on_vector2() {
        assert!(i64::try_from(Variant::Vector2(Vector2::ZERO)).is_err());
    }

    #[test]
    fn try_from_float_error_on_string() {
        assert!(f64::try_from(Variant::String("3.14".into())).is_err());
    }

    #[test]
    fn try_from_float_error_on_nil() {
        assert!(f64::try_from(Variant::Nil).is_err());
    }

    #[test]
    fn try_from_vector2_error_on_vector3() {
        let v3 = gdcore::math::Vector3::new(1.0, 2.0, 3.0);
        assert!(Vector2::try_from(Variant::Vector3(v3)).is_err());
    }

    #[test]
    fn try_from_vector3_error_on_vector2() {
        assert!(gdcore::math::Vector3::try_from(Variant::Vector2(Vector2::ZERO)).is_err());
    }

    #[test]
    fn try_from_vector3_error_on_string() {
        assert!(gdcore::math::Vector3::try_from(Variant::String("x".into())).is_err());
    }

    #[test]
    fn try_from_string_name_error_on_int() {
        assert!(gdcore::StringName::try_from(Variant::Int(42)).is_err());
    }

    #[test]
    fn try_from_string_name_error_on_nil() {
        assert!(gdcore::StringName::try_from(Variant::Nil).is_err());
    }

    #[test]
    fn try_from_node_path_error_on_int() {
        assert!(gdcore::NodePath::try_from(Variant::Int(42)).is_err());
    }

    #[test]
    fn try_from_node_path_from_string_coercion() {
        let v = Variant::String("/root/Player".into());
        let np = gdcore::NodePath::try_from(v).unwrap();
        assert!(np.is_absolute());
        assert_eq!(np.get_name_count(), 2);
    }

    // -- Roundtrip conversions ----------------------------------------------

    #[test]
    fn i32_to_variant_to_i64_roundtrip() {
        let original: i32 = 42;
        let v = Variant::from(original);
        let back = i64::try_from(v).unwrap();
        assert_eq!(back, 42_i64);
    }

    #[test]
    fn f32_to_variant_to_f64_roundtrip() {
        let original: f32 = 1.5;
        let v = Variant::from(original);
        let back = f64::try_from(v).unwrap();
        assert!((back - 1.5).abs() < 1e-6);
    }

    #[test]
    fn bool_int_roundtrip() {
        // true -> Int(1) -> back to bool
        let v = Variant::Int(1);
        assert_eq!(bool::try_from(v).unwrap(), true);
        let v = Variant::Int(0);
        assert_eq!(bool::try_from(v).unwrap(), false);
    }

    #[test]
    fn float_to_int_truncation() {
        assert_eq!(i64::try_from(Variant::Float(3.9)).unwrap(), 3);
        assert_eq!(i64::try_from(Variant::Float(-1.7)).unwrap(), -1);
    }

    #[test]
    fn bool_to_float() {
        assert_eq!(f64::try_from(Variant::Bool(true)).unwrap(), 1.0);
        assert_eq!(f64::try_from(Variant::Bool(false)).unwrap(), 0.0);
    }

    // -- ConversionError display --------------------------------------------

    #[test]
    fn conversion_error_display() {
        let err = ConversionError {
            from: VariantType::String,
            to: VariantType::Int,
        };
        assert_eq!(format!("{err}"), "cannot convert String to int");
    }

    // -- From impls ---------------------------------------------------------

    #[test]
    fn from_vec_variant() {
        let v = Variant::from(vec![Variant::Int(1), Variant::Bool(true)]);
        match v {
            Variant::Array(items) => assert_eq!(items.len(), 2),
            _ => panic!("expected Array"),
        }
    }

    #[test]
    fn from_color() {
        let c = gdcore::math::Color::WHITE;
        let v = Variant::from(c);
        assert_eq!(v.variant_type(), VariantType::Color);
    }

    #[test]
    fn from_vector3() {
        let v3 = gdcore::math::Vector3::ONE;
        let v = Variant::from(v3);
        assert_eq!(v.variant_type(), VariantType::Vector3);
    }
}
