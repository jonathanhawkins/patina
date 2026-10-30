//! Godot-compatible [`Dictionary`] value: an insertion-ordered map keyed by
//! arbitrary [`Variant`] values.
//!
//! Godot's `Dictionary` differs from a plain `HashMap<String, Variant>` in two
//! important ways that this type reproduces:
//!
//! 1. **Variant keys.** Keys may be any hashable Variant (`int`, `float`,
//!    `String`, `Vector2`, …), not just strings. The previous implementation
//!    stringified keys, so `1` (int) and `"1"` (string) collided and typed keys
//!    such as vectors were impossible. Here each key keeps its type.
//! 2. **Insertion order.** Iteration follows insertion order (like Godot / an
//!    `OrderedDict`), not the random order of a `HashMap`. Re-assigning an
//!    existing key keeps its original position, matching Godot.
//!
//! Numeric keys follow Godot's rule that `1` and `1.0` are the *same* key: an
//! integral float hashes and compares equal to the corresponding int.
//!
//! Storage is a `Vec` of entries (for order) plus a `HashMap` from key to
//! position (for O(1) lookup). Game-script dictionaries are typically small, so
//! the occasional O(n) index rebuild on removal is negligible.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use crate::variant::Variant;

/// Hash/eq wrapper so `Variant` keys can live in a `HashMap` index even though
/// `Variant` itself is not `Eq`/`Hash` (it contains floats).
#[derive(Clone)]
struct DictKey(Variant);

impl PartialEq for DictKey {
    fn eq(&self, other: &Self) -> bool {
        key_eq(&self.0, &other.0)
    }
}

impl Eq for DictKey {}

impl Hash for DictKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        key_hash(&self.0, state);
    }
}

/// Godot-style key equality. Numeric keys unify (`1 == 1.0`); everything else
/// falls back to `Variant`'s structural equality.
fn key_eq(a: &Variant, b: &Variant) -> bool {
    match (a, b) {
        (Variant::Int(x), Variant::Int(y)) => x == y,
        (Variant::Float(x), Variant::Float(y)) => x == y,
        (Variant::Int(x), Variant::Float(y)) | (Variant::Float(y), Variant::Int(x)) => {
            (*x as f64) == *y
        }
        _ => a == b,
    }
}

/// Returns `true` if `f` represents an exact `i64`; such floats hash into the
/// integer bucket so that `1.0` and `1` share a slot.
fn float_is_integral(f: f64) -> bool {
    f.is_finite() && f.fract() == 0.0 && f >= i64::MIN as f64 && f <= i64::MAX as f64
}

/// Key hashing consistent with [`key_eq`]. Integral numbers share a bucket;
/// strings and bools get their own; every other Variant kind is bucketed by its
/// discriminant (equality still disambiguates within the bucket).
fn key_hash<H: Hasher>(v: &Variant, state: &mut H) {
    match v {
        Variant::Int(i) => {
            0u8.hash(state);
            i.hash(state);
        }
        Variant::Float(f) if float_is_integral(*f) => {
            0u8.hash(state);
            (*f as i64).hash(state);
        }
        Variant::Float(f) => {
            1u8.hash(state);
            f.to_bits().hash(state);
        }
        Variant::String(s) => {
            2u8.hash(state);
            s.hash(state);
        }
        Variant::Bool(b) => {
            3u8.hash(state);
            b.hash(state);
        }
        other => {
            // Per-kind bucket; `key_eq` distinguishes distinct keys inside it.
            std::mem::discriminant(other).hash(state);
        }
    }
}

/// An insertion-ordered, Variant-keyed map — Patina's equivalent of Godot's
/// `Dictionary`.
#[derive(Clone, Default)]
pub struct Dictionary {
    /// Key/value pairs in insertion order.
    entries: Vec<(Variant, Variant)>,
    /// Key → index into `entries` for O(1) lookup.
    index: HashMap<DictKey, usize>,
}

impl Dictionary {
    /// Creates an empty dictionary.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an empty dictionary with room for `cap` entries.
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            entries: Vec::with_capacity(cap),
            index: HashMap::with_capacity(cap),
        }
    }

    /// Number of key/value pairs.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the dictionary has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Removes every entry.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.index.clear();
    }

    fn position(&self, key: &Variant) -> Option<usize> {
        self.index.get(&DictKey(key.clone())).copied()
    }

    /// Inserts `key`/`value`, returning the previous value if the key existed.
    /// Re-inserting an existing key keeps its original position (Godot
    /// semantics).
    pub fn insert(
        &mut self,
        key: impl Into<Variant>,
        value: impl Into<Variant>,
    ) -> Option<Variant> {
        let key = key.into();
        let value = value.into();
        if let Some(pos) = self.position(&key) {
            let old = std::mem::replace(&mut self.entries[pos].1, value);
            Some(old)
        } else {
            let pos = self.entries.len();
            self.index.insert(DictKey(key.clone()), pos);
            self.entries.push((key, value));
            None
        }
    }

    /// Returns a reference to the value stored for `key`.
    pub fn get(&self, key: impl Into<Variant>) -> Option<&Variant> {
        let pos = self.index.get(&DictKey(key.into()))?;
        Some(&self.entries[*pos].1)
    }

    /// Returns a mutable reference to the value stored for `key`.
    pub fn get_mut(&mut self, key: impl Into<Variant>) -> Option<&mut Variant> {
        let pos = *self.index.get(&DictKey(key.into()))?;
        Some(&mut self.entries[pos].1)
    }

    /// Returns `true` if `key` is present.
    pub fn contains_key(&self, key: impl Into<Variant>) -> bool {
        self.index.contains_key(&DictKey(key.into()))
    }

    /// Godot alias for [`contains_key`](Self::contains_key).
    pub fn has(&self, key: impl Into<Variant>) -> bool {
        self.contains_key(key)
    }

    /// Returns `true` if every key in `keys` is present (Godot `has_all`).
    pub fn has_all(&self, keys: &[Variant]) -> bool {
        keys.iter().all(|k| self.contains_key(k))
    }

    /// Removes `key`, returning its value if present. Remaining entries keep
    /// their relative order (Godot `erase`).
    pub fn remove(&mut self, key: impl Into<Variant>) -> Option<Variant> {
        let pos = self.index.remove(&DictKey(key.into()))?;
        let (_, value) = self.entries.remove(pos);
        // Positions after `pos` shifted down by one; rebuild the index.
        for (i, (k, _)) in self.entries.iter().enumerate().skip(pos) {
            self.index.insert(DictKey(k.clone()), i);
        }
        Some(value)
    }

    /// Returns the existing value for `key`, or inserts and returns `default`
    /// (Godot `get_or_add`).
    pub fn get_or_add(&mut self, key: impl Into<Variant>, default: impl Into<Variant>) -> Variant {
        let key = key.into();
        if let Some(pos) = self.position(&key) {
            self.entries[pos].1.clone()
        } else {
            let value = default.into();
            self.insert(key, value.clone());
            value
        }
    }

    /// Returns the first key mapping to `value`, or `Nil` if none (Godot
    /// `find_key`).
    pub fn find_key(&self, value: &Variant) -> Variant {
        self.entries
            .iter()
            .find(|(_, v)| v == value)
            .map(|(k, _)| k.clone())
            .unwrap_or(Variant::Nil)
    }

    /// Copies entries from `other`. When `overwrite` is `false`, keys already
    /// present in `self` are left unchanged (Godot `merge`).
    pub fn merge(&mut self, other: &Dictionary, overwrite: bool) {
        for (k, v) in other.iter() {
            if overwrite || !self.contains_key(k) {
                self.insert(k.clone(), v.clone());
            }
        }
    }

    /// Returns a new dictionary equal to `self` merged with `other` (Godot
    /// `merged`). Does not mutate either input.
    pub fn merged(&self, other: &Dictionary, overwrite: bool) -> Dictionary {
        let mut out = self.clone();
        out.merge(other, overwrite);
        out
    }

    /// Returns a copy. When `deep` is `true`, nested arrays and dictionaries are
    /// recursively cloned (Godot `duplicate(true)`).
    pub fn duplicate(&self, deep: bool) -> Dictionary {
        if !deep {
            return self.clone();
        }
        let mut out = Dictionary::with_capacity(self.len());
        for (k, v) in self.iter() {
            out.insert(k.clone(), deep_clone_variant(v));
        }
        out
    }

    /// Iterates entries in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&Variant, &Variant)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    /// Iterates keys in insertion order.
    pub fn keys(&self) -> impl Iterator<Item = &Variant> {
        self.entries.iter().map(|(k, _)| k)
    }

    /// Iterates values in insertion order.
    pub fn values(&self) -> impl Iterator<Item = &Variant> {
        self.entries.iter().map(|(_, v)| v)
    }

    /// Iterates values mutably in insertion order.
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut Variant> {
        self.entries.iter_mut().map(|(_, v)| v)
    }
}

/// Recursively deep-clones nested containers for `duplicate(true)`.
fn deep_clone_variant(v: &Variant) -> Variant {
    match v {
        Variant::Array(a) => Variant::Array(a.to_vec().iter().map(deep_clone_variant).collect()),
        Variant::Dictionary(d) => Variant::Dictionary(d.duplicate(true)),
        other => other.clone(),
    }
}

impl std::fmt::Debug for Dictionary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map()
            .entries(self.entries.iter().map(|(k, v)| (k, v)))
            .finish()
    }
}

impl PartialEq for Dictionary {
    /// Order-independent equality: same size and every key maps to an equal
    /// value in the other (Godot `==`).
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }
        self.entries.iter().all(|(k, v)| other.get(k) == Some(v))
    }
}

impl FromIterator<(Variant, Variant)> for Dictionary {
    fn from_iter<I: IntoIterator<Item = (Variant, Variant)>>(iter: I) -> Self {
        let mut d = Dictionary::new();
        for (k, v) in iter {
            d.insert(k, v);
        }
        d
    }
}

impl FromIterator<(String, Variant)> for Dictionary {
    fn from_iter<I: IntoIterator<Item = (String, Variant)>>(iter: I) -> Self {
        let mut d = Dictionary::new();
        for (k, v) in iter {
            d.insert(k, v);
        }
        d
    }
}

impl From<HashMap<String, Variant>> for Dictionary {
    fn from(map: HashMap<String, Variant>) -> Self {
        map.into_iter().collect()
    }
}

impl<'a> IntoIterator for &'a Dictionary {
    type Item = (&'a Variant, &'a Variant);
    type IntoIter = std::iter::Map<
        std::slice::Iter<'a, (Variant, Variant)>,
        fn(&'a (Variant, Variant)) -> (&'a Variant, &'a Variant),
    >;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter().map(|(k, v)| (k, v))
    }
}

impl IntoIterator for Dictionary {
    type Item = (Variant, Variant);
    type IntoIter = std::vec::IntoIter<(Variant, Variant)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

impl std::ops::Index<&str> for Dictionary {
    type Output = Variant;

    fn index(&self, key: &str) -> &Variant {
        self.get(key).expect("dictionary key not found")
    }
}

impl std::ops::Index<&Variant> for Dictionary {
    type Output = Variant;

    fn index(&self, key: &Variant) -> &Variant {
        self.get(key).expect("dictionary key not found")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insertion_order_preserved() {
        let mut d = Dictionary::new();
        d.insert("z", Variant::Int(1));
        d.insert("a", Variant::Int(2));
        d.insert("m", Variant::Int(3));
        let keys: Vec<&Variant> = d.keys().collect();
        assert_eq!(
            keys,
            vec![
                &Variant::String("z".into()),
                &Variant::String("a".into()),
                &Variant::String("m".into()),
            ]
        );
    }

    #[test]
    fn reassign_keeps_position_and_returns_old() {
        let mut d = Dictionary::new();
        d.insert("a", Variant::Int(1));
        d.insert("b", Variant::Int(2));
        let old = d.insert("a", Variant::Int(9));
        assert_eq!(old, Some(Variant::Int(1)));
        let keys: Vec<&Variant> = d.keys().collect();
        assert_eq!(keys[0], &Variant::String("a".into()));
        assert_eq!(d.get("a"), Some(&Variant::Int(9)));
    }

    #[test]
    fn typed_keys_do_not_collide() {
        let mut d = Dictionary::new();
        d.insert(Variant::Int(1), Variant::String("int-one".into()));
        d.insert(
            Variant::String("1".into()),
            Variant::String("str-one".into()),
        );
        assert_eq!(d.len(), 2);
        assert_eq!(
            d.get(Variant::Int(1)),
            Some(&Variant::String("int-one".into()))
        );
        assert_eq!(
            d.get(Variant::String("1".into())),
            Some(&Variant::String("str-one".into()))
        );
    }

    #[test]
    fn int_and_integral_float_are_same_key() {
        let mut d = Dictionary::new();
        d.insert(Variant::Int(2), Variant::String("two".into()));
        // 2.0 addresses the same slot as 2.
        assert_eq!(
            d.get(Variant::Float(2.0)),
            Some(&Variant::String("two".into()))
        );
        let old = d.insert(Variant::Float(2.0), Variant::String("TWO".into()));
        assert_eq!(old, Some(Variant::String("two".into())));
        assert_eq!(d.len(), 1);
        // A non-integral float is a distinct key.
        d.insert(Variant::Float(2.5), Variant::Int(0));
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn vector_keys_supported() {
        use gdcore::math::Vector2;
        let mut d = Dictionary::new();
        d.insert(Variant::Vector2(Vector2::new(1.0, 2.0)), Variant::Int(10));
        d.insert(Variant::Vector2(Vector2::new(3.0, 4.0)), Variant::Int(20));
        assert_eq!(d.len(), 2);
        assert_eq!(
            d.get(Variant::Vector2(Vector2::new(3.0, 4.0))),
            Some(&Variant::Int(20))
        );
    }

    #[test]
    fn remove_preserves_order() {
        let mut d = Dictionary::new();
        for (k, v) in [("a", 1), ("b", 2), ("c", 3), ("d", 4)] {
            d.insert(k, Variant::Int(v));
        }
        assert_eq!(d.remove("b"), Some(Variant::Int(2)));
        assert_eq!(d.remove("z"), None);
        let keys: Vec<&Variant> = d.keys().collect();
        assert_eq!(
            keys,
            vec![
                &Variant::String("a".into()),
                &Variant::String("c".into()),
                &Variant::String("d".into()),
            ]
        );
        // Index still resolves the shifted entries.
        assert_eq!(d.get("d"), Some(&Variant::Int(4)));
        assert_eq!(d.get("c"), Some(&Variant::Int(3)));
    }

    #[test]
    fn has_all_and_get_or_add() {
        let mut d = Dictionary::new();
        d.insert("a", Variant::Int(1));
        assert!(d.has_all(&[Variant::String("a".into())]));
        assert!(!d.has_all(&[Variant::String("a".into()), Variant::String("b".into())]));

        let got = d.get_or_add("a", Variant::Int(99));
        assert_eq!(got, Variant::Int(1)); // existing, not overwritten
        let added = d.get_or_add("b", Variant::Int(42));
        assert_eq!(added, Variant::Int(42));
        assert_eq!(d.get("b"), Some(&Variant::Int(42)));
    }

    #[test]
    fn find_key() {
        let mut d = Dictionary::new();
        d.insert("a", Variant::Int(1));
        d.insert("b", Variant::Int(2));
        assert_eq!(d.find_key(&Variant::Int(2)), Variant::String("b".into()));
        assert_eq!(d.find_key(&Variant::Int(9)), Variant::Nil);
    }

    #[test]
    fn merge_with_and_without_overwrite() {
        let mut a = Dictionary::new();
        a.insert("x", Variant::Int(1));
        a.insert("y", Variant::Int(2));
        let mut b = Dictionary::new();
        b.insert("y", Variant::Int(20));
        b.insert("z", Variant::Int(30));

        let mut no_ow = a.clone();
        no_ow.merge(&b, false);
        assert_eq!(no_ow.get("y"), Some(&Variant::Int(2))); // kept
        assert_eq!(no_ow.get("z"), Some(&Variant::Int(30)));

        let ow = a.merged(&b, true);
        assert_eq!(ow.get("y"), Some(&Variant::Int(20))); // overwritten
        assert_eq!(ow.get("z"), Some(&Variant::Int(30)));
        // `merged` did not mutate `a`.
        assert_eq!(a.get("y"), Some(&Variant::Int(2)));
    }

    #[test]
    fn duplicate_deep_vs_shallow() {
        let mut inner = Dictionary::new();
        inner.insert("n", Variant::Int(1));
        let mut d = Dictionary::new();
        d.insert("inner", Variant::Dictionary(inner.into()));

        let deep = d.duplicate(true);
        // Mutate the original's nested dict; deep copy must be unaffected.
        if let Some(Variant::Dictionary(orig_inner)) = d.get_mut("inner") {
            orig_inner.insert("n", Variant::Int(999)).unwrap();
        }
        if let Some(Variant::Dictionary(copy_inner)) = deep.get("inner") {
            assert_eq!(copy_inner.get("n"), Some(Variant::Int(1)));
        } else {
            panic!("expected nested dictionary");
        }
    }

    #[test]
    fn equality_is_order_independent() {
        let mut a = Dictionary::new();
        a.insert("x", Variant::Int(1));
        a.insert("y", Variant::Int(2));
        let mut b = Dictionary::new();
        b.insert("y", Variant::Int(2));
        b.insert("x", Variant::Int(1));
        assert_eq!(a, b);
        b.insert("x", Variant::Int(5));
        assert_ne!(a, b);
    }

    #[test]
    fn from_hashmap_roundtrip() {
        let mut h = HashMap::new();
        h.insert("k".to_string(), Variant::Int(7));
        let d: Dictionary = h.into();
        assert_eq!(d.get("k"), Some(&Variant::Int(7)));
    }
}
