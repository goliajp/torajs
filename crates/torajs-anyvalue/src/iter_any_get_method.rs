//! §7.4.2 GetIterator's `GetMethod(obj, @@iterator)`, and the step
//! tier that a user-written iterator needs — split from
//! [`crate::iter_any`] at the size cap.
//!
//! Until the §6.1.7 symbol-key domain existed there was nothing for
//! GetIterator to look up, so `for..of` found its iterator by MANGLED
//! NAME through the class-methods vtable: only a class instance could
//! be iterable, and only because the parser had folded its
//! `[Symbol.iterator]()` member into a `__sym_Symbol_iterator__`
//! method. An object literal declaring the same member iterated
//! nothing.
//!
//! GetIterator asks the receiver for a property now, which is what the
//! spec says it does. The lookup is [`crate::member_get_symbol`]'s own
//! walk — own dict, then what the receiver inherits — so anything that
//! can hold a symbol key can be iterable: an object literal, a
//! dyn-assigned `o[Symbol.iterator] = f`, a `defineProperty`, an
//! inherited one off a user prototype.
//!
//! **Once per loop, not once per step.** GetIterator is a single spec
//! step; probing it per iteration would put a symbol-key walk in the
//! array / string hot path for no gain. The caller decides when to
//! ask — see `iter_any`'s `USER_ITERATOR_LANE` for how the two cursor
//! slots encode "first step" and "this loop is mine".
//!
//! **A user `@@iterator` outranks the builtin lane**, per §7.4.2 —
//! that is what makes `arr[Symbol.iterator] = …` mean anything. tr's
//! builtin prototypes carry no symbol keys of their own, so today this
//! only ever fires on a property somebody actually wrote.
//!
//! The step tier splits by how the iterator dispatches, not by how it
//! was found: a class instance (a generator object, a hand-written
//! iterator class) steps through the vtable exactly as before, while
//! everything else — the object literal `{ next() {…} }` an
//! `@@iterator` typically returns — steps through the `any` method
//! dispatcher and reads `done` / `value` as ordinary members.

use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, Ordering};

use torajs_rc::ANY_METHOD_NEXT;

use crate::member_get::__torajs_any_member_get_tag;
use crate::member_get_value::__torajs_any_member_get_value;
use crate::method_call::__torajs_any_method_call;
use crate::method_call_closure_dispatch::{closure_cell_entry, invoke_with_this};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};
use crate::nanbox_encode::__torajs_anyv_box_from_pair;
use crate::nanbox_ffi::{__torajs_anyv_rc_dec, __torajs_anyv_to_bool};

unsafe extern "C" {
    /// torajs-str — the §6.1.5.1 well-known singleton table; idx 5 is
    /// `@@iterator` (alphabetical property-name order). Answers an
    /// owned +1.
    fn __torajs_symbol_well_known(idx: i64) -> *mut c_void;
    /// torajs-str — fresh rc-1 Str block from ASCII bytes.
    fn __torajs_str_alloc_ascii(src: *const u8, len: i64) -> *mut u8;
    /// torajs-rc — the universal heap-header decrement (non-zero when
    /// the cell was freed; unused here).
    fn __torajs_rc_dec(p: *mut c_void) -> i32;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
}

/// `WELL_KNOWN_DESCS` index of `Symbol.iterator`.
const WK_ITERATOR: i64 = 5;

/// `AnySlotTag::Undef` — an absent property.
const TAG_UNDEF: u64 = 5;

/// `AnySlotTag::Null`.
const TAG_NULL: u64 = 0;

/// §7.3.11 GetMethod steps 3-4 over an already-read property pair:
/// `None` when the property is absent or nullish (the caller's own
/// spec step decides what that means), the closure's `(env, entry)`
/// when it is callable, and a recorded TypeError otherwise.
///
/// # Safety
/// `(tag, payload)` is a live borrowed member pair.
pub(crate) unsafe fn callable_entry(
    tag: u64,
    payload: u64,
    not_a_function: &core::ffi::CStr,
) -> Option<(*mut c_void, u64)> {
    unsafe {
        if tag == TAG_UNDEF || tag == TAG_NULL {
            return None;
        }
        let method = __torajs_anyv_box_from_pair(tag as i64, payload as i64);
        let entry = if is_cell(method) {
            closure_cell_entry(as_void_ptr(method) as *mut c_void)
        } else {
            None
        };
        if entry.is_none() {
            __torajs_throw_type_error(not_a_function.as_ptr());
        }
        entry
    }
}

/// Interned property-name cells the step tier keys off. Minted once
/// and never released — they are immortal by construction, and every
/// consumer only READS a key.
static NAME_NEXT: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut());
static NAME_RETURN: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut());

/// The interned cell for `bytes`, minting it on first use. A racing
/// initializer's loser drops its own mint; the winner is what
/// everybody reads from then on.
///
/// # Safety
/// `bytes` is ASCII.
unsafe fn interned(slot: &AtomicPtr<u8>, bytes: &[u8]) -> *mut u8 {
    let existing = slot.load(Ordering::Acquire);
    if !existing.is_null() {
        return existing;
    }
    let fresh = unsafe { __torajs_str_alloc_ascii(bytes.as_ptr(), bytes.len() as i64) };
    match slot.compare_exchange(
        core::ptr::null_mut(),
        fresh,
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => fresh,
        Err(winner) => {
            unsafe { __torajs_rc_dec(fresh as *mut c_void) };
            winner
        }
    }
}

/// What [`get_iterator`] resolved.
pub(crate) enum GetIterator {
    /// The receiver has no `@@iterator` — the caller keeps its builtin
    /// lane (an array, a string, a Map).
    NoUserMethod,
    /// The derived iterator, owned; the caller parks it in `iter_slot`
    /// and releases it at loop exit.
    Iterator(AnyValue),
    /// A pending throw is in flight — a non-callable `@@iterator`, or
    /// one that threw. The caller reports done and lets its own
    /// throw-check forward it.
    Threw,
}

/// §7.4.2 steps 1-4 — `GetMethod(obj, @@iterator)`, then call it with
/// the receiver as `this`.
///
/// # Safety
/// `recv` is a live AnyValue.
pub(crate) unsafe fn get_iterator(recv: AnyValue) -> GetIterator {
    unsafe {
        get_iterator_wk(
            recv,
            WK_ITERATOR,
            c"Symbol.iterator is not a function",
            false,
        )
    }
}

/// [`get_iterator`] over any well-known symbol slot — the async
/// GetIterator (§7.4.2 hint=async step 2.a) probes `@@asyncIterator`
/// (table idx 1) through the same GetMethod-then-call walk before
/// falling back to the sync symbol.
///
/// `nullish_missing` is the §7.3.11 GetMethod step-2 split the two
/// probes disagree on: the async probe treats a null `@@asyncIterator`
/// as absent (there IS a fallback — the sync symbol), while the sync
/// probe keeps nullish → TypeError, because GetIterator has no other
/// source and `arr[Symbol.iterator] = null` must refuse rather than
/// ride the builtin lane.
///
/// # Safety
/// `recv` is a live AnyValue; `wk_idx` indexes `WELL_KNOWN_DESCS`.
pub(crate) unsafe fn get_iterator_wk(
    recv: AnyValue,
    wk_idx: i64,
    not_a_function: &core::ffi::CStr,
    nullish_missing: bool,
) -> GetIterator {
    unsafe {
        let sym = __torajs_symbol_well_known(wk_idx);
        if sym.is_null() {
            return GetIterator::NoUserMethod;
        }
        // GetMethod is a real Get, so an ACCESSOR-shaped `@@iterator`
        // runs its getter. The borrow-shaped pair answers the sentinel
        // instead, and this lane read it as a VALUE: neither undefined
        // nor null, so it slipped past the gate below into
        // `callable_entry`, which boxed it into something no closure
        // lookup recognises — every accessor spelling threw
        // "Symbol.iterator is not a function" with the getter never
        // running (RFC 20260828 knife 3b).
        let (tag, payload, owned) = crate::member_get_symbol::symbol_key_get(recv, sym);
        let _ = __torajs_rc_dec(sym);
        // A getter that threw answers undefined with the throw pending;
        // the gate below would read that as "no user method" and ride
        // the builtin lane with the throw still in flight.
        if __torajs_throw_check() != 0 {
            return GetIterator::Threw;
        }
        if tag == TAG_UNDEF || (nullish_missing && tag == TAG_NULL) {
            // Nullish carries no cell, so `owned` needs no release.
            return GetIterator::NoUserMethod;
        }
        // A getter-produced method is ours across all four exits below,
        // and the invoke's `env` aliases its cell — so the dispatch
        // moves into its own fn and this frame owns the one release.
        let out = iterator_from_method(recv, tag, payload, not_a_function);
        if owned {
            __torajs_anyv_rc_dec(__torajs_anyv_box_from_pair(tag as i64, payload as i64));
        }
        out
    }
}

/// The GetMethod-verdict-then-call half of [`get_iterator_wk`], split
/// out so its caller can release a getter-produced method across every
/// exit exactly once (`index_any_method_call`'s
/// `call_resolved_symbol_method` is the same shape).
///
/// # Safety
/// `recv` is a live AnyValue; `(tag, payload)` is a live member pair.
unsafe fn iterator_from_method(
    recv: AnyValue,
    tag: u64,
    payload: u64,
    not_a_function: &core::ffi::CStr,
) -> GetIterator {
    unsafe {
        // F0 follow-up (RFC 20260728-gen-forof-yieldstar) — the
        // symbol walk now ends in a builtin `@@iterator` reify for
        // native iterable tags, which used to be a miss. That cell
        // IS the aliased prototype method whose semantics the
        // builtin lane implements, so answer the fast-lane verdict;
        // a USER override still wins because the dict probes run
        // before the reify. Without this arm the reified cell rode
        // the user-method invoke and bare-entry-threw (the 27-fail
        // gate on the F0 commit).
        if tag == 4 && payload != 0 {
            let cell = payload as *mut core::ffi::c_void;
            let ct = (cell as *const u8).add(4).cast::<u16>().read();
            if ct == torajs_rc::Tag::Closure as u16
                && crate::method_value::builtin_method_mid(cell).is_some()
            {
                return GetIterator::NoUserMethod;
            }
        }
        // §7.4.2 step 3 — present but not callable is a TypeError, not
        // a fallback: `o[Symbol.iterator] = 5` must throw rather than
        // quietly iterating `o` some other way. A nullish one is the
        // same refusal here, since GetIterator has no other source.
        let Some((env, entry)) = callable_entry(tag, payload, not_a_function) else {
            if __torajs_throw_check() == 0 {
                __torajs_throw_type_error(c"value is not iterable".as_ptr());
            }
            return GetIterator::Threw;
        };
        let argv: [u64; 0] = [];
        let iter = invoke_with_this(env, entry, recv, argv.as_ptr(), 0);
        if __torajs_throw_check() != 0 {
            return GetIterator::Threw;
        }
        if !is_cell(iter) {
            __torajs_anyv_rc_dec(iter);
            __torajs_throw_type_error(c"iterator is not an object".as_ptr());
            return GetIterator::Threw;
        }
        GetIterator::Iterator(iter)
    }
}

/// One step of an iterator that is NOT a class instance — §7.4.6
/// IteratorNext through the `any` method dispatcher, then §7.4.4
/// IteratorComplete / §7.4.5 IteratorValue as ordinary member reads.
///
/// Returns 1 with `*out` owned, or 0 with `*out` undefined (done, or
/// a throw left in flight for the caller's check).
///
/// `await_mode` (§14.7.5.6 async-iterate): a `next()` that answered
/// a Promise is awaited before §7.4.4 reads the result; a sync
/// result's VALUE is awaited instead, per the §27.1.4.4
/// Async-from-Sync wrapper this lane stands in for.
///
/// # Safety
/// `iter` is a live cell; `out` is a valid writable pointer.
pub(crate) unsafe fn generic_iter_step(
    iter: AnyValue,
    out: *mut AnyValue,
    await_mode: bool,
) -> i64 {
    unsafe {
        *out = VALUE_UNDEFINED;
        let name_next = interned(&NAME_NEXT, b"next");
        let argv: [u64; 0] = [];
        let mut step = __torajs_any_method_call(
            iter,
            ANY_METHOD_NEXT,
            name_next,
            4,
            core::ptr::null_mut(),
            argv.as_ptr(),
            0,
        );
        // The dispatcher's own TypeError (no `next`, not callable) is
        // already recorded; a user `next()` that threw is too.
        if __torajs_throw_check() != 0 {
            return 0;
        }
        let was_async = await_mode && crate::iter_any_await::await_settle(&mut step);
        if was_async && __torajs_throw_check() != 0 {
            __torajs_anyv_rc_dec(step);
            return 0;
        }
        if !is_cell(step) {
            __torajs_anyv_rc_dec(step);
            __torajs_throw_type_error(c"iterator result is not an object".as_ptr());
            return 0;
        }
        // §7.4.4 / §7.4.5 — the shared IteratorResult [[Get]]
        // (struct-probe fast lane, accessor-aware dispatcher lane).
        // The old inline reads went through the member dispatcher but
        // treated an ANY_ACCESSOR answer as a plain data pair — the
        // getter never fired, so a poisoned `value` getter (test262's
        // iter-val-err family) never threw and `done` never turned
        // true: an infinite loop where the spec owes an abrupt exit.
        let step_ptr = as_void_ptr(step) as *mut c_void;
        let done = match crate::iter_any_result::iter_result_get(step, step_ptr, b"done") {
            None => {
                __torajs_anyv_rc_dec(step);
                return 0;
            }
            Some(v) => {
                let b = __torajs_anyv_to_bool(v);
                __torajs_anyv_rc_dec(v);
                b
            }
        };
        if done || __torajs_throw_check() != 0 {
            __torajs_anyv_rc_dec(step);
            return 0;
        }
        let value = match crate::iter_any_result::iter_result_get(step, step_ptr, b"value") {
            None => {
                __torajs_anyv_rc_dec(step);
                return 0;
            }
            Some(v) => v,
        };
        __torajs_anyv_rc_dec(step);
        *out = value;
        if await_mode && !was_async {
            return crate::iter_any_await::settle_out(out);
        }
        1
    }
}

/// §7.4.9 IteratorClose for an iterator whose `return` is an ordinary
/// property rather than a vtable method.
///
/// The optional-call flavor of the dispatcher is exactly the spec's
/// step 4: an iterator with no `return` closes silently, while one
/// whose `return` resolves to a non-callable still throws. A `return`
/// that throws leaves its throw in flight — on a normal completion
/// §7.4.9 step 6 propagates it, and touching the result would be
/// reading an aborted call's sentinel.
///
/// # Safety
/// `iter` is a live cell.
pub(crate) unsafe fn generic_iter_close(iter: AnyValue) {
    unsafe {
        let key = interned(&NAME_RETURN, b"return") as *const c_void;
        let tag = __torajs_any_member_get_tag(iter, key);
        let payload = if tag == TAG_UNDEF {
            0
        } else {
            __torajs_any_member_get_value(iter, key)
        };
        // §7.4.9 step 4 — GetMethod answering undefined ends the
        // close; an iterator without a `return` is closed already.
        let Some((env, entry)) = callable_entry(tag, payload, c"return is not a function") else {
            return;
        };
        let argv: [u64; 0] = [];
        let result = invoke_with_this(env, entry, iter, argv.as_ptr(), 0);
        if __torajs_throw_check() == 0 {
            // §7.4.6 step 9 — non-Object answer → TypeError (the
            // shared reject; original-throw precedence is the abrupt
            // wrapper's job, same as the Tag::Obj arm).
            crate::iter_any_close::reject_non_object_close_result(result);
        }
    }
}
