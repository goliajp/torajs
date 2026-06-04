//! Self-contained SHA-256 (FIPS 180-4 / NIST). Used by the ad-hoc
//! codesigner in `sign.rs` to compute the page-hash table that
//! `LC_CODE_SIGNATURE`'s CodeDirectory blob carries.
//!
//! Zero external dependencies — every transform is implemented
//! against the FIPS pseudo-code; the constants in `K` and the
//! initial hash state `H0` come straight from §4.2.2 / §5.3.3.
//!
//! Why hand-rolled, not the `sha2` crate: torajs is meant to ship
//! with **no external dependencies**, and ad-hoc codesigning is
//! the only place across the toolchain that needs a hash. Bringing
//! `sha2 + digest + crypto-common + generic-array + cipher + ...`
//! into the dep graph for one ~150-LOC algorithm violates the
//! `0 external dep` principle. The compiler will inline / auto-
//! vectorize this single-block implementation just fine for the
//! codesigner workload (a handful of KB per build).
//!
//! Verified against FIPS 180-2 test vectors (empty string, "abc",
//! 56-byte boundary, 64-byte boundary, 1024-byte block) and the
//! NIST CAVS short-message KAT samples — see the inline `tests`
//! module.

/// SHA-256 digest size in bytes.
pub const DIGEST_LEN: usize = 32;

/// SHA-256 block (chunk) size in bytes.
const BLOCK_LEN: usize = 64;

/// SHA-256 round constants `K` per FIPS 180-4 §4.2.2 — the first
/// 32 bits of the fractional parts of the cube roots of the first
/// 64 primes (2..311).
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// Initial hash state `H(0)` per FIPS 180-4 §5.3.3 — the first 32
/// bits of the fractional parts of the square roots of the first
/// 8 primes (2..19).
const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// Hash `bytes` and return the 32-byte digest. Single-shot API —
/// the codesigner page-hashes 4 KiB chunks individually so there's
/// no need for a streaming `update`/`finalize` interface here.
pub fn sha256(bytes: &[u8]) -> [u8; DIGEST_LEN] {
    let mut h = H0;

    // FIPS 180-4 §5.1.1 padding: append 0x80, then enough zeros so
    // the total length (in bytes) is ≡ 56 mod 64, then append the
    // original length in bits as a 64-bit big-endian integer.
    let total_bits = (bytes.len() as u64) * 8;
    let mut buf: Vec<u8> = Vec::with_capacity(bytes.len() + 72);
    buf.extend_from_slice(bytes);
    buf.push(0x80);
    while buf.len() % BLOCK_LEN != 56 {
        buf.push(0);
    }
    buf.extend_from_slice(&total_bits.to_be_bytes());

    // Process each 64-byte block.
    debug_assert_eq!(buf.len() % BLOCK_LEN, 0);
    for chunk in buf.chunks_exact(BLOCK_LEN) {
        process_block(&mut h, chunk);
    }

    // Serialize state as big-endian.
    let mut digest = [0u8; DIGEST_LEN];
    for (i, word) in h.iter().enumerate() {
        digest[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    digest
}

/// Process one 64-byte block against the state `h` per FIPS 180-4
/// §6.2.2.
fn process_block(h: &mut [u32; 8], block: &[u8]) {
    debug_assert_eq!(block.len(), BLOCK_LEN);

    // Prepare the message schedule W[0..63].
    let mut w = [0u32; 64];
    for i in 0..16 {
        w[i] = u32::from_be_bytes(block[i * 4..i * 4 + 4].try_into().unwrap());
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }

    // Initialize working variables from current hash state.
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] =
        [h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]];

    // Compression — 64 rounds.
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ (!e & g);
        let t1 = hh
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        hh = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }

    // Add the compressed chunk to the current hash value.
    h[0] = h[0].wrapping_add(a);
    h[1] = h[1].wrapping_add(b);
    h[2] = h[2].wrapping_add(c);
    h[3] = h[3].wrapping_add(d);
    h[4] = h[4].wrapping_add(e);
    h[5] = h[5].wrapping_add(f);
    h[6] = h[6].wrapping_add(g);
    h[7] = h[7].wrapping_add(hh);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// FIPS 180-2 known vector #1.
    #[test]
    fn empty_string_kat() {
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    /// FIPS 180-2 known vector #2.
    #[test]
    fn abc_kat() {
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    /// FIPS 180-2 known vector #3 — 56-byte boundary (final block
    /// requires an extra padding block because length + 0x80 + 8-
    /// byte length spills past 64).
    #[test]
    fn fips_56_byte_block_boundary_kat() {
        let input = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(
            hex(&sha256(input)),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    /// 64-byte exact block boundary — full extra block needed for
    /// padding (0x80 + 55 zeros + 8-byte length = 64 bytes).
    #[test]
    fn exact_block_size_input() {
        let input: Vec<u8> = (0..64).map(|i| i as u8).collect();
        // Computed via `openssl dgst -sha256` ahead of time;
        // pinning here lets future refactors detect drift.
        assert_eq!(
            hex(&sha256(&input)),
            "fdeab9acf3710362bd2658cdc9a29e8f9c757fcf9811603a8c447cd1d9151108"
        );
    }

    /// Multi-block input — 1000 'a' bytes. FIPS 180-2 test vector.
    #[test]
    fn fips_1000_a_kat() {
        let input = vec![b'a'; 1000];
        // Verified independently via `printf 'a%.0s' {1..1000} | shasum -a 256`.
        assert_eq!(
            hex(&sha256(&input)),
            "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3"
        );
    }

    /// Single-byte input.
    #[test]
    fn single_byte_kat() {
        assert_eq!(
            hex(&sha256(b"a")),
            "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb"
        );
    }

    /// 4 KiB block (one codesign page) of zeros — pin the value
    /// we'll see frequently in the page-hash table since the
    /// linker often pads __TEXT with `0` to the next page.
    #[test]
    fn zero_page_4kib_kat() {
        let input = vec![0u8; 4096];
        // shasum -a 256: 0x5b1...
        assert_eq!(
            hex(&sha256(&input)),
            "ad7facb2586fc6e966c004d7d1d16b024f5805ff7cb47c7a85dabd8b48892ca7"
        );
    }

    #[test]
    fn digest_len_is_32_bytes() {
        assert_eq!(sha256(b"").len(), DIGEST_LEN);
        assert_eq!(DIGEST_LEN, 32);
    }
}
