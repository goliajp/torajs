//! The hash maps and sets the compiler keys by its own ids and names.
//!
//! Almost every key here is a dense `u32` id (value, block, function),
//! a small tuple of them or a short name, hashed millions of times per
//! compile. std's default SipHash-1-3 is built to resist adversarial
//! keys, which compiler-internal ids are not, and was the largest single
//! cost of the pipeline. This is the multiply-rotate hash rustc uses for
//! its own tables (FxHash): one rotate, xor and multiply per word.
//!
//! Nothing the compiler emits may depend on a map's iteration order.
//! The hash is fixed, so a pass that did would still be deterministic,
//! but its output would change with the hash and with the keys' values;
//! a pass that walks a map and feeds what it reads back into the same
//! walk must take its inputs from a snapshot or sort first.

use std::hash::{BuildHasherDefault, Hasher};

pub type HashMap<K, V> = std::collections::HashMap<K, V, BuildHasherDefault<IdHasher>>;
pub type HashSet<T> = std::collections::HashSet<T, BuildHasherDefault<IdHasher>>;

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

#[derive(Default, Clone, Copy)]
pub struct IdHasher {
    hash: u64,
}

impl IdHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for IdHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let (words, rest) = bytes.as_chunks::<8>();
        for w in words {
            self.add(u64::from_le_bytes(*w));
        }
        if !rest.is_empty() {
            let mut word = [0u8; 8];
            word[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(word));
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

#[cfg(test)]
mod tests {
    use std::hash::{BuildHasher, BuildHasherDefault};

    use super::IdHasher;

    fn hash<T: std::hash::Hash>(t: T) -> u64 {
        BuildHasherDefault::<IdHasher>::default().hash_one(t)
    }

    #[test]
    fn consecutive_ids_spread_over_the_high_bits() {
        // hashbrown picks the control byte from the top 7 bits
        let tops: std::collections::BTreeSet<u64> = (0u32..256).map(|i| hash(i) >> 57).collect();
        assert!(tops.len() > 100, "{}", tops.len());
    }

    #[test]
    fn byte_strings_differ_in_their_tail() {
        assert_ne!(hash("abcdefgh1"), hash("abcdefgh2"));
        assert_ne!(hash("a"), hash("b"));
    }
}
