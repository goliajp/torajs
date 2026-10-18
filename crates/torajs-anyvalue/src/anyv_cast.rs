//! Checked casts from an `any` value into an annotated typed slot
//! (RFC 20260922-any-to-typed-checked-cast, slice S2).
//!
//! A slot the program annotated (`let n: number`, a parameter, a
//! declared return) holds exactly its type — that is what lets the
//! typed lanes skip every runtime check. An `any` value crossing into
//! one used to be converted silently (ToNumber / ToString /
//! ToBoolean), which the spec never does: the value would simply pass
//! through, and a program whose annotation lied read a different value
//! than every other engine. These kernels let a value that already IS
//! of the slot's type through unchanged, and record a TypeError for
//! anything else — loud, never a silently different value.
//!
//! `site` is a compile-time Str literal (the lowering's interned
//! string block) naming the slot, with one `{}` where the runtime
//! `typeof` class of the rejected value goes:
//! `cannot assign {} to \`n: number\``.

use core::ffi::{c_char, c_void};

mod as_is;
mod members;
mod obj;
mod plan;

pub use members::cast_obj_members;
pub use obj::cast_obj;
pub use plan::{Plain, Walk};

use crate::STR_HDR_SIZE;
use crate::member_set::STR_LEN_OFF;

use torajs_rc::Tag;

use crate::nanbox::{
    AnyValue, as_bool, as_double, as_int32, as_void_ptr, is_bool, is_cell, is_double, is_int32,
    is_null, is_short_str, is_undefined,
};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const c_char);
    fn __torajs_rc_inc(p: *mut c_void);
    /// The Str slot's undefined sentinel — what a `string | undefined`
    /// slot holds when the union's other member arrives.
    fn __torajs_str_undef() -> *mut u8;
}

/// The `typeof` answer for `v`, as the word the message prints.
///
/// # Safety
/// A cell payload of `v` is a live heap cell.
unsafe fn typeof_class(v: AnyValue) -> &'static [u8] {
    if is_undefined(v) {
        return b"undefined";
    }
    if is_null(v) {
        return b"null";
    }
    if is_bool(v) {
        return b"boolean";
    }
    if is_int32(v) || is_double(v) {
        return b"number";
    }
    if is_short_str(v) {
        return b"string";
    }
    if !is_cell(v) {
        return b"value";
    }
    match unsafe { cell_tag(v) } {
        t if t == Tag::Str as u16 => b"string",
        t if t == Tag::Symbol as u16 => b"symbol",
        t if t == Tag::BigInt as u16 => b"bigint",
        t if t == Tag::Closure as u16 => b"function",
        t if t == Tag::Undefined as u16 => b"undefined",
        _ => b"object",
    }
}

/// The heap header's type tag of a cell value.
///
/// # Safety
/// `v` boxes a live heap cell.
unsafe fn cell_tag(v: AnyValue) -> u16 {
    unsafe { (as_void_ptr(v) as *const u8).add(4).cast::<u16>().read() }
}

/// `site` with its first `{}` replaced by `class`, NUL-terminated in
/// `buf` (truncated to fit).
fn format_msg<'a>(site: &[u8], class: &[u8], buf: &'a mut [u8; 256]) -> &'a [u8] {
    let mut n = 0;
    let mut put = |bytes: &[u8], n: &mut usize| {
        for &b in bytes {
            if *n + 1 >= buf.len() {
                return;
            }
            buf[*n] = b;
            *n += 1;
        }
    };
    match site.windows(2).position(|w| w == b"{}") {
        Some(at) => {
            put(&site[..at], &mut n);
            put(class, &mut n);
            put(&site[at + 2..], &mut n);
        }
        None => put(site, &mut n),
    }
    buf[n] = 0;
    &buf[..=n]
}

/// Record the TypeError for a value that is not of the slot's type.
///
/// # Safety
/// `site` is a live Str block with a one-byte-per-unit payload; a cell
/// payload of `v` is live.
pub(crate) unsafe fn reject(v: AnyValue, site: *const u8) {
    let len = unsafe { site.add(STR_LEN_OFF).cast::<u32>().read_unaligned() } as usize;
    let site = unsafe { core::slice::from_raw_parts(site.add(STR_HDR_SIZE), len) };
    let mut buf = [0u8; 256];
    let msg = format_msg(site, unsafe { typeof_class(v) }, &mut buf);
    unsafe { __torajs_throw_type_error(msg.as_ptr() as *const c_char) };
}

/// Cast into a `number` slot. A non-number records a TypeError and
/// answers NaN (the caller's throw check unwinds before it is used).
///
/// # Safety
/// `site` is a live Str literal block; a cell payload of `v` is live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_cast_number(v: AnyValue, site: *const u8) -> f64 {
    if is_int32(v) {
        return as_int32(v) as f64;
    }
    if is_double(v) {
        return as_double(v);
    }
    unsafe { reject(v, site) };
    f64::NAN
}

/// Cast into a `boolean` slot.
///
/// # Safety
/// As [`__torajs_anyv_cast_number`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_cast_bool(v: AnyValue, site: *const u8) -> bool {
    if is_bool(v) {
        return as_bool(v);
    }
    unsafe { reject(v, site) };
    false
}

/// Cast into a `string` slot: an owned Str (a short string
/// materializes, a heap Str takes +1). A non-string records a
/// TypeError and answers NULL.
///
/// # Safety
/// As [`__torajs_anyv_cast_number`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_cast_str(v: AnyValue, site: *const u8) -> *mut c_void {
    let is_str = is_short_str(v) || (is_cell(v) && unsafe { cell_tag(v) } == Tag::Str as u16);
    if is_str {
        // ToString of a string is the string itself: no user code
        // runs, and the result is owned exactly as a cast wants.
        return unsafe { crate::nanbox_ffi::__torajs_anyv_to_str(v) };
    }
    unsafe { reject(v, site) };
    core::ptr::null_mut()
}

/// As [`__torajs_anyv_cast_str`], for a slot whose annotation admits a
/// nullish value beside its type (`string | undefined`, `string |
/// null`). Both members pass in their own spelling — the immortal
/// undefined sentinel and the NULL pointer (RFC 20260707 chunk 2) —
/// and everything that is neither a string nor nullish is rejected.
///
/// One kernel serves both unions because the parser folds `| null` and
/// `| undefined` into the same `__nullable(` marker: SSA cannot tell
/// them apart by the time the boundary picks a kernel, and neither can
/// this. Admitting both is the choice that never rejects a member the
/// program wrote; the price is that `string | undefined` also lets a
/// `null` through, which the annotation did not name.
///
/// Nullable information does not survive into SSA (`__nullable(T)` is
/// storage-identical to `T`, `ssa_lower_parse_type`), so the lowering
/// reads it off the annotation text and picks this kernel there.
///
/// NULL is also this function's rejection answer; the two are told
/// apart the way every other kernel here is, by whether a pending
/// throw was recorded.
///
/// # Safety
/// As [`__torajs_anyv_cast_number`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_cast_str_or_undef(
    v: AnyValue,
    site: *const u8,
) -> *mut c_void {
    if is_undefined(v) {
        return unsafe { __torajs_str_undef() } as *mut c_void;
    }
    if is_null(v) {
        return core::ptr::null_mut();
    }
    unsafe { __torajs_anyv_cast_str(v, site) }
}

/// Cast into a slot holding one heap family (`tag` is its
/// `torajs_rc::Tag`): the cell with +1, or NULL after recording a
/// TypeError. The slot's element / class shape is the caller's
/// question (arrays and class instances have their own casts).
///
/// # Safety
/// As [`__torajs_anyv_cast_number`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_cast_cell(
    v: AnyValue,
    tag: u16,
    site: *const u8,
) -> *mut c_void {
    if is_cell(v) && unsafe { cell_tag(v) } == tag {
        let p = as_void_ptr(v);
        unsafe { __torajs_rc_inc(p) };
        return p;
    }
    unsafe { reject(v, site) };
    core::ptr::null_mut()
}

/// Check a value written into a function slot: a function object (any
/// `Closure` cell, a bound or builtin function included) passes as it
/// is, anything else records a TypeError. The value's stake stays with
/// the caller either way.
///
/// # Safety
/// As [`__torajs_anyv_cast_number`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_check_fn(v: AnyValue, site: *const u8) {
    if is_cell(v) && unsafe { cell_tag(v) } == Tag::Closure as u16 {
        return;
    }
    unsafe { reject(v, site) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nanbox::{VALUE_NULL, VALUE_TRUE, VALUE_UNDEFINED, box_double, box_int32};

    #[test]
    fn message_names_the_rejected_class() {
        let mut buf = [0u8; 256];
        let msg = format_msg(b"cannot assign {} to `n: number`", b"string", &mut buf);
        assert_eq!(msg, b"cannot assign string to `n: number`\0");
    }

    #[test]
    fn message_without_a_placeholder_is_the_site() {
        let mut buf = [0u8; 256];
        assert_eq!(format_msg(b"bad cast", b"null", &mut buf), b"bad cast\0");
    }

    #[test]
    fn overlong_message_truncates_and_stays_terminated() {
        let mut buf = [0u8; 256];
        let site = [b'x'; 400];
        let msg = format_msg(&site, b"number", &mut buf);
        assert_eq!(msg.len(), 256);
        assert_eq!(msg[255], 0);
    }

    #[test]
    fn primitive_classes_follow_typeof() {
        unsafe {
            assert_eq!(typeof_class(VALUE_UNDEFINED), b"undefined");
            assert_eq!(typeof_class(VALUE_NULL), b"null");
            assert_eq!(typeof_class(VALUE_TRUE), b"boolean");
            assert_eq!(typeof_class(box_int32(3)), b"number");
            assert_eq!(typeof_class(box_double(2.5)), b"number");
        }
    }

    #[test]
    fn a_number_passes_the_number_cast_unchanged() {
        unsafe {
            assert_eq!(__torajs_anyv_cast_number(box_int32(7), b"s".as_ptr()), 7.0);
            assert_eq!(
                __torajs_anyv_cast_number(box_double(2.5), b"s".as_ptr()),
                2.5
            );
            assert!(__torajs_anyv_cast_bool(VALUE_TRUE, b"s".as_ptr()));
        }
    }
}
