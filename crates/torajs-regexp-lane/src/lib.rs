//! The RegExp protocol seam `__torajs_regexp_protocol_call`: whether a
//! call of `RegExp.prototype[@@match]`, `[@@matchAll]`, `[@@replace]`,
//! `[@@search]` or `[@@split]` (§22.2.6.8 / .9 / .11 / .12 / .14) can
//! run the native kernels, and the spec algorithm behind the second
//! seam `__torajs_regexp_protocol_run` (torajs-regexp-protocol) when
//! it cannot.
//!
//! The native kernels compute what the algorithm would only for a
//! plain RegExp cell (no own property, no subclass, a writable numeric
//! `lastIndex`) under an untouched %RegExp.prototype% — and, for
//! @@split, an untouched `RegExp[@@species]` and a primitive `limit`;
//! @@matchAll always runs the algorithm, which answers a lazy iterator
//! no kernel builds.
//!
//! The crate is an archive member of its own, and that is
//! load-bearing twice. The always-pulled members import only the seam,
//! so a program whose dispatch judgment stubs it neither links nor
//! walks this member, nor the algorithms member it imports. And a
//! program whose RegExps all stay plain can stub the algorithms seam
//! alone: this judgment still runs in front of it, so a receiver that
//! needs the algorithm reaches the loud stub, never the kernels'
//! answer.

#![no_std]

use core::ffi::c_void;

use torajs_anyvalue::nanbox::{AnyValue, VALUE_UNDEFINED, box_bool};
use torajs_anyvalue::regexp_support as av;
use torajs_rc::{
    ANY_METHOD_GET_FLAGS, ANY_METHOD_MATCH_ALL, ANY_METHOD_NEXT, ANY_METHOD_REPLACE_ALL,
    ANY_METHOD_SPLIT,
};

unsafe extern "C" {
    /// torajs-regex — 1 when the cell itself carries nothing the
    /// protocol algorithms could observe (no own bag, no subclass,
    /// a plain writable numeric `lastIndex`).
    fn __torajs_regex_protocol_plain(re: *const c_void) -> i64;
    /// torajs-regex — non-zero iff the cell carries the flag bit.
    fn __torajs_regex_has_flag(re: *const c_void, bit: i64) -> i64;
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-regexp-protocol — the spec algorithm of a protocol
    /// method, or a RegExp String Iterator step.
    fn __torajs_regexp_protocol_run(
        recv: AnyValue,
        mid: i64,
        argv: *const u64,
        argc: i64,
    ) -> AnyValue;
}

/// %RegExp.prototype%'s builtin-prototype tag.
const REGEXP_PROTO_TAG: i64 = torajs_rc::builtin_proto::REGEXP_PROTO_TAG as i64;

/// Whether %RegExp.prototype% still holds exactly what its mint
/// installed: no entry created, rewritten, redefined or deleted, no
/// interned method deleted, no [[Prototype]] change since. A singleton
/// nobody minted answers untouched.
fn regexp_proto_untouched() -> bool {
    torajs_rc::builtin_proto::__torajs_singleton_touched(REGEXP_PROTO_TAG) == 0
}

/// Whether `RegExp[@@species]` is still the installed accessor.
fn regexp_species_untouched() -> bool {
    let ctor = av::ctor_cell_peek(REGEXP_PROTO_TAG);
    ctor.is_null() || !unsafe { av::species_replaced(REGEXP_PROTO_TAG, ctor.cast()) }
}

/// Whether the protocol method `mid` on `this` must run its spec
/// algorithm rather than the native kernels (see the crate doc).
///
/// # Safety
/// `argv` addresses `argc` live slots.
unsafe fn spec_needed(this: AnyValue, mid: i64, argv: *const u64, argc: i64) -> bool {
    let Some(re) = av::regexp_cell(this) else {
        return true;
    };
    if mid == ANY_METHOD_MATCH_ALL
        || unsafe { __torajs_regex_protocol_plain(re) } == 0
        || !regexp_proto_untouched()
    {
        return true;
    }
    mid == ANY_METHOD_SPLIT
        && (!regexp_species_untouched() || (argc > 1 && unsafe { av::av_is_object(*argv.add(1)) }))
}

/// `g` bit mirror (torajs-regex parser).
const RE_FLAG_G: i64 = 0x02;

/// String.prototype.matchAll / replaceAll (`string_mid`) step 2.b over
/// the RegExp `re`: ? ToString(? Get(re, "flags")) must contain "g",
/// else a TypeError. A RegExp whose read cannot observe more than its
/// own flag bits answers from the bits. Nonzero when the call is
/// abandoned with a throw pending.
///
/// # Safety
/// `re` boxes a live RegExp cell.
unsafe fn step_2b(re: AnyValue, string_mid: i64) -> AnyValue {
    unsafe {
        let global = match av::regexp_cell(re) {
            Some(cell) if !spec_needed(re, ANY_METHOD_GET_FLAGS, core::ptr::null(), 0) => {
                __torajs_regex_has_flag(cell, RE_FLAG_G) != 0
            }
            _ => {
                let g =
                    __torajs_regexp_protocol_run(re, ANY_METHOD_GET_FLAGS, core::ptr::null(), 0);
                if g == VALUE_UNDEFINED {
                    return 1;
                }
                g == box_bool(true)
            }
        };
        if global {
            return 0;
        }
        __torajs_throw_type_error(if string_mid == ANY_METHOD_REPLACE_ALL {
            c"String.prototype.replaceAll called with a non-global RegExp argument".as_ptr()
        } else {
            c"String.prototype.matchAll called with a non-global RegExp argument".as_ptr()
        });
        1
    }
}

/// The protocol seam's body: one RegExp String Iterator step for a
/// `next` on one of its cells; [`av::KERNEL_LANE`] for a protocol
/// method whose receiver cannot tell the native kernels from the
/// algorithm; else the spec algorithm of the protocol method `mid`
/// over the Object `recv` — its completion, undefined with a pending
/// throw on an abrupt one.
///
/// A primitive `recv` is a protocol method's `this` handed over by a
/// station that invokes the method cell on a receiver it was not read
/// from (`RegExp.prototype[Symbol.search].call(86)`): step 2 of each
/// (§22.2.6.8 / .9 / .11 / .12 / .14) throws.
///
/// [`ANY_METHOD_GET_FLAGS`] names no method here: it asks [`step_2b`]
/// over the RegExp `recv`, with the String method's id in `argc`.
///
/// # Safety
/// `recv` is a live value; `argv` addresses `argc` live slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regexp_protocol_call(
    recv: AnyValue,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        if mid == ANY_METHOD_GET_FLAGS {
            return step_2b(recv, argc);
        }
        if !av::av_is_object(recv) {
            __torajs_throw_type_error(c"RegExp protocol method called on a non-object".as_ptr());
            return VALUE_UNDEFINED;
        }
        let step = mid == ANY_METHOD_NEXT && av::is_regexp_string_iter(recv);
        if !step && !spec_needed(recv, mid, argv, argc) {
            return av::KERNEL_LANE;
        }
        __torajs_regexp_protocol_run(recv, mid, argv, argc)
    }
}
