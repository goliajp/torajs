//! Fixed-literal Str factories — port of `runtime_str.c` L1434-1448.
//!
//! `String(null)` and `String(undefined)` per ES §7.1.17 / §6.1.1 —
//! both produce a freshly-allocated Str with the literal name
//! ("null" / "undefined"). Used by the `any_to_str` dispatch when
//! the AnyValue tag is ANY_NULL / ANY_UNDEF.
//!
//! Lives in torajs-str (not torajs-anyvalue) because the only thing
//! these fns do is allocate a Str — they don't touch the anyvalue
//! tag space. Keeps them within reach for callers that go through
//! the str-only path (template literals, string concat).

use crate::alloc::StrBlock;

/// `String(null)` → fresh "null" Str.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_null_to_str() -> *mut u8 {
    alloc_literal_str(b"null")
}

/// `String(undefined)` → fresh "undefined" Str.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_undefined_to_str() -> *mut u8 {
    alloc_literal_str(b"undefined")
}

#[inline]
fn alloc_literal_str(data: &[u8]) -> *mut u8 {
    let mut block = StrBlock::alloc(data.len() as u64);
    // SAFETY: block was just allocated with payload capacity matching
    // the literal's byte length.
    let dst = unsafe { block.as_bytes_mut(data.len() as u64) };
    dst.copy_from_slice(data);
    block.into_raw()
}
