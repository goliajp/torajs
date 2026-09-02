//! `__torajs_key_cell_inspect_hidden` — the own keys bun's inspect
//! leaves out even though it prints every other own property,
//! enumerable or not (`JSC__JSValue__forEachProperty`,
//! `DontEnumPropertiesMode::Include`):
//!
//! - `constructor`, always — a class prototype's back-pointer, and a
//!   plain object's own `constructor: 1` goes with it;
//! - `__proto__` when it is non-enumerable — an enumerable own data
//!   property of that name (`defineProperty(o, "__proto__", …)`)
//!   still prints;
//! - the `@@toStringTag` key — its value becomes the block's name
//!   prefix (`X { … }`) instead of a row.
//!
//! Shared by the dynobj, struct-expando and array-props walkers so
//! the three faces cannot drift on which keys they hide.

use core::ffi::c_void;

use torajs_rc::Tag;
use torajs_rc::str_wtf8::StrWtf8;

use super::formatters::heap_type_tag;

unsafe extern "C" {
    /// torajs-str — the well-known Symbol singleton by registry
    /// index (13 = `Symbol.toStringTag`).
    fn __torajs_symbol_well_known(idx: i64) -> *mut c_void;
}

const WK_TO_STRING_TAG: i64 = 13;
/// `enumerable` — bit 1 of the dynobj entry's W/E/C flags (mirror of
/// torajs-dynobj's `BUCKET_FLAG_ENUMERABLE`).
const FLAG_ENUMERABLE: u64 = 1 << 1;

/// 1 when `console.log` leaves this own key out, 0 when it prints.
/// `flags` are the entry's W/E/C bits (`__torajs_dynobj_iter_flags`).
///
/// # Safety
///
/// `key` is NULL (answers 0) or a live Str / Substr / Symbol cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_key_cell_inspect_hidden(key: *const c_void, flags: u64) -> i32 {
    if key.is_null() {
        return 0;
    }
    if unsafe { heap_type_tag(key) } == Tag::Symbol as u16 {
        let tag_sym = unsafe { __torajs_symbol_well_known(WK_TO_STRING_TAG) };
        return (!tag_sym.is_null() && core::ptr::eq(key, tag_sym)) as i32;
    }
    let spelling = unsafe { StrWtf8::of(key) };
    let bytes = spelling.as_bytes();
    if bytes == b"constructor" {
        return 1;
    }
    (bytes == b"__proto__" && flags & FLAG_ENUMERABLE == 0) as i32
}
