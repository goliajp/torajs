//! Bridge to torajs-str's allocator — extern decl only.
//!
//! torajs-bigint is a Layer-2 sibling of torajs-str (per architecture-
//! rewrite.md DAG: Layer-N → Layer-(N-1) only, same-layer cross
//! deps forbidden), so we declare the alloc fn as a C-ABI extern
//! instead of adding torajs-str as a Cargo dep. At `tr build` /
//! `tr run` link time the symbol resolves against libtorajs_str.a;
//! for `cargo test` we provide a `#[cfg(test)]` stub in [`crate`]
//! (see `lib.rs`) so the test binary still links.
//!
//! Mirrors `torajs-num::str_bridge` exactly — same constraint, same
//! solution. A future torajs-types Layer-1 crate would unify.

unsafe extern "C" {
    /// `__torajs_str_alloc_latin1(src, len) -> *mut u8` — `len` bytes
    /// as Latin-1 units, or past the longest Str a pending RangeError
    /// and an empty placeholder; provided by libtorajs_str.a.
    fn __torajs_str_alloc_latin1(src: *const u8, len: i64) -> *mut u8;
}

/// Allocate a fresh Str heap block and copy `payload` into it.
/// Returns the raw pointer for FFI handoff. Used by BigInt → string
/// surface (toString and friends).
#[inline]
pub fn alloc_str(payload: &[u8]) -> *mut u8 {
    // a radix-2 spelling of a large BigInt can pass the longest Str
    unsafe { __torajs_str_alloc_latin1(payload.as_ptr(), payload.len() as i64) }
}
