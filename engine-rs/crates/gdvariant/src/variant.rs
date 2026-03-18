//! Core Variant enum and value container types.
//!
//! Godot's Variant is a tagged union that can hold any engine value.
//! This implementation covers the subset needed for Phase 3 (headless
//! runtime): scalars, strings, math types, collections, and object refs.

use gdcore::math::{Color, Rect2, Transform2D, Vector2, Vector3};
use gdcore::id::ObjectId;
use gdcore::node_path::NodePath;
use gdcore::string_name::StringName;
use std::collections::HashMap;
use std::fmt;

/// The set of type tags a Variant can carry.
///
/// Mirrors `Variant::Type` from upstream Godot. We start with the
/// types needed for scene/resource work and expand as needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VariantType {
    Nil,
    Bool,
    Int,
    Float,
    String,
    StringName,
    NodePath,
    Vector2,
    Vector3,
    Rect2,
    Transform2D,
    Color,
    ObjectId,
    Array,
    Dictionary,
}

/// A dynamically-typed engine value, analogous to Godot's `Variant`.
#[derive(Debug, Clone, PartialEq)]
pub enum Variant {
    /// The null / default value.
    Nil,
    /// Boolean.
    Bool(bool),
    /// 64-bit signed integer (Godot uses i64 internally).
    Int(i64),
    /// 64-bit float (Godot uses f64 for Variant floats).
    Float(f64),
    /// A UTF-8 string.
    String(String),
    /// An interned string name.
    StringName(StringName),
    /// A scene-tree node path.
    NodePath(NodePath),
    /// A 2D vector.
    Vector2(Vector2),
    /// A 3D vector.
    Vector3(Vector3),
    /// An axis-aligned 2D rectangle.
    Rect2(Rect2),
    /// A 2D affine transform.
    Transform2D(Transform2D),
    /// An RGBA color.
    Color(Color),
    /// A reference to an engine object by ID.
    ObjectId(ObjectId),
    /// A heterogeneous ordered list.
    Array(Vec<Variant>),
    /// A string-keyed map of variants.
    Dictionary(HashMap<String, Variant>),
}

impl Variant {
    /// Returns the type tag for this value.
    pub fn variant_type(&self) -> VariantType {
        match self {
            Self::Nil => VariantType::Nil,
            Self::Bool(_) => VariantType::Bool,
            Self::Int(_) => VariantType::Int,
            Self::Float(_) => VariantType::Float,
            Self::String(_) => VariantType::String,
            Self::StringName(_) => VariantType::StringName,
            Self::NodePath(_) => VariantType::NodePath,
            Self::Vector2(_) => VariantType::Vector2,
            Self::Vector3(_) => VariantType::Vector3,
            Self::Rect2(_) => VariantType::Rect2,
            Self::Transform2D(_) => VariantType::Transform2D,
            Self::Color(_) => VariantType::Color,
            Self::ObjectId(_) => VariantType::ObjectId,
            Self::Array(_) => VariantType::Array,
            Self::Dictionary(_) => VariantType::Dictionary,
        }
    }

    /// Returns `true` if this variant is `Nil`.
    pub fn is_nil(&self) -> bool {
        matches!(self, Self::Nil)
    }

    /// Returns the Godot-style "truthiness" of this value.
    ///
    /// Godot rules: Nil/false/0/0.0/empty-string/empty-array/empty-dict → false.
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Nil => false,
            Self::Bool(b) => *b,
            Self::Int(i) => *i != 0,
            Self::Float(f) => *f != 0.0,
            Self::String(s) => !s.is_empty(),
            Self::StringName(sn) => !sn.as_str().is_empty(),
            Self::NodePath(np) => !np.is_empty(),
            Self::Array(a) => !a.is_empty(),
            Self::Dictionary(d) => !d.is_empty(),
            _ => true,
        }
    }
}

impl Default for Variant {
    fn default() -> Self {
        Self::Nil
    }
}

impl fmt::Display for Variant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nil => write!(f, "<null>"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Int(i) => write!(f, "{i}"),
            Self::Float(v) => write!(f, "{v}"),
            Self::String(s) => write!(f, "{s}"),
            Self::StringName(sn) => write!(f, "&{sn}"),
            Self::NodePath(np) => write!(f, "NodePath(\"{np}\")"),
            Self::Vector2(v) => write!(f, "({}, {})", v.x, v.y),
            Self::Vector3(v) => write!(f, "({}, {}, {})", v.x, v.y, v.z),
            Self::Rect2(r) => write!(f, "[({}, {}), ({}, {})]", r.position.x, r.position.y, r.size.x, r.size.y),
            Self::Transform2D(_) => write!(f, "<Transform2D>"),
            Self::Color(c) => write!(f, "Color({}, {}, {}, {})", c.r, c.g, c.b, c.a),
            Self::ObjectId(id) => write!(f, "<Object#{id}>"),
            Self::Array(a) => write!(f, "[Array; len={}]", a.len()),
            Self::Dictionary(d) => write!(f, "{{Dict; len={}}}", d.len()),
        }
    }
}

impl fmt::Display for VariantType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Nil => "Nil",
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Float => "float",
            Self::String => "String",
            Self::StringName => "StringName",
            Self::NodePath => "NodePath",
            Self::Vector2 => "Vector2",
            Self::Vector3 => "Vector3",
            Self::Rect2 => "Rect2",
            Self::Transform2D => "Transform2D",
            Self::Color => "Color",
            Self::ObjectId => "ObjectId",
            Self::Array => "Array",
            Self::Dictionary => "Dictionary",
        };
        write!(f, "{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_type_tag() {
        assert_eq!(Variant::Nil.variant_type(), VariantType::Nil);
        assert_eq!(Variant::Int(42).variant_type(), VariantType::Int);
        assert_eq!(Variant::String("hi".into()).variant_type(), VariantType::String);
    }

    #[test]
    fn truthy_falsy() {
        assert!(!Variant::Nil.is_truthy());
        assert!(!Variant::Bool(false).is_truthy());
        assert!(Variant::Bool(true).is_truthy());
        assert!(!Variant::Int(0).is_truthy());
        assert!(Variant::Int(1).is_truthy());
        assert!(!Variant::Float(0.0).is_truthy());
        assert!(Variant::Float(0.1).is_truthy());
        assert!(!Variant::String(String::new()).is_truthy());
        assert!(Variant::String("x".into()).is_truthy());
        assert!(!Variant::Array(vec![]).is_truthy());
        assert!(Variant::Array(vec![Variant::Nil]).is_truthy());
        // Math types are always truthy (even zero vectors).
        assert!(Variant::Vector2(Vector2::ZERO).is_truthy());
    }

    #[test]
    fn default_is_nil() {
        assert!(Variant::default().is_nil());
    }

    #[test]
    fn variant_string_name_type_tag() {
        let sn = StringName::new("test");
        assert_eq!(Variant::StringName(sn).variant_type(), VariantType::StringName);
    }

    #[test]
    fn variant_node_path_type_tag() {
        let np = NodePath::new("/root/Player");
        assert_eq!(Variant::NodePath(np).variant_type(), VariantType::NodePath);
    }

    #[test]
    fn string_name_truthy() {
        assert!(Variant::StringName(StringName::new("x")).is_truthy());
        assert!(!Variant::StringName(StringName::new("")).is_truthy());
    }

    #[test]
    fn node_path_truthy() {
        assert!(Variant::NodePath(NodePath::new("/root")).is_truthy());
        assert!(!Variant::NodePath(NodePath::new("")).is_truthy());
    }

    // -- Display for every variant type -------------------------------------

    #[test]
    fn display_nil() {
        assert_eq!(format!("{}", Variant::Nil), "<null>");
    }

    #[test]
    fn display_bool() {
        assert_eq!(format!("{}", Variant::Bool(true)), "true");
        assert_eq!(format!("{}", Variant::Bool(false)), "false");
    }

    #[test]
    fn display_int() {
        assert_eq!(format!("{}", Variant::Int(42)), "42");
        assert_eq!(format!("{}", Variant::Int(-7)), "-7");
    }

    #[test]
    fn display_float() {
        assert_eq!(format!("{}", Variant::Float(3.14)), "3.14");
    }

    #[test]
    fn display_string() {
        assert_eq!(format!("{}", Variant::String("hello".into())), "hello");
    }

    #[test]
    fn display_string_name() {
        let sn = StringName::new("test_sn");
        assert_eq!(format!("{}", Variant::StringName(sn)), "&test_sn");
    }

    #[test]
    fn display_node_path() {
        let np = NodePath::new("/root/Player");
        assert_eq!(format!("{}", Variant::NodePath(np)), "NodePath(\"/root/Player\")");
    }

    #[test]
    fn display_vector2() {
        let v = Vector2::new(1.0, 2.0);
        assert_eq!(format!("{}", Variant::Vector2(v)), "(1, 2)");
    }

    #[test]
    fn display_vector3() {
        let v = gdcore::math::Vector3::new(1.0, 2.0, 3.0);
        assert_eq!(format!("{}", Variant::Vector3(v)), "(1, 2, 3)");
    }

    #[test]
    fn display_rect2() {
        let r = gdcore::math::Rect2::new(Vector2::new(1.0, 2.0), Vector2::new(3.0, 4.0));
        assert_eq!(format!("{}", Variant::Rect2(r)), "[(1, 2), (3, 4)]");
    }

    #[test]
    fn display_transform2d() {
        let t = gdcore::math::Transform2D::IDENTITY;
        assert_eq!(format!("{}", Variant::Transform2D(t)), "<Transform2D>");
    }

    #[test]
    fn display_color() {
        let c = gdcore::math::Color::new(1.0, 0.5, 0.0, 1.0);
        assert_eq!(format!("{}", Variant::Color(c)), "Color(1, 0.5, 0, 1)");
    }

    #[test]
    fn display_object_id() {
        let id = gdcore::id::ObjectId::from_raw(99);
        assert_eq!(format!("{}", Variant::ObjectId(id)), "<Object#99>");
    }

    #[test]
    fn display_array() {
        let a = Variant::Array(vec![Variant::Int(1), Variant::Int(2)]);
        assert_eq!(format!("{a}"), "[Array; len=2]");
    }

    #[test]
    fn display_dictionary() {
        let mut d = std::collections::HashMap::new();
        d.insert("key".to_string(), Variant::Int(1));
        let v = Variant::Dictionary(d);
        assert_eq!(format!("{v}"), "{Dict; len=1}");
    }

    // -- Clone correctness --------------------------------------------------

    #[test]
    fn clone_nested_array() {
        let inner = Variant::Array(vec![Variant::Int(1), Variant::Int(2)]);
        let outer = Variant::Array(vec![inner.clone(), Variant::String("x".into())]);
        let cloned = outer.clone();
        assert_eq!(outer, cloned);
    }

    #[test]
    fn clone_nested_dictionary() {
        let mut inner = std::collections::HashMap::new();
        inner.insert("a".into(), Variant::Int(1));
        let outer = Variant::Dictionary(inner);
        let cloned = outer.clone();
        assert_eq!(outer, cloned);
    }

    // -- Truthy for remaining types -----------------------------------------

    #[test]
    fn dictionary_truthy() {
        assert!(!Variant::Dictionary(std::collections::HashMap::new()).is_truthy());
        let mut d = std::collections::HashMap::new();
        d.insert("k".into(), Variant::Nil);
        assert!(Variant::Dictionary(d).is_truthy());
    }

    #[test]
    fn object_id_is_always_truthy() {
        assert!(Variant::ObjectId(gdcore::id::ObjectId::from_raw(0)).is_truthy());
    }

    #[test]
    fn color_is_always_truthy() {
        assert!(Variant::Color(gdcore::math::Color::TRANSPARENT).is_truthy());
    }

    // -- VariantType Display ------------------------------------------------

    #[test]
    fn variant_type_display_all() {
        assert_eq!(format!("{}", VariantType::Nil), "Nil");
        assert_eq!(format!("{}", VariantType::Bool), "bool");
        assert_eq!(format!("{}", VariantType::Int), "int");
        assert_eq!(format!("{}", VariantType::Float), "float");
        assert_eq!(format!("{}", VariantType::String), "String");
        assert_eq!(format!("{}", VariantType::StringName), "StringName");
        assert_eq!(format!("{}", VariantType::NodePath), "NodePath");
        assert_eq!(format!("{}", VariantType::Vector2), "Vector2");
        assert_eq!(format!("{}", VariantType::Vector3), "Vector3");
        assert_eq!(format!("{}", VariantType::Rect2), "Rect2");
        assert_eq!(format!("{}", VariantType::Transform2D), "Transform2D");
        assert_eq!(format!("{}", VariantType::Color), "Color");
        assert_eq!(format!("{}", VariantType::ObjectId), "ObjectId");
        assert_eq!(format!("{}", VariantType::Array), "Array");
        assert_eq!(format!("{}", VariantType::Dictionary), "Dictionary");
    }
}
