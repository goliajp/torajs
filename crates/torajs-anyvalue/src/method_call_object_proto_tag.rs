//! §20.1.3.6 step 15 — `Object.prototype.toString`'s
//! `Get(O, @@toStringTag)`.
//!
//! Steps 4-14 pick a `builtinTag` from what the receiver IS; step 15
//! then asks the object what it would rather be called, and step 16
//! keeps that answer only when it is a String. Until this module
//! existed tr stopped after the builtinTag walk, so a user object
//! carrying the well-known tag answered `[object Object]` while bun
//! answered its tag, and the four namespace singletons had to be
//! recognised by pointer identity inside that walk instead of by the
//! property §21.3.1.9 / §25.5.3 / §28.1.14 actually give them.
//!
//! The lookup is a real [[Get]] — [`symbol_key_pair`] walks the own
//! dict and then the prototype chain — so an inherited tag counts,
//! which is how `Object.create({[Symbol.toStringTag]: "P"})` answers
//! `[object P]`.

// The substrate externs below are not linkable under `cargo test`, so
// the two workers compile to stubs there and everything they need goes
// with them (`to_primitive::exotic_to_primitive`'s arrangement).
use core::ffi::c_void;

use crate::AnyValue;

#[cfg(not(test))]
use torajs_rc::Tag;

#[cfg(not(test))]
use crate::nanbox_encode::__torajs_anyv_box_pointer;
#[cfg(not(test))]
use crate::{__torajs_str_concat, __torajs_str_drop};

#[cfg(not(test))]
unsafe extern "C" {
    /// torajs-str — the idx-th §6.1.5.1 well-known symbol (owned +1).
    fn __torajs_symbol_well_known(idx: i64) -> *mut c_void;
    /// torajs-str — allocate a fresh Str from raw bytes.
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    /// torajs-rc — release a cell reference (the well-known symbol's).
    fn __torajs_rc_dec(p: *mut c_void) -> i32;
    /// torajs-throw — did the step-15 getter leave a pending throw?
    fn __torajs_throw_check() -> i64;
    /// torajs-rc — release a getter-produced cell (owned answer).
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// Index of `Symbol.toStringTag` in torajs-str's alphabetical
/// well-known table.
#[cfg(not(test))]
const WK_TO_STRING_TAG: i64 = 13;

/// `AnySlotTag::Heap` — the only slot tag a Str payload arrives under.
#[cfg(not(test))]
const SLOT_HEAP: u64 = 4;

/// The receiver's `@@toStringTag`, as a BORROWED Str cell, or `None`
/// when absent or not a String (step 16's "is not a String" arm, which
/// covers `undefined`, a number, a symbol — anything the object
/// happens to have parked there).
///
/// # Safety
/// `recv` carries a valid AnyValue bit pattern.
#[cfg(not(test))]
pub(crate) unsafe fn to_string_tag_cell(recv: AnyValue) -> Option<(*mut c_void, bool)> {
    unsafe {
        let sym = __torajs_symbol_well_known(WK_TO_STRING_TAG);
        if sym.is_null() {
            return None;
        }
        // §20.1.3.6 step 15 is a real Get, so an ACCESSOR-shaped
        // `@@toStringTag` has to run. The pair alone cannot do that
        // (it answers the sentinel — see `symbol_key_get`), and reading
        // the sentinel as "not a heap value" is exactly what made every
        // accessor form answer `[object Object]` with the getter never
        // running, exception forms included (517-01 / 516-03 u4).
        let (tag, payload, owned) = crate::member_get_symbol::symbol_key_get(recv, sym);
        __torajs_rc_dec(sym);
        // A getter that threw answers undefined with the throw pending;
        // falling through to the builtinTag walk would swallow it.
        if __torajs_throw_check() != 0 {
            return None;
        }
        if tag != SLOT_HEAP || payload == 0 {
            return None;
        }
        let cell = payload as *mut c_void;
        // HeapHeader: type_tag @ +4 (u16). Only a Str cell is a String;
        // a Substr shares that tag and reads the same through concat.
        let cell_tag = cell.cast::<u8>().add(4).cast::<u16>().read();
        if cell_tag != Tag::Str as u16 {
            // A getter's non-String answer is ours to release.
            if owned {
                __torajs_value_drop_heap(cell);
            }
            return None;
        }
        Some((cell, owned))
    }
}

#[cfg(test)]
pub(crate) unsafe fn to_string_tag_cell(_recv: AnyValue) -> Option<(*mut c_void, bool)> {
    None
}

/// `"[object " + tag + "]"` as an owned Str box, for a tag whose
/// length is not known ahead of time (the literal-badge path keeps its
/// own stack-buffer builder).
///
/// # Safety
/// `tag` is a live borrowed Str cell.
#[cfg(not(test))]
pub(crate) unsafe fn tag_badge_string(tag: *mut c_void) -> AnyValue {
    unsafe {
        let prefix = __torajs_str_alloc(b"[object ".as_ptr(), 8);
        let head = __torajs_str_concat(prefix, tag.cast::<u8>());
        __torajs_str_drop(prefix as *mut c_void);
        let suffix = __torajs_str_alloc(b"]".as_ptr(), 1);
        let full = __torajs_str_concat(head.cast::<u8>(), suffix);
        __torajs_str_drop(head);
        __torajs_str_drop(suffix as *mut c_void);
        __torajs_anyv_box_pointer(full)
    }
}

#[cfg(test)]
pub(crate) unsafe fn tag_badge_string(_tag: *mut c_void) -> AnyValue {
    crate::nanbox::VALUE_UNDEFINED
}

/// Release a getter-produced tag cell. Split by cfg like its
/// neighbours because the extern block above is `cfg(not(test))` —
/// `try_tag_badge` itself is shared by both builds.
#[cfg(not(test))]
unsafe fn release_owned_tag(cell: *mut c_void) {
    unsafe { __torajs_value_drop_heap(cell) };
}

#[cfg(test)]
unsafe fn release_owned_tag(_cell: *mut c_void) {}

/// Step 15 + 16 as one call: the finished `"[object X]"` box when the
/// receiver names itself with a String tag, `None` to fall through to
/// the builtinTag walk.
///
/// # Safety
/// `recv` carries a valid AnyValue bit pattern.
pub(crate) unsafe fn try_tag_badge(recv: AnyValue) -> Option<AnyValue> {
    // Steps 1-3 already rejected undefined / null; a primitive still
    // reaches here because ToObject's wrapper inherits from a builtin
    // prototype, and a user monkey-patch there is observable.
    let (cell, owned) = unsafe { to_string_tag_cell(recv) }?;
    let badge = unsafe { tag_badge_string(cell) };
    // The badge copies out of the cell; a getter-produced tag is ours.
    if owned {
        unsafe { release_owned_tag(cell) };
    }
    Some(badge)
}
