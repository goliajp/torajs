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
//! - **Strings / arrays** (ShortStr immediate, `Tag::Str`,
//!   `Tag::Arr`) — indexed tier. `*idx_slot` is the cursor; the
//!   element read reuses [`__torajs_any_index_get`], the bound
//!   re-reads [`__torajs_any_iter_len`] every step (ES §23.1.5.1
//!   ArrayIterator re-reads length live; mid-loop pushes are
//!   visited). Strings step per UTF-16 code unit — same documented
//!   deviation from per-code-point iteration as the RFC's S5 note.
//! - **`Tag::Map` / `Tag::Set`** — the collection itself, not one of
//!   its iterator cells. ES §23.1.4 / §24.2.5.1: a Map's default
//!   iterator is `entries()`, a Set's is `values()`. Mint it once
//!   into `iter_slot` and step it through the MapIter lane below.
//! - **`Tag::MapIter` / `Tag::ArrIter`** — the cell carries its own
//!   cursor; route through the `*_iter_step` kernels. Step payloads
//!   come out borrowed (ENTRIES pair arrays pre-decremented to 0),
//!   so `payload_rc_inc` converts to owned before boxing — the same
//!   ledger the C4+ `next()` arm uses.
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

use core::ffi::c_void;

use crate::index_any::{__torajs_any_index_get, __torajs_any_iter_len};
use crate::method_call::invoke_boxed;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_int32, is_short_str};
use crate::nanbox_encode::__torajs_anyv_box_from_pair;
use crate::nanbox_ffi::__torajs_anyv_rc_dec;
use crate::payload_rc_inc;
use torajs_rc::{AnySlotTag, Tag};

unsafe extern "C" {
    /// torajs-collections — MapIter cursor advance (out pair is a
    /// borrow; ENTRIES pair arrays come back pre-decremented to 0).
    fn __torajs_map_iter_step(p: *mut c_void, out_tag: *mut i64, out_payload: *mut i64) -> i64;
    /// torajs-arr — ArrIter cursor advance (same out-pair contract).
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
    /// torajs-throw — non-zero iff a throw is in flight. Every method
    /// this module invokes runs USER code (a generator body, a custom
    /// `next()`), so each call has to be checked before its result is
    /// touched.
    fn __torajs_throw_check() -> i64;
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

/// `class_tag` u32 offset inside a `Tag::Obj` instance — mirror of
/// torajs-core `ssa_lower::OBJ_CLASS_TAG_OFF`.
const OBJ_CLASS_TAG_OFF: usize = 8;

/// The desugared name of a `[Symbol.iterator]()` class member (the
/// parser mangles the computed key; torajs-core emits
/// `__cm_<C>____sym_Symbol_iterator__`). Knife 4 of RFC
/// 20260725-getiterator-getmethod retires this: once a class member
/// keyed by a symbol lands in the symbol-key domain, the real
/// GetIterator finds it and this lane goes away.
pub(crate) const SYM_ITERATOR_METHOD: &[u8] = b"__sym_Symbol_iterator__";

/// `idx_slot` marker for "this loop is being driven by the
/// `@@iterator` GetIterator resolved". The indexed lane only ever
/// stores a cursor ≥ 0, so a negative value is free — and parking the
/// marker in `idx_slot` rather than `iter_slot` leaves the latter
/// holding exactly one thing: the caller-owned iterator it releases at
/// loop exit.
const USER_ITERATOR_LANE: i64 = -1;

/// What came back from [`call_obj_method_0`].
pub(crate) enum MethodOutcome {
    /// The receiver has no layout, or declares no method by that name.
    Missing,
    /// The method ran and threw. Its "result" is the sentinel an
    /// aborted fn returns — NOT a value: it must not be read, released,
    /// or handed on. The pending throw is left in flight for the
    /// caller's own throw-check to propagate.
    Threw,
    Ok(AnyValue),
}

/// Invoke a zero-argument method on a `Tag::Obj` receiver through the
/// class-methods dispatch table.
///
/// Every method reached from this module is USER code — a generator
/// body, a hand-written `next()` — so it can throw, and ES §7.4.6
/// IteratorNext forwards that abrupt completion rather than looking at
/// a result. Reading the sentinel as an IteratorResult instead is a
/// wild deref: `for (const v of gen)` over a generator that throws on
/// its first step was a SIGSEGV, and the destructuring lanes silently
/// clobbered the thrown Error (`e.message` came back empty).
///
/// # Safety
/// `obj` is a live `Tag::Obj` heap pointer.
pub(crate) unsafe fn call_obj_method_0(obj: *mut c_void, name: &[u8]) -> MethodOutcome {
    let class_tag = unsafe { obj.cast::<u8>().add(OBJ_CLASS_TAG_OFF).cast::<u32>().read() };
    let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
    if layout.is_null() {
        return MethodOutcome::Missing;
    }
    let adapter = unsafe { __torajs_struct_method_find(layout, name.as_ptr(), name.len() as u32) };
    if adapter.is_null() {
        return MethodOutcome::Missing;
    }
    // argc 0 — invoke_boxed hands the adapter its undefined-filled
    // argv buffer, so a defaulted param (a generator `next`'s
    // `__yield_arg`) materializes its own default.
    let argv: [u64; 0] = [];
    let result = unsafe { invoke_boxed(obj, adapter as u64, argv.as_ptr(), 0) };
    if unsafe { __torajs_throw_check() } != 0 {
        return MethodOutcome::Threw;
    }
    MethodOutcome::Ok(result)
}

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
            let src = as_void_ptr(recv) as *mut c_void;
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
    iter_slot: *mut AnyValue,
    out: *mut AnyValue,
) -> Option<i64> {
    unsafe {
        // GetIterator, once per loop: the cached iterator is the
        // caller's to release, so a re-entry with a live slot skips
        // straight to the step. A class instance still finds its
        // iterator by mangled vtable name — the class-side folding is
        // knife 4's to retire; by here the real `@@iterator` lookup
        // has already missed.
        if *iter_slot == VALUE_UNDEFINED {
            match call_obj_method_0(obj, SYM_ITERATOR_METHOD) {
                MethodOutcome::Ok(iter) => *iter_slot = iter,
                // No iterator anywhere on this object. That verdict is
                // not automatically an error — §23.1.2.1 step 3 hands
                // `Array.from` to the array-like walk on exactly this
                // answer — so it goes back to the single tail below,
                // which is where the two callers differ.
                MethodOutcome::Missing => return None,
                // The user's throw is already in flight — say done and
                // let the caller's throw-check forward it.
                MethodOutcome::Threw => {
                    *out = VALUE_UNDEFINED;
                    return Some(0);
                }
            }
        }
        Some(step_derived_iterator(*iter_slot, out))
    }
}

/// One step of an already-derived iterator, whichever way it was
/// found — §7.4.6 IteratorNext plus §7.4.4/§7.4.5 on the result.
///
/// The tier split is by how `next` DISPATCHES, and `Tag::Obj` is not
/// the answer to that: an anon-stamped object literal wears the same
/// tag as a class instance, but its `next` is a closure-valued FIELD,
/// not a vtable method. So the vtable is tried first and a miss falls
/// through to the generic tier rather than becoming an error — the
/// same entry-probe-then-by-name shape the member cascades use. A
/// generator object and a hand-written iterator class keep the exact
/// path they had.
///
/// # Safety
/// `iter` is the caller's live iterator box; `out` is writable.
pub(crate) unsafe fn step_derived_iterator(iter: AnyValue, out: *mut AnyValue) -> i64 {
    unsafe {
        if !is_cell(iter) {
            __torajs_throw_type_error(c"iterator is not an object".as_ptr());
            *out = VALUE_UNDEFINED;
            return 0;
        }
        let iter_ptr = as_void_ptr(iter) as *mut c_void;
        if (iter_ptr.cast::<u8>().add(4) as *const u16).read() != Tag::Obj as u16 {
            return crate::iter_any_get_method::generic_iter_step(iter, out);
        }
        // ES §7.4.6 IteratorNext — a step that throws forwards the
        // abrupt completion; there is no result to inspect.
        let step = match call_obj_method_0(iter_ptr, b"next") {
            MethodOutcome::Ok(step) => step,
            // Not a vtable method — an object literal keeps `next` as
            // a closure-valued field. Same iterator, other tier; the
            // "no next()" diagnostic belongs to that tier's dispatcher.
            MethodOutcome::Missing => {
                return crate::iter_any_get_method::generic_iter_step(iter, out);
            }
            MethodOutcome::Threw => {
                *out = VALUE_UNDEFINED;
                return 0;
            }
        };
        if !is_cell(step) {
            __torajs_anyv_rc_dec(step);
            __torajs_throw_type_error(c"iterator result is not an object".as_ptr());
            *out = VALUE_UNDEFINED;
            return 0;
        }
        let step_ptr = as_void_ptr(step) as *mut c_void;
        // §7.4.4 IteratorComplete — ToBoolean(Get(step, "done")). The
        // Get may run a getter that throws (forward it); the answer is
        // truthiness, not a Bool-tag check (`{done: 1}` is done).
        let done = match crate::iter_any_result::iter_result_get(step, step_ptr, b"done") {
            None => {
                __torajs_anyv_rc_dec(step);
                *out = VALUE_UNDEFINED;
                return 0;
            }
            Some(v) => {
                let b = crate::nanbox_ffi::__torajs_anyv_to_bool(v);
                __torajs_anyv_rc_dec(v);
                b
            }
        };
        if done {
            __torajs_anyv_rc_dec(step);
            *out = VALUE_UNDEFINED;
            return 0;
        }
        // §7.4.5 IteratorValue — Get(step, "value"), abrupt forwarded
        // (the test262 poisoned-getter shape: the ONLY loop exit).
        let value = match crate::iter_any_result::iter_result_get(step, step_ptr, b"value") {
            None => {
                __torajs_anyv_rc_dec(step);
                *out = VALUE_UNDEFINED;
                return 0;
            }
            Some(v) => v,
        };
        __torajs_anyv_rc_dec(step);
        *out = value;
        1
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
    unsafe { iter_next_inner(recv, idx_slot, iter_slot, out, false) }
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
            return crate::iter_any_array_like::step(recv, idx_slot, iter_slot, out);
        }
        iter_next_inner(recv, idx_slot, iter_slot, out, true)
    }
}

/// The shared cascade. `array_like_fallback` decides only what the
/// tail does when nothing in it claimed the receiver.
///
/// # Safety
/// As [`__torajs_any_iter_next`].
unsafe fn iter_next_inner(
    recv: AnyValue,
    idx_slot: *mut i64,
    iter_slot: *mut AnyValue,
    out: *mut AnyValue,
    array_like_fallback: bool,
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
        if *idx_slot == USER_ITERATOR_LANE {
            return step_derived_iterator(*iter_slot, out);
        }
        if *idx_slot == 0 && *iter_slot == VALUE_UNDEFINED {
            match crate::iter_any_get_method::get_iterator(recv) {
                crate::iter_any_get_method::GetIterator::Iterator(iter) => {
                    *iter_slot = iter;
                    *idx_slot = USER_ITERATOR_LANE;
                    return step_derived_iterator(iter, out);
                }
                crate::iter_any_get_method::GetIterator::NoUserMethod => {}
                crate::iter_any_get_method::GetIterator::Threw => {
                    *out = VALUE_UNDEFINED;
                    return 0;
                }
            }
        }
    }
    let cell_tag = if is_cell(recv) {
        Some(unsafe { (as_void_ptr(recv).cast::<u8>().add(4) as *const u16).read() })
    } else {
        None
    };
    // RFC 20260716 刀 12 — StringWrapper receiver iterates its inner
    // [[StringData]] via the same indexed lane. `__torajs_any_index_get`
    // + `__torajs_any_iter_len` already view-through the wrapper cell
    // (刀 3 view-through + 刀 12 iter_len arm below), so accepting the
    // wrapper tag as `indexed` is the only site left. NumberWrapper /
    // BooleanWrapper stay non-indexed (a `for..of` on those throws per
    // spec, matching bun).
    let indexed = is_short_str(recv)
        || matches!(cell_tag, Some(t) if t == Tag::Str as u16
                                    || t == Tag::Arr as u16
                                    || t == Tag::StringWrapper as u16);
    if indexed {
        unsafe {
            let idx = *idx_slot;
            if idx >= __torajs_any_iter_len(recv) {
                *out = VALUE_UNDEFINED;
                return 0;
            }
            *idx_slot = idx + 1;
            *out = __torajs_any_index_get(recv, idx);
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
            if step_fn(as_void_ptr(recv) as *mut c_void, &mut tag, &mut payload) == 0 {
                *out = VALUE_UNDEFINED;
                return 0;
            }
            // Step payloads are borrows — convert to owned before
            // boxing (ENTRIES pre-decrement lands the pair at 1).
            payload_rc_inc(tag, payload);
            *out = __torajs_anyv_box_from_pair(tag, payload);
        }
        return 1;
    }
    if matches!(cell_tag, Some(t) if t == Tag::Obj as u16) {
        let obj = as_void_ptr(recv) as *mut c_void;
        if let Some(live) = unsafe { obj_iter_step(obj, iter_slot, out) } {
            return live;
        }
    }
    // Nothing claimed the receiver. §23.1.2.1 step 3's array-like walk
    // reads `length` off whatever this is (a number answers an empty
    // walk, which is what `Array.from(5)` means); every other consumer
    // of the protocol says the value is not iterable.
    if array_like_fallback {
        return unsafe { crate::iter_any_array_like::step(recv, idx_slot, iter_slot, out) };
    }
    unsafe {
        __torajs_throw_type_error(c"value is not iterable".as_ptr());
        *out = VALUE_UNDEFINED;
    }
    0
}
