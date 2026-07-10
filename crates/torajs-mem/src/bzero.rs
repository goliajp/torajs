//! `bzero(s, n)` — zero `n` bytes at `s` (legacy BSD signature).
//!
//! On AArch64 the LLVM backend lowers a zero-fill
//! `@llvm.memset.p0.i64(ptr, i8 0, i64 n)` to a `bzero` call (the
//! non-zero fills stay `memset`). So even with an in-house `memset`,
//! a residual `_bzero` import survives — confirmed in
//! `docs/v0.7-A5-finding.md` § "Step 16-a-3 audit": the memset shim
//! dropped `_memset` but left `_bzero`, proving the two come from
//! distinct lowered symbols.
//!
//! ## The self-recursion trap (why a naive byte loop is wrong)
//!
//! A naive `while i < n { *s.add(i) = 0; i += 1 }` is **miscompiled
//! into infinite recursion**. LLVM's LoopIdiomRecognize folds the
//! zero-store loop into `@llvm.memset(p, 0, n)`; the name-aware
//! self-recursion guard in that pass only protects a function
//! literally named `memset`, so inside `bzero` it fires. The
//! AArch64 backend then lowers that zero-memset to a `bzero` call —
//! and we are *in* `bzero`. Result: `bzero: cbz x1, ret; b bzero` —
//! an unconditional self-branch (the first 16-a-3-b ship hit exactly
//! this: 155 conformance fixtures timed out at 60 s). `memcpy` /
//! `memset` escape because their idiom fold targets the same-named
//! libcall, so the pass's guard catches it; `bzero` does not share a
//! name with `memset`, so nothing guards the late backend rewrite.
//!
//! ## Fix — delegate to `memset` through an opaque barrier
//!
//! `bzero(s, n)` ≡ `memset(s, 0, n)`. We call the already-safe,
//! name-guarded in-house `memset` *indirectly* through
//! [`core::hint::black_box`], which makes the function pointer opaque
//! to the optimizer. That blocks LTO from inlining `memset`'s loop
//! back into `bzero` (which would re-form the zero-fill loop and
//! re-trigger the trap). `bzero` itself stays loop-free → no idiom
//! recognition → no self-call; `memset` runs its own NEON-vectorized
//! zero-fill at full speed.

/// `bzero(s, n)` — write `n` zero bytes at `s`.
///
/// # Safety
///
/// `s` must be writable for ≥ `n` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bzero(s: *mut u8, n: usize) {
    // opaque fn-ptr barrier: prevents the optimizer from inlining
    // memset's zero-fill loop into bzero (see module docs — that
    // would re-create the AArch64 memset→bzero self-recursion).
    let memset_fn: unsafe extern "C" fn(
        *mut core::ffi::c_void,
        i32,
        usize,
    ) -> *mut core::ffi::c_void = core::hint::black_box(crate::memset::memset);
    unsafe {
        memset_fn(s.cast(), 0, n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_n_is_noop() {
        let mut buf = [0xAAu8; 4];
        unsafe { bzero(buf.as_mut_ptr(), 0) };
        assert_eq!(&buf, &[0xAA, 0xAA, 0xAA, 0xAA]);
    }

    #[test]
    fn full_zero() {
        let mut buf = [0xFFu8; 8];
        unsafe { bzero(buf.as_mut_ptr(), 8) };
        assert_eq!(&buf, &[0u8; 8]);
    }

    #[test]
    fn partial() {
        let mut buf = [b'x'; 6];
        unsafe { bzero(buf.as_mut_ptr(), 3) };
        assert_eq!(&buf, b"\0\0\0xxx");
    }
}
