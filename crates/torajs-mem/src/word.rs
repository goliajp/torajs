//! Unaligned word access for the copy / fill / compare kernels.
//!
//! The loads and stores go through packed scalars, which codegen
//! spells as a single `load` / `store` with alignment 1 at every
//! optimisation level. `ptr::read_unaligned` would do the same once
//! optimised, but unoptimised it is a `memcpy` call — made from
//! inside `memcpy`.

#[repr(C, packed)]
struct U64(u64);
#[repr(C, packed)]
struct U32(u32);
#[repr(C, packed)]
struct U16(u16);

#[inline(always)]
pub(crate) unsafe fn ld8(p: *const u8) -> u64 {
    unsafe { (*p.cast::<U64>()).0 }
}

#[inline(always)]
pub(crate) unsafe fn st8(p: *mut u8, v: u64) {
    unsafe { (*p.cast::<U64>()).0 = v }
}

#[inline(always)]
pub(crate) unsafe fn ld4(p: *const u8) -> u32 {
    unsafe { (*p.cast::<U32>()).0 }
}

#[inline(always)]
pub(crate) unsafe fn st4(p: *mut u8, v: u32) {
    unsafe { (*p.cast::<U32>()).0 = v }
}

#[inline(always)]
pub(crate) unsafe fn ld2(p: *const u8) -> u16 {
    unsafe { (*p.cast::<U16>()).0 }
}

#[inline(always)]
pub(crate) unsafe fn st2(p: *mut u8, v: u16) {
    unsafe { (*p.cast::<U16>()).0 = v }
}

/// Copy 16 bytes, both words read before either is written.
#[inline(always)]
pub(crate) unsafe fn cp16(d: *mut u8, s: *const u8) {
    unsafe {
        let a = ld8(s);
        let b = ld8(s.add(8));
        st8(d, a);
        st8(d.add(8), b);
    }
}

/// Copy `n < 16` bytes as two possibly overlapping words of the
/// widest size that fits, every byte read before any is written — so
/// the ranges may overlap.
#[inline(always)]
pub(crate) unsafe fn cp_small(d: *mut u8, s: *const u8, n: usize) {
    unsafe {
        if n >= 8 {
            let a = ld8(s);
            let b = ld8(s.add(n - 8));
            st8(d, a);
            st8(d.add(n - 8), b);
        } else if n >= 4 {
            let a = ld4(s);
            let b = ld4(s.add(n - 4));
            st4(d, a);
            st4(d.add(n - 4), b);
        } else if n >= 2 {
            let a = ld2(s);
            let b = ld2(s.add(n - 2));
            st2(d, a);
            st2(d.add(n - 2), b);
        } else if n == 1 {
            *d = *s;
        }
    }
}
