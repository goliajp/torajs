//! The Promise settle statics' dispatch arms — split from
//! `ns_static_ctor.rs` at the 500-line cap (rotation 449, the
//! receiver-channel knife pushed it over).

use core::ffi::c_void;

use crate::nanbox::{VALUE_UNDEFINED, as_void_ptr, box_void_ptr, is_cell};

use super::ns_static::arg_at;
use super::ns_static_table::{__torajs_throw_type_error, PromiseComb};

unsafe extern "C" {
    /// torajs-promise — §27.2.4.7 step 2 / §27.2.4.6 step 3 through
    /// the ANY lane. Both adopt one ref on the box.
    fn __torajs_promise_resolve_any(bits: i64) -> *mut c_void;
    fn __torajs_promise_reject_any(bits: i64) -> *mut c_void;
    /// torajs-promise — the iterator-interleaved combinator entries
    /// (the same kernels the direct `Promise.all(x)` lowering bakes
    /// for a non-sync-elem argument). The box is BORROWED: the
    /// kernel shares what it keeps; the answer is an owned Promise
    /// cell.
    fn __torajs_promise_all_dyn(v: u64) -> *mut c_void;
    fn __torajs_promise_allsettled_dyn(v: u64) -> *mut c_void;
    fn __torajs_promise_any_dyn(v: u64) -> *mut c_void;
    fn __torajs_promise_race_dyn(v: u64) -> *mut c_void;
}

/// §27.2.4.{1,3,5,6,8} combinator / withResolvers reified cells —
/// every algorithm's step 1/2 reads |this| as the species
/// constructor, and these cells stay receiver-less: any detached
/// call raises the catchable TypeError bun/JSC does for
/// `const a = Promise.all; a([])`. The cells still serve the
/// reflection surface (name / length / print / gOPD identity).
pub(super) unsafe fn promise_settle() -> u64 {
    unsafe { __torajs_throw_type_error(c"|this| is not an object".as_ptr()) };
    VALUE_UNDEFINED
}

/// The settle statics' |this| gate — the interned builtin Promise
/// constructor cell by pointer identity, or a CLASS OBJECT whose
/// [[Prototype]] chain reaches it (`class CP extends Promise` —
/// §15.7.14 wires the class object to the ctor cell, so `CP.resolve`
/// / `CP.all` inherit the statics and run the builtin settle). The
/// walk admits only `FLAG_DYNOBJ_CLASS_CTOR` hops: a non-constructor
/// object on the chain (`Object.create(Promise)`) fails
/// IsConstructor inside NewPromiseCapability per spec, so it keeps
/// the loud TypeError. A subclass |this| answers a PLAIN Promise for
/// now — minting C instances needs NewPromiseCapability(C), the
/// recorded follow-up.
fn this_reaches_promise_ctor(this: u64) -> bool {
    let promise_ctor = crate::method_value::ctor::ctor_cell_peek(10);
    if promise_ctor.is_null() || !is_cell(this) {
        return false;
    }
    let mut cur = this;
    loop {
        let p = as_void_ptr(cur);
        if p == promise_ctor.cast() {
            return true;
        }
        // SAFETY: is_cell guarantees a live heap header; the chain
        // links were stored by ordinary_set_prototype_of (cycle-free
        // per its §10.1.9.1 walk).
        let (tag, flags) = unsafe {
            (
                (p.cast::<u8>().add(4) as *const u16).read(),
                (p.cast::<u8>().add(6) as *const u16).read(),
            )
        };
        if tag != torajs_rc::Tag::DynObj as u16 || flags & torajs_rc::FLAG_DYNOBJ_CLASS_CTOR == 0 {
            return false;
        }
        match unsafe { crate::member_get_own::user_proto_cell(p) } {
            Some(next) if is_cell(next) => cur = next,
            _ => return false,
        }
    }
}

/// §27.2.4.{1,3,5,6} combinators with a receiver channel — the same
/// gate as [`promise_settle_fn`]: |this| (argv[0], prepended by every
/// honoring caller) must reach the interned builtin Promise
/// constructor cell ([`this_reaches_promise_ctor`]); the iterable in
/// argv[1] then rides the iterator-interleaved dyn kernel —
/// `Promise.all.call(Promise, xs)` and a bound/detached spelling with
/// the right receiver answer what the direct spelling does,
/// patched-`resolve` consult included. Any other thisValue keeps the
/// step-1/2 TypeError — loud beats a wrong-identity promise. argv
/// slots are borrowed; the kernel shares what it keeps and answers
/// an owned Promise cell.
pub(super) unsafe fn promise_combinator_fn(kind: PromiseComb, argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let this = arg_at(argv, argc, 0);
        if !this_reaches_promise_ctor(this) {
            return promise_settle();
        }
        let v = arg_at(argv, argc, 1);
        let p = match kind {
            PromiseComb::All => __torajs_promise_all_dyn(v),
            PromiseComb::AllSettled => __torajs_promise_allsettled_dyn(v),
            PromiseComb::Any => __torajs_promise_any_dyn(v),
            PromiseComb::Race => __torajs_promise_race_dyn(v),
        };
        box_void_ptr(p)
    }
}

/// §27.2.4.7/.6 Promise.resolve / reject with a receiver channel
/// (rotation 449 — the `this_aware_id` recv-first shape, RFC
/// 20260720 刀 6's recorded follow-up face): argv[0] is the thisArg
/// every honoring caller prepended (`.call` / `.apply` / bind /
/// HOF), undefined on a bare detached call. A |this| reaching the
/// interned builtin Promise constructor cell
/// ([`this_reaches_promise_ctor`]) runs the real settle through the
/// any-lane kernels — `r.apply(Promise, [v])`, `r.call(Promise, v)`
/// and the inherited `CP.resolve(v)` answer what the direct spelling
/// does. Any other thisValue keeps the step-1 TypeError — loud beats
/// a wrong-identity promise.
pub(super) unsafe fn promise_settle_fn(reject: bool, argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let this = arg_at(argv, argc, 0);
        if !this_reaches_promise_ctor(this) {
            return promise_settle();
        }
        let v = arg_at(argv, argc, 1);
        // The kernels adopt one ref on the box; the argv slot is
        // borrowed, so share first.
        crate::nanbox_ffi::__torajs_anyv_rc_inc(v);
        let p = if reject {
            __torajs_promise_reject_any(v as i64)
        } else {
            __torajs_promise_resolve_any(v as i64)
        };
        box_void_ptr(p)
    }
}
