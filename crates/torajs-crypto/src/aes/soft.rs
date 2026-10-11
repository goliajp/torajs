//! Bitsliced AES on 64-bit words, four blocks at a time.
//!
//! The state of four blocks is spread over eight `u64` words so that word
//! `i` holds bit `i` of every byte; the S-box is then a fixed Boolean
//! circuit (Boyar–Peralta, 113 gates) applied to all 64 bytes at once.
//! There are no table lookups, so no memory access depends on the key or
//! the data. The layout follows the `ct64` construction of BearSSL.
#![forbid(unsafe_code)]

use super::{MAX_ROUNDS, MAX_WORDS};

/// Transposes eight words between the byte-per-lane layout and the
/// bitsliced layout. It is its own inverse.
fn ortho(q: &mut [u64; 8]) {
    #[inline(always)]
    fn swap(q: &mut [u64; 8], i: usize, j: usize, lo: u64, s: u32) {
        let (a, b) = (q[i], q[j]);
        q[i] = (a & lo) | ((b & lo) << s);
        q[j] = ((a & !lo) >> s) | (b & !lo);
    }
    for (i, j) in [(0, 1), (2, 3), (4, 5), (6, 7)] {
        swap(q, i, j, 0x5555_5555_5555_5555, 1);
    }
    for (i, j) in [(0, 2), (1, 3), (4, 6), (5, 7)] {
        swap(q, i, j, 0x3333_3333_3333_3333, 2);
    }
    for (i, j) in [(0, 4), (1, 5), (2, 6), (3, 7)] {
        swap(q, i, j, 0x0f0f_0f0f_0f0f_0f0f, 4);
    }
}

/// Spreads one block (four little-endian words) over two words, leaving
/// gaps for the other three blocks.
fn interleave_in(w: &[u32; 4]) -> (u64, u64) {
    let spread = |x: u32| {
        let mut x = u64::from(x);
        x |= x << 16;
        x &= 0x0000_ffff_0000_ffff;
        x |= x << 8;
        x & 0x00ff_00ff_00ff_00ff
    };
    let [x0, x1, x2, x3] = w.map(spread);
    (x0 | (x2 << 8), x1 | (x3 << 8))
}

fn interleave_out(q0: u64, q1: u64) -> [u32; 4] {
    let gather = |x: u64| {
        let mut x = x & 0x00ff_00ff_00ff_00ff;
        x |= x >> 8;
        x &= 0x0000_ffff_0000_ffff;
        x as u32 | (x >> 16) as u32
    };
    [gather(q0), gather(q1), gather(q0 >> 8), gather(q1 >> 8)]
}

/// The AES S-box on all 64 bytes of a bitsliced state.
#[allow(clippy::many_single_char_names)]
fn sbox(q: &mut [u64; 8]) {
    let [x7, x6, x5, x4, x3, x2, x1, x0] = *q;

    // top linear transformation
    let y14 = x3 ^ x5;
    let y13 = x0 ^ x6;
    let y9 = x0 ^ x3;
    let y8 = x0 ^ x5;
    let t0 = x1 ^ x2;
    let y1 = t0 ^ x7;
    let y4 = y1 ^ x3;
    let y12 = y13 ^ y14;
    let y2 = y1 ^ x0;
    let y5 = y1 ^ x6;
    let y3 = y5 ^ y8;
    let t1 = x4 ^ y12;
    let y15 = t1 ^ x5;
    let y20 = t1 ^ x1;
    let y6 = y15 ^ x7;
    let y10 = y15 ^ t0;
    let y11 = y20 ^ y9;
    let y7 = x7 ^ y11;
    let y17 = y10 ^ y11;
    let y19 = y10 ^ y8;
    let y16 = t0 ^ y11;
    let y21 = y13 ^ y16;
    let y18 = x0 ^ y16;

    // non-linear section
    let t2 = y12 & y15;
    let t3 = y3 & y6;
    let t4 = t3 ^ t2;
    let t5 = y4 & x7;
    let t6 = t5 ^ t2;
    let t7 = y13 & y16;
    let t8 = y5 & y1;
    let t9 = t8 ^ t7;
    let t10 = y2 & y7;
    let t11 = t10 ^ t7;
    let t12 = y9 & y11;
    let t13 = y14 & y17;
    let t14 = t13 ^ t12;
    let t15 = y8 & y10;
    let t16 = t15 ^ t12;
    let t17 = t4 ^ t14;
    let t18 = t6 ^ t16;
    let t19 = t9 ^ t14;
    let t20 = t11 ^ t16;
    let t21 = t17 ^ y20;
    let t22 = t18 ^ y19;
    let t23 = t19 ^ y21;
    let t24 = t20 ^ y18;

    let t25 = t21 ^ t22;
    let t26 = t21 & t23;
    let t27 = t24 ^ t26;
    let t28 = t25 & t27;
    let t29 = t28 ^ t22;
    let t30 = t23 ^ t24;
    let t31 = t22 ^ t26;
    let t32 = t31 & t30;
    let t33 = t32 ^ t24;
    let t34 = t23 ^ t33;
    let t35 = t27 ^ t33;
    let t36 = t24 & t35;
    let t37 = t36 ^ t34;
    let t38 = t27 ^ t36;
    let t39 = t29 & t38;
    let t40 = t25 ^ t39;

    let t41 = t40 ^ t37;
    let t42 = t29 ^ t33;
    let t43 = t29 ^ t40;
    let t44 = t33 ^ t37;
    let t45 = t42 ^ t41;
    let z0 = t44 & y15;
    let z1 = t37 & y6;
    let z2 = t33 & x7;
    let z3 = t43 & y16;
    let z4 = t40 & y1;
    let z5 = t29 & y7;
    let z6 = t42 & y11;
    let z7 = t45 & y17;
    let z8 = t41 & y10;
    let z9 = t44 & y12;
    let z10 = t37 & y3;
    let z11 = t33 & y4;
    let z12 = t43 & y13;
    let z13 = t40 & y5;
    let z14 = t29 & y2;
    let z15 = t42 & y9;
    let z16 = t45 & y14;
    let z17 = t41 & y8;

    // bottom linear transformation
    let t46 = z15 ^ z16;
    let t47 = z10 ^ z11;
    let t48 = z5 ^ z13;
    let t49 = z9 ^ z10;
    let t50 = z2 ^ z12;
    let t51 = z2 ^ z5;
    let t52 = z7 ^ z8;
    let t53 = z0 ^ z3;
    let t54 = z6 ^ z7;
    let t55 = z16 ^ z17;
    let t56 = z12 ^ t48;
    let t57 = t50 ^ t53;
    let t58 = z4 ^ t46;
    let t59 = z3 ^ t54;
    let t60 = t46 ^ t57;
    let t61 = z14 ^ t57;
    let t62 = t52 ^ t58;
    let t63 = t49 ^ t58;
    let t64 = z4 ^ t59;
    let t65 = t61 ^ t62;
    let t66 = z1 ^ t63;
    let s0 = t59 ^ t63;
    let s6 = t56 ^ !t62;
    let s7 = t48 ^ !t60;
    let t67 = t64 ^ t65;
    let s3 = t53 ^ t66;
    let s4 = t51 ^ t66;
    let s5 = t47 ^ t65;
    let s1 = t64 ^ !s3;
    let s2 = t55 ^ !t67;

    *q = [s7, s6, s5, s4, s3, s2, s1, s0];
}

fn shift_rows(q: &mut [u64; 8]) {
    for x in q.iter_mut() {
        let v = *x;
        *x = (v & 0x0000_0000_0000_ffff)
            | ((v & 0x0000_0000_fff0_0000) >> 4)
            | ((v & 0x0000_0000_000f_0000) << 12)
            | ((v & 0x0000_ff00_0000_0000) >> 8)
            | ((v & 0x0000_00ff_0000_0000) << 8)
            | ((v & 0xf000_0000_0000_0000) >> 12)
            | ((v & 0x0fff_0000_0000_0000) << 4);
    }
}

fn mix_columns(q: &mut [u64; 8]) {
    let r: [u64; 8] = core::array::from_fn(|i| q[i].rotate_right(16));
    let rot32 = |x: u64| x.rotate_right(32);
    let [q0, q1, q2, q3, q4, q5, q6, q7] = *q;
    let [r0, r1, r2, r3, r4, r5, r6, r7] = r;
    *q = [
        q7 ^ r7 ^ r0 ^ rot32(q0 ^ r0),
        q0 ^ r0 ^ q7 ^ r7 ^ r1 ^ rot32(q1 ^ r1),
        q1 ^ r1 ^ r2 ^ rot32(q2 ^ r2),
        q2 ^ r2 ^ q7 ^ r7 ^ r3 ^ rot32(q3 ^ r3),
        q3 ^ r3 ^ q7 ^ r7 ^ r4 ^ rot32(q4 ^ r4),
        q4 ^ r4 ^ r5 ^ rot32(q5 ^ r5),
        q5 ^ r5 ^ r6 ^ rot32(q6 ^ r6),
        q6 ^ r6 ^ r7 ^ rot32(q7 ^ r7),
    ];
}

fn add_round_key(q: &mut [u64; 8], sk: &[u64]) {
    for (x, k) in q.iter_mut().zip(sk) {
        *x ^= k;
    }
}

/// Applies the S-box to each byte of a word (the key schedule's SubWord).
pub(crate) fn sub_word(x: u32) -> u32 {
    let mut q = [0u64; 8];
    q[0] = u64::from(x);
    ortho(&mut q);
    sbox(&mut q);
    ortho(&mut q);
    q[0] as u32
}

/// FIPS 197 §5.2 key expansion. Returns the round-key words, each the
/// little-endian reading of four key-schedule bytes, and the round count.
/// SubWord goes through the bitsliced S-box, so the schedule does not
/// index a table with key bytes.
///
/// `key.len()` must be 16, 24 or 32; the callers pass fixed-size arrays.
pub(crate) fn expand_key(key: &[u8]) -> ([u32; MAX_WORDS], usize) {
    const RCON: [u32; 10] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];
    let nk = key.len() / 4;
    let rounds = nk + 6;
    debug_assert!(matches!(nk, 4 | 6 | 8));
    let mut w = [0u32; MAX_WORDS];
    for (word, bytes) in w.iter_mut().zip(key.as_chunks::<4>().0) {
        *word = u32::from_le_bytes(*bytes);
    }
    let mut tmp = w[nk - 1];
    for i in nk..4 * (rounds + 1) {
        if i % nk == 0 {
            tmp = sub_word(tmp.rotate_right(8)) ^ RCON[i / nk - 1];
        } else if nk > 6 && i % nk == 4 {
            tmp = sub_word(tmp);
        }
        tmp ^= w[i - nk];
        w[i] = tmp;
    }
    (w, rounds)
}

/// Round keys in bitsliced form: eight words per round.
pub(crate) struct Keys {
    sk: [u64; 8 * (MAX_ROUNDS + 1)],
    rounds: usize,
}

impl Keys {
    /// Bitslices the expanded key words (`4 * (rounds + 1)` of them).
    pub(crate) fn new(words: &[u32], rounds: usize) -> Self {
        let mut sk = [0u64; 8 * (MAX_ROUNDS + 1)];
        for (round, w) in words.as_chunks::<4>().0.iter().enumerate() {
            let (a, b) = interleave_in(w);
            let mut q = [a, a, a, a, b, b, b, b];
            ortho(&mut q);
            sk[8 * round..8 * round + 8].copy_from_slice(&q);
        }
        Keys { sk, rounds }
    }

    pub(crate) fn wipe(&mut self) {
        crate::ct::zeroize::zeroize(&mut self.sk);
    }

    /// Encrypts four blocks in place.
    pub(crate) fn encrypt4(&self, blocks: &mut [[u8; 16]; 4]) {
        let mut q = [0u64; 8];
        for (i, b) in blocks.iter().enumerate() {
            let w: [u32; 4] = core::array::from_fn(|j| {
                u32::from_le_bytes(b[4 * j..4 * j + 4].try_into().unwrap())
            });
            (q[i], q[i + 4]) = interleave_in(&w);
        }
        ortho(&mut q);
        let sk = &self.sk;
        add_round_key(&mut q, &sk[..8]);
        for round in 1..self.rounds {
            sbox(&mut q);
            shift_rows(&mut q);
            mix_columns(&mut q);
            add_round_key(&mut q, &sk[8 * round..8 * round + 8]);
        }
        sbox(&mut q);
        shift_rows(&mut q);
        add_round_key(&mut q, &sk[8 * self.rounds..8 * self.rounds + 8]);
        ortho(&mut q);
        for (i, b) in blocks.iter_mut().enumerate() {
            for (j, w) in interleave_out(q[i], q[i + 4]).iter().enumerate() {
                b[4 * j..4 * j + 4].copy_from_slice(&w.to_le_bytes());
            }
        }
    }
}
