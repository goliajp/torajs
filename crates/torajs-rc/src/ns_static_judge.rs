//! Per-static stub-judgment table (RFC 20260824-s2-5 Phase B, the
//! ns-static fallback narrowing) — for every namespace static a
//! program can reify as a VALUE (`const f = Math.max` bakes
//! `__torajs_ns_static_cell(id)`), answer what the compiler's
//! dispatch-family judgment must KEEP for the cell's call face.
//!
//! The judgment scan sees the constant id; the cell's kernel runs
//! in the staticlib where the scan cannot look, so this table IS
//! the model of that kernel's re-dispatch surface. Three shapes:
//!
//! - [`NsStaticJudge::Keep`]: the kernel can cross exactly these
//!   family arm seams. Two contributions compose per row: the
//!   coercion/probe face (a kernel that runs ToNumber / ToString /
//!   ToPropertyDescriptor / an own-property walk against arbitrary
//!   arguments keeps [`FAM_OBJ_WORLD`] — same truth as the
//!   compiler's coercion keep) and the MINT face (a kernel that
//!   mints a value of an exotic-coercion family keeps that family's
//!   bit, because the minted value's to-primitive / iterator faces
//!   dispatch on its tag with no construction symbol in the user
//!   `.o` to witness it). Iterable-walking kernels also keep
//!   [`FAM_ITER`] (they mint/step iterator cells) and [`FAM_STR`]
//!   (a string argument is a spec iterable and its iteration
//!   dispatches on the str tag).
//! - [`NsStaticJudge::Print`]: the console family — the cell's call
//!   IS the per-tag inspect walk, so the printer kernels stay.
//! - [`NsStaticJudge::Fallback`]: not modelled (Reflect.apply /
//!   Reflect.construct invoke arbitrary callables) — the whole
//!   judgment punts, exactly the pre-table behavior.
//!
//! Errs toward KEEPING: a missing bit is a loud stub TypeError
//! caught by the conformance gate / test262 sweep, never silent;
//! rows resolve by (ns, name) off [`crate::ns_static::NS_STATIC_TABLE`],
//! so an appended static defaults to `Fallback` until someone
//! models it here — no index-lockstep to drift.

use crate::any_method_family::{
    FAM_BIGINT, FAM_ITER, FAM_MAPSET, FAM_OBJ_WORLD, FAM_PROMISE, FAM_STR, FAM_SYMBOL,
};
use crate::ns_static::NS_STATIC_TABLE;

/// What the stub judgment must keep for one reified static — see
/// the module doc for the three shapes.
pub enum NsStaticJudge {
    Keep(u16),
    Print,
    Fallback,
}

/// Judge one `__torajs_ns_static_cell(id)` site. Out-of-table ids
/// answer `Fallback` (a lowering/table skew is unknowable).
pub fn ns_static_judge(id: i64) -> NsStaticJudge {
    use NsStaticJudge::{Fallback, Keep, Print};
    let Some(row) = usize::try_from(id)
        .ok()
        .and_then(|i| NS_STATIC_TABLE.get(i))
    else {
        return Fallback;
    };
    // an iterable-walking kernel: GetIterator + step over an
    // arbitrary argument (see the module doc for STR).
    const ITER_WALK: u16 = FAM_OBJ_WORLD | FAM_ITER | FAM_STR;
    match (row.ns, row.name) {
        // every console cell is the per-tag inspect walk.
        ("console", _) => Print,
        // pure NaN-box / header reads — no coercion, no dispatch.
        ("Math", "random") | ("Date", "now") => Keep(0),
        ("Number", "isInteger" | "isNaN" | "isFinite" | "isSafeInteger") => Keep(0),
        ("Array", "isArray") => Keep(0),
        ("Object", "is" | "isFrozen" | "isExtensible" | "isSealed") => Keep(0),
        // loud-TypeError call faces (constant message, no arm) and
        // the empty-list symbols kernel.
        (
            "Object",
            "getOwnPropertyDescriptors"
            | "create"
            | "defineProperty"
            | "defineProperties"
            | "getOwnPropertySymbols",
        ) => Keep(0),
        ("Array", "from") => Keep(0),
        ("globalThis", "eval") => Keep(0),
        // strict String gate (non-string throws, no coercion).
        ("RegExp", "escape") => Keep(0),
        ("JSON", "isRawJSON") => Keep(0),
        ("Reflect", "isExtensible") => Keep(0),
        // ToNumber / ToString / own-property-probe kernels — the
        // obj-world coercion face only.
        ("Math", _) => Keep(FAM_OBJ_WORLD),
        ("Number", "parseInt" | "parseFloat") => Keep(FAM_OBJ_WORLD),
        ("Date", "parse" | "UTC") => Keep(FAM_OBJ_WORLD),
        ("String", "fromCharCode" | "fromCodePoint" | "raw") => Keep(FAM_OBJ_WORLD),
        ("globalThis", _) => Keep(FAM_OBJ_WORLD), // isFinite/isNaN + the four URI kernels
        ("Reflect", "apply" | "construct") => Fallback, // arbitrary-callable invoke
        ("Reflect", _) => Keep(FAM_OBJ_WORLD),
        ("JSON", "rawJSON" | "parse" | "stringify") => Keep(FAM_OBJ_WORLD),
        // minting statics: the exotic family rides along.
        ("Symbol", "for") => Keep(FAM_OBJ_WORLD | FAM_SYMBOL),
        ("Symbol", "keyFor") => Keep(FAM_SYMBOL),
        ("BigInt", "asIntN" | "asUintN") => Keep(FAM_OBJ_WORLD | FAM_BIGINT),
        // iterable walkers (all of these also mint their result
        // family: iterator helpers / a fresh Map / promises).
        ("Object", "fromEntries" | "groupBy") => Keep(ITER_WALK),
        ("Iterator", _) => Keep(ITER_WALK),
        ("Map", "groupBy") => Keep(ITER_WALK | FAM_MAPSET),
        ("Promise", "all" | "allSettled" | "any" | "race") => Keep(ITER_WALK | FAM_PROMISE),
        ("Array", "fromAsync") => Keep(ITER_WALK | FAM_PROMISE),
        ("Promise", _) => Keep(FAM_OBJ_WORLD | FAM_PROMISE),
        // the remaining Object.* reflection surface probes user
        // objects (expando walks, getter invokes, ToString(P)).
        ("Object", _) => Keep(FAM_OBJ_WORLD),
        // an appended static nobody has modelled yet.
        _ => Fallback,
    }
}
