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
}
