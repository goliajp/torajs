//! ChaCha20 on AArch64 Advanced SIMD (NEON), in groups of four blocks.
//!
//! Within a group, register `s[w]` holds state word `w` of the four
//! blocks, one block per lane, so the rounds are lane-wise adds, XORs and
//! rotations with no shuffles. Bulk data runs three groups at once with
//! their quarter rounds interleaved: one group's dependency chains leave
//! most of the vector pipes idle. Rotation by 16 is `rev32` on halfwords, by 8 a `tbl` with a
//! constant index, by 12 and 7 a shift and a shift-insert. A 4x4
//! transpose turns the word-major result into the four keystream blocks.
//!
//! A [`Neon`] is only constructed when the caller's [`CpuFeatures`] has
//! [`Feature::Aarch64Neon`]; that is the invariant every `// SAFETY:`
//! comment below relies on.
#![allow(unsafe_code)]

use core::arch::aarch64::*;

use super::BLOCK_LEN;
use super::soft::{self, SIGMA};
use crate::cpu::{CpuFeatures, Feature};
use crate::ct::zeroize::zeroize;

const LANES: usize = 4;
const QUAD: usize = LANES * BLOCK_LEN;
/// Groups of four blocks per iteration on the main path.
const WIDE: usize = 3;
/// Tails up to this long run on the portable code: one group of four
/// blocks costs about as much as two blocks computed one at a time.
const SCALAR_TAIL: usize = 2 * BLOCK_LEN;

/// Rotate each 32-bit lane left by 8: byte `i` of a word moves to `i + 1`.
const ROT8: [u8; 16] = [3, 0, 1, 2, 7, 4, 5, 6, 11, 8, 9, 10, 15, 12, 13, 14];

/// Evidence that the CPU has NEON.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Neon(());

impl Neon {
    /// `Some` only when `cpu` vouches for NEON.
    pub(crate) fn new(cpu: CpuFeatures) -> Option<Neon> {
        cpu.has(Feature::Aarch64Neon).then_some(Neon(()))
    }

    /// XORs `data` with the keystream that starts at block `counter`.
    pub(crate) fn xor_keystream(
        self,
        key: &[u32; 8],
        counter: u32,
        nonce: &[u32; 3],
        data: &mut [u8],
    ) {
        // SAFETY: a `Neon` exists only when the CPU has NEON (see `new`).
        unsafe { xor_keystream(key, counter, nonce, data) }
    }
}

#[inline]
#[target_feature(enable = "neon")]
fn rotl16(x: uint32x4_t) -> uint32x4_t {
    vreinterpretq_u32_u16(vrev32q_u16(vreinterpretq_u16_u32(x)))
}

#[inline]
#[target_feature(enable = "neon")]
fn rotl8(x: uint32x4_t, idx: uint8x16_t) -> uint32x4_t {
    vreinterpretq_u32_u8(vqtbl1q_u8(vreinterpretq_u8_u32(x), idx))
}

#[inline]
#[target_feature(enable = "neon")]
fn rotl<const L: i32, const R: i32>(x: uint32x4_t) -> uint32x4_t {
    vsriq_n_u32::<R>(vshlq_n_u32::<L>(x), x)
}

#[inline]
#[target_feature(enable = "neon")]
fn quarter(s: &mut [uint32x4_t; 16], idx: uint8x16_t, a: usize, b: usize, c: usize, d: usize) {
    s[a] = vaddq_u32(s[a], s[b]);
    s[d] = rotl16(veorq_u32(s[d], s[a]));
    s[c] = vaddq_u32(s[c], s[d]);
    s[b] = rotl::<12, 20>(veorq_u32(s[b], s[c]));
    s[a] = vaddq_u32(s[a], s[b]);
    s[d] = rotl8(veorq_u32(s[d], s[a]), idx);
    s[c] = vaddq_u32(s[c], s[d]);
    s[b] = rotl::<7, 25>(veorq_u32(s[b], s[c]));
}

/// One quarter round on every group, interleaved.
#[inline]
#[target_feature(enable = "neon")]
fn quarters<const N: usize>(
    s: &mut [[uint32x4_t; 16]; N],
    idx: uint8x16_t,
    a: usize,
    b: usize,
    c: usize,
    d: usize,
) {
    for sg in s {
        quarter(sg, idx, a, b, c, d);
    }
}

/// `N` groups of four blocks starting at `counter`, word-major: lane `j`
/// of `out[g][w]` is word `w` of block `counter + 4 * g + j`.
#[inline]
#[target_feature(enable = "neon")]
fn blocks<const N: usize>(key: &[u32; 8], counter: u32, nonce: &[u32; 3]) -> [[uint32x4_t; 16]; N] {
    // SAFETY: the pointer addresses 16 bytes.
    let idx = unsafe { vld1q_u8(ROT8.as_ptr()) };
    let mut init = [vdupq_n_u32(0); 16];
    for (w, &x) in init.iter_mut().zip(SIGMA.iter().chain(key)) {
        *w = vdupq_n_u32(x);
    }
    for (w, &x) in init[13..].iter_mut().zip(nonce) {
        *w = vdupq_n_u32(x);
    }
    let mut s = [init; N];
    for (g, sg) in s.iter_mut().enumerate() {
        let base = counter.wrapping_add((LANES * g) as u32);
        let lane: [u32; LANES] = core::array::from_fn(|j| base.wrapping_add(j as u32));
        // SAFETY: the pointer addresses 16 bytes.
        sg[12] = unsafe { vld1q_u32(lane.as_ptr()) };
    }
    let start = s;
    for _ in 0..10 {
        quarters(&mut s, idx, 0, 4, 8, 12);
        quarters(&mut s, idx, 1, 5, 9, 13);
        quarters(&mut s, idx, 2, 6, 10, 14);
        quarters(&mut s, idx, 3, 7, 11, 15);
        quarters(&mut s, idx, 0, 5, 10, 15);
        quarters(&mut s, idx, 1, 6, 11, 12);
        quarters(&mut s, idx, 2, 7, 8, 13);
        quarters(&mut s, idx, 3, 4, 9, 14);
    }
    for (sg, ig) in s.iter_mut().zip(start) {
        for (w, i) in sg.iter_mut().zip(ig) {
            *w = vaddq_u32(*w, i);
        }
    }
    s
}

/// Transposes four words of four blocks into the 16 bytes each block
/// holds at that position.
#[inline]
#[target_feature(enable = "neon")]
fn transpose(a: uint32x4_t, b: uint32x4_t, c: uint32x4_t, d: uint32x4_t) -> [uint8x16_t; 4] {
    let ab0 = vreinterpretq_u64_u32(vtrn1q_u32(a, b));
    let ab1 = vreinterpretq_u64_u32(vtrn2q_u32(a, b));
    let cd0 = vreinterpretq_u64_u32(vtrn1q_u32(c, d));
    let cd1 = vreinterpretq_u64_u32(vtrn2q_u32(c, d));
    [
        vreinterpretq_u8_u64(vtrn1q_u64(ab0, cd0)),
        vreinterpretq_u8_u64(vtrn1q_u64(ab1, cd1)),
        vreinterpretq_u8_u64(vtrn2q_u64(ab0, cd0)),
        vreinterpretq_u8_u64(vtrn2q_u64(ab1, cd1)),
    ]
}

/// XORs the `4 * N` blocks starting at `counter` into `buf`.
#[inline]
#[target_feature(enable = "neon")]
fn xor_blocks<const N: usize>(
    key: &[u32; 8],
    counter: u32,
    nonce: &[u32; 3],
    buf: &mut [[u8; QUAD]; N],
) {
    let s = blocks::<N>(key, counter, nonce);
    for (sg, bg) in s.iter().zip(buf) {
        let p = bg.as_mut_ptr();
        for (r, w) in sg.as_chunks::<4>().0.iter().enumerate() {
            for (j, k) in transpose(w[0], w[1], w[2], w[3]).into_iter().enumerate() {
                // SAFETY: `BLOCK_LEN * j + 16 * r + 16 <= QUAD` for
                // `j, r < 4`, so the 16 bytes lie inside `bg`.
                unsafe {
                    let q = p.add(BLOCK_LEN * j + 16 * r);
                    vst1q_u8(q, veorq_u8(vld1q_u8(q), k));
                }
            }
        }
    }
}

#[target_feature(enable = "neon")]
fn xor_keystream(key: &[u32; 8], counter: u32, nonce: &[u32; 3], data: &mut [u8]) {
    let mut ctr = counter;
    let (quads, rest) = data.as_chunks_mut::<QUAD>();
    let (wide, quads) = quads.as_chunks_mut::<WIDE>();
    for w in wide {
        xor_blocks::<WIDE>(key, ctr, nonce, w);
        ctr = ctr.wrapping_add((WIDE * LANES) as u32);
    }
    let (pairs, single) = quads.as_chunks_mut::<2>();
    for p in pairs {
        xor_blocks::<2>(key, ctr, nonce, p);
        ctr = ctr.wrapping_add((2 * LANES) as u32);
    }
    for q in single {
        xor_blocks::<1>(key, ctr, nonce, core::array::from_mut(q));
        ctr = ctr.wrapping_add(LANES as u32);
    }
    match rest.len() {
        0 => {}
        1..=SCALAR_TAIL => soft::xor_keystream(key, ctr, nonce, rest),
        n => {
            // the tail runs through a stack copy of the data, wiped afterwards
            let mut buf = [0u8; QUAD];
            buf[..n].copy_from_slice(rest);
            xor_blocks::<1>(key, ctr, nonce, core::array::from_mut(&mut buf));
            rest.copy_from_slice(&buf[..n]);
            zeroize(&mut buf);
        }
    }
}
