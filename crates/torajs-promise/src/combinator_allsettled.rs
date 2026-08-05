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

use crate::combinator::{absorb_inputs, arr_len, arr_slot_ptr, defer_settle, unbox_target};
use crate::layout::{
    ALLSETTLED_OBJ_HEADER_SIZE, ALLSETTLED_OBJ_TAG, REPR_ANY, REPR_HEAP, REPR_STR, REPR_VOID,
    STATE_FULFILLED, STATE_REJECTED, STR_HDR_SIZE,
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

/// The value the record's second slot must hold.
///
/// An executor-minted element settles through the any lane, so its slot
/// carries a NaN box while the record's field is typed `T` — the same
/// mismatch `Promise.all`'s result array had, in the one place left
/// that buries a value where the awaiting site's own repr decode cannot
/// reach it. It read as box bits (`{"value":-562949953421311}`) and was
/// invisible until the records became readable at all.
/// The record's value slot, and whether the CALLER still owes a raw
/// `rc_inc` for what is in it.
///
/// Two ways that slot gets paid for. Normally the record co-owns a
/// heap-typed field and the caller incs the pointer. But when the
/// field is typed `any` the slot holds a NaN box, whose share has to
/// go through the box-aware path instead — a boxed immediate is not an
/// address, and incrementing it would write through a non-pointer. So
/// that case settles its own ownership here and answers `false`.
#[inline]
pub(crate) unsafe fn record_slot(src_repr: u8, target_repr: u8, v: i64) -> (i64, bool) {
    // A heterogeneous input types the field `any` — the checker says
    // `{status, value: Any}` for `allSettled([resolve(2), resolve("s")])`
    // — so the slot has to carry a box. Without this a number's raw
    // bits sat where everything downstream decodes a box and read back
    // as null, while a string happened to survive: exactly the shape
    // that makes a representation bug look like a per-type one.
    if target_repr == REPR_ANY {
        let boxed = if src_repr == REPR_ANY {
            // Already a box; take one more share, box-aware.
            unsafe { crate::combinator_any::box_share(v as u64) };
            v
        } else {
            unsafe {
                crate::combinator_any::box_settled_owned(src_repr, v)
                    .unwrap_or_else(|| crate::combinator_any::box_undefined())
                    as i64
            }
        };
        return (boxed, false);
    }
    (
        unsafe { record_value(src_repr, target_repr, v) },
        value_field_is_owned(target_repr),
    )
}

#[inline]
unsafe fn record_value(src_repr: u8, target_repr: u8, v: i64) -> i64 {
    match unbox_target(src_repr, target_repr) {
        Some(lane) => unsafe { crate::then_box::unbox_settled(lane, v) },
        None => v,
    }
}

/// Does the record co-own what sits in its value slot? The layout walk
/// that eventually drops the record releases exactly the fields whose
/// type is refcounted, so the payment here has to follow the same rule
/// — reading it off the target form rather than the source cell's
/// `value_is_heap`, which says nothing about the record's field type.
#[inline]
fn value_field_is_owned(target_repr: u8) -> bool {
    matches!(target_repr, REPR_STR | REPR_HEAP | REPR_ANY)
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
    value_repr: i64,
) -> *mut c_void {
    let record_tags = record_tags as u64;
    let value_repr = value_repr as u8;
    if promises_arr.is_null() {
        return unsafe { defer_settle(STATE_REJECTED, 0, 0, REPR_VOID) };
    }
    // An `Array<Any>` input carries NaN-box slots — route to the
    // any-lane sibling (same gate as all / race / any).
    if unsafe { crate::combinator_any::arr_is_any(promises_arr) } {
        return unsafe { crate::combinator_any::allsettled_sync_any(promises_arr, record_tags) };
    }
    // A pending element is the one thing the walk below cannot answer;
    // it used to reject the result with a placeholder, which is a
    // strange thing for a combinator that per §27.2.4.3 never rejects
    // at all. The fan-in waits. An all-settled input keeps this walk so
    // its microtask position does not move.
    if unsafe { crate::combinator_all_fanin::has_pending(promises_arr) } {
        return unsafe {
            crate::combinator_all_fanin::allsettled_fan_in(promises_arr, record_tags, value_repr)
        };
    }
    unsafe { absorb_inputs(promises_arr) };
    let len = unsafe { arr_len(promises_arr) };
    let mut result_arr = unsafe { __torajs_arr_alloc(len) };
    for i in 0..len {
        let pp = unsafe { arr_slot_ptr(promises_arr, i) };
        if pp.is_null() {
            let s = unsafe { alloc_settled_struct(STATE_REJECTED, 0, record_tags) };
            result_arr = unsafe { __torajs_arr_push(result_arr, s as i64) };
            continue;
        }
        let (v, owed) = unsafe { record_slot((*pp).value_repr, value_repr, (*pp).value) };
        let s = unsafe { alloc_settled_struct((*pp).state, v, record_tags) };
        // T-17.c-A3 — the record co-owns a heap-typed inner value, so
        // inc to pair the drop its layout walk will perform. An `any`
        // field has already settled its own share inside `record_slot`.
        if owed && v != 0 {
            unsafe { __torajs_rc_inc(v as *mut c_void) };
        }
        result_arr = unsafe { __torajs_arr_push(result_arr, s as i64) };
    }
    unsafe {
        // {status, value} obj cells — heap-ptr slots (chain 4).
        __torajs_arr_mark_kind(result_arr, 4);
        crate::combinator::settle_result(len, STATE_FULFILLED, result_arr as i64, 1, REPR_HEAP)
    }
}
