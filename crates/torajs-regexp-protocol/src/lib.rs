//! §22.2.6.8 / .9 / .11 / .12 / .14 — `RegExp.prototype[@@match]`,
//! `[@@matchAll]`, `[@@replace]`, `[@@search]` and `[@@split]` as their
//! spec algorithms, and §22.2.3.1 for a first-class RegExp constructor
//! value.
//!
//! Each protocol method talks to its receiver only through Get, Set
//! and §22.2.7.1 RegExpExec, and every one of those steps is a `?`: a
//! user `exec`, an own `flags` / `global` accessor, a `lastIndex` with
//! a `valueOf`, a species constructor — each may answer something the
//! native kernels never ask about, or throw, and the throw is the
//! method's completion. The native kernels compute the same answer
//! only while nothing of the sort can be observed, so they stay the
//! lane for a receiver that is a plain RegExp cell under an untouched
//! %RegExp.prototype% (and, for @@split, an untouched
//! `RegExp[@@species]`); every other receiver — any object at all, per
//! step 2 — runs the algorithm here. @@matchAll always does: what it
//! answers is a lazy iterator no kernel builds.
//!
//! The crate is its own archive member, and that is load-bearing: the
//! two seams below are the only ways into it, a program that cannot
//! enter them gets a loud-reject stub for each in its user `.o`, and
//! then this member is never pulled into the link at all — the link
//! walks every byte and relocation of each member it pulls, so code
//! that lived in the always-linked `torajs-anyvalue` member cost every
//! `tr build` its size.

use core::ffi::c_void;

use torajs_anyvalue::nanbox::{AnyValue, VALUE_UNDEFINED};
use torajs_anyvalue::regexp_support as av;
use torajs_rc::{
    ANY_METHOD_MATCH, ANY_METHOD_MATCH_ALL, ANY_METHOD_REPLACE, ANY_METHOD_SEARCH,
    ANY_METHOD_SPLIT, Tag,
};

mod ctor_build;
mod match_all;
mod match_search;
mod ops;
mod replace;
mod species;
mod split;
mod subst;

unsafe extern "C" {
    /// torajs-regex — 1 when the cell itself carries nothing the
    /// protocol algorithms could observe (no own bag, no subclass,
    /// a plain writable numeric `lastIndex`).
    fn __torajs_regex_protocol_plain(re: *const c_void) -> i64;
}

/// %RegExp.prototype%'s builtin-prototype tag.
const REGEXP_PROTO_TAG: i64 = 7;

/// Whether a program has put anything on %RegExp.prototype%. A
/// singleton nobody minted carries nothing; a minted one holds exactly
/// the protocol-method entries its mint installed until a program
/// adds (appends past them) or deletes (drops the live count) one.
fn regexp_proto_untouched() -> bool {
    unsafe {
        let proto = torajs_rc::builtin_proto::__torajs_peek_builtin_prototype(REGEXP_PROTO_TAG);
        if proto.is_null() {
            return true;
        }
        let p = proto.cast::<u8>();
        let minted = av::REGEXP_PROTO_MINT_LEN.load(core::sync::atomic::Ordering::Relaxed);
        (p.add(4) as *const u16).read() == Tag::DynObj as u16
            && (p.add(av::DYNOBJ_ENTRIES_LEN_OFF) as *const u32).read() == minted
            && (p.add(av::DYNOBJ_COUNT_OFF) as *const u32).read() == minted
    }
}

/// Whether `RegExp[@@species]` is still the installed accessor.
fn regexp_species_untouched() -> bool {
    let ctor = av::ctor_cell_peek(REGEXP_PROTO_TAG);
    ctor.is_null() || !unsafe { av::species_replaced(REGEXP_PROTO_TAG, ctor.cast()) }
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
            || (regexp_species_untouched() && !unsafe { av::av_is_object(limit) }))
}

/// Whether the protocol method `mid` on `this` must run its spec
/// algorithm rather than the native lane — every receiver except a
/// plain RegExp cell under an untouched prototype (and @@matchAll
/// always: it answers an iterator, which no kernel builds).
///
/// # Safety
/// `argv` addresses `argc` live slots.
unsafe fn spec_needed(this: AnyValue, mid: i64, argv: *const u64, argc: i64) -> bool {
    let limit = if argc > 1 {
        unsafe { *argv.add(1) }
    } else {
        VALUE_UNDEFINED
    };
    match av::regexp_cell(this) {
        Some(re) => mid == ANY_METHOD_MATCH_ALL || !native_lane_exact(re, mid, limit),
        None => true,
    }
}

/// The protocol seam's body: one RegExp String Iterator step for a
/// `next` on one of its cells; [`av::KERNEL_LANE`] for a protocol method
/// whose receiver cannot tell the native kernels from the algorithm
/// (or is no Object at all — the string symbol-dispatch gate's
/// coerce lane); else the spec algorithm of the protocol method `mid`
/// over the Object `recv` — its completion, undefined with a pending
/// throw on an abrupt one. The judgment of which lane runs sits
/// behind the seam with the algorithms, so a program whose seam is
/// stubbed links neither.
///
/// # Safety
/// `recv` is a live Object value; `argv` addresses `argc` live slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regexp_protocol_call(
    recv: AnyValue,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        if av::is_regexp_string_iter(recv) && mid == torajs_rc::ANY_METHOD_NEXT {
            return match_all::regexp_string_step(recv);
        }
        if !av::av_is_object(recv) || !spec_needed(recv, mid, argv, argc) {
            return av::KERNEL_LANE;
        }
        spec_run(recv, mid, argv, argc)
    }
}

/// The spec algorithm of the protocol method `mid` over the Object
/// `this` — see [`__torajs_regexp_protocol_call`].
unsafe fn spec_run(this: AnyValue, mid: i64, argv: *const u64, argc: i64) -> AnyValue {
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
            _ => av::method_no_such(),
        }
    }
}
