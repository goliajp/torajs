//! Property-KEY shape predicates shared by the own-keys walks —
//! split out of [`crate::obj_own_keys`] (file-size cap). Each reads a
//! live Str key cell's `len` u32 at +8 and its payload at +16, and
//! answers a pure question about the spelling: is it the internal
//! [[Prototype]]-slot simulation key, a canonical array index, or a
//! given fixed name.

use core::ffi::c_void;

use crate::reflect::PROTO_SLOT_KEY;

/// `true` iff the live Str key spells exactly [`PROTO_SLOT_KEY`]
/// (the [[Prototype]]-slot simulation key hidden from own-keys
/// walks).
pub(crate) unsafe fn key_is_proto_slot(key: *const c_void) -> bool {
    let len = unsafe { key.cast::<u8>().add(8).cast::<u32>().read() } as usize;
    len == PROTO_SLOT_KEY.len()
        && unsafe { core::slice::from_raw_parts(key.cast::<u8>().add(16), len) } == PROTO_SLOT_KEY
}

/// Byte-equality of a live Str key against a fixed name.
pub(crate) unsafe fn key_bytes_are(key: *const c_void, name: &[u8]) -> bool {
    let len = unsafe { key.cast::<u8>().add(8).cast::<u32>().read() } as usize;
    len == name.len()
        && unsafe { core::slice::from_raw_parts(key.cast::<u8>().add(16), len) } == name
}

/// Canonical array-index test on a live Str key — the shadow-entry
/// filter's key shape check ([`crate::arr_reflect`] twin).
pub(crate) unsafe fn key_is_canonical_index(key: *const c_void) -> bool {
    let len = unsafe { key.cast::<u8>().add(8).cast::<u32>().read() } as usize;
    if len == 0 || len > 10 {
        return false;
    }
    let bytes = unsafe { core::slice::from_raw_parts(key.cast::<u8>().add(16), len) };
    if bytes == b"0" {
        return true;
    }
    if bytes[0] == b'0' || !bytes.iter().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let mut v: u64 = 0;
    for &b in bytes {
        v = v * 10 + (b - b'0') as u64;
    }
    v < u32::MAX as u64
}
