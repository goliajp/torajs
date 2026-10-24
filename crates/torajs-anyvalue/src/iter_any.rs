//! `__torajs_any_iter_next` — the unified for-of iteration protocol
//! over an `any` receiver (Any-dynamic-access RFC 20260704 S5+).
//!
//! One runtime call per iteration replaces the old two-phase shape
//! (hoisted `__torajs_any_iter_len` + per-iter `recv[i]`), extending
//! `for (x of recv)` from indexed receivers (strings / arrays) to
//! the stateful iterator cells the C4+ method-call surface mints
//! (`m.keys()` / `m.values()` / `m.entries()` on an `any` Map/Set,
//! `arr.values()` boxed into `any`).
//!
//! Dispatch tree:
//! - **Arrays** (`Tag::Arr`, a TypedArray) — indexed tier.
//!   `*idx_slot` is the cursor; the element read reuses
//!   [`__torajs_any_index_get`], the bound re-reads
//!   [`__torajs_any_iter_len`] every step (ES §23.1.5.1
//!   ArrayIterator re-reads length live; mid-loop pushes are
//!   visited).
//! - **Strings** (ShortStr immediate, `Tag::Str`, a String wrapper)
//!   — §22.1.5.1: step per CODE POINT. The receiver is flattened
//!   into `iter_slot` once (an owned plain Str the caller releases
//!   with the slot), `*idx_slot` is the code-unit cursor and each
//!   step answers `__torajs_str_code_point_str_at` — one unit, or a
//!   surrogate pair as one two-unit Str. Until RFC 20260915 this lane
//!   read one code unit per step (`"a😀b"` iterated as four).
//! - **`Tag::Map` / `Tag::Set`** — the collection itself, not one of
//!   its iterator cells. ES §23.1.4 / §24.2.5.1: a Map's default
//!   iterator is `entries()`, a Set's is `values()`. Mint it once
//!   into `iter_slot` and step it through the MapIter lane below.
//! - **`Tag::MapIter` / `Tag::ArrIter`** — the cell carries its own
//!   cursor; route through the `*_iter_step` kernels. Step payloads
//!   arrive OWNED (each kernel says so per arm) — the box adopts
//!   them, no inc (rotation 543's `next()`-twin ledger; 548-01).
//! - **`Tag::Obj`** — a class instance (a generator object, or any
//!   user class declaring `[Symbol.iterator]()`). ES §7.4.3
//!   GetIterator: call the receiver's `[Symbol.iterator]()` once,
//!   then step the returned iterator's `next()` per iteration,
//!   reading `done` / `value` off the IteratorResult struct it
//!   answers. Both calls go through the class-methods dispatch table
//!   (torajs-structmeta `__torajs_struct_method_find`) and the boxed
//!   `(this-as-env, argv, argc)` adapter ABI. The iterator itself
//!   lives in `iter_slot` across the loop — re-deriving it per step
//!   would restart a fresh-iterator iterable — and the CALLER owns
//!   that reference: it releases the slot at loop exit, which is
//!   also what makes a `break` release it.
//! - anything else — catchable TypeError (ES §7.4.3 GetIterator on
//!   a non-iterable), returns 0 so the loop body never runs.
//!
//! Return contract: 1 = `*out` holds an owned AnyValue (heap cells
//! +1, released by the loop var's scope drop); 0 = done, `*out` is
//! `undefined`.

use crate::iter_any_await::AwaitMode;
use core::ffi::c_void;

use crate::index_any::__torajs_any_index_get;
use crate::index_any_iter_len::__torajs_any_iter_len;
use crate::iter_any_call::{MethodOutcome, call_obj_method_0};
use crate::iter_any_step::step_derived_iterator;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_int32, is_short_str};
use crate::nanbox_encode::__torajs_anyv_box_from_pair;
use torajs_rc::{AnySlotTag, Tag};

unsafe extern "C" {
    /// torajs-collections — MapIter cursor advance. The out pair is
    /// OWNED (cells +1'd by the kernel, ENTRIES pairs fresh at rc 1).
    fn __torajs_map_iter_step(p: *mut c_void, out_tag: *mut i64, out_payload: *mut i64) -> i64;
    /// torajs-arr — ArrIter cursor advance (same OWNED out contract).
    fn __torajs_arr_iter_step(p: *mut c_void, out_tag: *mut i64, out_payload: *mut i64) -> i64;
    /// torajs-collections — mint an ENTRIES-kind MapIter (a Map's
    /// default iterator). Answers a fresh cell at rc 1 and rc_inc's
    /// the source Map.
    fn __torajs_map_iter_create_entries(map_p: *mut c_void) -> *mut c_void;
    /// torajs-collections — mint a KEYS-kind MapIter (a Set's default
    /// iterator; Set shares the Map layout and parks `undefined` in
    /// every value slot, so keys == values). Same rc contract.
    fn __torajs_map_iter_create_keys(map_p: *mut c_void) -> *mut c_void;
    /// torajs-throw — record a pending catchable TypeError; returns
    /// normally (caller's throw-check propagates).
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-structmeta — read side over `__torajs_class_layouts`
    /// (NULL for class_tag 0 / past the table).
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    /// torajs-structmeta — class-method boxed adapter by name bytes
    /// (NULL miss).
    fn __torajs_struct_method_find(
        layout: *const c_void,
        name: *const u8,
        name_len: u32,
    ) -> *const c_void;
}

/// The desugared name of a `[Symbol.iterator]()` class member (the
/// parser mangles the computed key; torajs-core emits
/// `__cm_<C>____sym_Symbol_iterator__`). Knife 4 of RFC
/// 20260725-getiterator-getmethod retires this: once a class member
/// keyed by a symbol lands in the symbol-key domain, the real
/// GetIterator finds it and this lane goes away.
pub(crate) const SYM_ITERATOR_METHOD: &[u8] = b"__sym_Symbol_iterator__";
/// D4 — the `[Symbol.asyncIterator]()` twin an async generator's class
/// carries instead (`desugar_generators_class::self_iter_method`): the
/// async drive probes it first, the sync probe never sees it.
pub(crate) const SYM_ASYNC_ITERATOR_METHOD: &[u8] = b"__sym_Symbol_asyncIterator__";

/// `idx_slot` marker for "this loop is being driven by the
/// `@@iterator` GetIterator resolved". The indexed lane only ever
/// stores a cursor ≥ 0, so a negative value is free — and parking the
/// marker in `idx_slot` rather than `iter_slot` leaves the latter
/// holding exactly one thing: the caller-owned iterator it releases at
/// loop exit.
pub(crate) const USER_ITERATOR_LANE: i64 = -1;
/// The same for the `@@asyncIterator` an async drive's GetIterator
/// resolved: its close awaits what `return()` answers (§7.4.11), where
/// a sync iterator's goes through the Async-from-Sync wrapper.
pub(crate) const USER_ASYNC_ITERATOR_LANE: i64 = -2;
/// The loop's GetIterator threw: a throw out of the step is then not one
/// of `next()`'s, which the Async-from-Sync wrapper would reject with.
pub(crate) const OPEN_THREW_LANE: i64 = -3;

/// The `Tag::Map` / `Tag::Set` lane of [`__torajs_any_iter_next`] —
/// derive the collection's default iterator once and answer it, so the
/// MapIter lane steps it from then on. The mint answers a cell at rc 1
/// and `box_from_pair` adds no stake of its own, so the box in
/// `iter_slot` IS that single reference — the caller's to release at
/// loop exit, exactly like the class-instance lane's.
///
/// # Safety
/// `recv` is a live `Tag::Map` / `Tag::Set` heap cell; `iter_slot` is a
/// valid writable pointer holding `undefined` or a previously derived
/// MapIter box.
unsafe fn map_set_derive_iter(recv: AnyValue, is_map: bool, iter_slot: *mut AnyValue) -> AnyValue {
    unsafe {
        if *iter_slot == VALUE_UNDEFINED {
            let src = as_void_ptr(recv);
            let it = if is_map {
                __torajs_map_iter_create_entries(src)
            } else {
                __torajs_map_iter_create_keys(src)
            };
            *iter_slot = __torajs_anyv_box_from_pair(AnySlotTag::Heap as i64, it as i64);
        }
        *iter_slot
    }
}

/// The `Tag::Obj` lane of [`__torajs_any_iter_next`] — see the module
/// doc's dispatch tree. Answers `Some(live)` once the receiver is a
/// class instance (including the non-iterable TypeError, which is
/// `Some(0)`); `None` when it is not a class instance at all, so the
/// caller falls through to its own TypeError.
///
/// # Safety
/// `obj` is a live `Tag::Obj` heap pointer; `iter_slot` / `out` are
/// valid writable pointers.
unsafe fn obj_iter_step(
    obj: *mut c_void,
    idx_slot: *mut i64,
    iter_slot: *mut AnyValue,
    out: *mut AnyValue,
    mode: AwaitMode,
) -> Option<i64> {
    unsafe {
        // GetIterator, once per loop: the cached iterator is the
        // caller's to release, so a re-entry with a live slot skips
        // straight to the step. A class instance still finds its
        // iterator by mangled vtable name — the class-side folding is
        // knife 4's to retire; by here the real `@@iterator` lookup
        // has already missed.
        if *iter_slot == VALUE_UNDEFINED {
            // §7.4.2 hint=async step 2.a — the async drive asks for
            // `@@asyncIterator` first (an async generator object has
            // only that one), then the sync symbol.
            let outcome = if mode.is_async() {
                match call_obj_method_0(obj, SYM_ASYNC_ITERATOR_METHOD) {
                    MethodOutcome::Missing => call_obj_method_0(obj, SYM_ITERATOR_METHOD),
                    found => {
                        *idx_slot = USER_ASYNC_ITERATOR_LANE;
                        found
                    }
                }
            } else {
                call_obj_method_0(obj, SYM_ITERATOR_METHOD)
            };
            match outcome {
                MethodOutcome::Ok(iter) => *iter_slot = iter,
                // No iterator anywhere on this object. That verdict is
                // not automatically an error — §23.1.2.1 step 3 hands
                // `Array.from` to the array-like walk on exactly this
                // answer — so it goes back to the single tail below,
                // which is where the two callers differ.
                MethodOutcome::Missing => return None,
                // The user's throw is already in flight — say done and
                // let the caller's throw-check forward it.
                MethodOutcome::Threw => return Some(open_threw(idx_slot, out)),
            }
        }
        Some(step_derived_iterator(
            *iter_slot,
            out,
            mode.for_lane(*idx_slot),
        ))
    }
}

/// See module doc.
///
/// # Safety
/// Cell receivers must be valid heap pointers matching their header
/// tag layout; `idx_slot` / `iter_slot` / `out` are valid writable
/// pointers. `iter_slot` starts at `undefined` and belongs to the
/// caller, which releases it when the loop exits.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_iter_next(
    recv: AnyValue,
    idx_slot: *mut i64,
    iter_slot: *mut AnyValue,
    out: *mut AnyValue,
) -> i64 {
    unsafe { iter_next_inner(recv, idx_slot, iter_slot, out, false, AwaitMode::Sync) }
}

/// `Array.from`'s entry to the same walk. §23.1.2.1 step 3 splits on
/// whether the source has an iterator: it does not throw on the `no`
/// side, it walks `length` and the index keys instead. Only this entry
/// takes that side — `[...x]` on an array-like still has to throw, so
/// it keeps the plain entry above.
///
/// Once the array-like lane is entered it stays entered: it parks the
/// length in `iter_slot`, and an immediate integer there is a shape no
/// other lane produces (they hold `undefined` or an iterator cell), so
/// re-entry is told apart without spending a marker.
///
/// # Safety
/// As [`__torajs_any_iter_next`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_iter_next_array_like(
    recv: AnyValue,
    idx_slot: *mut i64,
    iter_slot: *mut AnyValue,
    out: *mut AnyValue,
) -> i64 {
    unsafe {
        if is_int32(*iter_slot) {
            return crate::iter_any_array_like::step(
                recv,
                idx_slot,
                iter_slot,
                out,
                AwaitMode::Sync,
            );
        }
        iter_next_inner(recv, idx_slot, iter_slot, out, true, AwaitMode::Sync)
    }
}

/// The shared cascade. `array_like_fallback` decides only what the
/// tail does when nothing in it claimed the receiver; an async `mode`
/// is the §14.7.5.6 async-iterate drive (`for await`). `Array.from`
/// sets only the first, `for await` only the second, and
/// `Array.fromAsync` both — proposal-array-from-async §2.1.1 asks
/// `@@asyncIterator` first AND keeps the array-like tail.
///
/// # Safety
/// As [`__torajs_any_iter_next`].
pub(crate) unsafe fn iter_next_inner(
    recv: AnyValue,
    idx_slot: *mut i64,
    iter_slot: *mut AnyValue,
    out: *mut AnyValue,
    array_like_fallback: bool,
    mode: AwaitMode,
) -> i64 {
    // §7.4.2 GetIterator — a real `@@iterator` the receiver owns or
    // inherits OUTRANKS every builtin lane below, which is what makes
    // `arr[Symbol.iterator] = …` mean anything.
    //
    // It is a single spec step, so it runs once per loop, not once per
    // iteration: probing per step would put a symbol-key walk in the
    // array / string hot path. Both slots are pristine only on the
    // first step — the indexed lane advances `idx_slot`, every derived
    // lane fills `iter_slot` — and `idx_slot == USER_ITERATOR_LANE`
    // records that this loop belongs to the method GetIterator found.
    unsafe {
        if *idx_slot == USER_ITERATOR_LANE || *idx_slot == USER_ASYNC_ITERATOR_LANE {
            return step_derived_iterator(*iter_slot, out, mode.for_lane(*idx_slot));
        }
        if *idx_slot == 0 && *iter_slot == VALUE_UNDEFINED {
            // §7.4.2 hint=async step 2.a — `for await` asks for
            // `@@asyncIterator` first; a receiver without one falls to
            // the sync `@@iterator` below and rides the
            // Async-from-Sync value-await in the step tier.
            if mode.is_async() {
                match crate::iter_any_get_method::get_async_iterator(recv) {
                    crate::iter_any_get_method::GetIterator::Iterator(iter) => {
                        *iter_slot = iter;
                        *idx_slot = USER_ASYNC_ITERATOR_LANE;
                        return step_derived_iterator(iter, out, mode.for_lane(*idx_slot));
                    }
                    crate::iter_any_get_method::GetIterator::NoUserMethod
                    | crate::iter_any_get_method::GetIterator::Absent => {}
                    crate::iter_any_get_method::GetIterator::Threw => {
                        return open_threw(idx_slot, out);
                    }
                }
            }
            match crate::iter_any_get_method::get_iterator(recv) {
                crate::iter_any_get_method::GetIterator::Iterator(iter) => {
                    *iter_slot = iter;
                    *idx_slot = USER_ITERATOR_LANE;
                    return step_derived_iterator(iter, out, mode);
                }
                crate::iter_any_get_method::GetIterator::NoUserMethod => {}
                // The builtin lane's own `@@iterator` was deleted:
                // §7.4.2 has nothing to call, and only the array-like
                // consumers (`Array.from` and friends) have a step 3
                // to fall back on.
                crate::iter_any_get_method::GetIterator::Absent => {
                    if !array_like_fallback {
                        __torajs_throw_type_error(c"value is not iterable".as_ptr());
                        return open_threw(idx_slot, out);
                    }
                }
                crate::iter_any_get_method::GetIterator::Threw => return open_threw(idx_slot, out),
            }
        }
    }
    let cell_tag = if is_cell(recv) {
        Some(unsafe { (as_void_ptr(recv).cast::<u8>().add(4) as *const u16).read() })
    } else {
        None
    };
    // §23.1.5.1 over a Proxy: `Array.prototype[Symbol.iterator]` is
    // what the GetIterator above resolved (a builtin cell, hence
    // `NoUserMethod`), and running it with the proxy as `this` is an
    // ArrayIterator whose [[IteratedObject]] is the proxy — every
    // step a `length` read and an index read, both through the traps.
    // A proxy over a shape with a different builtin lane (a Map) has
    // no such reading, and falls through to this fn's TypeError.
    if matches!(cell_tag, Some(t) if t == Tag::Proxy as u16)
        && unsafe { crate::iter_any_array_like::proxy_wraps_indexed(recv) }
    {
        return unsafe { crate::iter_any_array_like::step_uncached(recv, idx_slot, out, mode) };
    }
    // A String wrapper iterates its inner [[StringData]] (RFC
    // 20260716 刀 12; `__torajs_anyv_to_str` views through the
    // wrapper cell). NumberWrapper / BooleanWrapper stay non-iterable
    // (a `for..of` on those throws per spec, matching bun).
    if is_short_str(recv)
        || matches!(cell_tag, Some(t) if t == Tag::Str as u16 || t == Tag::StringWrapper as u16)
    {
        return unsafe {
            crate::iter_any_string::string_step(recv, idx_slot, iter_slot, out, mode)
        };
    }
    // A TypedArray joins the indexed lane rather than deriving a
    // cursor: §23.2.5.1's `values` IS the Array Iterator, and the
    // pair this lane already uses answers both of its questions —
    // `__torajs_any_iter_len` re-validates per step (a detach
    // mid-loop throws), `__torajs_any_index_get` is §10.4.5.
    let indexed =
        matches!(cell_tag, Some(t) if t == Tag::Arr as u16 || t == Tag::TypedArray as u16);
    if indexed {
        unsafe {
            let idx = *idx_slot;
            if idx >= __torajs_any_iter_len(recv) {
                *out = VALUE_UNDEFINED;
                return 0;
            }
            *idx_slot = idx + 1;
            *out = __torajs_any_index_get(recv, idx);
            // §27.1.4.4 — a sync lane's value is awaited under
            // `for await` (a Promise element unwraps to its settled
            // value; a rejection forwards).
            if mode.is_async() {
                return crate::iter_any_await::settle_out(out, mode);
            }
        }
        return 1;
    }
    // A Map / Set receiver is not itself a cursor — derive its default
    // iterator into `iter_slot` on the first step and route every step
    // (this one included) through the MapIter lane below.
    let mut recv = recv;
    let mut cell_tag = cell_tag;
    if matches!(cell_tag, Some(t) if t == Tag::Map as u16 || t == Tag::Set as u16) {
        let is_map = cell_tag == Some(Tag::Map as u16);
        recv = unsafe { map_set_derive_iter(recv, is_map, iter_slot) };
        cell_tag = Some(Tag::MapIter as u16);
    }
    // An Iterator Helper cell steps itself (RFC 20260730 刀 2) —
    // the shared core the next() method face wraps; owned-out
    // contract matches this fn's directly.
    if matches!(cell_tag, Some(t) if t == Tag::IterHelper as u16) {
        let hit = unsafe { crate::iter_helper::iter_helper_step(as_void_ptr(recv), out) };
        if hit != 0 && mode.is_async() {
            return unsafe { crate::iter_any_await::settle_out(out, mode) };
        }
        return hit;
    }
    type StepFn = unsafe extern "C" fn(*mut c_void, *mut i64, *mut i64) -> i64;
    let step: Option<StepFn> = match cell_tag {
        Some(t) if t == Tag::MapIter as u16 => Some(__torajs_map_iter_step),
        Some(t) if t == Tag::ArrIter as u16 => Some(__torajs_arr_iter_step),
        _ => None,
    };
    if let Some(step_fn) = step {
        let mut tag = 0i64;
        let mut payload = 0i64;
        unsafe {
            if step_fn(as_void_ptr(recv), &mut tag, &mut payload) == 0 {
                *out = VALUE_UNDEFINED;
                return 0;
            }
            // Step payloads arrive OWNED (each kernel's arms say
            // so); rotation 543 deleted this same surplus inc from
            // the `next()` twins in method_call_mapset — this third
            // consumer kept it and double-staked every yielded heap
            // value (548-01: one pair array leaked per entries step).
            *out = __torajs_anyv_box_from_pair(tag, payload);
            // Sync lane under `for await` — same §27.1.4.4 value
            // await as the indexed lane above.
            if mode.is_async() {
                return crate::iter_any_await::settle_out(out, mode);
            }
        }
        return 1;
    }
    if matches!(cell_tag, Some(t) if t == Tag::Obj as u16) {
        let obj = as_void_ptr(recv);
        if let Some(live) = unsafe { obj_iter_step(obj, idx_slot, iter_slot, out, mode) } {
            return live;
        }
    }
    // Nothing claimed the receiver. §23.1.2.1 step 3's array-like walk
    // reads `length` off whatever this is (a number answers an empty
    // walk, which is what `Array.from(5)` means); every other consumer
    // of the protocol says the value is not iterable.
    if array_like_fallback {
        return unsafe { crate::iter_any_array_like::step(recv, idx_slot, iter_slot, out, mode) };
    }
    unsafe {
        __torajs_throw_type_error(c"value is not iterable".as_ptr());
        open_threw(idx_slot, out)
    }
}

/// GetIterator threw, or found nothing to iterate: the step answers 0
/// with the throw in flight and marks the cursor [`OPEN_THREW_LANE`].
///
/// # Safety
/// `idx_slot` / `out` are valid writable pointers.
unsafe fn open_threw(idx_slot: *mut i64, out: *mut AnyValue) -> i64 {
    unsafe {
        *idx_slot = OPEN_THREW_LANE;
        *out = VALUE_UNDEFINED;
    }
    0
}
