//! Deterministic, cheap hashers for the internal hash maps on hot paths.
//!
//! std's default SipHash exists to resist adversarial collision flooding on
//! maps an attacker can key. These maps are not that: every lookup confirms
//! real equality on hit, and no map's iteration order reaches output — each
//! consumption site is checked for that when it adopts one of these types —
//! so a collision costs time, never correctness or determinism. What SipHash
//! costs here is real: it was ~25% of the samples in a 2026-08-05 profile of
//! an identical million-row pair.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// A map keyed by a digest that is already the output of xxh3.
///
/// Re-hashing a uniform 128-bit digest buys nothing, so the hasher keeps the
/// low 64 bits as the bucket index.
pub(crate) type DigestMap<V> = HashMap<u128, V, BuildHasherDefault<DigestHasher>>;

#[derive(Default)]
pub(crate) struct DigestHasher(u64);

impl Hasher for DigestHasher {
    fn write(&mut self, _bytes: &[u8]) {
        unreachable!("DigestMap keys are u128 digests, hashed through write_u128");
    }

    fn write_u128(&mut self, digest: u128) {
        self.0 = digest as u64;
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

/// A map whose keys hash with an Fx-style multiply-and-rotate fold.
///
/// The same fixed algorithm on every run, so it is deterministic; not xxh3,
/// because these keys arrive through `Hash` implementations as a stream of
/// small writes, where a streaming xxh3 state costs more to set up than the
/// whole key costs to fold.
pub(crate) type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<FastHasher>>;

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

#[derive(Default)]
pub(crate) struct FastHasher(u64);

impl FastHasher {
    fn fold(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for FastHasher {
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            self.fold(u64::from_le_bytes(
                chunk.try_into().expect("chunks are 8 bytes"),
            ));
        }
        let remainder = chunks.remainder();
        if !remainder.is_empty() {
            let mut word = [0_u8; 8];
            word[..remainder.len()].copy_from_slice(remainder);
            self.fold(u64::from_le_bytes(word));
        }
    }

    fn write_u8(&mut self, value: u8) {
        self.fold(u64::from(value));
    }

    fn write_u32(&mut self, value: u32) {
        self.fold(u64::from(value));
    }

    fn write_u64(&mut self, value: u64) {
        self.fold(value);
    }

    fn write_u128(&mut self, value: u128) {
        self.fold(value as u64);
        self.fold((value >> 64) as u64);
    }

    fn write_usize(&mut self, value: usize) {
        self.fold(value as u64);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use std::hash::{BuildHasher, BuildHasherDefault, Hash};

    use super::{DigestMap, FastHasher, FastMap};

    #[test]
    fn digest_lookups_round_trip() {
        let mut map: DigestMap<usize> = DigestMap::default();
        map.insert(u128::MAX, 1);
        map.insert(0, 2);
        map.insert(1 << 64, 3);

        assert_eq!(map.get(&u128::MAX), Some(&1));
        assert_eq!(map.get(&0), Some(&2));
        assert_eq!(map.get(&(1 << 64)), Some(&3));
        assert_eq!(map.get(&1), None);
    }

    #[test]
    fn fast_hashes_are_deterministic_and_spread() {
        let build = BuildHasherDefault::<FastHasher>::default();
        let hash = |value: &str| build.hash_one(value);

        assert_eq!(hash("alpha"), hash("alpha"));
        assert_ne!(hash("alpha"), hash("alphb"));
        // A trailing partial chunk changes the hash rather than vanishing.
        assert_ne!(hash("12345678"), hash("123456789"));
    }

    #[test]
    fn fast_maps_key_by_equality_not_by_hash() {
        let mut map: FastMap<Vec<u8>, u64> = FastMap::default();
        map.insert(b"a".to_vec(), 1);
        *map.entry(b"a".to_vec()).or_default() += 1;
        map.insert(b"b".to_vec(), 7);

        assert_eq!(map[&b"a".to_vec()], 2);
        assert_eq!(map[&b"b".to_vec()], 7);
    }

    #[test]
    fn derived_hash_reaches_the_fast_hasher() {
        #[derive(Hash, PartialEq, Eq)]
        enum Value {
            Int(i64),
            Text(String),
        }

        let build = BuildHasherDefault::<FastHasher>::default();
        assert_ne!(
            build.hash_one(Value::Int(1)),
            build.hash_one(Value::Text("1".into()))
        );
    }
}
