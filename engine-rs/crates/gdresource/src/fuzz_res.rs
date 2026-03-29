//! Fuzz and property-based testing for the resource system.
//!
//! This module provides two categories of tests:
//!
//! 1. **Binary `.res` fuzz tests** — deterministic PRNG-based mutation testing
//!    against the binary resource parsers (`is_res_binary`, `parse_res_header`,
//!    `load_res_binary`).
//!
//! 2. **Property-based tests** (via `proptest`) — exercising `ResourceCache`,
//!    `ResourceLoader`, `TresLoader`, `Resource` metadata, and edge cases
//!    around paths, type tags, flags, and concurrent-style access patterns.

// All helpers are test-only; gate the entire support code so the compiler
// does not warn about dead code in non-test builds.
#[cfg(test)]
use crate::res_loader::{is_res_binary, load_res_binary, parse_res_header, RES_MAGIC};

// ---------------------------------------------------------------------------
// Deterministic PRNG (xorshift64)
// ---------------------------------------------------------------------------

/// A simple xorshift64 PRNG for deterministic fuzz testing.
#[cfg(test)]
#[derive(Debug, Clone)]
struct FuzzRng {
    state: u64,
}

#[cfg(test)]
impl FuzzRng {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 1 } else { seed },
        }
    }

    fn next_u64(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }

    fn next_u8(&mut self) -> u8 {
        self.next_u64() as u8
    }

    fn next_u32(&mut self) -> u32 {
        self.next_u64() as u32
    }

    fn range(&mut self, lo: usize, hi: usize) -> usize {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u64() as usize % (hi - lo))
    }

    #[allow(dead_code)]
    fn next_bool(&mut self) -> bool {
        self.next_u64() & 1 == 0
    }

    /// Generates random bytes of the given length.
    fn random_bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| self.next_u8()).collect()
    }

    /// Flips `n` random bits in the data.
    fn flip_bits(&mut self, data: &mut [u8], n: usize) {
        for _ in 0..n {
            if data.is_empty() {
                break;
            }
            let idx = self.range(0, data.len());
            let bit = self.range(0, 8);
            data[idx] ^= 1 << bit;
        }
    }

    /// Inserts random bytes at a random position.
    fn insert_random(&mut self, data: &mut Vec<u8>, count: usize) {
        let pos = self.range(0, data.len() + 1);
        for _ in 0..count {
            data.insert(pos, self.next_u8());
        }
    }

    /// Removes random bytes.
    fn remove_random(&mut self, data: &mut Vec<u8>, count: usize) {
        for _ in 0..count {
            if data.is_empty() {
                break;
            }
            let idx = self.range(0, data.len());
            data.remove(idx);
        }
    }
}

// ---------------------------------------------------------------------------
// Binary fuzz test helpers
// ---------------------------------------------------------------------------

/// Builds a valid .res binary header for mutation testing.
#[cfg(test)]
fn build_valid_header(resource_type: &str) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(RES_MAGIC);
    // big_endian = false
    data.extend_from_slice(&0u32.to_le_bytes());
    // use_64bit = false
    data.extend_from_slice(&0u32.to_le_bytes());
    // version_major = 4
    data.extend_from_slice(&4u32.to_le_bytes());
    // version_minor = 3
    data.extend_from_slice(&3u32.to_le_bytes());
    // format_version = 5
    data.extend_from_slice(&5u32.to_le_bytes());
    // resource type string
    let type_bytes = resource_type.as_bytes();
    data.extend_from_slice(&(type_bytes.len() as u32).to_le_bytes());
    data.extend_from_slice(type_bytes);
    // Pad to 4-byte alignment
    let padding = (4 - (type_bytes.len() % 4)) % 4;
    data.extend(std::iter::repeat(0u8).take(padding));
    // Add some trailing "body" data
    data.extend_from_slice(&[0u8; 64]);
    data
}

/// Feeds data to all parsers, asserts none panic. Returns true if any succeeded.
#[cfg(test)]
fn feed_parsers(data: &[u8]) -> bool {
    let _ = is_res_binary(data);
    let header_ok = parse_res_header(data).is_ok();
    let _ = load_res_binary(data, "fuzz://test.res");
    header_ok
}

// ===========================================================================
// Binary .res fuzz tests
// ===========================================================================

#[cfg(test)]
mod binary_fuzz_tests {
    use super::*;

    const FUZZ_ITERATIONS: usize = 500;

    #[test]
    fn fuzz_random_bytes_short() {
        let mut rng = FuzzRng::new(1000);
        for _ in 0..FUZZ_ITERATIONS {
            let len = rng.range(0, 32);
            let data = rng.random_bytes(len);
            feed_parsers(&data);
        }
    }

    #[test]
    fn fuzz_random_bytes_medium() {
        let mut rng = FuzzRng::new(1001);
        for _ in 0..FUZZ_ITERATIONS {
            let len = rng.range(32, 256);
            let data = rng.random_bytes(len);
            feed_parsers(&data);
        }
    }

    #[test]
    fn fuzz_random_bytes_large() {
        let mut rng = FuzzRng::new(1002);
        for _ in 0..200 {
            let len = rng.range(256, 4096);
            let data = rng.random_bytes(len);
            feed_parsers(&data);
        }
    }

    #[test]
    fn fuzz_bit_flip_single() {
        let mut rng = FuzzRng::new(2000);
        let base = build_valid_header("Resource");
        for _ in 0..FUZZ_ITERATIONS {
            let mut data = base.clone();
            rng.flip_bits(&mut data, 1);
            feed_parsers(&data);
        }
    }

    #[test]
    fn fuzz_bit_flip_multi() {
        let mut rng = FuzzRng::new(2001);
        let base = build_valid_header("PackedScene");
        for _ in 0..FUZZ_ITERATIONS {
            let mut data = base.clone();
            let n = rng.range(2, 10);
            rng.flip_bits(&mut data, n);
            feed_parsers(&data);
        }
    }

    #[test]
    fn fuzz_byte_insertion() {
        let mut rng = FuzzRng::new(2002);
        let base = build_valid_header("Theme");
        for _ in 0..FUZZ_ITERATIONS {
            let mut data = base.clone();
            let count = rng.range(1, 5);
            rng.insert_random(&mut data, count);
            feed_parsers(&data);
        }
    }

    #[test]
    fn fuzz_byte_removal() {
        let mut rng = FuzzRng::new(2003);
        let base = build_valid_header("Resource");
        for _ in 0..FUZZ_ITERATIONS {
            let mut data = base.clone();
            let count = rng.range(1, 8);
            rng.remove_random(&mut data, count);
            feed_parsers(&data);
        }
    }

    #[test]
    fn fuzz_byte_overwrite() {
        let mut rng = FuzzRng::new(2004);
        let base = build_valid_header("Texture2D");
        for _ in 0..FUZZ_ITERATIONS {
            let mut data = base.clone();
            let count = rng.range(1, 6);
            for _ in 0..count {
                if !data.is_empty() {
                    let idx = rng.range(0, data.len());
                    data[idx] = rng.next_u8();
                }
            }
            feed_parsers(&data);
        }
    }

    #[test]
    fn fuzz_truncation_every_byte() {
        let base = build_valid_header("Resource");
        for len in 0..base.len() {
            feed_parsers(&base[..len]);
        }
    }

    #[test]
    fn fuzz_truncation_random_length() {
        let mut rng = FuzzRng::new(3001);
        let base = build_valid_header("PackedScene");
        for _ in 0..FUZZ_ITERATIONS {
            let len = rng.range(0, base.len() + 20);
            let mut data = base.clone();
            data.truncate(len);
            feed_parsers(&data);
        }
    }

    #[test]
    fn fuzz_extreme_version_numbers() {
        let mut rng = FuzzRng::new(4000);
        for _ in 0..FUZZ_ITERATIONS {
            let mut data = build_valid_header("Resource");
            let major = rng.next_u32();
            data[12..16].copy_from_slice(&major.to_le_bytes());
            let minor = rng.next_u32();
            data[16..20].copy_from_slice(&minor.to_le_bytes());
            let fmt = rng.next_u32();
            data[20..24].copy_from_slice(&fmt.to_le_bytes());
            feed_parsers(&data);
        }
    }

    #[test]
    fn fuzz_max_u32_in_all_fields() {
        let mut data = build_valid_header("Resource");
        for offset in (4..24).step_by(4) {
            data[offset..offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        }
        feed_parsers(&data);
    }

    #[test]
    fn fuzz_zero_in_all_fields() {
        let mut data = build_valid_header("Resource");
        for byte in data[4..].iter_mut() {
            *byte = 0;
        }
        feed_parsers(&data);
    }

    #[test]
    fn fuzz_type_string_huge_length() {
        let mut data = Vec::new();
        data.extend_from_slice(RES_MAGIC);
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&4u32.to_le_bytes());
        data.extend_from_slice(&3u32.to_le_bytes());
        data.extend_from_slice(&5u32.to_le_bytes());
        data.extend_from_slice(&u32::MAX.to_le_bytes());
        data.extend_from_slice(b"Resource");
        feed_parsers(&data);
    }

    #[test]
    fn fuzz_type_string_with_nulls() {
        let type_str = "Re\0so\0urce\0\0";
        let data = build_valid_header(type_str);
        let result = parse_res_header(&data);
        if let Ok(header) = result {
            assert!(!header.resource_type.is_empty() || type_str.is_empty());
        }
    }

    #[test]
    fn fuzz_type_string_random_lengths() {
        let mut rng = FuzzRng::new(5002);
        for _ in 0..FUZZ_ITERATIONS {
            let type_len = rng.range(0, 100);
            let type_str: String = (0..type_len).map(|_| rng.next_u8() as char).collect();
            let data = build_valid_header(&type_str);
            feed_parsers(&data);
        }
    }

    #[test]
    fn fuzz_type_string_mismatched_length() {
        let mut rng = FuzzRng::new(5003);
        for _ in 0..FUZZ_ITERATIONS {
            let mut data = build_valid_header("Resource");
            let fake_len = rng.next_u32();
            data[24..28].copy_from_slice(&fake_len.to_le_bytes());
            feed_parsers(&data);
        }
    }

    #[test]
    fn fuzz_endian_flag_mismatch() {
        let mut data = Vec::new();
        data.extend_from_slice(RES_MAGIC);
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&4u32.to_le_bytes());
        data.extend_from_slice(&3u32.to_le_bytes());
        data.extend_from_slice(&5u32.to_le_bytes());
        data.extend_from_slice(&8u32.to_le_bytes());
        data.extend_from_slice(b"Resource");
        feed_parsers(&data);
    }

    #[test]
    fn fuzz_random_endian_flags() {
        let mut rng = FuzzRng::new(6001);
        for _ in 0..FUZZ_ITERATIONS {
            let mut data = build_valid_header("Resource");
            let endian_val = rng.next_u32();
            data[4..8].copy_from_slice(&endian_val.to_le_bytes());
            let flag_64 = rng.next_u32();
            data[8..12].copy_from_slice(&flag_64.to_le_bytes());
            feed_parsers(&data);
        }
    }

    #[test]
    fn fuzz_near_miss_magic() {
        let near_misses: &[&[u8]] = &[
            b"RSRD", b"RSCC", b"rSRC", b"RSRc", b"RSRC", b"\x00SRC", b"R\x00RC", b"RS\x00C",
            b"RSR\x00",
        ];
        for magic in near_misses {
            let mut data = build_valid_header("Resource");
            data[..4].copy_from_slice(magic);
            feed_parsers(&data);
        }
    }

    #[test]
    fn fuzz_magic_byte_single_mutation() {
        let mut rng = FuzzRng::new(7001);
        for _ in 0..FUZZ_ITERATIONS {
            let mut data = build_valid_header("Resource");
            let idx = rng.range(0, 4);
            data[idx] = rng.next_u8();
            feed_parsers(&data);
        }
    }

    #[test]
    fn fuzz_combined_mutations() {
        let mut rng = FuzzRng::new(8000);
        let base = build_valid_header("PackedScene");
        for _ in 0..FUZZ_ITERATIONS {
            let mut data = base.clone();
            let ops = rng.range(1, 4);
            for _ in 0..ops {
                match rng.range(0, 5) {
                    0 => {
                        let n = rng.range(1, 5);
                        rng.flip_bits(&mut data, n);
                    }
                    1 => {
                        let n = rng.range(1, 4);
                        rng.insert_random(&mut data, n);
                    }
                    2 => {
                        let n = rng.range(1, 4);
                        rng.remove_random(&mut data, n);
                    }
                    3 => {
                        if !data.is_empty() {
                            let idx = rng.range(0, data.len());
                            data[idx] = rng.next_u8();
                        }
                    }
                    _ => {
                        let len = rng.range(0, data.len() + 1);
                        data.truncate(len);
                    }
                }
            }
            feed_parsers(&data);
        }
    }

    #[test]
    fn fuzz_mixed_valid_and_random_trailing_data() {
        let mut rng = FuzzRng::new(8001);
        for _ in 0..FUZZ_ITERATIONS {
            let mut data = build_valid_header("Resource");
            let extra = rng.range(0, 512);
            let trailing = rng.random_bytes(extra);
            data.extend_from_slice(&trailing);
            feed_parsers(&data);
        }
    }

    #[test]
    fn fuzz_empty_input() {
        feed_parsers(&[]);
    }

    #[test]
    fn fuzz_just_magic() {
        feed_parsers(RES_MAGIC);
    }

    #[test]
    fn fuzz_magic_plus_one() {
        let mut data = Vec::from(RES_MAGIC.as_slice());
        data.push(0xFF);
        feed_parsers(&data);
    }

    #[test]
    fn fuzz_all_0xff() {
        let data = vec![0xFF; 128];
        feed_parsers(&data);
    }

    #[test]
    fn fuzz_all_zeros() {
        let data = vec![0u8; 128];
        feed_parsers(&data);
    }

    #[test]
    fn fuzz_alternating_bytes() {
        let data: Vec<u8> = (0..128)
            .map(|i| if i % 2 == 0 { 0x55 } else { 0xAA })
            .collect();
        feed_parsers(&data);
    }

    #[test]
    fn fuzz_valid_header_then_garbage() {
        let mut rng = FuzzRng::new(9000);
        for _ in 0..200 {
            let mut data = build_valid_header("Resource");
            let garbage_start = 28;
            for byte in data[garbage_start..].iter_mut() {
                *byte = rng.next_u8();
            }
            feed_parsers(&data);
        }
    }

    #[test]
    fn fuzz_is_res_binary_random_prefixes() {
        let mut rng = FuzzRng::new(10000);
        for _ in 0..FUZZ_ITERATIONS {
            let len = rng.range(0, 8);
            let data = rng.random_bytes(len);
            let _ = is_res_binary(&data);
        }
    }

    #[test]
    fn fuzz_is_res_binary_near_magic() {
        let mut rng = FuzzRng::new(10001);
        for _ in 0..FUZZ_ITERATIONS {
            let mut data = Vec::from(RES_MAGIC.as_slice());
            let idx = rng.range(0, 4);
            data[idx] = rng.next_u8();
            let _ = is_res_binary(&data);
        }
    }

    #[test]
    fn fuzz_load_with_random_paths() {
        let mut rng = FuzzRng::new(11000);
        let data = build_valid_header("Resource");
        let paths = [
            "",
            "res://",
            "res://test.res",
            "res://really/long/path/to/resource.res",
            "\0",
            "res://\x00null.res",
            "res://\u{00fc}\u{00f1}\u{00ef}c\u{00f6}d\u{00e9}.res",
        ];
        for path in &paths {
            let _ = load_res_binary(&data, path);
        }
        for _ in 0..200 {
            let path_len = rng.range(0, 64);
            let path: String = (0..path_len)
                .map(|_| (rng.next_u8() % 95 + 32) as char)
                .collect();
            let _ = load_res_binary(&data, &path);
        }
    }

    #[test]
    fn fuzz_load_random_data() {
        let mut rng = FuzzRng::new(11001);
        for _ in 0..FUZZ_ITERATIONS {
            let len = rng.range(0, 512);
            let data = rng.random_bytes(len);
            let _ = load_res_binary(&data, "fuzz://test.res");
        }
    }
}

// ===========================================================================
// Property-based tests for ResourceCache, ResourceLoader, Resource metadata
// ===========================================================================

#[cfg(test)]
mod proptest_resource {
    use super::*;
    use crate::cache::ResourceCache;
    use crate::loader::ResourceLoader;
    use crate::resource::{ExtResource, Resource};
    use gdcore::error::{EngineError, EngineResult};
    use gdvariant::Variant;
    use proptest::prelude::*;
    use std::sync::Arc;

    // -- Test loader that echoes its path back as a Resource -------------------

    /// A loader that always succeeds: returns a Resource whose path and
    /// class_name are derived from the input path.
    struct EchoLoader;

    impl ResourceLoader for EchoLoader {
        fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
            let mut r = Resource::new("EchoResource");
            r.path = path.to_string();
            r.set_property("source_path", Variant::String(path.to_string()));
            Ok(Arc::new(r))
        }
    }

    /// A loader that always fails.
    struct FailLoader;

    impl ResourceLoader for FailLoader {
        fn load(&self, path: &str) -> EngineResult<Arc<Resource>> {
            Err(EngineError::NotFound(format!("not found: {}", path)))
        }
    }

    // -- Proptest strategies --------------------------------------------------

    /// Strategy for generating `res://`-prefixed paths with arbitrary suffixes.
    fn res_path_strategy() -> impl Strategy<Value = String> {
        "[a-zA-Z0-9_/.-]{0,80}".prop_map(|s| format!("res://{}", s))
    }

    /// Strategy for generating arbitrary (non-res://) path strings.
    fn arbitrary_path_strategy() -> impl Strategy<Value = String> {
        prop::string::string_regex("[^\0]{0,120}").unwrap()
    }

    /// Strategy for class name strings.
    fn class_name_strategy() -> impl Strategy<Value = String> {
        "[A-Z][a-zA-Z0-9]{0,30}"
    }

    // ======================================================================
    // ResourceCache property tests
    // ======================================================================

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        /// insert then get must return the same Arc.
        #[test]
        fn cache_insert_get_roundtrip(path in res_path_strategy()) {
            let mut cache = ResourceCache::new(EchoLoader);
            let r = Arc::new(Resource::new("Test"));
            cache.insert(&path, Arc::clone(&r));
            let got = cache.get(&path).expect("must be present after insert");
            prop_assert!(Arc::ptr_eq(&r, &got));
        }

        /// load() returns the same Arc on repeated calls.
        #[test]
        fn cache_load_deduplicates(path in res_path_strategy()) {
            let mut cache = ResourceCache::new(EchoLoader);
            let a = cache.load(&path).unwrap();
            let b = cache.load(&path).unwrap();
            prop_assert!(Arc::ptr_eq(&a, &b));
        }

        /// invalidate removes exactly the targeted path.
        #[test]
        fn cache_invalidate_targeted(
            p1 in res_path_strategy(),
            p2 in res_path_strategy(),
        ) {
            // Skip if both paths happen to be identical.
            prop_assume!(p1 != p2);

            let mut cache = ResourceCache::new(EchoLoader);
            cache.load(&p1).unwrap();
            cache.load(&p2).unwrap();
            cache.invalidate(&p1);
            prop_assert!(!cache.contains(&p1));
            prop_assert!(cache.contains(&p2));
        }

        /// After invalidation, a subsequent load returns a *new* Arc.
        #[test]
        fn cache_reload_after_invalidation(path in res_path_strategy()) {
            let mut cache = ResourceCache::new(EchoLoader);
            let before = cache.load(&path).unwrap();
            cache.invalidate(&path);
            let after = cache.load(&path).unwrap();
            prop_assert!(!Arc::ptr_eq(&before, &after));
        }

        /// clear() empties the cache regardless of how many entries.
        #[test]
        fn cache_clear_empties(paths in prop::collection::vec(res_path_strategy(), 1..20)) {
            let mut cache = ResourceCache::new(EchoLoader);
            for p in &paths {
                cache.load(p).unwrap();
            }
            cache.clear();
            prop_assert!(cache.is_empty());
            prop_assert_eq!(cache.len(), 0);
        }

        /// len() tracks distinct paths correctly.
        #[test]
        fn cache_len_tracks_distinct(paths in prop::collection::vec(res_path_strategy(), 1..30)) {
            let mut cache = ResourceCache::new(EchoLoader);
            let mut unique = std::collections::HashSet::new();
            for p in &paths {
                cache.load(p).unwrap();
                unique.insert(p.clone());
            }
            prop_assert_eq!(cache.len(), unique.len());
        }

        /// Duplicate insertions overwrite — get returns the latest Arc.
        #[test]
        fn cache_duplicate_insert_overwrites(path in res_path_strategy()) {
            let mut cache = ResourceCache::new(EchoLoader);
            let r1 = Arc::new(Resource::new("First"));
            let r2 = Arc::new(Resource::new("Second"));
            cache.insert(&path, Arc::clone(&r1));
            cache.insert(&path, Arc::clone(&r2));
            let got = cache.get(&path).unwrap();
            prop_assert!(Arc::ptr_eq(&got, &r2));
            prop_assert!(!Arc::ptr_eq(&got, &r1));
        }

        /// Inserting and immediately invalidating leaves the cache empty for that key.
        #[test]
        fn cache_insert_then_invalidate(path in res_path_strategy()) {
            let mut cache = ResourceCache::new(EchoLoader);
            cache.insert(&path, Arc::new(Resource::new("X")));
            prop_assert!(cache.contains(&path));
            cache.invalidate(&path);
            prop_assert!(!cache.contains(&path));
            prop_assert!(cache.get(&path).is_none());
        }

        /// Concurrent-style: interleave loads and invalidations, no panics.
        #[test]
        fn cache_interleaved_load_invalidate(
            ops in prop::collection::vec(
                (res_path_strategy(), prop::bool::ANY),
                1..50
            )
        ) {
            let mut cache = ResourceCache::new(EchoLoader);
            for (path, do_invalidate) in &ops {
                let _ = cache.load(path);
                if *do_invalidate {
                    cache.invalidate(path);
                }
            }
            // No panic — that is the assertion.
        }
    }

    // ======================================================================
    // ResourceLoader / TresLoader property tests
    // ======================================================================

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        /// FailLoader always returns Err for any path.
        #[test]
        fn fail_loader_always_errors(path in arbitrary_path_strategy()) {
            let mut cache = ResourceCache::new(FailLoader);
            let result = cache.load(&path);
            prop_assert!(result.is_err());
        }

        /// EchoLoader preserves the path in the loaded resource.
        #[test]
        fn echo_loader_preserves_path(path in res_path_strategy()) {
            let r = EchoLoader.load(&path).unwrap();
            prop_assert_eq!(&r.path, &path);
            prop_assert_eq!(
                r.get_property("source_path"),
                Some(&Variant::String(path.clone()))
            );
        }

        /// TresLoader rejects garbage input without panicking.
        #[test]
        fn tres_loader_rejects_garbage(garbage in "[^\0]{0,200}") {
            let loader = crate::TresLoader::new();
            // Garbage is very unlikely to be valid .tres — just must not panic.
            let _ = loader.parse_str(&garbage, "res://fuzz.tres");
        }

        /// TresLoader roundtrips a minimal valid .tres.
        #[test]
        fn tres_loader_roundtrip_minimal(
            name in "[a-zA-Z_][a-zA-Z0-9_]{0,20}",
            int_val in -1000i64..1000i64,
        ) {
            let source = format!(
                "[gd_resource type=\"Resource\" format=3]\n\n[resource]\nname = \"{}\"\nvalue = {}\n",
                name, int_val
            );
            let loader = crate::TresLoader::new();
            let res = loader.parse_str(&source, "res://prop.tres").unwrap();
            prop_assert_eq!(res.get_property("name"), Some(&Variant::String(name)));
            prop_assert_eq!(res.get_property("value"), Some(&Variant::Int(int_val)));
        }
    }

    // ======================================================================
    // Resource metadata property tests
    // ======================================================================

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        /// class_name is preserved exactly.
        #[test]
        fn resource_class_name_preserved(cn in class_name_strategy()) {
            let r = Resource::new(&cn);
            prop_assert_eq!(&r.class_name, &cn);
        }

        /// set_property / get_property roundtrips for String variant.
        #[test]
        fn resource_property_string_roundtrip(
            key in "[a-z_]{1,20}",
            val in ".*",
        ) {
            let mut r = Resource::new("R");
            r.set_property(&key, Variant::String(val.clone()));
            prop_assert_eq!(r.get_property(&key), Some(&Variant::String(val)));
        }

        /// set_property / get_property roundtrips for Int variant.
        #[test]
        fn resource_property_int_roundtrip(
            key in "[a-z_]{1,20}",
            val in prop::num::i64::ANY,
        ) {
            let mut r = Resource::new("R");
            r.set_property(&key, Variant::Int(val));
            prop_assert_eq!(r.get_property(&key), Some(&Variant::Int(val)));
        }

        /// set_property / get_property roundtrips for Bool variant.
        #[test]
        fn resource_property_bool_roundtrip(
            key in "[a-z_]{1,20}",
            val in prop::bool::ANY,
        ) {
            let mut r = Resource::new("R");
            r.set_property(&key, Variant::Bool(val));
            prop_assert_eq!(r.get_property(&key), Some(&Variant::Bool(val)));
        }

        /// remove_property returns the previously set value.
        #[test]
        fn resource_remove_returns_value(
            key in "[a-z_]{1,20}",
            val in -1000i64..1000i64,
        ) {
            let mut r = Resource::new("R");
            r.set_property(&key, Variant::Int(val));
            prop_assert_eq!(r.remove_property(&key), Some(Variant::Int(val)));
            prop_assert!(r.get_property(&key).is_none());
        }

        /// property_count reflects the number of unique keys.
        #[test]
        fn resource_property_count(
            kvs in prop::collection::vec(
                ("[a-z]{1,8}", -100i64..100i64),
                0..20
            )
        ) {
            let mut r = Resource::new("R");
            let mut unique = std::collections::HashSet::new();
            for (k, v) in &kvs {
                r.set_property(k.as_str(), Variant::Int(*v));
                unique.insert(k.clone());
            }
            prop_assert_eq!(r.property_count(), unique.len());
        }

        /// ExtResource fields survive construction.
        #[test]
        fn ext_resource_fields(
            rtype in class_name_strategy(),
            uid in "uid://[a-z0-9]{4,12}",
            path in res_path_strategy(),
            id in "[0-9]{1,4}",
        ) {
            let ext = ExtResource {
                resource_type: rtype.clone(),
                uid: uid.clone(),
                path: path.clone(),
                id: id.clone(),
            };
            prop_assert_eq!(&ext.resource_type, &rtype);
            prop_assert_eq!(&ext.uid, &uid);
            prop_assert_eq!(&ext.path, &path);
            prop_assert_eq!(&ext.id, &id);
        }
    }

    // ======================================================================
    // Edge-case tests (non-proptest, but counted towards the 28+ minimum)
    // ======================================================================

    #[test]
    fn cache_empty_path() {
        let mut cache = ResourceCache::new(EchoLoader);
        let r = cache.load("").unwrap();
        assert_eq!(r.path, "");
        assert!(cache.contains(""));
    }

    #[test]
    fn cache_very_long_path() {
        let long = format!("res://{}", "a".repeat(10_000));
        let mut cache = ResourceCache::new(EchoLoader);
        let r = cache.load(&long).unwrap();
        assert_eq!(r.path, long);
    }

    #[test]
    fn cache_special_characters_path() {
        let paths = [
            "res://\u{00e9}\u{00e8}\u{00ea}.tres",
            "res://path with spaces/file.tres",
            "res://\u{1F600}/emoji.tres",
            "res://../../../etc/passwd",
            "res://a\tb\nc.tres",
        ];
        let mut cache = ResourceCache::new(EchoLoader);
        for p in &paths {
            let r = cache.load(p).unwrap();
            assert_eq!(&r.path, p);
        }
        assert_eq!(cache.len(), paths.len());
    }

    #[test]
    fn cache_path_normalization_is_identity() {
        // The cache uses exact string keys — "res://A" and "res://a" are distinct.
        let mut cache = ResourceCache::new(EchoLoader);
        let a = cache.load("res://A.tres").unwrap();
        let b = cache.load("res://a.tres").unwrap();
        assert!(!Arc::ptr_eq(&a, &b));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn cache_trailing_slash_difference() {
        let mut cache = ResourceCache::new(EchoLoader);
        let a = cache.load("res://dir/").unwrap();
        let b = cache.load("res://dir").unwrap();
        assert!(!Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn resource_sorted_property_keys_deterministic() {
        let mut r = Resource::new("R");
        r.set_property("z", Variant::Int(1));
        r.set_property("a", Variant::Int(2));
        r.set_property("m", Variant::Int(3));
        let keys = r.sorted_property_keys();
        assert_eq!(
            keys.iter().map(|k| k.as_str()).collect::<Vec<_>>(),
            vec!["a", "m", "z"]
        );
    }

    #[test]
    fn resource_overwrite_property_updates_value() {
        let mut r = Resource::new("R");
        r.set_property("x", Variant::Int(1));
        r.set_property("x", Variant::Int(2));
        assert_eq!(r.get_property("x"), Some(&Variant::Int(2)));
        assert_eq!(r.property_count(), 1);
    }

    #[test]
    fn tres_format_detection_requires_header() {
        let loader = crate::TresLoader::new();
        // No gd_resource header — parser is lenient, returns a default Resource.
        let result = loader.parse_str("name = \"hello\"\n", "res://bad.tres");
        let res = result.unwrap();
        // Without a [resource] section, top-level properties are ignored.
        assert_eq!(res.class_name, "Resource");
        assert_eq!(res.property_count(), 0);
    }
}
