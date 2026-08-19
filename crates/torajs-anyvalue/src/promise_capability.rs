//! NewPromiseCapability(C) (§27.2.1.5) over the runtime construct
//! channel — the custom-species half the settle statics' recv-first
//! arms need: `CP.resolve(v)` on `class CP extends Promise` must
//! answer a CP INSTANCE (spec: Construct(C, «executor») runs the
//! subclass ctor chain), not a plain Promise.
//!
//! Shape: a GetCapabilitiesExecutor cell (§27.2.1.5.1 — the
//! [`crate::promise_with_resolvers::mint_resolver`] owned-cell
//! layout, capture slot pointing at a heap [`CapRecord`]) goes
//! through [`crate::construct::__torajs_anyv_construct`]; the
//! subclass ctor's `super(executor)` kernel calls it with the
//! §27.2.1.3 resolving pair bound to the minted instance, and the
//! executor records both. The capability consumer then settles
//! through Call(resolve/reject) like any spec algorithm.
//!
//! Recorded boundaries: an executor the user ctor stores and calls
//! again AFTER the capability was consumed re-fills a drained
//! record (the slots were handed to the consumer) — harmless writes
//! into a record the cell's drop still owns, released on cell drop.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::nanbox::{VALUE_UNDEFINED, as_void_ptr, is_cell};
use crate::nanbox_encode::__torajs_anyv_box_pointer;

// ---- closure cell layout (promise_with_resolvers mirror) ----
const CLOSURE_FN_ADDR_OFF: usize = 8;
const CLOSURE_DROP_FN_OFF: usize = 16;
const CLOSURE_PROPS_OFF: usize = 24;
const CLOSURE_BOXED_ENTRY_OFF: usize = 32;
const CLOSURE_TRACE_FN_OFF: usize = 40;
const CAP_RECORD_OFF: usize = 48;
const CELL_SIZE: usize = 56;

/// dynobj bucket tags (torajs-dynobj `layout.rs` mirror).
const ANY_HEAP: u64 = 4;
const ANY_I64: u64 = 2;

/// W0/E0/C1 reflection entry flags (promise_with_resolvers mirror).
const REFLECT_ENTRY_FLAGS: u64 = (1 << 6) | (1 << 5) | (1 << 4) | (1 << 3) | (1 << 2);

unsafe extern "C" {
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_define(
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
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// §27.2.1.5 PromiseCapability Record — `[[Resolve]]` / `[[Reject]]`
/// as boxed AnyValues (undefined until the executor runs).
#[repr(C)]
struct CapRecord {
    resolve: u64,
    reject: u64,
}

/// §27.2.1.5.1 GetCapabilitiesExecutor [[Call]] — record the
/// resolving pair the ctor chain hands over; already-filled slots
/// refuse per steps 2-3 (loud, catchable).
unsafe extern "C" fn cap_executor_entry(env: *mut c_void, argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let rec = *(env.cast::<u8>().add(CAP_RECORD_OFF) as *const u64) as *mut CapRecord;
        if rec.is_null() {
            return VALUE_UNDEFINED;
        }
        if (*rec).resolve != VALUE_UNDEFINED || (*rec).reject != VALUE_UNDEFINED {
            __torajs_throw_type_error(c"promise capability is already settled".as_ptr());
            return VALUE_UNDEFINED;
        }
        let r = if argc >= 1 { *argv } else { VALUE_UNDEFINED };
        let j = if argc >= 2 {
            *argv.add(1)
        } else {
            VALUE_UNDEFINED
        };
        // argv slots are borrows; the record keeps its own stakes.
        crate::nanbox_ffi::__torajs_anyv_rc_inc(r);
        crate::nanbox_ffi::__torajs_anyv_rc_inc(j);
        (*rec).resolve = r;
        (*rec).reject = j;
        VALUE_UNDEFINED
    }
}

/// drop_fn — release the record's held boxes and the record itself,
/// then the props bag and the cell block.
unsafe extern "C" fn cap_executor_drop(env: *mut c_void) {
    unsafe {
        __torajs_cycle_unbuffer(env);
        let cell = env.cast::<u8>();
        let rec = *(cell.add(CAP_RECORD_OFF) as *const u64) as *mut CapRecord;
        if !rec.is_null() {
            crate::nanbox_ffi::__torajs_anyv_rc_dec((*rec).resolve);
            crate::nanbox_ffi::__torajs_anyv_rc_dec((*rec).reject);
            drop(Box::from_raw(rec));
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

/// trace_fn — the record's held resolvers are the cell's extra
/// children (each holds the minted promise).
unsafe extern "C" fn cap_executor_trace(
    env: *mut c_void,
    visit: unsafe extern "C" fn(i64, *mut c_void, *mut c_void, *mut c_void),
    ctx: *mut c_void,
) {
    unsafe {
        let rec = *(env.cast::<u8>().add(CAP_RECORD_OFF) as *const u64) as *mut CapRecord;
        if rec.is_null() {
            return;
        }
        for slot in [&raw mut (*rec).resolve, &raw mut (*rec).reject] {
            let v = *slot;
            if is_cell(v) {
                visit(0, as_void_ptr(v), slot as *mut c_void, ctx);
            }
        }
    }
}

/// Mint the executor cell — `name` "" / `length` 2 per
/// §27.2.1.5.1's function definition.
unsafe fn mint_cap_executor(rec: *mut CapRecord) -> *mut u8 {
    unsafe {
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        let cell = std::alloc::alloc_zeroed(layout);
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::Closure as u16;
        *(cell.add(6) as *mut u16) = 0;
        *(cell.add(CLOSURE_FN_ADDR_OFF) as *mut u64) =
            crate::method_value::native_entry as *const () as u64;
        *(cell.add(CLOSURE_DROP_FN_OFF) as *mut u64) = cap_executor_drop as *const () as u64;
        *(cell.add(CLOSURE_BOXED_ENTRY_OFF) as *mut u64) = cap_executor_entry as *const () as u64;
        *(cell.add(CLOSURE_TRACE_FN_OFF) as *mut u64) = cap_executor_trace as *const () as u64;
        *(cell.add(CAP_RECORD_OFF) as *mut u64) = rec as u64;

        let props_slot = cell.add(CLOSURE_PROPS_OFF) as *mut *mut c_void;
        *props_slot = __torajs_dynobj_alloc();
        let name_key = __torajs_str_alloc(c"name".as_ptr() as *const u8, 4);
        let empty = __torajs_str_alloc(c"".as_ptr() as *const u8, 0);
        __torajs_dynobj_define(
            props_slot,
            name_key as *mut c_void,
            ANY_HEAP,
            empty as u64,
            REFLECT_ENTRY_FLAGS,
        );
        __torajs_str_drop(name_key as *mut c_void);
        let len_key = __torajs_str_alloc(c"length".as_ptr() as *const u8, 6);
        __torajs_dynobj_define(
            props_slot,
            len_key as *mut c_void,
            ANY_I64,
            2,
            REFLECT_ENTRY_FLAGS,
        );
        __torajs_str_drop(len_key as *mut c_void);
        cell
    }
}

/// Is `v` a callable value in the §27.2.1.5 step-8/9 sense — a
/// closure cell with a boxed dual entry (exactly what Call would
/// accept)? Shared with the settle statics' GetPromiseResolve(C)
/// gate (§27.2.4.1.1 step 2 asks the same question).
pub(crate) fn is_callable(v: u64) -> bool {
    is_cell(v)
        && unsafe { crate::method_call_closure_dispatch::closure_cell_entry(as_void_ptr(v)) }
            .is_some()
}

/// §27.2.1.5 NewPromiseCapability(C) — `Some((promise, resolve,
/// reject))`, each an OWNED box, or `None` with the pending throw
/// recorded (a ctor-chain raise propagates; a ctor that never ran
/// the executor fails the step-8/9 IsCallable check).
///
/// # Safety
/// `c` is a live AnyValue (the construct kernel gates
/// IsConstructor itself).
pub(crate) unsafe fn new_promise_capability(c: u64) -> Option<(u64, u64, u64)> {
    unsafe {
        let rec = Box::into_raw(Box::new(CapRecord {
            resolve: VALUE_UNDEFINED,
            reject: VALUE_UNDEFINED,
        }));
        let ex = mint_cap_executor(rec);
        let ex_box = __torajs_anyv_box_pointer(ex as *mut c_void);
        let argv = [ex_box];
        let promise = crate::construct::__torajs_anyv_construct(c, argv.as_ptr(), 1);
        // Drain the record BEFORE releasing the cell (the cell's drop
        // owns the record): the stakes transfer out with the values.
        let resolve = (*rec).resolve;
        let reject = (*rec).reject;
        (*rec).resolve = VALUE_UNDEFINED;
        (*rec).reject = VALUE_UNDEFINED;
        crate::nanbox_ffi::__torajs_anyv_rc_dec(ex_box);
        if __torajs_throw_check() != 0 {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(promise);
            crate::nanbox_ffi::__torajs_anyv_rc_dec(resolve);
            crate::nanbox_ffi::__torajs_anyv_rc_dec(reject);
            return None;
        }
        if !is_callable(resolve) || !is_callable(reject) {
            __torajs_throw_type_error(
                c"Promise resolve or reject function is not callable".as_ptr(),
            );
            crate::nanbox_ffi::__torajs_anyv_rc_dec(promise);
            crate::nanbox_ffi::__torajs_anyv_rc_dec(resolve);
            crate::nanbox_ffi::__torajs_anyv_rc_dec(reject);
            return None;
        }
        Some((promise, resolve, reject))
    }
}
