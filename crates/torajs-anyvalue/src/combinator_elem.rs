//! The element functions of §27.2.4.{1,3,6} — the per-run record and
//! the closure cells that carry it (RFC
//! 20260820-combinator-any-constructor blades 3-5).
//!
//! Each PerformPromiseX mints one or two functions per element, each
//! holding its own `[[Index]]` and SHARING the run's values list,
//! `[[RemainingElements]]` and one capability function. The shared
//! half lives in a manually refcounted [`ElemState`]; the per-element
//! half rides two extra capture slots.
//!
//! `[[AlreadyCalled]]` lives in the state, keyed by index, rather than
//! on the cell — allSettled's resolve and reject functions for one
//! element share ONE such record (§27.2.4.3.2 step 4.n), which a
//! per-cell byte could not express.
//!
//! Cell layout (the [`crate::promise_capability`] executor's shape,
//! two slots wider):
//!
//! ```text
//!   offset  0 — universal header (rc=1, Tag::Closure, flags=0)
//!   offset  8 — fn_addr = the throwing native entry
//!   offset 16 — drop_fn
//!   offset 24 — props — own `name` "" / `length` 1 (§27.2.4.1.3)
//!   offset 32 — boxed_entry = the element algorithm
//!   offset 40 — trace_fn = 0
//!   offset 48 — *mut ElemState (shared, manually refcounted)
//!   offset 56 — [[Index]] i64
//!   offset 64 — which algorithm's element function this is
//! ```
//!
//! `trace_fn` is 0 on purpose: the shared state is one block behind N
//! cells, and a Bacon-Rajan trial deletion that reached the same
//! `[[Values]]` slot once per cell would subtract N times for one
//! edge. Declaring the cells leaves means a reference cycle that runs
//! through a still-pending combinator is not collected until the
//! combinator settles — a recorded boundary, not a correctness one
//! (settling drops the last cell and frees the state).

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::nanbox::VALUE_UNDEFINED;

const CLOSURE_FN_ADDR_OFF: usize = 8;
const CLOSURE_DROP_FN_OFF: usize = 16;
const CLOSURE_PROPS_OFF: usize = 24;
const CLOSURE_BOXED_ENTRY_OFF: usize = 32;
const STATE_OFF: usize = 48;
const INDEX_OFF: usize = 56;
const KIND_OFF: usize = 64;
const CELL_SIZE: usize = 72;

/// dynobj bucket tags + reflection flags (promise_capability mirror).
const ANY_HEAP: u64 = 4;
const ANY_I64: u64 = 2;
const REFLECT_ENTRY_FLAGS: u64 = (1 << 6) | (1 << 5) | (1 << 4) | (1 << 3) | (1 << 2);

/// Which spec function a cell is. The value slot each writes and the
/// `[[AlreadyCalled]]` record they share are the only differences.
#[derive(Clone, Copy, PartialEq)]
#[repr(u8)]
pub(crate) enum ElemKind {
    /// §27.2.4.1.3 — records `x` itself.
    AllResolve = 0,
    /// §27.2.4.3.3 — records `{ status: "fulfilled", value: x }`.
    SettledResolve = 1,
    /// §27.2.4.3.4 — records `{ status: "rejected", reason: x }`.
    SettledReject = 2,
    /// §27.2.4.6.3 — records `x` in the errors list.
    AnyReject = 3,
}

unsafe extern "C" {
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_set(obj_slot: *mut *mut c_void, key: *mut c_void, tag: u64, value: u64);
    /// torajs-throw — the injected `__new_AggregateError` factory, or
    /// NULL when the program has no such class.
    fn __torajs_make_aggregate_error(errors: i64) -> *mut c_void;
    fn __torajs_dynobj_define_plain(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_cycle_unbuffer(p: *mut c_void);
    fn __torajs_arr_alloc_any_filled(n: u64) -> *mut u8;
    fn __torajs_arr_set_any(arr: *mut c_void, i: u64, tag: u64, value: u64);
}

/// The run-wide half of the element functions' internal slots.
/// `refs` counts the cells pointing here (plus 1 for the walk itself
/// while it is still minting elements).
pub(crate) struct ElemState {
    refs: usize,
    /// §27.2.4.1.2 step 2 `[[RemainingElements]].[[Value]]`.
    pub(crate) remaining: i64,
    /// `[[Values]]` (or `[[Errors]]`) — one OWNED box per element,
    /// `undefined` until that element's function runs.
    pub(crate) values: Vec<u64>,
    /// `[[AlreadyCalled]]` per element — one record shared by an
    /// allSettled element's resolve / reject pair.
    already: Vec<u8>,
    /// The capability function the drained counter calls: resolve for
    /// all / allSettled, reject for any. An OWNED box.
    settle: u64,
    /// any-lane — wrap the finished list in an AggregateError first
    /// (§27.2.4.6.3 step 8.a).
    aggregate: bool,
}

impl ElemState {
    /// A fresh state holding one stake on the capability function and
    /// one self-reference for the minting walk.
    pub(crate) unsafe fn new(settle: u64, aggregate: bool) -> *mut ElemState {
        unsafe { crate::nanbox_ffi::__torajs_anyv_rc_inc(settle) };
        Box::into_raw(Box::new(ElemState {
            refs: 1,
            remaining: 1,
            values: Vec::new(),
            already: Vec::new(),
            settle,
            aggregate,
        }))
    }

    /// Step 4.c of every PerformPromiseX — reserve this element's slot
    /// before it is resolved, so a synchronously-settling element
    /// writes into a list that is already long enough.
    pub(crate) unsafe fn reserve_slot(st: *mut ElemState) {
        unsafe {
            (*st).values.push(VALUE_UNDEFINED);
            (*st).already.push(0);
        }
    }
}

/// Release one reference; the last one frees the held boxes.
pub(crate) unsafe fn state_release(st: *mut ElemState) {
    unsafe {
        (*st).refs -= 1;
        if (*st).refs != 0 {
            return;
        }
        let owned = Box::from_raw(st);
        for v in &owned.values {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(*v);
        }
        crate::nanbox_ffi::__torajs_anyv_rc_dec(owned.settle);
    }
}

/// §27.2.4.1.2 step 4.b.ii / §27.2.4.1.3 step 9 — the counter reached
/// zero, so hand `CreateArrayFromList(values)` to the capability's
/// resolve function. The values transfer their stakes into the array.
unsafe fn resolve_with_values(st: *mut ElemState) {
    unsafe {
        let values = core::mem::take(&mut (*st).values);
        let arr = __torajs_arr_alloc_any_filled(values.len() as u64);
        if arr.is_null() {
            for v in values {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(v);
            }
            return;
        }
        for (i, v) in values.into_iter().enumerate() {
            let tag = crate::nanbox_encode::__torajs_anyv_unbox_tag(v);
            let value = crate::nanbox_encode::__torajs_anyv_unbox_value(v);
            // Owned transfer — the slot inherits our stake.
            __torajs_arr_set_any(arr.cast(), i as u64, tag as u64, value as u64);
        }
        let mut boxed = crate::nanbox_encode::__torajs_anyv_box_pointer(arr.cast());
        if (*st).aggregate {
            // §27.2.4.6.3 step 8.a. A program without the injected
            // class gets NULL back; forwarding the errors array is a
            // better answer than none (the torajs-promise sibling's
            // recorded posture).
            let agg = __torajs_make_aggregate_error(boxed as i64);
            if !agg.is_null() {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(boxed);
                boxed = crate::nanbox_encode::__torajs_anyv_box_pointer(agg);
            }
        }
        let one = [boxed];
        let out =
            crate::method_call_closure_dispatch::__torajs_any_call((*st).settle, one.as_ptr(), 1);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(out);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(boxed);
    }
}

/// Steps 1-3 of §27.2.4.1.2 step 4.b.ii's counter drain, shared by the
/// element function and the walk's own final decrement.
pub(crate) unsafe fn count_down(st: *mut ElemState) {
    unsafe {
        (*st).remaining -= 1;
        if (*st).remaining == 0 {
            resolve_with_values(st);
        }
    }
}

/// The shared body of §27.2.4.1.3 / .3.3 / .3.4 / .6.3 — record this
/// element's answer at its index, then drain the counter. The
/// [[AlreadyCalled]] steps make every call after the first a no-op,
/// including the one from the sibling of an allSettled pair.
unsafe extern "C" fn elem_entry(env: *mut c_void, argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let cell = env.cast::<u8>();
        let st = *(cell.add(STATE_OFF) as *const u64) as *mut ElemState;
        let index = *(cell.add(INDEX_OFF) as *const i64) as usize;
        let already = &raw mut (*st).already;
        let flag = (*already).as_mut_ptr().add(index);
        if *flag != 0 {
            return VALUE_UNDEFINED;
        }
        *flag = 1;
        let x = if argc >= 1 { *argv } else { VALUE_UNDEFINED };
        let recorded = match kind_of(cell) {
            ElemKind::AllResolve | ElemKind::AnyReject => {
                // argv slots are borrows; the list keeps its own stake.
                crate::nanbox_ffi::__torajs_anyv_rc_inc(x);
                x
            }
            ElemKind::SettledResolve => settled_record(b"fulfilled", b"value", x),
            ElemKind::SettledReject => settled_record(b"rejected", b"reason", x),
        };
        let values = &raw mut (*st).values;
        let slot = (*values).as_mut_ptr().add(index);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(*slot);
        *slot = recorded;
        count_down(st);
        VALUE_UNDEFINED
    }
}

/// Read a cell's kind slot back. The byte is written once at mint
/// from an `ElemKind` discriminant, so every value is in range.
unsafe fn kind_of(cell: *const u8) -> ElemKind {
    match unsafe { *cell.add(KIND_OFF) } {
        0 => ElemKind::AllResolve,
        1 => ElemKind::SettledResolve,
        2 => ElemKind::SettledReject,
        _ => ElemKind::AnyReject,
    }
}

/// §27.2.4.3.3 steps 9-11 / .3.4 steps 9-11 — the ordinary object
/// `{ status, value | reason }`, as an OWNED box. Both properties are
/// CreateDataPropertyOrThrow, which on a fresh object is the plain
/// writable / enumerable / configurable shape an ordinary [[Set]]
/// makes.
unsafe fn settled_record(status: &[u8], value_key: &[u8], x: u64) -> u64 {
    unsafe {
        let mut obj = __torajs_dynobj_alloc();
        let slot = &raw mut obj;
        let status_key = __torajs_str_alloc(b"status".as_ptr(), 6);
        let status_val = __torajs_str_alloc(status.as_ptr(), status.len() as i64);
        // The entry takes ownership of the VALUE; only the key is
        // copied, so only the key is dropped here (the capability
        // executor's own `name` seed reads the same way).
        __torajs_dynobj_set(slot, status_key.cast(), ANY_HEAP, status_val as u64);
        __torajs_str_drop(status_key.cast());
        let vkey = __torajs_str_alloc(value_key.as_ptr(), value_key.len() as i64);
        let tag = crate::nanbox_encode::__torajs_anyv_unbox_tag(x);
        let value = crate::nanbox_encode::__torajs_anyv_unbox_value(x);
        // The entry takes ownership of a heap payload.
        crate::nanbox_ffi::__torajs_anyv_rc_inc(x);
        __torajs_dynobj_set(slot, vkey.cast(), tag as u64, value as u64);
        __torajs_str_drop(vkey.cast());
        crate::nanbox_encode::__torajs_anyv_box_pointer(obj)
    }
}

unsafe extern "C" fn elem_drop(env: *mut c_void) {
    unsafe {
        // The collector may hold this cell as a cycle candidate from
        // an earlier rc_dec; freeing the block without retiring that
        // entry leaves it a dangling root (Guard Malloc caught the
        // read at teardown). Both sibling owned-cell drops open the
        // same way.
        __torajs_cycle_unbuffer(env);
        let cell = env.cast::<u8>();
        let st = *(cell.add(STATE_OFF) as *const u64) as *mut ElemState;
        if !st.is_null() {
            state_release(st);
        }
        let props = *(cell.add(CLOSURE_PROPS_OFF) as *const u64);
        if props != 0 {
            __torajs_value_drop_heap(props as *mut c_void);
        }
        std::alloc::dealloc(
            cell,
            core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap(),
        );
    }
}

/// Mint one element function over `st` at `index` — `name` "" /
/// `length` 1 per its definition. Takes a reference on the state.
pub(crate) unsafe fn mint_elem(st: *mut ElemState, index: i64, kind: ElemKind) -> u64 {
    unsafe {
        (*st).refs += 1;
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        let cell = std::alloc::alloc_zeroed(layout);
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::Closure as u16;
        *(cell.add(6) as *mut u16) = 0;
        *(cell.add(CLOSURE_FN_ADDR_OFF) as *mut u64) =
            crate::method_value::native_entry as *const () as u64;
        *(cell.add(CLOSURE_DROP_FN_OFF) as *mut u64) = elem_drop as *const () as u64;
        *(cell.add(CLOSURE_BOXED_ENTRY_OFF) as *mut u64) = elem_entry as *const () as u64;
        *(cell.add(STATE_OFF) as *mut u64) = st as u64;
        *(cell.add(INDEX_OFF) as *mut i64) = index;
        *cell.add(KIND_OFF) = kind as u8;
        seed_reflection(cell);
        crate::nanbox_encode::__torajs_anyv_box_pointer(cell.cast())
    }
}

/// Own `name` "" / `length` 1, the W0/E0/C1 shape every synthesized
/// spec function wears here.
unsafe fn seed_reflection(cell: *mut u8) {
    unsafe {
        let props_slot = cell.add(CLOSURE_PROPS_OFF) as *mut *mut c_void;
        *props_slot = __torajs_dynobj_alloc();
        let name_key = __torajs_str_alloc(c"name".as_ptr() as *const u8, 4);
        let empty = __torajs_str_alloc(c"".as_ptr() as *const u8, 0);
        __torajs_dynobj_define_plain(
            props_slot,
            name_key as *mut c_void,
            ANY_HEAP,
            empty as u64,
            REFLECT_ENTRY_FLAGS,
        );
        __torajs_str_drop(name_key as *mut c_void);
        let len_key = __torajs_str_alloc(c"length".as_ptr() as *const u8, 6);
        __torajs_dynobj_define_plain(
            props_slot,
            len_key as *mut c_void,
            ANY_I64,
            1,
            REFLECT_ENTRY_FLAGS,
        );
        __torajs_str_drop(len_key as *mut c_void);
    }
}
