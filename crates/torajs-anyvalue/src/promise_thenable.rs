//! User thenables — §27.2.1.3.2 NewPromiseResolveThenableJob, and
//! §27.2.4.7 PromiseResolve(%Promise%, x) over the any lane
//! (RFC 20260913-thenable-adoption).
//!
//! Resolving a promise with a value goes four ways (§27.2.1.3.2 steps
//! 7–13): not an Object → fulfill; `Get(value, "then")` abrupt →
//! reject with that error; `then` not callable → fulfill; otherwise
//! a job — `CreateResolvingFunctions(promise)`, then `Call(then,
//! value, «resolve, reject»)`, an abrupt call rejecting through the
//! pair's `reject`. Every settle path in this crate used to know only
//! the first and the last-but-one of these, with a native %Promise%
//! cell as the single adopted shape; a plain `{ then(res) { res(7) } }`
//! was stored verbatim and `await`ed as itself.
//!
//! [`probe_then`] answers the Get + IsCallable pair, leaving native
//! promise cells to the callers' existing adoption (an attach, no
//! job — the fast path engines take for `await`) UNLESS the cell
//! carries an own `then` ([`has_own_then`]): step 8 reads the
//! property off every Object, so an override is the resolution's
//! `then` even on a promise. §27.2.4.7 PromiseResolve is a different
//! algorithm — it compares `constructor` and returns x itself, never
//! reading `then` — so the kernel below answers that step before it
//! reaches the probe. [`enqueue_thenable_
//! job`] is the job; its resolving pair is a FRESH `[[AlreadyResolved]]`
//! record over the same promise (`promise_with_resolvers::
//! mint_resolving_pair`), which is why that record is per pair. The
//! kernel [`__torajs_anyv_promise_resolve`] is the whole of
//! PromiseResolve for the two lowering sites that spell it (`await`'s
//! `__torajs_await_resolve`, `Promise.resolve(any)`), falling back to
//! torajs-promise's `resolve_any` for the promise pass-through and the
//! plain fulfill.

use core::ffi::c_void;

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};
use crate::promise_with_resolvers::{
    P_IS_HEAP_OFF, P_REPR_OFF, REPR_ANY, is_native_promise, mint_resolving_pair,
};

unsafe extern "C" {
    /// torajs-microtask — HostEnqueuePromiseJob.
    fn __torajs_microtask_enqueue(fn_: Option<unsafe extern "C" fn(i64)>, arg: i64);
    fn __torajs_promise_alloc_pending() -> *mut c_void;
    fn __torajs_promise_reject(p: *mut c_void, reason: i64);
    /// torajs-promise — %Promise% pass-through / plain fulfill; adopts
    /// the transferred ref on the box.
    fn __torajs_promise_resolve_any(bits: i64) -> *mut c_void;
    fn __torajs_promise_reject_any(bits: i64) -> *mut c_void;
    /// torajs-dynobj — own-entry existence probe over an expando bag.
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_take() -> i64;
    fn __torajs_throw_take_tag() -> i64;
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// What `Get(v, "then")` says about a resolution value.
pub(crate) enum ThenProbe {
    /// Not an object, a native %Promise% (the caller's own adoption
    /// owns that shape), or an object whose `then` is not callable.
    NotThenable,
    /// The read itself raised; the throw is pending for the caller
    /// to take ([`take_throw_box`]).
    Threw,
    /// A callable `then`, as an OWNED box.
    Callable(AnyValue),
}

/// §27.2.1.3.2 steps 7–10 — the Get + IsCallable probe. `v` is
/// borrowed; the returned `then` is owned (the job runs a tick later,
/// after user code may have deleted the property).
///
/// # Safety
/// `v` is a live AnyValue.
pub(crate) unsafe fn probe_then(v: AnyValue) -> ThenProbe {
    unsafe {
        if !is_cell(v) || (is_native_promise(v) && !has_own_then(v)) {
            return ThenProbe::NotThenable;
        }
        // The full [[Get]] — an accessor `then` runs against the
        // thenable itself (the getter-throws shapes); the answer is
        // owned either way.
        let k = __torajs_str_alloc(b"then".as_ptr(), 4);
        let f = crate::member_get_receiver::__torajs_any_member_get_with_receiver(v, k.cast(), v);
        __torajs_str_drop(k.cast());
        if __torajs_throw_check() != 0 {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(f);
            return ThenProbe::Threw;
        }
        if !crate::promise_capability::is_callable(f) {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(f);
            return ThenProbe::NotThenable;
        }
        ThenProbe::Callable(f)
    }
}

/// Does this native %Promise% cell carry an OWN `then`?
///
/// §27.2.1.3.2 step 8 reads `then` off every Object, so a promise is
/// only the callers' adopt-by-attach shape while `Promise.prototype
/// .then` is the one that read answers. A user store (`p.then = f`)
/// lands in the cell's lazy expando bag, which is NULL until the
/// first such write — so the normal program pays one load and a
/// branch, and the string only gets minted for a cell that has a bag
/// at all.
///
/// Own-entry, not the full Get: a bag holding some OTHER expando must
/// keep the fast path, because falling through would hand the job the
/// builtin `then` and spend a microtask tick the adopt does not.
///
/// # Safety
/// `v` is a live AnyValue that [`is_native_promise`] accepted.
pub(crate) unsafe fn has_own_then(v: AnyValue) -> bool {
    unsafe {
        let props = crate::member_get_layout::promise_props(as_void_ptr(v));
        if props.is_null() {
            return false;
        }
        let k = __torajs_str_alloc(b"then".as_ptr(), 4);
        let hit = __torajs_dynobj_has(props, k.cast()) != 0;
        __torajs_str_drop(k.cast());
        hit
    }
}

/// The pending throw as an OWNED box (tag peek before value — the
/// take order every reaction dispatcher uses).
///
/// # Safety
/// A throw is pending (`__torajs_throw_check() != 0`).
pub(crate) unsafe fn take_throw_box() -> AnyValue {
    unsafe {
        let tag = __torajs_throw_take_tag();
        let value = __torajs_throw_take();
        crate::nanbox_encode::__torajs_anyv_box_from_pair(tag, value)
    }
}

/// The job's three operands; every one an owned stake the dispatch
/// releases.
#[repr(C)]
struct ThenableJob {
    promise: *mut c_void,
    thenable: AnyValue,
    then: AnyValue,
}

/// §27.2.1.3.2 step 12–13 — queue the job that settles `promise`
/// through `then`. `thenable` and `then` transfer in owned; the job
/// takes its own stake on `promise` (the caller keeps whatever it
/// holds).
///
/// # Safety
/// `promise` is a live pending cell whose value form is already
/// stamped (the resolving pair stores REPR_ANY boxes).
pub(crate) unsafe fn enqueue_thenable_job(
    promise: *mut c_void,
    thenable: AnyValue,
    then: AnyValue,
) {
    unsafe {
        torajs_rc::__torajs_rc_inc(promise);
        let job = Box::into_raw(Box::new(ThenableJob {
            promise,
            thenable,
            then,
        }));
        __torajs_microtask_enqueue(Some(thenable_job_dispatch), job as i64);
    }
}

/// NewPromiseResolveThenableJob's body: a fresh resolving pair over the
/// promise, `Call(then, thenable, «resolve, reject»)`, an abrupt call
/// rejecting through `reject` (which honours the pair's
/// `[[AlreadyResolved]]` — a `then` that settled before throwing
/// keeps its settlement).
unsafe extern "C" fn thenable_job_dispatch(arg: i64) {
    unsafe {
        let job = Box::from_raw(arg as *mut ThenableJob);
        let ThenableJob {
            promise,
            thenable,
            then,
        } = *job;
        let pair = mint_resolving_pair(promise);
        let resolve = crate::nanbox_encode::__torajs_anyv_box_pointer(pair.resolve.cast());
        let reject = crate::nanbox_encode::__torajs_anyv_box_pointer(pair.reject.cast());
        let argv = [resolve, reject];
        let out = crate::method_call_closure_dispatch::__torajs_any_call_with_this(
            then,
            thenable,
            argv.as_ptr(),
            2,
        );
        crate::nanbox_ffi::__torajs_anyv_rc_dec(out);
        if __torajs_throw_check() != 0 {
            let reason = take_throw_box();
            let one = [reason];
            let out = crate::method_call_closure_dispatch::__torajs_any_call_with_this(
                reject,
                VALUE_UNDEFINED,
                one.as_ptr(),
                1,
            );
            crate::nanbox_ffi::__torajs_anyv_rc_dec(out);
            crate::nanbox_ffi::__torajs_anyv_rc_dec(reason);
        }
        crate::nanbox_ffi::__torajs_anyv_rc_dec(resolve);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(reject);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(then);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(thenable);
        __torajs_value_drop_heap(promise);
    }
}

/// The typed `.then` kernels' hook (torajs-promise `then_box::
/// settle_handler_return`): a handler return under REPR_ANY (6) or
/// REPR_HEAP (5) that is a user thenable — or whose `then` read raised
/// — settles `result` here. Answers 1 when it did (the value's stake is
/// consumed), 0 to let the caller store the value as before.
///
/// # Safety
/// `result` is a live pending cell; `value` carries the form `repr`
/// names, owned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_promise_settle_thenable(
    result: *mut c_void,
    repr: i64,
    value: i64,
) -> i64 {
    unsafe {
        let v: AnyValue = match repr {
            6 => value as AnyValue,
            // A heap-tagged box IS the cell pointer (`then_adopt`).
            5 => crate::nanbox_encode::__torajs_anyv_box_from_pair(4, value),
            _ => return 0,
        };
        match probe_then(v) {
            ThenProbe::NotThenable => 0,
            ThenProbe::Threw => {
                let reason = take_throw_box();
                crate::nanbox_ffi::__torajs_anyv_rc_dec(v);
                result.cast::<u8>().add(P_REPR_OFF).write(REPR_ANY);
                result.cast::<u8>().add(P_IS_HEAP_OFF).write(1);
                __torajs_promise_reject(result, reason as i64);
                1
            }
            ThenProbe::Callable(then) => {
                result.cast::<u8>().add(P_REPR_OFF).write(REPR_ANY);
                result.cast::<u8>().add(P_IS_HEAP_OFF).write(1);
                enqueue_thenable_job(result, v, then);
                1
            }
        }
    }
}

/// §27.2.4.7.1 step 2 — `Get(x, "constructor")` on a native promise
/// through the any-lane member read (own expando first, then the
/// prototype's builtin ctor cell), and the answer is %Promise% when
/// it is the builtin-constructor cell carrying the Promise tag (the
/// cell is interned on the read itself, so a program that never
/// spelled `Promise` as a value has no cell to compare pointers
/// with — an earlier cut that peeked for one answered "builtin" for
/// every promise in such a program). A read that throws answers
/// false (the fresh promise then adopts x; the abrupt is the recorded
/// residue).
unsafe fn constructor_is_builtin_promise(v: AnyValue) -> bool {
    unsafe {
        let key = __torajs_str_alloc(b"constructor".as_ptr(), 11);
        let tag = crate::member_get::__torajs_any_member_get_tag(v, key.cast());
        let value = crate::member_get_value::__torajs_any_member_get_value(v, key.cast());
        __torajs_str_drop(key.cast());
        if __torajs_throw_check() != 0 {
            let _ = take_throw_box();
            return false;
        }
        let c = crate::nanbox_encode::__torajs_anyv_box_from_pair(tag as i64, value as i64);
        is_cell(c)
            && crate::method_value::ctor::ctor_tag_of_cell(crate::nanbox::as_void_ptr(c))
                == Some(10)
    }
}

/// An async function settling its own result promise with `x`
/// (§27.7.5.1 step 3.b calls the capability's resolve function): unlike
/// PromiseResolve, a native promise is never answered itself — a fresh
/// cell adopts it through the §27.2.1.3.2 job, so `f() !== p` and the
/// result settles two jobs after `p`. Anything else is
/// [`__torajs_anyv_promise_resolve`]. The caller transfers one ref on
/// the box; the answer is owned.
///
/// # Safety
/// `bits` is a live AnyValue whose stake transfers in.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_promise_resolve_fresh(bits: i64) -> *mut c_void {
    unsafe {
        let v = bits as AnyValue;
        if !is_native_promise(v) {
            return __torajs_anyv_promise_resolve(bits);
        }
        let p = __torajs_promise_alloc_pending();
        p.cast::<u8>().add(P_REPR_OFF).write(REPR_ANY);
        p.cast::<u8>().add(P_IS_HEAP_OFF).write(1);
        // an own `then` is the one the job calls
        match probe_then(v) {
            ThenProbe::Callable(then) => enqueue_thenable_job(p, v, then),
            ThenProbe::Threw => {
                let reason = take_throw_box();
                crate::nanbox_ffi::__torajs_anyv_rc_dec(v);
                __torajs_promise_reject(p, reason as i64);
            }
            ThenProbe::NotThenable => {
                crate::promise_with_resolvers::adopt_promise(p, crate::nanbox::as_void_ptr(v));
                crate::nanbox_ffi::__torajs_anyv_rc_dec(v);
            }
        }
        p
    }
}

/// §27.2.4.7 PromiseResolve(%Promise%, x) — the any-lane kernel behind
/// `await` and `Promise.resolve(any)`. A %Promise% cell passes through
/// and a non-thenable fulfills (torajs-promise's `resolve_any`, which
/// stamps); a thenable answers a fresh pending cell the job settles;
/// an abrupt `then` read answers a cell rejected with that error. The
/// caller transfers one ref on the box; the answer is owned.
///
/// # Safety
/// `bits` is a live AnyValue whose stake transfers in.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_promise_resolve(bits: i64) -> *mut c_void {
    unsafe {
        let v = bits as AnyValue;
        // §27.2.4.7.1 PromiseResolve steps 1-2 — a native promise
        // answers ITSELF only when `x.constructor` is C (%Promise%
        // here); a subclass instance, or a promise whose `constructor`
        // was reassigned, mints a fresh promise that adopts it (the
        // test262 `arg-uniq-ctor` / `arg-non-thenable`-family shape).
        // Until rotation 650 every native promise took the identity
        // road.
        if is_native_promise(v) {
            if !constructor_is_builtin_promise(v) {
                let p = __torajs_promise_alloc_pending();
                p.cast::<u8>().add(P_REPR_OFF).write(REPR_ANY);
                p.cast::<u8>().add(P_IS_HEAP_OFF).write(1);
                crate::promise_with_resolvers::adopt_promise(p, crate::nanbox::as_void_ptr(v));
                // The caller transferred one ref on the box; the
                // adoption took its own stake above.
                crate::nanbox_ffi::__torajs_anyv_rc_dec(v);
                return p;
            }
            // Step 2 returns x ITSELF — PromiseResolve never reads
            // `then`. That read belongs to §27.2.1.3.2, whose probe
            // an own `then` does divert; routing this step through it
            // too would break the identity `Promise.all` relies on to
            // Invoke the override (step 5.r over the SAME cell).
            return __torajs_promise_resolve_any(bits);
        }
        match probe_then(v) {
            ThenProbe::NotThenable => __torajs_promise_resolve_any(bits),
            ThenProbe::Threw => {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(v);
                let reason = take_throw_box();
                __torajs_promise_reject_any(reason as i64)
            }
            ThenProbe::Callable(then) => {
                let p = __torajs_promise_alloc_pending();
                p.cast::<u8>().add(P_REPR_OFF).write(REPR_ANY);
                p.cast::<u8>().add(P_IS_HEAP_OFF).write(1);
                enqueue_thenable_job(p, v, then);
                p
            }
        }
    }
}
