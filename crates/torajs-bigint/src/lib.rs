//! BigInt primitives for the torajs AOT TypeScript runtime.
//!
//! Layer-2 substrate of the architecture rewrite
//! (`docs/architecture-rewrite.md` P3.3). Self-hosted arbitrary-
//! precision integer — sign-and-magnitude with u64-limb little-
//! endian magnitude:
//!
//! ```text
//! [universal header (8B)] [sign u32] [len u32] [words u64[len]]
//! ```
//!
//! Sign rules: 0 = non-negative (zero is canonical positive),
//! 1 = negative. Magnitude invariant: `words[len - 1] != 0` (no
//! leading zero limbs); if `len == 0` then `sign == 0` (no signed
//! zero). Every constructor maintains both.
//!
//! ## Sub-step matrix (P3.3-{a..i})
//!
//! | Phase    | Adds                                                       |
//! |----------|------------------------------------------------------------|
//! | P3.3-a   | Scaffold + Layout + drop + drop_rc (this commit)           |
//! | P3.3-b   | Construction: from_decimal / from_hex / from_str /         |
//! |          | from_number / from_i64 / clone                             |
//! | P3.3-c   | Arith: add / sub                                           |
//! | P3.3-d   | Arith: mul (schoolbook + Karatsuba)                        |
//! | P3.3-e   | Arith: div / mod / pow / neg                               |
//! | P3.3-f   | Compare: cmp / eq                                          |
//! | P3.3-g   | Format: to_string                                          |
//! | P3.3-h   | Bitwise: and / or / xor / not                              |
//! | P3.3-i   | Shift: shl / shr                                           |
//!
//! Each ship deletes the corresponding C fns from `runtime_bigint.c`;
//! P3.3 closure = `runtime_bigint.c` removed entirely.
//!
//! ## Why `std`, not `no_std`
//!
//! Same reason as [`torajs-rc`] / [`torajs-str`] / [`torajs-num`]:
//! cargo's `cargo test` + dual `crate-type = ["rlib", "staticlib"]`
//! + `no_std` combination triggers a precompiled-core panic-strategy
//! mismatch that has no clean fix on stable. `std` staticlibs link
//! cleanly at `tr build` time.

pub mod arith;
pub mod bitwise;
pub mod compare;
pub mod construct;
pub mod divmod;
pub mod drop;
pub mod internal;
pub mod layout;
pub mod mul;
pub mod shift;
pub mod str_bridge;
pub mod tostring;

pub use arith::{__torajs_bigint_add, __torajs_bigint_sub};
pub use bitwise::{
    __torajs_bigint_and, __torajs_bigint_not, __torajs_bigint_or, __torajs_bigint_xor,
};
pub use compare::{__torajs_bigint_cmp, __torajs_bigint_eq};
pub use construct::{
    __torajs_bigint_clone, __torajs_bigint_from_decimal, __torajs_bigint_from_hex,
    __torajs_bigint_from_i64, __torajs_bigint_from_number, __torajs_bigint_from_str,
};
pub use divmod::{
    __torajs_bigint_div, __torajs_bigint_mod, __torajs_bigint_neg, __torajs_bigint_pow,
};
pub use drop::{__torajs_bigint_drop, __torajs_bigint_drop_rc};
pub use mul::__torajs_bigint_mul;
pub use shift::{__torajs_bigint_shl, __torajs_bigint_shr};
pub use tostring::__torajs_bigint_to_string;

// `__torajs_str_alloc_pooled` is provided by `libtorajs_str.a` at
// `tr build` link time. cargo unit tests of torajs-bigint don't link
// torajs-str's staticlib — provide a panicking stub so the test
// binary still links. Same pattern as torajs-num.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_alloc_pooled(_len: u64) -> *mut u8 {
    panic!(
        "torajs-bigint unit-test stub: __torajs_str_alloc_pooled should not be called from cargo test paths"
    );
}

// `__torajs_rc_dec` is provided by `libtorajs_rc.a` at `tr build`
// link time. For cargo unit tests of torajs-bigint (which don't
// link torajs-rc's staticlib), provide a `#[cfg(test)]` stub so
// the test binary still links — torajs-bigint's unit tests only
// exercise NULL-path early returns that never actually call into
// `__torajs_rc_dec`, but the linker resolves the symbol unconditionally.
// Same pattern as torajs-num's `__torajs_str_alloc_pooled` stub.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_rc_dec(_p: *mut core::ffi::c_void) -> i32 {
    // 1 = "last owner, caller should free". Unit-test path never
    // reaches here (NULL early-return guards in drop_rc), so the
    // return value is unobservable; we still pick the "free" branch
    // for predictability if future tests do reach it.
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ffi::c_void;

    /// `__torajs_bigint_drop(NULL)` must be a no-op (no UB / no abort).
    #[test]
    fn drop_null_is_noop() {
        unsafe { __torajs_bigint_drop(core::ptr::null_mut()) };
    }

    /// `__torajs_bigint_drop_rc(NULL)` must be a no-op.
    /// Cannot exercise the non-NULL path here because cargo test
    /// doesn't link libtorajs_rc.a's `__torajs_rc_dec` symbol —
    /// the FFI extern is unresolved at test time. End-to-end
    /// coverage is via the conformance fixture that exercises a
    /// real BigInt binding's drop path through `tr run`.
    #[test]
    fn drop_rc_null_is_noop() {
        unsafe { __torajs_bigint_drop_rc(core::ptr::null_mut()) };
    }

    /// Sanity-check the layout constants match the C struct.
    #[test]
    fn layout_constants() {
        assert_eq!(layout::HEAP_HEADER_SIZE, 8);
        assert_eq!(layout::SIGN_OFF, 8);
        assert_eq!(layout::LEN_OFF, 12);
        assert_eq!(layout::WORDS_OFF, 16);
        assert_eq!(layout::TAG_BIGINT, 10);
    }

    /// `__torajs_bigint_drop` signature must accept `*mut c_void`
    /// for C-ABI compatibility with `runtime_str.c`'s call site.
    #[test]
    fn drop_signature_takes_void_ptr() {
        let p: *mut c_void = core::ptr::null_mut();
        unsafe { __torajs_bigint_drop(p) };
    }
}
