//! The census's hash maps and sets.
//!
//! Nearly every key is a name, a tuple holding one, or an id. The
//! compiler's id hash takes a name's bytes a word at a time and copies
//! the last partial word out through a call; this one reads the last
//! word in place, overlapping the one before it, and short names in two
//! halves. Nothing the census answers depends on a map's order.

use std::hash::{BuildHasherDefault, Hasher};

pub(super) type HashMap<K, V> = std::collections::HashMap<K, V, BuildHasherDefault<NameHasher>>;
pub(super) type HashSet<T> = std::collections::HashSet<T, BuildHasherDefault<NameHasher>>;

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

#[derive(Default, Clone, Copy)]
pub(super) struct NameHasher {
    hash: u64,
}

impl NameHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for NameHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        if let Some(tail) = bytes.last_chunk::<8>() {
            let (words, _) = bytes.as_chunks::<8>();
            for w in words {
                self.add(u64::from_le_bytes(*w));
            }
            self.add(u64::from_le_bytes(*tail));
        } else if let (Some(head), Some(tail)) = (bytes.first_chunk::<4>(), bytes.last_chunk::<4>())
        {
            let head = u64::from(u32::from_le_bytes(*head));
            self.add(head << 32 | u64::from(u32::from_le_bytes(*tail)));
        } else {
            let word = (bytes.iter().enumerate()).fold(0, |w, (i, b)| w | u64::from(*b) << (8 * i));
            self.add(word);
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
