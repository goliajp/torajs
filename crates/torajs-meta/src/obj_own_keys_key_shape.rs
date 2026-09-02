//! Property-KEY shape predicates shared by the own-keys walks —
//! split out of [`crate::obj_own_keys`] (file-size cap). Each reads a
//! live key cell's WTF-8 spelling ([`crate::str_wtf8`]) and answers a
//! pure question about it: is it the internal [[Prototype]]-slot
//! simulation key, a canonical array index, or a given fixed name.

use core::ffi::c_void;

use crate::reflect::PROTO_SLOT_KEY;
use crate::str_wtf8::StrWtf8;

/// `true` iff the live key spells exactly [`PROTO_SLOT_KEY`] (the
/// [[Prototype]]-slot simulation key hidden from own-keys walks).
pub(crate) unsafe fn key_is_proto_slot(key: *const c_void) -> bool {
    unsafe { StrWtf8::of(key) }.as_bytes() == PROTO_SLOT_KEY
}

/// Equality of a live key's spelling against a fixed name.
pub(crate) unsafe fn key_bytes_are(key: *const c_void, name: &[u8]) -> bool {
    unsafe { StrWtf8::of(key) }.as_bytes() == name
}

/// The array index a live key canonically spells (§10.4.2), or
/// `None` — the shadow-entry filter's key shape check
/// ([`crate::arr_reflect::canonical_index`] on the spelling).
pub(crate) unsafe fn key_canonical_index(key: *const c_void) -> Option<u64> {
    crate::arr_reflect::canonical_index(unsafe { StrWtf8::of(key) }.as_bytes())
}

/// Canonical array-index test on a live key.
pub(crate) unsafe fn key_is_canonical_index(key: *const c_void) -> bool {
    unsafe { key_canonical_index(key) }.is_some()
}
