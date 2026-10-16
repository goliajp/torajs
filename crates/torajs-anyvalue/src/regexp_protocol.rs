//! What the rest of this crate knows of the RegExp protocol methods
//! (§22.2.6.8 / .9 / .11 / .12 / .14): the receiver tests, the RegExp
//! String Iterator's cell kind, and the two answers the protocol seam
//! gives besides a completion. Whether a call needs the spec algorithm
//! is `torajs-regexp-lane`, the seam's body; the algorithms are
//! `torajs-regexp-protocol` behind it (see [`crate::dispatch_seam`]).

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};

unsafe extern "C" {
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
}

/// %RegExp.prototype%'s builtin-prototype tag.
const REGEXP_PROTO_TAG: i64 = torajs_rc::builtin_proto::REGEXP_PROTO_TAG as i64;

/// The RegExp String Iterator's helper kind (`iter_helper` kinds run
/// 0-8); torajs-rc's `iter_proto` mirrors it to name the prototype.
pub const REGEXP_STRING_ITER_KIND: u8 =
    torajs_rc::builtin_proto::iter_proto::ITER_HELPER_KIND_REGEXP_STRING;

/// Whether `v` is a RegExp String Iterator cell.
pub fn is_regexp_string_iter(v: AnyValue) -> bool {
    if !is_cell(v) {
        return false;
    }
    let p = as_void_ptr(v).cast::<u8>();
    // SAFETY: a cell box addresses a live heap header; the kind byte
    // is read only off a helper-tagged cell.
    unsafe {
        (p.add(4) as *const u16).read() == Tag::IterHelper as u16
            && p.add(crate::iter_helper::KIND_OFF).read() == REGEXP_STRING_ITER_KIND
    }
}

/// The receiver's RegExp cell, or `None` for every other value.
pub fn regexp_cell(av: AnyValue) -> Option<*const c_void> {
    if !is_cell(av) {
        return None;
    }
    let p = as_void_ptr(av);
    // SAFETY: a cell box addresses a live heap header.
    let tag = unsafe { (p.cast::<u8>().add(4) as *const u16).read() };
    (tag == Tag::RegExp as u16).then_some(p as *const c_void)
}

/// Whether `key` names an own property of the RegExp cell `rx` or of
/// %RegExp.prototype% — the two levels whose entries outrank the
/// builtin accessors a RegExp's direct member reads answer from.
///
/// # Safety
/// `rx` boxes a live RegExp cell; `key` is a live Str cell.
#[inline]
pub unsafe fn shadowed(rx: AnyValue, key: *const c_void) -> bool {
    unsafe {
        let bag = *(as_void_ptr(rx).cast::<u8>().add(8) as *const *const c_void);
        if !bag.is_null() && __torajs_dynobj_has(bag, key) != 0 {
            return true;
        }
        let proto = torajs_rc::builtin_proto::__torajs_peek_builtin_prototype(REGEXP_PROTO_TAG);
        !proto.is_null() && crate::method_support_proto::proto_own_has(proto, key)
    }
}

/// The protocol seam's answer when the native kernels compute exactly
/// what the spec algorithm would: the caller runs its kernel lane. The
/// reserved no-such bit pattern, which no completion value can carry.
pub const KERNEL_LANE: AnyValue = crate::method_call::ANY_METHOD_NO_SUCH;

/// A RegExp String Iterator step's "no value" answer through the
/// dispatch arm — the reserved no-such bit pattern, which no yielded
/// value can carry.
pub const ITER_STEP_DONE: AnyValue = crate::method_call::ANY_METHOD_NO_SUCH;

/// One RegExp String Iterator step from the helper cell's own step
/// (for-of / `next()`): through the protocol seam, which alone mints
/// these cells, so the iterator family links neither the regex engine
/// nor the RegExp arm. 1 with `*out` the owned value, 0 when done (a
/// pending throw tells abrupt apart).
///
/// # Safety
/// `ptr` is a live RegExp String Iterator cell; `out` is writable.
pub(crate) unsafe fn regexp_string_iter_step(ptr: *mut c_void, out: *mut AnyValue) -> i64 {
    unsafe {
        let recv = crate::nanbox_encode::__torajs_anyv_box_pointer(ptr);
        let v = crate::dispatch_seam::__torajs_regexp_protocol_call(
            recv,
            torajs_rc::ANY_METHOD_NEXT,
            core::ptr::null(),
            0,
        );
        if v == ITER_STEP_DONE {
            *out = VALUE_UNDEFINED;
            return 0;
        }
        *out = v;
        1
    }
}

/// A builtin method cell of family `fam` carrying `mid`, invoked with
/// a receiver it was not read from (`.call`, an own or inherited
/// property holding the cell, a symbol-keyed call): `None` unless it is
/// one of %RegExp.prototype%'s @@match / @@matchAll / @@replace /
/// @@search / @@split. Those share their ids with the String methods
/// they delegate to, so the receiver's own dispatch arm would answer
/// the String method; they go to the RegExp arm instead, whatever the
/// receiver, and the call stays behind that seam, which a program
/// with no RegExp value stubs: a RegExp `this` runs its own method,
/// any other value the protocol seam (step 2's TypeError for a
/// primitive, else the spec algorithm, whose RegExpExec reads that
/// object's `exec`).
///
/// # Safety
/// `this` is a live value; `argv` addresses `argc` live slots.
#[inline(never)]
pub(crate) unsafe fn foreign_this_call(
    fam: i64,
    mid: i64,
    this: AnyValue,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    if fam != REGEXP_PROTO_TAG || !crate::method_value::regexp_protocol_mid(mid) {
        return None;
    }
    Some(unsafe {
        crate::dispatch_seam::__torajs_dispatch_regexp_arm(
            this,
            mid,
            core::ptr::null(),
            core::ptr::null_mut(),
            argv,
            argc,
        )
    })
}
