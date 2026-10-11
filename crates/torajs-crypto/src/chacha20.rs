//! The ChaCha20 stream cipher (RFC 8439 §2.3, §2.4), IETF variant: 256-bit
//! key, 96-bit nonce, 32-bit block counter.
//!
//! Bulk data runs four blocks at a time with each state word held as four
//! lanes, one per block, so the compiler can map a word onto one SIMD
//! register (NEON, SSE2, wasm `simd128`) without target-specific code.
#![forbid(unsafe_code)]

pub(crate) const BLOCK_LEN: usize = 64;

const SIGMA: [u32; 4] = [0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574];

#[inline(always)]
fn quarter(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(16);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(12);
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(8);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(7);
}

/// Decodes a 32-byte key into the eight little-endian key words.
pub(crate) fn key_words(key: &[u8; 32]) -> [u32; 8] {
    core::array::from_fn(|i| u32::from_le_bytes(key[4 * i..4 * i + 4].try_into().unwrap()))
}

/// Decodes a 12-byte nonce into its three little-endian words.
pub(crate) fn nonce_words(nonce: &[u8; 12]) -> [u32; 3] {
    core::array::from_fn(|i| u32::from_le_bytes(nonce[4 * i..4 * i + 4].try_into().unwrap()))
}

/// The ChaCha20 block function: the 16 output words for one counter value.
pub(crate) fn block(key: &[u32; 8], counter: u32, nonce: &[u32; 3]) -> [u32; 16] {
    let mut init = [0u32; 16];
    init[..4].copy_from_slice(&SIGMA);
    init[4..12].copy_from_slice(key);
    init[12] = counter;
    init[13..].copy_from_slice(nonce);
    let mut s = init;
    for _ in 0..10 {
        quarter(&mut s, 0, 4, 8, 12);
        quarter(&mut s, 1, 5, 9, 13);
        quarter(&mut s, 2, 6, 10, 14);
        quarter(&mut s, 3, 7, 11, 15);
        quarter(&mut s, 0, 5, 10, 15);
        quarter(&mut s, 1, 6, 11, 12);
        quarter(&mut s, 2, 7, 8, 13);
        quarter(&mut s, 3, 4, 9, 14);
    }
    for (w, i) in s.iter_mut().zip(init) {
        *w = w.wrapping_add(i);
    }
    s
}

const LANES: usize = 4;

/// One state word across four consecutive blocks.
type Lanes = [u32; LANES];

#[inline(always)]
fn add(a: Lanes, b: Lanes) -> Lanes {
    core::array::from_fn(|i| a[i].wrapping_add(b[i]))
}

#[inline(always)]
fn xor_rotl(a: Lanes, b: Lanes, r: u32) -> Lanes {
    core::array::from_fn(|i| (a[i] ^ b[i]).rotate_left(r))
}

#[inline(always)]
fn quarter_lanes(s: &mut [Lanes; 16], a: usize, b: usize, c: usize, d: usize) {
    s[a] = add(s[a], s[b]);
    s[d] = xor_rotl(s[d], s[a], 16);
    s[c] = add(s[c], s[d]);
    s[b] = xor_rotl(s[b], s[c], 12);
    s[a] = add(s[a], s[b]);
    s[d] = xor_rotl(s[d], s[a], 8);
    s[c] = add(s[c], s[d]);
    s[b] = xor_rotl(s[b], s[c], 7);
}

/// Four consecutive blocks starting at `counter`, word-major: `out[w][j]`
/// is word `w` of block `j`.
fn block_lanes(key: &[u32; 8], counter: u32, nonce: &[u32; 3]) -> [Lanes; 16] {
    let mut init = [[0u32; LANES]; 16];
    for (w, &x) in init.iter_mut().zip(SIGMA.iter().chain(key)) {
        *w = [x; LANES];
    }
    init[12] = core::array::from_fn(|j| counter.wrapping_add(j as u32));
    for (w, &x) in init[13..].iter_mut().zip(nonce) {
        *w = [x; LANES];
    }
    let mut s = init;
    for _ in 0..10 {
        quarter_lanes(&mut s, 0, 4, 8, 12);
        quarter_lanes(&mut s, 1, 5, 9, 13);
        quarter_lanes(&mut s, 2, 6, 10, 14);
        quarter_lanes(&mut s, 3, 7, 11, 15);
        quarter_lanes(&mut s, 0, 5, 10, 15);
        quarter_lanes(&mut s, 1, 6, 11, 12);
        quarter_lanes(&mut s, 2, 7, 8, 13);
        quarter_lanes(&mut s, 3, 4, 9, 14);
    }
    for (w, i) in s.iter_mut().zip(init) {
        *w = add(*w, i);
    }
    s
}

/// XORs `data` with the keystream that starts at block `counter`.
///
/// The caller bounds `data.len()` so the counter does not wrap.
pub(crate) fn xor_keystream(key: &[u32; 8], counter: u32, nonce: &[u32; 3], data: &mut [u8]) {
    let mut ctr = counter;
    let (quads, rest) = data.as_chunks_mut::<{ LANES * BLOCK_LEN }>();
    for quad in quads {
        let ks = block_lanes(key, ctr, nonce);
        ctr = ctr.wrapping_add(LANES as u32);
        for (j, blk) in quad.as_chunks_mut::<BLOCK_LEN>().0.iter_mut().enumerate() {
            for (bytes, k) in blk.as_chunks_mut::<4>().0.iter_mut().zip(&ks) {
                *bytes = (u32::from_le_bytes(*bytes) ^ k[j]).to_le_bytes();
            }
        }
    }
    for chunk in rest.chunks_mut(BLOCK_LEN) {
        let ks = block(key, ctr, nonce);
        ctr = ctr.wrapping_add(1);
        let mut buf = [0u8; BLOCK_LEN];
        for (bytes, k) in buf.as_chunks_mut::<4>().0.iter_mut().zip(ks) {
            *bytes = k.to_le_bytes();
        }
        for (d, k) in chunk.iter_mut().zip(buf) {
            *d ^= k;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{hex, unhex};

    fn key_00_1f() -> [u32; 8] {
        let key: [u8; 32] = core::array::from_fn(|i| i as u8);
        key_words(&key)
    }

    /// RFC 8439 §2.3.2.
    #[test]
    fn block_function() {
        let nonce = nonce_words(&unhex("000000090000004a00000000").try_into().unwrap());
        let out = block(&key_00_1f(), 1, &nonce);
        let bytes: std::vec::Vec<u8> = out.iter().flat_map(|w| w.to_le_bytes()).collect();
        assert_eq!(
            hex(&bytes),
            "10f1e7e4d13b5915500fdd1fa32071c4c7d1f4c733c068030422aa9ac3d46c4e\
             d2826446079faa0914c2d705d98b02a2b5129cd1de164eb9cbd083e8a2503c4e"
        );
    }

    #[test]
    fn lanes_match_single_blocks() {
        let nonce = [7, 0x0102_0304, u32::MAX];
        for counter in [0, 1, u32::MAX - 1] {
            let lanes = block_lanes(&key_00_1f(), counter, &nonce);
            let cols: [[u32; 16]; LANES] =
                core::array::from_fn(|j| core::array::from_fn(|w| lanes[w][j]));
            for (j, col) in cols.iter().enumerate() {
                let one = block(&key_00_1f(), counter.wrapping_add(j as u32), &nonce);
                assert_eq!(*col, one, "counter {counter} lane {j}");
            }
        }
    }

    /// RFC 8439 §2.4.2.
    #[test]
    fn encryption() {
        let nonce = nonce_words(&unhex("000000000000004a00000000").try_into().unwrap());
        let mut data = *b"Ladies and Gentlemen of the class of '99: If I could offer you \
only one tip for the future, sunscreen would be it.";
        xor_keystream(&key_00_1f(), 1, &nonce, &mut data);
        assert_eq!(
            hex(&data),
            "6e2e359a2568f98041ba0728dd0d6981e97e7aec1d4360c20a27afccfd9fae0b\
             f91b65c5524733ab8f593dabcd62b3571639d624e65152ab8f530c359f0861d8\
             07ca0dbf500d6a6156a38e088a22b65e52bc514d16ccf806818ce91ab7793736\
             5af90bbf74a35be6b40b8eedf2785e42874d"
        );
    }
}
