//! SHA-256 block function on the ARMv8 SHA-256 instructions: each
//! `sha256h` / `sha256h2` pair runs four rounds, `sha256su0` /
//! `sha256su1` extend the message schedule four words at a time.
//!
//! Compiled only when the target enables `sha2` at build time, so every
//! machine that runs this code has the instructions.
#![allow(unsafe_code)]

use super::sha256::{BLOCK_LEN, K};

pub(super) fn compress(h: &mut [u32; 8], blocks: &[[u8; BLOCK_LEN]]) {
    use core::arch::aarch64::*;
    // SAFETY: this module is compiled only with `target_feature = "sha2"`
    // (which implies neon), so the intrinsics are available; every load
    // reads inside `h` (8 words), `K` (64 words) or a 64-byte block, and
    // the stores write inside `h`.
    unsafe {
        let mut abcd = vld1q_u32(h.as_ptr());
        let mut efgh = vld1q_u32(h.as_ptr().add(4));
        for block in blocks {
            let (abcd0, efgh0) = (abcd, efgh);
            let p = block.as_ptr();
            let mut w =
                [0, 16, 32, 48].map(|o| vreinterpretq_u32_u8(vrev32q_u8(vld1q_u8(p.add(o)))));
            for i in 0..16 {
                let t = vaddq_u32(w[i % 4], vld1q_u32(K.as_ptr().add(4 * i)));
                let prev = abcd;
                abcd = vsha256hq_u32(abcd, efgh, t);
                efgh = vsha256h2q_u32(efgh, prev, t);
                if i < 12 {
                    let s0 = vsha256su0q_u32(w[i % 4], w[(i + 1) % 4]);
                    w[i % 4] = vsha256su1q_u32(s0, w[(i + 2) % 4], w[(i + 3) % 4]);
                }
            }
            abcd = vaddq_u32(abcd, abcd0);
            efgh = vaddq_u32(efgh, efgh0);
        }
        vst1q_u32(h.as_mut_ptr(), abcd);
        vst1q_u32(h.as_mut_ptr().add(4), efgh);
    }
}
