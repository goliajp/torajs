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

use torajs_anyvalue::nanbox::AnyValue;
use torajs_anyvalue::regexp_support as av;
use torajs_rc::{ANY_METHOD_MATCH_ALL, ANY_METHOD_NEXT, ANY_METHOD_SPLIT};

unsafe extern "C" {
    /// torajs-regex — 1 when the cell itself carries nothing the
    /// protocol algorithms could observe (no own bag, no subclass,
    /// a plain writable numeric `lastIndex`).
    fn __torajs_regex_protocol_plain(re: *const c_void) -> i64;
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

/// The protocol seam's body: one RegExp String Iterator step for a
/// `next` on one of its cells; [`av::KERNEL_LANE`] for a protocol
/// method whose receiver cannot tell the native kernels from the
/// algorithm (or is no Object at all); else the spec algorithm of the
/// protocol method `mid` over the Object `recv` — its completion,
/// undefined with a pending throw on an abrupt one.
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
        let step = mid == ANY_METHOD_NEXT && av::is_regexp_string_iter(recv);
        if !step && (!av::av_is_object(recv) || !spec_needed(recv, mid, argv, argc)) {
            return av::KERNEL_LANE;
        }
        __torajs_regexp_protocol_run(recv, mid, argv, argc)
    }
}
