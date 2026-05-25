//! Internal BigInt block helpers — not exposed via C ABI.
//!
//! These mirror `runtime_bigint.c`'s static helpers
//! (`bigint_alloc_raw` / `bigint_normalize` / `bigint_words` /
//! `bigint_mul_u32_inplace` / `bigint_add_u32_inplace`) for the
//! Rust-side construct fns. They are NOT given `#[no_mangle]` /
//! `extern "C"` so they don't collide with the C-side `static`
//! versions still used by the remaining C-side BigInt operations
//! (add/sub/mul/div/etc, scheduled for P3.3-c..i ports).
//!
//! All public-to-crate (`pub(crate)`) and `unsafe` — raw pointer
//! ops over the BigInt heap layout.

use core::ffi::c_void;

use crate::layout::{LEN_OFF, SIGN_OFF, TAG_BIGINT, WORDS_OFF};

unsafe extern "C" {
    /// torajs-mmalloc libc-compat malloc. v0.7-A2 step 6b cutover:
    /// link symbol redirected from libc `malloc` to mmalloc's
    /// libc-compat shim (`__torajs_libc_malloc`) so user binaries
    /// pull libtorajs_mmalloc.a instead of libSystem.dylib.
    #[link_name = "__torajs_libc_malloc"]
    pub(crate) fn malloc(n: usize) -> *mut c_void;

    /// torajs-mmalloc libc-compat free. See note on `malloc` above.
    #[link_name = "__torajs_libc_free"]
    pub(crate) fn free(p: *mut c_void);
}

/// Allocate a fresh BigInt heap block with `len` u64 limbs.
///
/// Header init: `refcount=1`, `type_tag=TAG_BIGINT`, `flags=0`.
/// Body init: `sign=0`, `len=<arg>`, words zero-filled.
///
/// # Safety
/// Caller owns the returned pointer (refcount=1). Must eventually
/// be released via `__torajs_bigint_drop_rc` (or equivalent C-side
/// `free` once rc reaches 0).
#[inline]
pub(crate) unsafe fn alloc_raw(len: u32) -> *mut u8 {
    let total = WORDS_OFF + (len as usize) * 8;
    let p = unsafe { malloc(total) as *mut u8 };
    unsafe {
        *(p as *mut u32) = 1; // refcount
        *(p.add(4) as *mut u16) = TAG_BIGINT;
        *(p.add(6) as *mut u16) = 0; // flags
        *(p.add(SIGN_OFF) as *mut u32) = 0;
        *(p.add(LEN_OFF) as *mut u32) = len;
        core::ptr::write_bytes(p.add(WORDS_OFF), 0, (len as usize) * 8);
    }
    p
}

#[inline]
pub(crate) unsafe fn read_len(p: *const u8) -> u32 {
    unsafe { *(p.add(LEN_OFF) as *const u32) }
}

#[inline]
pub(crate) unsafe fn write_len(p: *mut u8, len: u32) {
    unsafe { *(p.add(LEN_OFF) as *mut u32) = len }
}

#[inline]
pub(crate) unsafe fn read_sign(p: *const u8) -> u32 {
    unsafe { *(p.add(SIGN_OFF) as *const u32) }
}

#[inline]
pub(crate) unsafe fn write_sign(p: *mut u8, sign: u32) {
    unsafe { *(p.add(SIGN_OFF) as *mut u32) = sign }
}

#[inline]
pub(crate) unsafe fn words_mut(p: *mut u8) -> *mut u64 {
    unsafe { p.add(WORDS_OFF) as *mut u64 }
}

#[inline]
pub(crate) unsafe fn words_ptr(p: *const u8) -> *const u64 {
    unsafe { p.add(WORDS_OFF) as *const u64 }
}

/// Strip trailing zero limbs; coerce signed-zero to positive-zero.
/// Maintains the BigInt invariant (`words[len - 1] != 0`, or `len == 0`).
#[inline]
pub(crate) unsafe fn normalize(p: *mut u8) {
    unsafe {
        let mut new_len = read_len(p) as usize;
        let w = words_ptr(p);
        while new_len > 0 && *w.add(new_len - 1) == 0 {
            new_len -= 1;
        }
        write_len(p, new_len as u32);
        if new_len == 0 {
            write_sign(p, 0);
        }
    }
}

/// Multiply magnitude by a u32 in place. On carry overflow, allocate
/// a fresh block one limb longer, copy + append carry, free old, and
/// update `*pp` to point to the new block. Sign is preserved across
/// realloc.
///
/// # Safety
/// `*pp` must point to a valid BigInt heap block owned by the caller.
/// After return, the pointer may have changed; old pointer is no
/// longer valid.
pub(crate) unsafe fn mul_u32_inplace(pp: *mut *mut u8, mul: u32) {
    unsafe {
        let b = *pp;
        let len = read_len(b) as usize;
        let w = words_mut(b);
        let mul128 = mul as u128;
        let mut carry: u64 = 0;
        for i in 0..len {
            let prod = (*w.add(i) as u128) * mul128 + (carry as u128);
            *w.add(i) = prod as u64;
            carry = (prod >> 64) as u64;
        }
        if carry != 0 {
            let nb = alloc_raw((len + 1) as u32);
            let sign = read_sign(b);
            write_sign(nb, sign);
            let nw = words_mut(nb);
            for i in 0..len {
                *nw.add(i) = *w.add(i);
            }
            *nw.add(len) = carry;
            free(b as *mut c_void);
            *pp = nb;
        }
    }
}

/// Add a u32 to the magnitude in place. Same realloc-on-carry shape
/// as [`mul_u32_inplace`].
pub(crate) unsafe fn add_u32_inplace(pp: *mut *mut u8, add: u32) {
    unsafe {
        let b = *pp;
        let len = read_len(b) as usize;
        let w = words_mut(b);
        let mut carry = add as u64;
        for i in 0..len {
            if carry == 0 {
                break;
            }
            let sum = (*w.add(i) as u128) + (carry as u128);
            *w.add(i) = sum as u64;
            carry = (sum >> 64) as u64;
        }
        if carry != 0 {
            let nb = alloc_raw((len + 1) as u32);
            let sign = read_sign(b);
            write_sign(nb, sign);
            let nw = words_mut(nb);
            for i in 0..len {
                *nw.add(i) = *w.add(i);
            }
            *nw.add(len) = carry;
            free(b as *mut c_void);
            *pp = nb;
        }
    }
}
