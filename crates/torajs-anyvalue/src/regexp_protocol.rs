//! §22.2.6.8 / .9 / .11 / .12 / .14 — `RegExp.prototype[@@match]`,
//! `[@@matchAll]`, `[@@replace]`, `[@@search]` and `[@@split]` as their
//! spec algorithms.
//!
//! Each of them talks to its receiver only through Get, Set and
//! §22.2.7.1 RegExpExec, and every one of those steps is a `?`: a user
//! `exec`, an own `flags` / `global` accessor, a `lastIndex` with a
//! `valueOf`, a species constructor — each may answer something the
//! native kernels never ask about, or throw, and the throw is the
//! method's completion. The native kernels compute the same answer
//! only while nothing of the sort can be observed, so they stay the
//! lane for a receiver that is a plain RegExp cell under an untouched
//! %RegExp.prototype% (and, for @@split, an untouched
//! `RegExp[@@species]`); every other receiver — any object at all, per
//! step 2 — runs the algorithm here. @@matchAll always does: what it
//! answers is a lazy iterator no kernel builds.

use core::ffi::c_void;

use torajs_rc::{
    ANY_METHOD_MATCH, ANY_METHOD_MATCH_ALL, ANY_METHOD_REPLACE, ANY_METHOD_SEARCH,
    ANY_METHOD_SPLIT, Tag,
};

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};

mod match_all;
mod match_search;
pub(crate) mod ops;
mod replace;
mod species;
mod split;
mod subst;

pub(crate) use match_all::{REGEXP_STRING_ITER_KIND, is_regexp_string_iter};

unsafe extern "C" {
    /// torajs-regex — 1 when the cell itself carries nothing the
    /// protocol algorithms could observe (no own bag, no subclass,
    /// a plain writable numeric `lastIndex`).
    fn __torajs_regex_protocol_plain(re: *const c_void) -> i64;
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
}

/// %RegExp.prototype%'s builtin-prototype tag.
const REGEXP_PROTO_TAG: i64 = 7;

/// The receiver's RegExp cell, or `None` for every other value.
pub(crate) fn regexp_cell(av: AnyValue) -> Option<*const c_void> {
    if !is_cell(av) {
        return None;
    }
    let p = as_void_ptr(av);
    // SAFETY: a cell box addresses a live heap header.
    let tag = unsafe { (p.cast::<u8>().add(4) as *const u16).read() };
    (tag == Tag::RegExp as u16).then_some(p as *const c_void)
}

/// Whether a program has put anything on %RegExp.prototype%. A
/// singleton nobody minted carries nothing; a minted one holds exactly
/// the protocol-method entries its mint installed until a program
/// adds (appends past them) or deletes (drops the live count) one.
fn regexp_proto_untouched() -> bool {
    use crate::method_value::symbol_lookup::{
        DYNOBJ_COUNT_OFF, DYNOBJ_ENTRIES_LEN_OFF, REGEXP_PROTO_MINT_LEN,
    };
    unsafe {
        let proto = torajs_rc::builtin_proto::__torajs_peek_builtin_prototype(REGEXP_PROTO_TAG);
        if proto.is_null() {
            return true;
        }
        let p = proto.cast::<u8>();
        let minted = REGEXP_PROTO_MINT_LEN.load(core::sync::atomic::Ordering::Relaxed);
        (p.add(4) as *const u16).read() == Tag::DynObj as u16
            && (p.add(DYNOBJ_ENTRIES_LEN_OFF) as *const u32).read() == minted
            && (p.add(DYNOBJ_COUNT_OFF) as *const u32).read() == minted
    }
}

/// Whether `key` names an own property of the RegExp cell `rx` or of
/// %RegExp.prototype% — the two levels whose entries outrank the
/// builtin accessors a RegExp's direct member reads answer from.
pub(crate) unsafe fn shadowed(rx: AnyValue, key: *const c_void) -> bool {
    unsafe {
        let bag = *(as_void_ptr(rx).cast::<u8>().add(8) as *const *const c_void);
        if !bag.is_null() && __torajs_dynobj_has(bag, key) != 0 {
            return true;
        }
        let proto = torajs_rc::builtin_proto::__torajs_peek_builtin_prototype(REGEXP_PROTO_TAG);
        !proto.is_null() && crate::method_support_proto::proto_own_has(proto, key)
    }
}

/// Whether `RegExp[@@species]` is still the installed accessor.
fn regexp_species_untouched() -> bool {
    let ctor = crate::method_value::ctor::ctor_cell_peek(REGEXP_PROTO_TAG);
    ctor.is_null()
        || !unsafe {
            crate::closure_proto::ctor_species_accessor::species_replaced(
                REGEXP_PROTO_TAG,
                ctor.cast(),
            )
        }
}

/// Whether the native kernel computes exactly what the spec
/// algorithm for `mid` would on `re`, with nothing observable lost.
/// @@split also copies the receiver into its splitter BEFORE it
/// coerces `limit`, so a `limit` whose `valueOf` recompiles the
/// receiver is observable too.
fn native_lane_exact(re: *const c_void, mid: i64, limit: AnyValue) -> bool {
    (unsafe { __torajs_regex_protocol_plain(re) } != 0)
        && regexp_proto_untouched()
        && (mid != ANY_METHOD_SPLIT
            || (regexp_species_untouched()
                && !unsafe { crate::iter_zip_shared::av_is_object(limit) }))
}

/// Whether the protocol method `mid` on `this` must run its spec
/// algorithm rather than the native lane — every receiver except a
/// plain RegExp cell under an untouched prototype (and @@matchAll
/// always: it answers an iterator, which no kernel builds). Reads
/// nothing the regex engine owns, so its callers outside the RegExp
/// arm do not link the engine.
///
/// # Safety
/// `argv` addresses `argc` live slots.
pub(crate) unsafe fn spec_needed(this: AnyValue, mid: i64, argv: *const u64, argc: i64) -> bool {
    let limit = if argc > 1 {
        unsafe { *argv.add(1) }
    } else {
        VALUE_UNDEFINED
    };
    match regexp_cell(this) {
        Some(re) => mid == ANY_METHOD_MATCH_ALL || !native_lane_exact(re, mid, limit),
        None => true,
    }
}

/// The spec algorithm of the protocol method `mid` over the Object
/// `this`: its completion, undefined with a pending throw on an
/// abrupt one. Reached only from the RegExp dispatch arm, so a
/// program the dispatch judgment proves regex-free strips it.
///
/// # Safety
/// `this` is a live Object value; `argv` addresses `argc` live slots.
pub(crate) unsafe fn spec_run(this: AnyValue, mid: i64, argv: *const u64, argc: i64) -> AnyValue {
    let arg = |i: i64| -> AnyValue {
        if i < argc {
            unsafe { *argv.add(i as usize) }
        } else {
            VALUE_UNDEFINED
        }
    };
    unsafe {
        match mid {
            m if m == ANY_METHOD_MATCH => match_search::symbol_match(this, arg(0)),
            m if m == ANY_METHOD_SEARCH => match_search::symbol_search(this, arg(0)),
            m if m == ANY_METHOD_REPLACE => replace::symbol_replace(this, arg(0), arg(1)),
            m if m == ANY_METHOD_SPLIT => split::symbol_split(this, arg(0), arg(1)),
            m if m == ANY_METHOD_MATCH_ALL => match_all::symbol_match_all(this, arg(0)),
            _ => crate::method_call::method_no_such(),
        }
    }
}

/// The RegExp dispatch arm's answer for a receiver that is not a
/// RegExp cell: a protocol method called with an ordinary object as
/// `this`, or one step of a RegExp String Iterator (the value it
/// yields, or [`ITER_STEP_DONE`] once it is done or threw).
///
/// # Safety
/// `recv` is a live Object value; `argv` addresses `argc` live slots.
pub(crate) unsafe fn foreign_receiver(
    recv: AnyValue,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        if is_regexp_string_iter(recv) && mid == torajs_rc::ANY_METHOD_NEXT {
            return match_all::regexp_string_step(recv);
        }
        if crate::method_value::regexp_protocol_mid(mid) {
            return spec_run(recv, mid, argv, argc);
        }
        crate::method_call::method_no_such()
    }
}

/// A RegExp String Iterator step's "no value" answer through the
/// dispatch arm — the reserved no-such bit pattern, which no yielded
/// value can carry.
pub(crate) const ITER_STEP_DONE: AnyValue = crate::method_call::ANY_METHOD_NO_SUCH;

/// One RegExp String Iterator step from the helper cell's own step
/// (for-of / `next()`): routed through the RegExp dispatch arm so the
/// iterator family does not link the regex engine. 1 with `*out` the
/// owned value, 0 when done (a pending throw tells abrupt apart).
///
/// # Safety
/// `ptr` is a live RegExp String Iterator cell; `out` is writable.
pub(crate) unsafe fn regexp_string_iter_step(ptr: *mut c_void, out: *mut AnyValue) -> i64 {
    unsafe {
        let recv = crate::nanbox_encode::__torajs_anyv_box_pointer(ptr);
        let v = crate::dispatch_seam::__torajs_dispatch_regexp_arm(
            recv,
            torajs_rc::ANY_METHOD_NEXT,
            core::ptr::null(),
            core::ptr::null_mut(),
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

/// A protocol method call whose receiver needs the spec algorithm,
/// from outside the RegExp arm — through the arm's seam, which the
/// dispatch judgment stubs in a program that never enters it.
///
/// # Safety
/// `this` is a live Object value; `argv` addresses `argc` live slots.
pub(crate) unsafe fn call_through_arm(
    this: AnyValue,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        crate::dispatch_seam::__torajs_dispatch_regexp_arm(
            this,
            mid,
            core::ptr::null(),
            core::ptr::null_mut(),
            argv,
            argc,
        )
    }
}
