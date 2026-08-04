//! `Promise.allSettled` (§27.2.4.2) — the typed sync kernel and the
//! `{status, value}` record it builds.
//!
//! Split out of [`crate::combinator`] verbatim: that file was at the
//! 500-line limit, and this is the combinator whose record-building
//! machinery is the one still growing (the record's own class identity
//! is an open defect — see `alloc_settled_struct`). The any-lane
//! sibling in `combinator_any` packs boxed AnyValue bits into the same
//! record, which is why the allocator stays `pub(crate)`.

use core::ffi::c_void;

use crate::combinator::{absorb_inputs, arr_len, arr_slot_ptr, defer_settle};
use crate::layout::{
    ALLSETTLED_OBJ_HEADER_SIZE, ALLSETTLED_OBJ_TAG, REPR_HEAP, REPR_VOID, STATE_FULFILLED,
    STATE_PENDING, STATE_REJECTED, STR_HDR_SIZE,
};

unsafe extern "C" {
    /// torajs-mmalloc libc-compat — v0.7-A2 step 6b cutover.
    #[link_name = "__torajs_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_memcpy"]
    fn memcpy(dst: *mut c_void, src: *const c_void, n: usize) -> *mut c_void;

    fn __torajs_rc_inc(p: *mut c_void);

    /// Str allocator (libtorajs_str.a). Returns a pointer offset
    /// `STR_HDR_SIZE` past the header; the status-string copy memcpys
    /// the literal into the body region.
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;

    fn __torajs_arr_alloc(initial_cap: u64) -> *mut c_void;
    fn __torajs_arr_push(arr: *mut c_void, val: i64) -> *mut c_void;
    fn __torajs_arr_mark_kind(arr: *mut c_void, chain: u64);
}

const STATUS_FULFILLED_LIT: &[u8] = b"fulfilled";
const STATUS_REJECTED_LIT: &[u8] = b"rejected";

unsafe fn make_settled_str(literal: &[u8]) -> *mut c_void {
    let len = literal.len() as u64;
    let s = unsafe { __torajs_str_alloc_pooled(len) };
    if !literal.is_empty() {
        unsafe {
            memcpy(
                s.add(STR_HDR_SIZE) as *mut c_void,
                literal.as_ptr() as *const c_void,
                literal.len(),
            );
        }
    }
    s as *mut c_void
}

/// Allocate a `{status: string, value: T}` Obj: header(8) +
/// class_tag(8) + vtable(8 zeroed) + props(8 zeroed, blade 1)
/// + status_ptr(8) + value(8) = 48 bytes.
/// `pub(crate)` — the any-lane sibling packs boxed AnyValue bits
/// into the same value slot.
///
/// `class_tag` is what makes the record a real object rather than 48
/// anonymous bytes. With the 0 it used to carry, nothing could look a
/// field up by name: an UNANNOTATED handler (whose parameter infers
/// `any`) read the record as `{}` and `JSON.stringify` agreed, while
/// an ANNOTATED one was fine because the checker types the element
/// `Struct([status, value])` and the static read lands on the fixed
/// offsets written below. The tag has to be agreed with the compiler
/// rather than invented here — `__torajs_class_layouts` is
/// link-emitted rodata, so nothing can append to it at startup — so
/// the call site mints one out of the same anon-stamp pool that gives
/// an ordinary `{x: 1}` literal its identity, and hands it down.
///
/// §27.2.4.2 names the second field `value` when fulfilled and
/// `reason` when rejected, and one layout cannot answer to two names
/// at the same offset — so `tags` carries both stamps and the outcome
/// picks. The bytes written are identical either way; only the name
/// the layout advertises differs.
///
/// A real tag also puts the record's fields on the drop walk
/// (`__torajs_obj_drop_rc` reads the layout's `child_offsets`), where
/// tag 0 left them unwalkable. That changes no measurement today —
/// a 200k-iteration allSettled churn sits at the same RSS either way
/// — which is what you would expect while the enclosing result promise
/// is itself still leaking, since the walk never runs. Recorded as
/// measured rather than as a fix.
///
/// `0` stays honest for a site with no layout to point at, and keeps
/// the old posture.
pub(crate) unsafe fn alloc_settled_struct(state: u8, value: i64, tags: u64) -> *mut c_void {
    // Low word tags the fulfilled shape, high word the rejected one.
    // A site that could only name one (or none) leaves the high word
    // clear and both outcomes share it — the pre-two-tag posture.
    let rejected_tag = (tags >> 32) as u32;
    let class_tag = if state == STATE_FULFILLED || rejected_tag == 0 {
        tags as u32
    } else {
        rejected_tag
    };
    let p = unsafe { malloc(ALLSETTLED_OBJ_HEADER_SIZE + 16) } as *mut u8;
    unsafe {
        // Universal heap header.
        *(p as *mut u32) = 1;
        *(p.add(4) as *mut u16) = ALLSETTLED_OBJ_TAG;
        *(p.add(6) as *mut u16) = 0;
        // class_tag is the u32 at +8 (`torajs-cycle::is_class_obj`
        // reads it there); zero the full word first so the 4 bytes
        // above it stay clear. vtable (+16) stays 0 — field reads
        // dispatch off the layout, not a method table. props dynobj
        // (+24) — NULL, malloc'd so an explicit store.
        *(p.add(8) as *mut u64) = 0;
        *(p.add(8) as *mut u32) = class_tag;
        *(p.add(16) as *mut u64) = 0;
        *(p.add(24) as *mut u64) = 0;
        // status (+32)
        let lit = if state == STATE_FULFILLED {
            STATUS_FULFILLED_LIT
        } else {
            STATUS_REJECTED_LIT
        };
        let status_str = make_settled_str(lit);
        *(p.add(32) as *mut *mut c_void) = status_str;
        // value (+40)
        *(p.add(40) as *mut i64) = value;
    }
    p as *mut c_void
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_allsettled_sync(
    promises_arr: *mut c_void,
    record_tags: i64,
) -> *mut c_void {
    let record_tags = record_tags as u64;
    if promises_arr.is_null() {
        return unsafe { defer_settle(STATE_REJECTED, 0, 0, REPR_VOID) };
    }
    // An `Array<Any>` input carries NaN-box slots — route to the
    // any-lane sibling (same gate as all / race / any).
    if unsafe { crate::combinator_any::arr_is_any(promises_arr) } {
        return unsafe { crate::combinator_any::allsettled_sync_any(promises_arr, record_tags) };
    }
    unsafe { absorb_inputs(promises_arr) };
    let len = unsafe { arr_len(promises_arr) };
    for i in 0..len {
        let pp = unsafe { arr_slot_ptr(promises_arr, i) };
        if pp.is_null() {
            continue;
        }
        if unsafe { (*pp).state } == STATE_PENDING {
            return unsafe { defer_settle(STATE_REJECTED, 0, 0, REPR_VOID) };
        }
    }
    let mut result_arr = unsafe { __torajs_arr_alloc(len) };
    for i in 0..len {
        let pp = unsafe { arr_slot_ptr(promises_arr, i) };
        if pp.is_null() {
            let s = unsafe { alloc_settled_struct(STATE_REJECTED, 0, record_tags) };
            result_arr = unsafe { __torajs_arr_push(result_arr, s as i64) };
            continue;
        }
        let s = unsafe { alloc_settled_struct((*pp).state, (*pp).value, record_tags) };
        // T-17.c-A3 — heap-typed inner value: settled struct co-owns
        // it, so inc to pair the struct's eventual drop_heap call.
        unsafe {
            if (*pp).value_is_heap != 0 && (*pp).value != 0 {
                __torajs_rc_inc((*pp).value as *mut c_void);
            }
        }
        result_arr = unsafe { __torajs_arr_push(result_arr, s as i64) };
    }
    unsafe {
        // {status, value} obj cells — heap-ptr slots (chain 4).
        __torajs_arr_mark_kind(result_arr, 4);
        defer_settle(STATE_FULFILLED, result_arr as i64, 1, REPR_HEAP)
    }
}
