//! §27.2.4 Promise combinators over an ARBITRARY constructor `this`
//! — RFC 20260820-combinator-any-constructor.
//!
//! `Promise.race.call(C, xs)` names C as the species constructor, and
//! the spec never assumes C is Promise or a subclass of it: step 1 is
//! `NewPromiseCapability(C)` (whose only demand is IsConstructor) and
//! everything after it reaches C's world through `Call` and `Invoke`
//! alone. The receiver-first arms next door take two shortcuts that
//! only hold for the builtin and its heirs — they hand the element
//! walk to the typed fan-in kernels in torajs-promise, which mint a
//! BUILTIN promise and only then resolve it into the capability. A
//! user constructor observes the difference twice over: its resolve
//! function is handed that inner promise instead of the combined
//! value, and a bare thenable element never sees its own `then`
//! invoked with the capability's functions.
//!
//! So this module is the literal algorithm instead of a shortcut:
//! iterate, `Call(promiseResolve, C, «next»)`, then
//! `Invoke(nextPromise, "then", «…»)`. The builtin and heir paths keep
//! their fast kernels untouched.
//!
//! Race (§27.2.4.5.1) is the whole algorithm with no per-element
//! function at all — the capability's own resolve / reject go
//! straight into every element's `then`, and first settle wins. All
//! The other three mint element functions over a shared values /
//! counter record — one per element for all (§27.2.4.1.2) and any
//! (§27.2.4.6.2), a resolve / reject PAIR for allSettled
//! (§27.2.4.3.2). That record and its cells live in the
//! [`crate::combinator_elem`] sibling.

use crate::combinator_elem::ElemState;
use crate::nanbox::VALUE_UNDEFINED;

unsafe extern "C" {
    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut core::ffi::c_void);
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_take() -> i64;
    fn __torajs_throw_take_tag() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// One iteration step's outcome — `Err` means a throw is pending and
/// the caller owes the capability an IfAbruptRejectPromise.
type Abrupt = Result<(), ()>;

/// Which algorithm the spec walk runs.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum SpecComb {
    All,
    AllSettled,
    Any,
    Race,
}

/// §27.2.4.{1,5} over `c` — steps 1-4 plus the matching
/// PerformPromiseX. Answers the capability's promise as an OWNED box,
/// or `undefined` with the throw pending when the capability itself
/// could not be built (a ctor that raises, or one that never handed
/// over a callable pair).
///
/// # Safety
/// `c` and `v` are live AnyValues the caller keeps across the call.
pub(crate) unsafe fn run(kind: SpecComb, c: u64, v: u64) -> u64 {
    unsafe {
        let Some((promise, resolve_f, reject_f)) =
            crate::promise_capability::new_promise_capability(c)
        else {
            return VALUE_UNDEFINED;
        };
        // Step 3 GetPromiseResolve(C) runs BEFORE GetIterator, so a
        // non-callable `C.resolve` rejects without touching the
        // iterable (the t262 invoke-resolve-get-error family asserts
        // exactly that order).
        let pr = get_promise_resolve(c);
        let outcome = if pr == 0 {
            Err(())
        } else {
            match kind {
                SpecComb::Race => perform_race(c, pr, v, resolve_f, reject_f),
                _ => perform_elementwise(kind, c, pr, v, resolve_f, reject_f),
            }
        };
        if pr != 0 {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(pr);
        }
        if outcome.is_err() {
            reject_with_pending(reject_f);
        }
        crate::nanbox_ffi::__torajs_anyv_rc_dec(resolve_f);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(reject_f);
        promise
    }
}

/// §27.2.4.5.1 PerformPromiseRace — every element rides
/// `Call(promiseResolve, C, «next»)` and then
/// `Invoke(nextPromise, "then", «cap.[[Resolve]], cap.[[Reject]]»)`.
/// The capability functions are passed VERBATIM, which is what makes
/// `p.then` see the same function object for every element.
unsafe fn perform_race(c: u64, pr: u64, v: u64, resolve_f: u64, reject_f: u64) -> Abrupt {
    unsafe {
        let mut idx: i64 = 0;
        let mut iter_slot: u64 = VALUE_UNDEFINED;
        let mut next: u64 = VALUE_UNDEFINED;
        loop {
            let has =
                crate::iter_any::__torajs_any_iter_next(v, &mut idx, &mut iter_slot, &mut next);
            if __torajs_throw_check() != 0 {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(iter_slot);
                return Err(());
            }
            if has == 0 {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(iter_slot);
                return Ok(());
            }
            // The step's rc is ours (the for-of ledger).
            let step = resolve_and_attach(c, pr, next, resolve_f, reject_f, core::ptr::null_mut());
            crate::nanbox_ffi::__torajs_anyv_rc_dec(next);
            if step.is_err() {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(iter_slot);
                return Err(());
            }
        }
    }
}

/// PerformPromiseAll (§27.2.4.1.2) / AllSettled (§27.2.4.3.2) / Any
/// (§27.2.4.6.2) — the three that mint element functions over a
/// shared values / counter record. They differ only in which
/// capability function the drained counter calls and which of the two
/// `then` handlers each element gets. The record outlives this walk
/// (the element functions hold it), so the walk releases only its own
/// reference on the way out.
unsafe fn perform_elementwise(
    kind: SpecComb,
    c: u64,
    pr: u64,
    v: u64,
    resolve_f: u64,
    reject_f: u64,
) -> Abrupt {
    unsafe {
        // any is the mirror image: it counts REJECTIONS down to an
        // AggregateError on the capability's reject function.
        let settle = if kind == SpecComb::Any {
            reject_f
        } else {
            resolve_f
        };
        let st = crate::combinator_elem::ElemState::new(settle, kind == SpecComb::Any);
        let out = perform_elementwise_inner(kind, c, pr, v, resolve_f, reject_f, st);
        if out.is_ok() {
            // Step 4.b — the iterator is drained, so the walk's own
            // hold on the counter comes off. An empty run, or one
            // whose elements all settled synchronously, resolves
            // right here.
            crate::combinator_elem::count_down(st);
        }
        crate::combinator_elem::state_release(st);
        out
    }
}

/// The element loop of [`perform_elementwise`] — steps 4.a through
/// 4.o of whichever algorithm is running.
#[allow(clippy::too_many_arguments)]
unsafe fn perform_elementwise_inner(
    kind: SpecComb,
    c: u64,
    pr: u64,
    v: u64,
    resolve_f: u64,
    reject_f: u64,
    st: *mut ElemState,
) -> Abrupt {
    unsafe {
        let mut idx: i64 = 0;
        let mut iter_slot: u64 = VALUE_UNDEFINED;
        let mut next: u64 = VALUE_UNDEFINED;
        let mut index: i64 = 0;
        loop {
            let has =
                crate::iter_any::__torajs_any_iter_next(v, &mut idx, &mut iter_slot, &mut next);
            if __torajs_throw_check() != 0 {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(iter_slot);
                return Err(());
            }
            if has == 0 {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(iter_slot);
                return Ok(());
            }
            // Step 4.c — the slot exists before the element resolves,
            // so an element that settles synchronously writes into a
            // list that is already the right length.
            crate::combinator_elem::ElemState::reserve_slot(st);
            let (on_ok, on_err) = mint_handlers(kind, st, index, resolve_f, reject_f);
            let step = resolve_and_attach(c, pr, next, on_ok, on_err, st);
            release_minted(kind, on_ok, on_err);
            crate::nanbox_ffi::__torajs_anyv_rc_dec(next);
            if step.is_err() {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(iter_slot);
                return Err(());
            }
            index += 1;
        }
    }
}

/// The `then` handler pair for one element. all attaches its element
/// function on the fulfilled side and the capability's own reject on
/// the other; any is the mirror; allSettled mints BOTH, sharing one
/// [[AlreadyCalled]] record through their common index.
unsafe fn mint_handlers(
    kind: SpecComb,
    st: *mut ElemState,
    index: i64,
    resolve_f: u64,
    reject_f: u64,
) -> (u64, u64) {
    use crate::combinator_elem::{ElemKind, mint_elem};
    unsafe {
        match kind {
            SpecComb::All => (mint_elem(st, index, ElemKind::AllResolve), reject_f),
            SpecComb::Any => (resolve_f, mint_elem(st, index, ElemKind::AnyReject)),
            _ => (
                mint_elem(st, index, ElemKind::SettledResolve),
                mint_elem(st, index, ElemKind::SettledReject),
            ),
        }
    }
}

/// Release only what [`mint_handlers`] minted — the capability
/// functions it passed through are the caller's.
unsafe fn release_minted(kind: SpecComb, on_ok: u64, on_err: u64) {
    unsafe {
        if kind != SpecComb::Any {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(on_ok);
        }
        if kind != SpecComb::All {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(on_err);
        }
    }
}

/// `Call(promiseResolve, C, «next»)` then `Invoke(nextPromise,
/// "then", «on_ok, on_err»)` — steps 8.i + 8.j of PerformPromiseRace,
/// and 4.d + 4.r of PerformPromiseAll. `st` is the all-lane counter
/// (null for race): the increment sits between the two calls exactly
/// where step 4.m puts it, so a `then` that settles synchronously
/// cannot drive the counter to zero before the iterator is drained.
unsafe fn resolve_and_attach(
    c: u64,
    pr: u64,
    next: u64,
    on_ok: u64,
    on_err: u64,
    st: *mut ElemState,
) -> Abrupt {
    unsafe {
        let one = [next];
        let next_promise = crate::method_call_closure_dispatch::__torajs_any_call_with_this(
            pr,
            c,
            one.as_ptr(),
            1,
        );
        if __torajs_throw_check() != 0 {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(next_promise);
            return Err(());
        }
        if !st.is_null() {
            (*st).remaining += 1;
        }
        let r = invoke_then(next_promise, on_ok, on_err);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(next_promise);
        r
    }
}

/// §7.3.20 Invoke(`p`, "then", «`on_ok`, `on_err`») — the member read
/// can run a getter and the call can raise, so both are abrupt
/// points. A non-callable `then` gets the same catchable TypeError
/// Call would raise.
unsafe fn invoke_then(p: u64, on_ok: u64, on_err: u64) -> Abrupt {
    unsafe {
        let f = member_fn(p, b"then");
        if f == 0 {
            return Err(());
        }
        let two = [on_ok, on_err];
        let out =
            crate::method_call_closure_dispatch::__torajs_any_call_with_this(f, p, two.as_ptr(), 2);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(out);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(f);
        if __torajs_throw_check() != 0 {
            return Err(());
        }
        Ok(())
    }
}

/// `Get(recv, key)` as an OWNED callable box, or 0 with the throw
/// pending (a getter's abrupt, or the non-callable TypeError). The
/// extra stake outlives the read because the callee may be reached
/// again after user code has had a chance to delete the property.
unsafe fn member_fn(recv: u64, key: &[u8]) -> u64 {
    unsafe {
        let k = __torajs_str_alloc(key.as_ptr(), key.len() as i64);
        let tag = crate::member_get::__torajs_any_member_get_tag(recv, k.cast());
        let value = crate::member_get_value::__torajs_any_member_get_value(recv, k.cast());
        __torajs_str_drop(k.cast());
        if __torajs_throw_check() != 0 {
            return 0;
        }
        let f = crate::nanbox_encode::__torajs_anyv_box_from_pair(tag as i64, value as i64);
        if !crate::promise_capability::is_callable(f) {
            __torajs_throw_type_error(c"promise element is not thenable".as_ptr());
            return 0;
        }
        crate::nanbox_ffi::__torajs_anyv_rc_inc(f);
        f
    }
}

/// §27.2.4.1.1 GetPromiseResolve(C) — Get(C, "resolve") plus the
/// step-2 IsCallable gate, as an OWNED box or 0 with the throw
/// pending.
unsafe fn get_promise_resolve(c: u64) -> u64 {
    unsafe {
        let k = __torajs_str_alloc(b"resolve".as_ptr(), 7);
        let tag = crate::member_get::__torajs_any_member_get_tag(c, k.cast());
        let value = crate::member_get_value::__torajs_any_member_get_value(c, k.cast());
        __torajs_str_drop(k.cast());
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

/// IfAbruptRejectPromise — the in-flight throw becomes the
/// capability's rejection reason.
unsafe fn reject_with_pending(reject_f: u64) {
    unsafe {
        let tag = __torajs_throw_take_tag();
        let value = __torajs_throw_take();
        let err = crate::nanbox_encode::__torajs_anyv_box_from_pair(tag, value);
        let one = [err];
        let out = crate::method_call_closure_dispatch::__torajs_any_call(reject_f, one.as_ptr(), 1);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(out);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(err);
    }
}
