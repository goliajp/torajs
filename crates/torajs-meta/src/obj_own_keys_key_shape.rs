//! Property-KEY shape predicates shared by the own-keys walks —
//! split out of [`crate::obj_own_keys`] (file-size cap). Each reads a
//! live key cell's WTF-8 spelling ([`crate::str_wtf8`]) and answers a
//! pure question about it: is it the internal [[Prototype]]-slot
//! simulation key, a canonical array index, or a given fixed name.

use core::ffi::c_void;

use crate::reflect::PROTO_SLOT_KEY;
use crate::str_wtf8::StrWtf8;

/// `true` iff the live key is one an own-keys walk must not report.
/// Two kinds:
///
/// - the runtime's internal slots — the [[Prototype]]-slot simulation
///   key [`PROTO_SLOT_KEY`] and a unit-class object's tag record
///   (`classmeta::unit::UNIT_RECORD_KEY`). Both start with NUL, so no
///   program can spell them;
/// - a PRIVATE element's mangled row (`__priv_<C>__<x>`). §7.3 keeps
///   private elements off every reflection face: they are not
///   properties. tr reifies a private method or accessor as an own
///   entry of the class prototype (and a static private member as one
///   of the class object), which is how its brand and its per-
///   evaluation identity are resolved — so the entry stays and the
///   walks skip it. `Object.getOwnPropertyNames(C.prototype)` answered
///   `["constructor", "__priv_C__m", "pub"]` where bun answers
///   `["constructor", "pub"]`, and a `static #x` even came out
///   enumerable, so `{...C}` carried it.
///
/// The private-field ROWS of an instance are a different walk and were
/// already filtered (`struct_enum_order::is_private_row`).
pub(crate) unsafe fn key_is_hidden_own(key: *const c_void) -> bool {
    let s = unsafe { StrWtf8::of(key) };
    let bytes = s.as_bytes();
    bytes == PROTO_SLOT_KEY
        || bytes == crate::classmeta::UNIT_RECORD_KEY
        || bytes.starts_with(b"__priv_")
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
