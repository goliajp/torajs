//! Branch-free stable merge sort for the native number comparators
//! (`(a, b) => a - b` / `b - a`, [`NumCmp`]).
//!
//! On random input every compare of the general kernel is a coin
//! flip for the branch predictor. Here the outcome of a compare only
//! ever feeds a select or a pointer step: runs of up to [`SMALL`]
//! slots sort through an odd-even transposition network, and two
//! sorted halves merge from both ends at once, the front taking the
//! smallest remaining slot and the back the largest, which gives two
//! independent dependency chains per loop iteration.
//!
//! Both pieces need the compare to be a strict weak order: the
//! bidirectional merge runs a fixed `n / 2` steps from each end and
//! only meets in the middle when the order is consistent. Integer
//! slots and non-NaN f64 slots compare that way, so this kernel
//! orders them exactly as any stable sort does (§23.1.3.30 asks for
//! a stable sort; `-0` and `+0` compare equal and keep their order).
//! A NaN compares false both ways round, which makes the
//! comparator inconsistent; an f64 array holding one (the undefined
//! slot sentinel is NaN-shaped too) keeps the general kernel's exact
//! sequence of compares.

use core::ffi::c_void;
use core::hint::select_unpredictable;

use crate::sort_cmp::{NumCmp, SortCmp};

unsafe extern "C" {
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);
}

/// Runs at or below this length go through the sorting network.
const SMALL: usize = 8;

impl<const F64: bool, const DESC: bool> NumCmp<F64, DESC> {
    /// A slot that sorts at or after every other slot — pads a short
    /// run up to the network's width; stability keeps a real slot
    /// equal to it ahead of the pads.
    const LAST: u64 = match (F64, DESC) {
        (true, false) => f64::INFINITY.to_bits(),
        (true, true) => f64::NEG_INFINITY.to_bits(),
        (false, false) => i64::MAX as u64,
        (false, true) => i64::MIN as u64,
    };
}

/// Sort `len >= 2` slots at `slots` in the order `NumCmp<F64, DESC>`
/// gives — stable, and identical to the general kernel's result.
///
/// # Safety
/// `slots` addresses `len` writable 8-byte slots.
pub(crate) unsafe fn sort<const F64: bool, const DESC: bool>(slots: *mut u64, len: usize) {
    unsafe {
        let cmp = NumCmp::<F64, DESC>;
        if F64 && has_nan(slots, len) {
            crate::sort::sort_slots(slots, len, &cmp);
            return;
        }
        if len <= SMALL {
            network(slots, len, &cmp);
            return;
        }
        let scratch = malloc(len * 8) as *mut u64;
        range(slots, scratch, len, &cmp);
        free(scratch as *mut c_void);
    }
}

unsafe fn has_nan(s: *const u64, n: usize) -> bool {
    unsafe {
        let mut nan = false;
        for k in 0..n {
            let x = f64::from_bits(*s.add(k));
            nan |= x.is_nan();
        }
        nan
    }
}

/// Top-down merge sort of `n` slots at `s`; `scratch` holds `n`.
unsafe fn range<const F64: bool, const DESC: bool>(
    s: *mut u64,
    scratch: *mut u64,
    n: usize,
    cmp: &NumCmp<F64, DESC>,
) {
    unsafe {
        if n <= SMALL {
            network(s, n, cmp);
            return;
        }
        let h = n / 2;
        range(s, scratch, h, cmp);
        range(s.add(h), scratch, n - h, cmp);
        core::ptr::copy_nonoverlapping(s, scratch, n);
        merge(scratch, h, n, s, cmp);
    }
}

/// Merge `src[..h]` and `src[h..n]` (both sorted, `h = n / 2`) into
/// `dst[..n]`. The front takes the left slot unless the right one
/// sorts strictly before it, the back takes the right slot unless the
/// left one sorts strictly after it, so ties keep their order. With a
/// consistent order neither end reads past its half: the front can
/// only run out of left slots on its last step.
unsafe fn merge<const F64: bool, const DESC: bool>(
    src: *const u64,
    h: usize,
    n: usize,
    dst: *mut u64,
    cmp: &NumCmp<F64, DESC>,
) {
    unsafe {
        let mut lf = src;
        let mut rf = src.add(h);
        let mut lb = src.add(h - 1);
        let mut rb = src.add(n - 1);
        let mut df = dst;
        let mut db = dst.add(n - 1);
        for _ in 0..n / 2 {
            // the slot to store is re-read through the chosen pointer,
            // so the compare stays the only consumer of the first loads
            let take_r = cmp.is_gt(*lf, *rf);
            *df = *select_unpredictable(take_r, rf, lf);
            rf = rf.wrapping_add(take_r as usize);
            lf = lf.wrapping_add((!take_r) as usize);
            df = df.add(1);

            let take_l = cmp.is_gt(*lb, *rb);
            *db = *select_unpredictable(take_l, lb, rb);
            lb = lb.wrapping_sub(take_l as usize);
            rb = rb.wrapping_sub((!take_l) as usize);
            db = db.wrapping_sub(1);
        }
        if n & 1 != 0 {
            *df = *select_unpredictable(lf <= lb, lf, rf);
        }
    }
}

/// Stable sort of `n <= SMALL` slots: padded to the full width with
/// [`NumCmp::LAST`], then an odd-even transposition network. It only
/// ever exchanges neighbours that are strictly out of order, so equal
/// slots never pass each other.
unsafe fn network<const F64: bool, const DESC: bool>(
    s: *mut u64,
    n: usize,
    cmp: &NumCmp<F64, DESC>,
) {
    unsafe {
        let pad = NumCmp::<F64, DESC>::LAST;
        // `n >= 1`: slot `n - 1` stands in for the loads past the run
        let mut v: [u64; SMALL] =
            core::array::from_fn(|k| select_unpredictable(k < n, *s.add(k.min(n - 1)), pad));
        for round in 0..SMALL {
            let mut a = round & 1;
            while a + 1 < SMALL {
                let (x, y) = (v[a], v[a + 1]);
                let swap = cmp.is_gt(x, y);
                v[a] = select_unpredictable(swap, y, x);
                v[a + 1] = select_unpredictable(swap, x, y);
                a += 2;
            }
        }
        // back to front: a pad lands on slot `n - 1` first and the real
        // slot overwrites it last, so every store is unconditional
        for k in (0..SMALL).rev() {
            *s.add(k.min(n - 1)) = v[k];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xorshift(seed: &mut u64) -> u64 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *seed
    }

    /// The kernel against the general one on the same slots: many
    /// ties, `-0` / `+0` pairs, infinities, the pad value itself.
    fn check<const F64: bool, const DESC: bool>(data: &[u64]) {
        let mut got = data.to_vec();
        let mut want = data.to_vec();
        unsafe {
            if got.len() >= 2 {
                sort::<F64, DESC>(got.as_mut_ptr(), got.len());
                crate::sort::sort_slots(want.as_mut_ptr(), want.len(), &NumCmp::<F64, DESC>);
            }
        }
        assert_eq!(got, want, "n = {}, f64 = {F64}, desc = {DESC}", data.len());
    }

    #[test]
    fn matches_the_general_kernel() {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let specials = [-0.0f64, 0.0, f64::INFINITY, f64::NEG_INFINITY, 1.5, -1.5];
        let mut lens: Vec<usize> = (0..=70).collect();
        lens.extend([127, 128, 129, 255, 1000, 1001]);
        for &n in &lens {
            let f: Vec<u64> = (0..n)
                .map(|_| match xorshift(&mut seed) % 4 {
                    0 => specials[(xorshift(&mut seed) % 6) as usize].to_bits(),
                    _ => ((xorshift(&mut seed) % 21) as f64 - 10.0).to_bits(),
                })
                .collect();
            let g: Vec<u64> = (0..n)
                .map(|_| match xorshift(&mut seed) % 8 {
                    0 => i64::MAX as u64,
                    1 => i64::MIN as u64,
                    _ => (xorshift(&mut seed) % 41) as i64 as u64 - 20,
                })
                .collect();
            check::<true, false>(&f);
            check::<true, true>(&f);
            check::<false, false>(&g);
            check::<false, true>(&g);
            let mut asc = f.clone();
            check::<true, false>(&asc);
            asc.sort_by(|a, b| f64::from_bits(*a).total_cmp(&f64::from_bits(*b)));
            check::<true, false>(&asc);
            asc.reverse();
            check::<true, false>(&asc);
        }
    }

    /// A NaN anywhere sends the slots through the general kernel.
    #[test]
    fn nan_takes_the_general_kernel() {
        let mut seed = 7u64;
        for &n in &[2usize, 8, 9, 40, 300] {
            let f: Vec<u64> = (0..n)
                .map(|i| {
                    if i % 5 == 3 {
                        f64::NAN.to_bits()
                    } else {
                        ((xorshift(&mut seed) % 50) as f64).to_bits()
                    }
                })
                .collect();
            check::<true, false>(&f);
            check::<true, true>(&f);
        }
    }
}
