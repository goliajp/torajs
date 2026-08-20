//! The Promise settle statics' dispatch arms — split from
//! `ns_static_ctor.rs` at the 500-line cap (rotation 449, the
//! receiver-channel knife pushed it over).

use core::ffi::c_void;

use crate::nanbox::{VALUE_UNDEFINED, as_void_ptr, box_void_ptr, is_cell};

use super::ns_static::arg_at;
use super::ns_static_table::{__torajs_throw_type_error, PromiseComb};

unsafe extern "C" {
    /// torajs-throw — pending-throw pair take (tag peek first, then
    /// value + clear; the `: any`-typed catch order).
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_take() -> i64;
    fn __torajs_throw_take_tag() -> i64;
    /// torajs-promise — fresh pending cell (the builtin trio mint).
    fn __torajs_promise_alloc_pending() -> *mut c_void;
    /// torajs-meta — a subclassed cell's class tag (-1 = none) and
    /// the registered class object for a tag (0 = unregistered);
    /// together they answer the §27.2.4.7 step-2 constructor
    /// comparison without a property read.
    fn __torajs_subclass_class_tag(cell: *const c_void) -> i64;
    fn __torajs_class_cell_raw(tag: i64) -> u64;
    /// torajs-dynobj — the withResolvers result-object alloc.
    fn __torajs_dynobj_alloc() -> *mut c_void;
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
    /// torajs-promise — the heir combinator kernel (§27.2.4.1 steps
    /// 4-8 with per-element `Call(promiseResolve, C, «v»)`); mode
    /// follows the PromiseComb order. All three boxes are borrowed;
    /// the answer is an owned Promise cell.
    fn __torajs_promise_combinator_dyn_c(
        mode: i64,
        v: u64,
        ctor: u64,
        resolve_f: u64,
    ) -> *mut c_void;
    /// torajs-str — the GetPromiseResolve(C) key cell.
    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-promise — the await-dictionary keyed kernels (object
    /// of promises in, null-proto result object out through a
    /// fulfilled promise; borrowed box, owned Promise answer).
    fn __torajs_promise_all_keyed_dyn(v: u64) -> *mut c_void;
    fn __torajs_promise_allsettled_keyed_dyn(v: u64) -> *mut c_void;
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
        let promise_ctor = crate::method_value::ctor::ctor_cell_peek(10);
        let is_builtin =
            !promise_ctor.is_null() && is_cell(this) && as_void_ptr(this) == promise_ctor.cast();
        let v = arg_at(argv, argc, 1);
        if !is_builtin {
            if this_reaches_promise_ctor(this) {
                return combinator_via_capability(kind, this, v);
            }
            // RFC 20260820-combinator-any-constructor — |this| is any
            // OTHER constructor: the spec's step 1 asks only
            // IsConstructor, and everything after it reaches C through
            // Call / Invoke. The heir shortcut above cannot serve it
            // (it hands the element walk to the builtin fan-in and
            // resolves the capability with the resulting promise, so a
            // user resolve function sees a promise where the algorithm
            // hands it the combined value, and a bare thenable element
            // never has its own `then` invoked). A |this| that is not a
            // constructor at all keeps the step-1 TypeError.
            let spec = match kind {
                PromiseComb::Race => crate::combinator_spec::SpecComb::Race,
                PromiseComb::All => crate::combinator_spec::SpecComb::All,
                PromiseComb::AllSettled => crate::combinator_spec::SpecComb::AllSettled,
                PromiseComb::Any => crate::combinator_spec::SpecComb::Any,
            };
            if crate::construct::is_constructor(this) {
                return crate::combinator_spec::run(spec, this, v);
            }
            return promise_settle();
        }
        let p = match kind {
            PromiseComb::All => __torajs_promise_all_dyn(v),
            PromiseComb::AllSettled => __torajs_promise_allsettled_dyn(v),
            PromiseComb::Any => __torajs_promise_any_dyn(v),
            PromiseComb::Race => __torajs_promise_race_dyn(v),
        };
        box_void_ptr(p)
    }
}

/// await-dictionary proposal Promise.allKeyed / allSettledKeyed with
/// a receiver channel — the same gate as the §27.2.4 combinators:
/// |this| (argv[0]) must reach the interned builtin Promise
/// constructor cell, and the object of promises in argv[1] rides the
/// keyed dyn kernel. A builtin-heir class object mints
/// resultCapability = NewPromiseCapability(C) and resolves the
/// kernel's plain answer into it, so the caller sees a C instance
/// (the element walk stays on the builtin kernel — per-element
/// GetPromiseResolve(C) for the keyed pair is recorded residue).
/// Any other thisValue keeps the step-1 IsConstructor TypeError.
pub(super) unsafe fn promise_keyed_fn(settled: bool, argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let this = arg_at(argv, argc, 0);
        let promise_ctor = crate::method_value::ctor::ctor_cell_peek(10);
        let is_builtin =
            !promise_ctor.is_null() && is_cell(this) && as_void_ptr(this) == promise_ctor.cast();
        if !is_builtin && !this_reaches_promise_ctor(this) {
            return promise_settle();
        }
        let v = arg_at(argv, argc, 1);
        let p = if settled {
            __torajs_promise_allsettled_keyed_dyn(v)
        } else {
            __torajs_promise_all_keyed_dyn(v)
        };
        if !is_builtin {
            let Some((promise, resolve_f, reject_f)) =
                crate::promise_capability::new_promise_capability(this)
            else {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(box_void_ptr(p));
                return VALUE_UNDEFINED;
            };
            let inner = box_void_ptr(p);
            let one = [inner];
            let out =
                crate::method_call_closure_dispatch::__torajs_any_call(resolve_f, one.as_ptr(), 1);
            crate::nanbox_ffi::__torajs_anyv_rc_dec(out);
            crate::nanbox_ffi::__torajs_anyv_rc_dec(inner);
            crate::nanbox_ffi::__torajs_anyv_rc_dec(resolve_f);
            crate::nanbox_ffi::__torajs_anyv_rc_dec(reject_f);
            return promise;
        }
        box_void_ptr(p)
    }
}

/// §27.2.4.1.1 GetPromiseResolve(C) — Get(C, "resolve") through the
/// any-lane member read (a patched slot answers the patch, an
/// unpatched heir answers the inherited recv-first static cell),
/// then the step-2 IsCallable gate. Answers an OWNED box (the pair
/// walk is borrow-shaped, and a patch body could `delete C.resolve`
/// mid-run — the extra stake keeps the callee alive across the
/// element loop), or 0 with the throw pending (a getter's abrupt or
/// the TypeError).
unsafe fn get_promise_resolve(c: u64) -> u64 {
    unsafe {
        let key = __torajs_str_alloc(b"resolve".as_ptr(), 7);
        let tag = crate::member_get::__torajs_any_member_get_tag(c, key.cast());
        let value = crate::member_get_value::__torajs_any_member_get_value(c, key.cast());
        __torajs_str_drop(key.cast());
        if __torajs_throw_check() != 0 {
            return 0;
        }
        let f = crate::nanbox_encode::__torajs_anyv_box_from_pair(tag as i64, value as i64);
        if !crate::promise_capability::is_callable(f) {
            __torajs_throw_type_error(c"Promise resolve function is not callable".as_ptr());
            return 0;
        }
        crate::nanbox_ffi::__torajs_anyv_rc_inc(f);
        f
    }
}

/// §27.2.4.1 steps 2-8 on a builtin-heir C, in spec order: mint
/// resultCapability = NewPromiseCapability(C) first (the C ctor
/// chain runs, observably), then GetPromiseResolve(C) — whose
/// abrupt settles the capability per step 3's
/// IfAbruptRejectPromise — then hand the iterable to the heir
/// kernel, which runs `Call(promiseResolve, C, «v»)` per element.
/// The kernel's plain result promise is resolved INTO the
/// capability, whose resolver adopts it (§27.2.1.3.2), so the C
/// instance settles when the combinator does.
unsafe fn combinator_via_capability(kind: PromiseComb, c: u64, v: u64) -> u64 {
    unsafe {
        let Some((promise, resolve_f, reject_f)) =
            crate::promise_capability::new_promise_capability(c)
        else {
            return VALUE_UNDEFINED;
        };
        let pr = get_promise_resolve(c);
        let settled = if pr == 0 {
            // IfAbruptRejectPromise — the pending throw becomes the
            // capability's rejection reason.
            let tag = __torajs_throw_take_tag();
            let value = __torajs_throw_take();
            crate::nanbox_encode::__torajs_anyv_box_from_pair(tag, value)
        } else {
            let mode = match kind {
                PromiseComb::All => 0,
                PromiseComb::AllSettled => 1,
                PromiseComb::Any => 2,
                PromiseComb::Race => 3,
            };
            let p = __torajs_promise_combinator_dyn_c(mode, v, c, pr);
            crate::nanbox_ffi::__torajs_anyv_rc_dec(pr);
            box_void_ptr(p)
        };
        let f = if pr == 0 { reject_f } else { resolve_f };
        let one = [settled];
        let out = crate::method_call_closure_dispatch::__torajs_any_call(f, one.as_ptr(), 1);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(out);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(settled);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(resolve_f);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(reject_f);
        promise
    }
}

/// §27.2.4.7/.6 Promise.resolve / reject with a receiver channel
/// (rotation 449 — the `this_aware_id` recv-first shape, RFC
/// 20260720 刀 6's recorded follow-up face): argv[0] is the thisArg
/// every honoring caller prepended (`.call` / `.apply` / bind /
/// HOF), undefined on a bare detached call. |this| = the interned
/// builtin Promise constructor cell runs the real settle through
/// the any-lane kernels; a builtin-heir CLASS OBJECT (`CP.resolve`
/// on `class CP extends Promise`) takes the §27.2.4.7/.6 custom-C
/// path — NewPromiseCapability(C), Call(cap.resolve/reject, v),
/// answer cap.promise — so the answer IS a C instance and the
/// subclass ctor chain observably runs. Any other thisValue keeps
/// the step-1 TypeError. Recorded boundary: PromiseResolve's step-2
/// identity fast path (`C.resolve(x) === x` when x.constructor is
/// C) is not taken — a C-instance argument mints a fresh C promise
/// that adopts it.
pub(super) unsafe fn promise_settle_fn(reject: bool, argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let this = arg_at(argv, argc, 0);
        let promise_ctor = crate::method_value::ctor::ctor_cell_peek(10);
        let is_builtin =
            !promise_ctor.is_null() && is_cell(this) && as_void_ptr(this) == promise_ctor.cast();
        if !is_builtin {
            if this_reaches_promise_ctor(this) {
                return settle_via_capability(this, reject, arg_at(argv, argc, 1));
            }
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

/// The custom-species settle: NewPromiseCapability(C) mints the C
/// instance through the runtime construct channel, then
/// Call(cap.resolve / cap.reject, undefined, «v») settles it. The
/// capability's function boxes are released here; the promise
/// transfers to the caller (undefined with the pending throw when
/// the ctor chain or the capability check raised).
unsafe fn settle_via_capability(c: u64, reject: bool, v: u64) -> u64 {
    unsafe {
        // §27.2.4.7 step 2 (PromiseResolve step 1-2) — a resolve
        // whose argument is already a C instance answers it by
        // identity (`CP.resolve(cpInst) === cpInst`). The subclass
        // registry stands in for the `constructor` read: the cell's
        // class tag resolving to THIS class object is the unpatched
        // SameValue(x.constructor, C); a patched `constructor` slip
        // past this into a fresh C promise is the recorded residue.
        if !reject && is_cell(v) {
            let p = as_void_ptr(v);
            // Universal heap header `type_tag` — a Promise cell.
            if (p.cast::<u8>().add(4) as *const u16).read() == 8 {
                let ctag = __torajs_subclass_class_tag(p);
                if ctag >= 0 && __torajs_class_cell_raw(ctag) == c {
                    crate::nanbox_ffi::__torajs_anyv_rc_inc(v);
                    return v;
                }
            }
        }
        let Some((promise, resolve_f, reject_f)) =
            crate::promise_capability::new_promise_capability(c)
        else {
            return VALUE_UNDEFINED;
        };
        let f = if reject { reject_f } else { resolve_f };
        let one = [v];
        let out = crate::method_call_closure_dispatch::__torajs_any_call(f, one.as_ptr(), 1);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(out);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(resolve_f);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(reject_f);
        if __torajs_throw_check() != 0 {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(promise);
            return VALUE_UNDEFINED;
        }
        promise
    }
}

/// The §27.2.1.5 capability behind the try / withResolvers arms —
/// `(promise, resolve, reject)` boxes, all owned. A builtin |this|
/// mints the trio directly (NewPromiseCapability(%Promise%) with no
/// user ctor to observe — a pending cell plus the §27.2.1.3
/// resolving pair, exactly the withResolvers kernel's front half); a
/// builtin-heir class object goes through the real construct-channel
/// capability. `None` = the TypeError is recorded (wrong |this|, or
/// the heir ctor chain raised).
unsafe fn capability_for(this: u64) -> Option<(u64, u64, u64)> {
    unsafe {
        let promise_ctor = crate::method_value::ctor::ctor_cell_peek(10);
        let is_builtin =
            !promise_ctor.is_null() && is_cell(this) && as_void_ptr(this) == promise_ctor.cast();
        if is_builtin {
            use crate::promise_with_resolvers as wr;
            let p = __torajs_promise_alloc_pending();
            p.cast::<u8>().add(wr::P_REPR_OFF).write(wr::REPR_ANY);
            p.cast::<u8>().add(wr::P_IS_HEAP_OFF).write(1);
            torajs_rc::__torajs_rc_inc(p);
            let r = wr::mint_resolver(p, wr::resolver_resolve_entry);
            torajs_rc::__torajs_rc_inc(p);
            let j = wr::mint_resolver(p, wr::resolver_reject_entry);
            return Some((
                crate::nanbox_encode::__torajs_anyv_box_pointer(p),
                crate::nanbox_encode::__torajs_anyv_box_pointer(r as *mut c_void),
                crate::nanbox_encode::__torajs_anyv_box_pointer(j as *mut c_void),
            ));
        }
        if this_reaches_promise_ctor(this) {
            return crate::promise_capability::new_promise_capability(this);
        }
        promise_settle();
        None
    }
}

/// ES2025 Promise.try with a receiver channel — capability first,
/// then Call(callbackfn = argv[1], undefined, argv[2..]) runs
/// synchronously (step 4): a normal completion goes through
/// Call(cap.resolve) — spec tick semantics, a thenable answer
/// settles via the §27.2.1.3.2 adoption microtask — and an abrupt
/// completion (a non-callable callback's TypeError included) pops
/// the pending throw into Call(cap.reject).
pub(super) unsafe fn promise_try_fn(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let Some((promise, resolve_f, reject_f)) = capability_for(arg_at(argv, argc, 0)) else {
            return VALUE_UNDEFINED;
        };
        let f = arg_at(argv, argc, 1);
        let (rest, n) = if argc >= 2 {
            (argv.add(2), argc - 2)
        } else {
            (core::ptr::null(), 0)
        };
        let r = crate::method_call_closure_dispatch::__torajs_any_call(f, rest, n);
        if __torajs_throw_check() != 0 {
            let tag = __torajs_throw_take_tag();
            let value = __torajs_throw_take();
            let boxed = crate::nanbox_encode::__torajs_anyv_box_from_pair(tag, value);
            let one = [boxed];
            let out =
                crate::method_call_closure_dispatch::__torajs_any_call(reject_f, one.as_ptr(), 1);
            crate::nanbox_ffi::__torajs_anyv_rc_dec(out);
            crate::nanbox_ffi::__torajs_anyv_rc_dec(boxed);
        } else {
            let one = [r];
            let out =
                crate::method_call_closure_dispatch::__torajs_any_call(resolve_f, one.as_ptr(), 1);
            crate::nanbox_ffi::__torajs_anyv_rc_dec(out);
            crate::nanbox_ffi::__torajs_anyv_rc_dec(r);
        }
        crate::nanbox_ffi::__torajs_anyv_rc_dec(resolve_f);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(reject_f);
        if __torajs_throw_check() != 0 {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(promise);
            return VALUE_UNDEFINED;
        }
        promise
    }
}

/// §27.2.4.8 Promise.withResolvers with a receiver channel —
/// capability first (a heir |this| answers a C-instance promise),
/// then the spec result object; all three stakes transfer into it.
pub(super) unsafe fn promise_with_resolvers_fn(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let Some((promise, resolve_f, reject_f)) = capability_for(arg_at(argv, argc, 0)) else {
            return VALUE_UNDEFINED;
        };
        let mut obj = __torajs_dynobj_alloc();
        crate::promise_with_resolvers::set_field(&mut obj, b"promise", promise);
        crate::promise_with_resolvers::set_field(&mut obj, b"resolve", resolve_f);
        crate::promise_with_resolvers::set_field(&mut obj, b"reject", reject_f);
        crate::nanbox_encode::__torajs_anyv_box_pointer(obj)
    }
}
