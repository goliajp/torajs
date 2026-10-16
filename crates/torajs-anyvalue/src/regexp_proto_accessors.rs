//! §22.2.6 — `RegExp.prototype`'s accessor properties: `flags`
//! (§22.2.6.4, body in [`crate::regexp_flags_getter`]), the eight
//! per-flag booleans and `source`. One table answers both the key
//! probe and the own-name list, and one dispatcher arm runs the getters.
//!
//! The eight flag getters and `source` share their first steps
//! (§22.2.6.x RegExpHasFlag / §22.2.6.13): a non-object `this` is a
//! TypeError; an object without [[OriginalFlags]] answers undefined
//! (`source`: "(?:)") when it IS %RegExp.prototype% and is a TypeError
//! otherwise; a RegExp answers its own flag bit / pattern text.

use core::ffi::c_void;

use torajs_rc::Tag;
use torajs_rc::any_method_regexp as rx;

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, box_bool};

unsafe extern "C" {
    fn __torajs_regex_get_source(re: *const c_void) -> *mut c_void;
    fn __torajs_regex_has_flag(re: *const c_void, bit: i64) -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// `RegExp.prototype`'s accessors as (getter id, name).
pub(crate) const REGEXP_ACCESSORS: &[(i64, &str)] = &[
    (torajs_rc::ANY_METHOD_GET_FLAGS, "flags"),
    (rx::ANY_METHOD_GET_HAS_INDICES, "hasIndices"),
    (rx::ANY_METHOD_GET_GLOBAL, "global"),
    (rx::ANY_METHOD_GET_IGNORE_CASE, "ignoreCase"),
    (rx::ANY_METHOD_GET_MULTILINE, "multiline"),
    (rx::ANY_METHOD_GET_DOT_ALL, "dotAll"),
    (rx::ANY_METHOD_GET_UNICODE, "unicode"),
    (rx::ANY_METHOD_GET_UNICODE_SETS, "unicodeSets"),
    (rx::ANY_METHOD_GET_STICKY, "sticky"),
    (rx::ANY_METHOD_GET_SOURCE, "source"),
];

/// The getter id `key` names on `RegExp.prototype`, if any.
///
/// # Safety
/// `key` is a live Str cell.
pub(crate) unsafe fn regexp_accessor_mid(key: *const c_void) -> Option<i64> {
    REGEXP_ACCESSORS
        .iter()
        .find(|(_, name)| unsafe { crate::prop_has::key_is(key, name.as_bytes()) })
        .map(|(mid, _)| *mid)
}

/// §22.2.6.13 step 3.a's "(?:)", minted once — the getter must not
/// allocate an immortal cell per read.
static EMPTY_SOURCE_CELL: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

fn empty_source_cell() -> *mut c_void {
    use core::sync::atomic::Ordering;
    let mut p = EMPTY_SOURCE_CELL.load(Ordering::Relaxed);
    if p == 0 {
        p = crate::method_value::mint_immortal_str(b"(?:)") as u64;
        EMPTY_SOURCE_CELL.store(p, Ordering::Relaxed);
    }
    p as *mut c_void
}

/// The `torajs-regex` `RE_FLAG_*` bit a per-flag getter reads.
fn flag_bit(mid: i64) -> Option<i64> {
    Some(match mid {
        rx::ANY_METHOD_GET_HAS_INDICES => 0x80,
        rx::ANY_METHOD_GET_GLOBAL => 0x02,
        rx::ANY_METHOD_GET_IGNORE_CASE => 0x01,
        rx::ANY_METHOD_GET_MULTILINE => 0x04,
        rx::ANY_METHOD_GET_DOT_ALL => 0x08,
        rx::ANY_METHOD_GET_UNICODE => 0x10,
        rx::ANY_METHOD_GET_UNICODE_SETS => 0x40,
        rx::ANY_METHOD_GET_STICKY => 0x20,
        _ => return None,
    })
}

/// A reified `RegExp.prototype` getter invoked on `recv` — `None` when
/// `mid` is not one of [`REGEXP_ACCESSORS`]. The answer is owned.
///
/// # Safety
/// `recv` is a valid AnyValue.
pub unsafe fn regexp_accessor_call(recv: AnyValue, mid: i64) -> Option<AnyValue> {
    if mid == torajs_rc::ANY_METHOD_GET_FLAGS {
        return Some(unsafe { crate::regexp_flags_getter::regexp_flags_getter(recv) });
    }
    let bit = flag_bit(mid);
    if bit.is_none() && mid != rx::ANY_METHOD_GET_SOURCE {
        return None;
    }
    if !crate::to_primitive::is_object_value(recv) {
        unsafe {
            __torajs_throw_type_error(c"RegExp.prototype getter called on a non-object".as_ptr())
        };
        return Some(VALUE_UNDEFINED);
    }
    let ptr = as_void_ptr(recv);
    // SAFETY: an object value is a live cell with a header.
    if unsafe { ptr.cast::<u8>().add(4).cast::<u16>().read() } != Tag::RegExp as u16 {
        let proto = unsafe { torajs_rc::builtin_proto::__torajs_get_builtin_prototype(7) };
        if !core::ptr::eq(proto.cast_const(), ptr.cast_const()) {
            unsafe {
                __torajs_throw_type_error(
                    c"RegExp.prototype getter called on a non-RegExp".as_ptr(),
                )
            };
            return Some(VALUE_UNDEFINED);
        }
        return Some(match bit {
            Some(_) => VALUE_UNDEFINED,
            None => unsafe { crate::nanbox_encode::__torajs_anyv_box_pointer(empty_source_cell()) },
        });
    }
    Some(match bit {
        Some(b) => box_bool(unsafe { __torajs_regex_has_flag(ptr, b) } != 0),
        None => unsafe {
            crate::nanbox_encode::__torajs_anyv_box_pointer(__torajs_regex_get_source(ptr))
        },
    })
}
