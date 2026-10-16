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

use torajs_anyvalue::nanbox::{AnyValue, VALUE_UNDEFINED};
use torajs_anyvalue::regexp_support as av;
use torajs_rc::{
    ANY_METHOD_MATCH, ANY_METHOD_MATCH_ALL, ANY_METHOD_REPLACE, ANY_METHOD_SEARCH, ANY_METHOD_SPLIT,
};

mod ctor_build;
mod match_all;
mod match_search;
mod ops;
mod replace;
mod species;
mod split;
mod subst;

/// The algorithms seam's body: one RegExp String Iterator step for a
/// `next` on one of its cells; else the spec algorithm of the protocol
/// method `mid` over the Object `recv` — its completion, undefined
/// with a pending throw on an abrupt one. The protocol seam's body in
/// `torajs-regexp-lane` judges first, so a receiver the native kernels
/// serve exactly never comes here.
///
/// # Safety
/// `recv` is a live Object value; `argv` addresses `argc` live slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regexp_protocol_run(
    recv: AnyValue,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        if av::is_regexp_string_iter(recv) && mid == torajs_rc::ANY_METHOD_NEXT {
            return match_all::regexp_string_step(recv);
        }
        spec_run(recv, mid, argv, argc)
    }
}

/// The spec algorithm of the protocol method `mid` over the Object
/// `this` — see [`__torajs_regexp_protocol_run`].
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
