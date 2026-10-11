use super::*;
use crate::testutil::{hex, unhex};

/// The S-box from its definition (FIPS 197 §5.1.1): inversion in GF(2^8)
/// followed by the affine map.
fn sbox_reference(x: u8) -> u8 {
    let gmul = |mut a: u8, mut b: u8| {
        let mut p = 0u8;
        while b != 0 {
            if b & 1 == 1 {
                p ^= a;
            }
            let hi = a & 0x80;
            a <<= 1;
            if hi != 0 {
                a ^= 0x1b;
            }
            b >>= 1;
        }
        p
    };
    // x^254 = x^-1 (0 maps to 0)
    let mut inv = 1u8;
    for _ in 0..254 {
        inv = gmul(inv, x);
    }
    let b = inv;
    b ^ b.rotate_left(1) ^ b.rotate_left(2) ^ b.rotate_left(3) ^ b.rotate_left(4) ^ 0x63
}

#[test]
fn bitsliced_sbox_matches_the_definition() {
    for x in 0..=255u8 {
        let word = u32::from_le_bytes([x, x.wrapping_add(1), x ^ 0x5a, !x]);
        let got = soft::sub_word(word).to_le_bytes();
        let want = [x, x.wrapping_add(1), x ^ 0x5a, !x].map(sbox_reference);
        assert_eq!(got, want, "input {x:#04x}");
    }
}

/// FIPS 197 Appendix A.1: the last word of the AES-128 schedule.
#[test]
fn key_expansion() {
    let key = unhex("2b7e151628aed2a6abf7158809cf4f3c");
    let (w, rounds) = expand_key(&key);
    assert_eq!(rounds, 10);
    // FIPS 197 A.1: w[43] = b6630ca6
    assert_eq!(w[43].to_le_bytes(), [0xb6, 0x63, 0x0c, 0xa6]);
    let key = unhex("603deb1015ca71be2b73aef0857d77811f352c073b6108d72d9810a30914dff4");
    let (w, rounds) = expand_key(&key);
    assert_eq!(rounds, 14);
    // FIPS 197 A.3: w[59] = 706c631e
    assert_eq!(w[59].to_le_bytes(), [0x70, 0x6c, 0x63, 0x1e]);
}

/// FIPS 197 Appendix C.1–C.3.
const FIPS197: [(&str, &str); 3] = [
    (
        "000102030405060708090a0b0c0d0e0f",
        "69c4e0d86a7b0430d8cdb78070b4c55a",
    ),
    (
        "000102030405060708090a0b0c0d0e0f1011121314151617",
        "dda97ca4864cdfe06eaf70a0ec0d7191",
    ),
    (
        "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
        "8ea2b7ca516745bfeafc49904b496089",
    ),
];

const PLAIN: &str = "00112233445566778899aabbccddeeff";

#[test]
fn soft_fips197_vectors() {
    for (key, want) in FIPS197 {
        let (w, rounds) = expand_key(&unhex(key));
        let keys = soft::Keys::new(&w[..4 * (rounds + 1)], rounds);
        let pt: [u8; 16] = unhex(PLAIN).try_into().unwrap();
        // the same block in every lane, and a different one in lane 2, to
        // catch lane mix-ups
        let mut blocks = [pt, pt, [0; 16], pt];
        keys.encrypt4(&mut blocks);
        for i in [0, 1, 3] {
            assert_eq!(hex(&blocks[i]), want, "key {key} lane {i}");
        }
        let mut lone = [[0; 16]; 4];
        keys.encrypt4(&mut lone);
        assert_eq!(blocks[2], lone[0]);
    }
}
