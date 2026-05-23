//! BigInt division / mod / pow / neg.
//!
//! Port of `runtime_bigint.c` lines 280-472 (P3.3-e, 2026-05-23).
//!
//! Public extern fns:
//! - [`__torajs_bigint_div`] — truncated division, sign = `a.sign ^ b.sign`
//! - [`__torajs_bigint_mod`] — truncated remainder, sign = `a.sign`
//! - [`__torajs_bigint_pow`] — square-and-multiply via cross-tier
//!   `__torajs_bigint_mul` extern (mul lives in torajs-bigint::mul)
//! - [`__torajs_bigint_neg`] — fresh clone with sign flipped (zero stays
//!   positive)
//!
//! Internal helpers:
//! - `bit_count` / `bit_at` / `shl_inplace_one` / `set_bit` — bit-level
//!   accessors on the limb array. shl_inplace_one may realloc on carry,
//!   same shape as the add/mul realloc paths in `internal.rs`.
//! - `mag_divmod` — bit-by-bit long division (O(bit_count(a))). Schoolbook
//!   long division; no Newton-Raphson / Knuth-D yet — perf bench can
//!   trigger that later if BigInt op profiles call for it.
//!
//! Cross-tier dependencies:
//! - `__torajs_bigint_mul` — pow calls it. Resolves at link time against
//!   torajs-bigint's own staticlib (same crate, same .a file).
//! - `__torajs_throw_range_error` — div/mod throw on zero divisor, pow
//!   throws on negative exponent. Provided by `libtorajs_throw.a`.

use core::ffi::c_void;

use crate::arith::{mag_cmp, mag_sub};
use crate::internal::{
    alloc_raw, free, normalize, read_len, read_sign, words_mut, words_ptr, write_sign,
};

unsafe extern "C" {
    /// Cross-tier — resolves to torajs-throw's wrapper that arms the
    /// thread-local throw slot. Returns normally; caller bails to a NULL
    /// result and the ssa_lower-emitted `throw_check` diverts the
    /// continuation.
    fn __torajs_throw_range_error(msg: *const u8);

    /// Cross-tier — torajs-bigint::mul. Same `extern "C"` ABI as the
    /// rest of the family. Same staticlib (linker resolves intra-crate
    /// references at `tr build` time).
    fn __torajs_bigint_mul(a: *const c_void, b: *const c_void) -> *mut u8;
}

// ============================================================
// Bit-level helpers (private)
// ============================================================

/// Magnitude bit count — index of the high-bit + 1 (i.e. ⌈log₂(|b| + 1)⌉).
/// Returns 0 for a zero-magnitude BigInt.
unsafe fn bit_count(b: *const u8) -> u32 {
    unsafe {
        let len = read_len(b);
        if len == 0 {
            return 0;
        }
        let w = words_ptr(b);
        let mut hi = *w.add(len as usize - 1);
        let mut hi_bits: u32 = 0;
        while hi != 0 {
            hi_bits += 1;
            hi >>= 1;
        }
        (len - 1) * 64 + hi_bits
    }
}

/// Read one bit (0 or 1) at `bit` position. Beyond allocation → 0.
unsafe fn bit_at(b: *const u8, bit: u32) -> i32 {
    unsafe {
        let limb = bit / 64;
        let off = bit % 64;
        if limb >= read_len(b) {
            return 0;
        }
        ((*words_ptr(b).add(limb as usize) >> off) & 1) as i32
    }
}

/// Shift magnitude left by 1 bit in place. On carry overflow, allocate a
/// fresh block one limb longer and update `*rp`. Mirrors the
/// `mul_u32_inplace` / `add_u32_inplace` shape.
unsafe fn shl_inplace_one(rp: *mut *mut u8) {
    unsafe {
        let r = *rp;
        let len = read_len(r) as usize;
        let w = words_mut(r);
        let mut carry: u64 = 0;
        for i in 0..len {
            let next = (*w.add(i) >> 63) & 1;
            *w.add(i) = (*w.add(i) << 1) | carry;
            carry = next;
        }
        if carry != 0 {
            let nr = alloc_raw((len + 1) as u32);
            let sign = read_sign(r);
            write_sign(nr, sign);
            let nw = words_mut(nr);
            for i in 0..len {
                *nw.add(i) = *w.add(i);
            }
            *nw.add(len) = carry;
            free(r as *mut c_void);
            *rp = nr;
        }
    }
}

/// Set one bit (`limb[bit/64] |= 1 << (bit % 64)`). No-op if `bit` is
/// beyond the allocated limb range; caller is expected to size the
/// destination upfront (true for mag_divmod where `q` is pre-allocated
/// at `a.len` limbs).
unsafe fn set_bit(b: *mut u8, bit: u32) {
    unsafe {
        let limb = bit / 64;
        let off = bit % 64;
        if limb >= read_len(b) {
            return;
        }
        *words_mut(b).add(limb as usize) |= 1u64 << off;
    }
}

// ============================================================
// Magnitude divmod (private)
// ============================================================

/// Returns `(q, r)` as fresh `+1`-rc heap blocks. Sign is left at 0 on
/// both outputs — caller stamps the spec-correct sign. Pre-condition:
/// `b` has `len > 0` (caller checks div-by-zero before calling this).
unsafe fn mag_divmod(a: *const u8, b: *const u8) -> (*mut u8, *mut u8) {
    unsafe {
        let a_len = read_len(a);
        let q = alloc_raw(a_len); // zero-filled by alloc_raw
        let mut r = alloc_raw(0);

        if mag_cmp(a, b) < 0 {
            // a < b → q = 0, r = a (clone).
            free(r as *mut c_void);
            let r_clone = alloc_raw(a_len);
            if a_len > 0 {
                let src = words_ptr(a);
                let dst = words_mut(r_clone);
                for i in 0..(a_len as usize) {
                    *dst.add(i) = *src.add(i);
                }
            }
            return (q, r_clone);
        }

        let a_bits = bit_count(a);
        let mut i = a_bits as i64 - 1;
        while i >= 0 {
            shl_inplace_one(&mut r);
            if bit_at(a, i as u32) != 0 {
                // r |= 1 — but r might still have len == 0 from the
                // initial alloc_raw(0). In that case allocate a fresh
                // 1-limb block.
                if read_len(r) == 0 {
                    free(r as *mut c_void);
                    r = alloc_raw(1);
                    *words_mut(r) = 1;
                } else {
                    *words_mut(r) |= 1;
                }
            }
            if mag_cmp(r, b) >= 0 {
                let new_r = mag_sub(r, b);
                free(r as *mut c_void);
                r = new_r;
                set_bit(q, i as u32);
            }
            i -= 1;
        }
        normalize(q);
        normalize(r);
        (q, r)
    }
}

// ============================================================
// extern "C" entry points
// ============================================================

/// `a / b` for BigInt. Truncated toward zero; result sign = a.sign XOR
/// b.sign. Throws RangeError on `b == 0n`.
///
/// # Safety
/// `a_` and `b_` must be valid BigInt heap pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_div(a_: *const c_void, b_: *const c_void) -> *mut u8 {
    let a = a_ as *const u8;
    let b = b_ as *const u8;
    unsafe {
        if read_len(b) == 0 {
            __torajs_throw_range_error(b"BigInt divide by zero\0".as_ptr());
            return core::ptr::null_mut();
        }
        let (q, r) = mag_divmod(a, b);
        free(r as *mut c_void);
        let a_sign = read_sign(a);
        let b_sign = read_sign(b);
        let q_sign = if (a_sign ^ b_sign) != 0 { 1 } else { 0 };
        write_sign(q, q_sign);
        normalize(q);
        q
    }
}

/// `a % b` for BigInt. Truncated remainder; result sign = a.sign.
/// Throws RangeError on `b == 0n`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_mod(a_: *const c_void, b_: *const c_void) -> *mut u8 {
    let a = a_ as *const u8;
    let b = b_ as *const u8;
    unsafe {
        if read_len(b) == 0 {
            __torajs_throw_range_error(b"BigInt divide by zero\0".as_ptr());
            return core::ptr::null_mut();
        }
        let (q, r) = mag_divmod(a, b);
        free(q as *mut c_void);
        let a_sign = read_sign(a);
        write_sign(r, a_sign);
        normalize(r);
        r
    }
}

/// `a ** b` for BigInt via square-and-multiply.
///
/// JS spec quirks:
/// - negative exponent → RangeError (`x ** -1n` undefined for BigInt
///   since no fractional)
/// - `base ** 0n` always 1n (including `0n ** 0n` per spec — V8 / bun
///   agree)
/// - sign of result: only negative iff base is negative AND exp is odd
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_pow(base_: *const c_void, exp_: *const c_void) -> *mut u8 {
    let base = base_ as *const u8;
    let exp = exp_ as *const u8;
    unsafe {
        if read_sign(exp) != 0 {
            __torajs_throw_range_error(b"BigInt negative exponent\0".as_ptr());
            return core::ptr::null_mut();
        }
        // Result starts at 1n.
        let result = alloc_raw(1);
        *words_mut(result) = 1;
        if read_len(exp) == 0 {
            return result;
        }
        // Local mutable copy of base, sign stripped (track separately).
        let base_len = read_len(base);
        let cur = alloc_raw(base_len);
        if base_len > 0 {
            let src = words_ptr(base);
            let dst = words_mut(cur);
            for i in 0..(base_len as usize) {
                *dst.add(i) = *src.add(i);
            }
        }
        let exp_lo = *words_ptr(exp); // low limb (exp.len > 0 here)
        let result_sign = if read_sign(base) != 0 && (exp_lo & 1) != 0 {
            1
        } else {
            0
        };
        // Square-and-multiply, low bit to high bit.
        let mut result = result;
        let mut cur = cur;
        let e_bits = bit_count(exp);
        for i in 0..e_bits {
            if bit_at(exp, i) != 0 {
                let next =
                    __torajs_bigint_mul(result as *const c_void, cur as *const c_void);
                free(result as *mut c_void);
                result = next;
            }
            if i + 1 < e_bits {
                let sq =
                    __torajs_bigint_mul(cur as *const c_void, cur as *const c_void);
                free(cur as *mut c_void);
                cur = sq;
            }
        }
        free(cur as *mut c_void);
        // mul stripped sign during magnitude loop and set product sign
        // by XOR — but we stripped base sign upfront so products are
        // positive throughout. Stamp the spec-correct sign now.
        write_sign(result, if read_len(result) == 0 { 0 } else { result_sign });
        normalize(result);
        result
    }
}

/// `-a` for BigInt. Fresh `+1`-rc clone with sign flipped. Zero stays
/// positive (BigInt has no signed zero).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_bigint_neg(a_: *const c_void) -> *mut u8 {
    let a = a_ as *const u8;
    unsafe {
        let len = read_len(a);
        let r = alloc_raw(len);
        if len > 0 {
            let src = words_ptr(a);
            let dst = words_mut(r);
            for i in 0..(len as usize) {
                *dst.add(i) = *src.add(i);
            }
        }
        let new_sign = if len == 0 {
            0
        } else if read_sign(a) == 0 {
            1
        } else {
            0
        };
        write_sign(r, new_sign);
        r
    }
}
