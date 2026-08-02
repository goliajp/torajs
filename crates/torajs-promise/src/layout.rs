//! Promise heap-block layout + state constants + callback record.
//!
//! Mirrors `runtime_promise.c`'s 1:1 — every byte offset matches so
//! ABI-compat is preserved at the `value_drop_heap` dispatch (TAG=8)
//! and at every C-side caller (e.g. ssa_lower's emitted dispatcher
//! functions read `pp->state` / `pp->value` directly).
//!
//! ## Promise struct (32 bytes)
//!
//! ```text
//!   +0..7   : universal heap header (refcount u32 + type_tag u16 + flags u16)
//!   +8      : state u8 (PENDING=0, FULFILLED=1, REJECTED=2)
//!   +9      : value_is_heap u8 — set on heap-typed Promise<Heap T>
//!   +10     : has_handler u8 — set by attach_then / get_value (P10.5-A3-a);
//!             read by HPRT-check microtask to decide unhandled-rejection
//!             reporting per spec §27.2.1.9 HostPromiseRejectionTracker
//!   +11..15 : _pad[5] (alignment)
//!   +16..23 : i64 value (primitive bits or heap-ptr cast)
//!   +24..31 : *mut callback list (NULL when no .then attached)
//! ```
//!
//! ## Callback chain (PromiseCb)
//!
//! Singly-linked list head-pushed onto `Promise.callbacks` by
//! `attach_then`. Each node carries:
//!
//! ```text
//!   +0      : invoke (extern "C" fn(int64_t)) — codegen-emitted
//!             dispatcher that knows how to call the user fn
//!   +8      : arg (i64) — packed dispatcher state
//!   +16     : next ptr (NULL = end of list)
//! ```
//!
//! On `resolve` / `reject`, `drain_callbacks` walks the list,
//! enqueues each node onto the microtask queue, and frees the
//! list (the queue's slot copies fn+arg by value, so nodes are
//! transient).

use core::ffi::c_void;

/// `type_tag` value for Promise heap blocks (matches
/// `runtime_promise.c::__TORAJS_TAG_PROMISE = 8` + the
/// `runtime_str.c::value_drop_heap` dispatch case).
pub const TAG_PROMISE: u16 = 8;

/// Promise lifecycle states (matches `runtime_promise.c` macros).
pub const STATE_PENDING: u8 = 0;
pub const STATE_FULFILLED: u8 = 1;
pub const STATE_REJECTED: u8 = 2;

/// `sizeof(Promise)` — 32 bytes. Matches `__TORAJS_PROMISE_SIZE`.
pub const PROMISE_SIZE: usize = 32;

/// `value_repr` codes (RFC 20260720-anylane-promise-methods knife 1)
/// — the storage form the typed tier put (or will put, for a pending
/// cell) into the `value` slot, keyed off the boxing site's static
/// `Promise<T>` inner type. Lockstep mirror: torajs-core
/// `ssa_lower_promise_repr_mark.rs::repr_of_inner`.
pub const REPR_UNSTAMPED: u8 = 0;
/// Raw integer bits (inner I64/I32).
pub const REPR_I64: u8 = 1;
/// f64 bits stored via bitcast (inner F64).
pub const REPR_F64: u8 = 2;
/// 0/1 bool (inner Bool).
pub const REPR_BOOL: u8 = 3;
/// Str slot — three shapes (NULL = null / sentinel = undefined /
/// heap Str ptr), boxed via the str-slot boxer.
pub const REPR_STR: u8 = 4;
/// Generic refcounted cell ptr (Obj/Arr/Closure/RegExp/Date/…) —
/// ANY_HEAP encode + rc share.
pub const REPR_HEAP: u8 = 5;
/// Bits are already an AnyValue NaN-box (inner Any) — pass through.
pub const REPR_ANY: u8 = 6;
/// Inner Void — the slot holds the 0 filler; the bridge answers
/// undefined (async `Promise<void>` resolve shape).
pub const REPR_VOID: u8 = 7;
/// `Promise.resolve(null)` — the slot holds 0, the bridge answers
/// null (distinct from REPR_I64's honest integer 0).
pub const REPR_NULL: u8 = 8;

/// `__torajs_str_alloc_pooled` returns a pointer offset HDR_SIZE
/// past the Str header; allsettled's status field copy uses this
/// offset to memcpy the literal into the data segment. Mirrors
/// `__TORAJS_STR_HDR_SIZE = 16` in runtime_str.c.
pub const STR_HDR_SIZE: usize = 16;

/// Array<T> layout offsets used by Promise.all / .race / .allSettled
/// / .any walks. Re-declared here so torajs-promise doesn't pull in
/// a torajs-arr dep — same independent-layout pattern the original
/// runtime_promise.c used. Must move in lockstep with
/// `torajs_arr::layout::ARR_DATA_PTR_OFF` — RFC 20260706-arr-grow-
/// alias-stability B1 (2026-07-06): slots live behind the data
/// pointer at offset 32; the cell is fixed across grow.
pub const ARR_DATA_PTR_OFF: usize = 32;
pub const ARR_LEN_OFF: usize = 8;
pub const ARR_HEAD_OFF: usize = 20;

/// allsettled inner-struct constants. The MVP packs every settled
/// outcome into the same `{status: string, value: number}` Obj.
/// Header size mirrors `ssa_lower::OBJ_HEADER_SIZE` (blade 1: 32 —
/// header 8 + class_tag 8 + vtable 8 + props dynobj 8).
pub const ALLSETTLED_OBJ_TAG: u16 = 1;
pub const ALLSETTLED_OBJ_HEADER_SIZE: usize = 32;

/// Throw-tag constants used by `get_value`'s rejection path.
/// Match runtime_str.c's torajs_throw_native conventions:
/// 2 = I64, 4 = ANY_HEAP. Mirror of the C source's literal values.
pub const THROW_TAG_I64: i64 = 2;
pub const THROW_TAG_ANY_HEAP: i64 = 4;

/// Universal heap header — 8 bytes, ABI-shared with `torajs_rc::HeapHeader`
/// + every `__torajs_heap_header_t` typedef in the C runtime.
#[repr(C, align(8))]
pub struct HeapHeader {
    pub refcount: u32,
    pub type_tag: u16,
    pub flags: u16,
}

/// Promise heap struct. `#[repr(C)]` for byte-equal ABI with
/// `runtime_promise.c::Promise`. Field layout test in this module
/// asserts the offsets match (catches accidental field-reorder bugs
/// that would silently corrupt ssa_lower-emitted dispatchers).
#[repr(C)]
pub struct Promise {
    pub header: HeapHeader,
    pub state: u8,
    pub value_is_heap: u8,
    /// `[[PromiseIsHandled]]` per spec §25.4.1.3. Set to 1 by
    /// `attach_then` (any .then/.catch/.finally) and `get_value`
    /// (await observation); read by `unhandled::hprt_check_dispatch`
    /// to decide whether the per-rejected-promise check fires the
    /// default reporter. Initial 0 means "no rejection handler attached
    /// yet" — a rejected promise reaching the HPRT microtask in that
    /// state IS the spec-defined unhandled-rejection event source.
    pub has_handler: u8,
    /// RFC 20260720-anylane-promise-methods knife 1 — the `value`
    /// slot's storage form, stamped when the cell crosses into the
    /// `any` world (`box_to_any` family; the box site statically
    /// holds `Promise<T>`). The typed tier never reads it; the
    /// any-lane `.then`/`.catch` bridge boxes the settled value per
    /// this code. `REPR_UNSTAMPED` (the alloc default) means "never
    /// crossed" — the bridge keeps the loud TypeError for it.
    /// Constants: [`REPR_UNSTAMPED`] .. [`REPR_VOID`]; the emit-side
    /// mapping lives in torajs-core `ssa_lower_promise_repr_mark.rs`
    /// (must move in lockstep).
    pub value_repr: u8,
    pub _pad: [u8; 4],
    pub value: i64,
    pub callbacks: *mut PromiseCb,
}

/// Promise callback record. `invoke` is a codegen-emitted dispatcher
/// (`extern "C" fn(int64_t)`) packed with the user's closure +
/// source value + result Promise via the `arg` slot. Storing it as
/// an opaque fn-ptr keeps the runtime free of codegen details.
#[repr(C)]
pub struct PromiseCb {
    pub invoke: MicrotaskFn,
    pub arg: i64,
    pub next: *mut PromiseCb,
}

/// Microtask fn-ptr signature. Matches the C
/// `__torajs_microtask_fn_t = void (*)(int64_t arg)` and
/// `torajs_microtask::MicrotaskFn`. Re-declared here to avoid a
/// torajs-microtask Cargo dep (we only need the typedef shape; the
/// `enqueue` extern resolves at link time via the C ABI).
pub type MicrotaskFn = unsafe extern "C" fn(arg: i64);

/// Cast a `*mut c_void` heap pointer to `*mut Promise`.
#[inline]
pub fn as_promise(p: *mut c_void) -> *mut Promise {
    p as *mut Promise
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn promise_struct_layout_matches_c() {
        assert_eq!(core::mem::size_of::<HeapHeader>(), 8);
        assert_eq!(core::mem::size_of::<Promise>(), PROMISE_SIZE);
        assert_eq!(core::mem::align_of::<Promise>(), 8);

        assert_eq!(core::mem::offset_of!(Promise, header), 0);
        assert_eq!(core::mem::offset_of!(Promise, state), 8);
        assert_eq!(core::mem::offset_of!(Promise, value_is_heap), 9);
        assert_eq!(core::mem::offset_of!(Promise, has_handler), 10);
        assert_eq!(core::mem::offset_of!(Promise, value_repr), 11);
        assert_eq!(core::mem::offset_of!(Promise, _pad), 12);
        assert_eq!(core::mem::offset_of!(Promise, value), 16);
        assert_eq!(core::mem::offset_of!(Promise, callbacks), 24);
    }

    #[test]
    fn cb_struct_layout() {
        // PromiseCb: fn-ptr (8) + i64 (8) + ptr (8) = 24B, align 8.
        assert_eq!(core::mem::size_of::<PromiseCb>(), 24);
        assert_eq!(core::mem::align_of::<PromiseCb>(), 8);
        assert_eq!(core::mem::offset_of!(PromiseCb, invoke), 0);
        assert_eq!(core::mem::offset_of!(PromiseCb, arg), 8);
        assert_eq!(core::mem::offset_of!(PromiseCb, next), 16);
    }

    #[test]
    fn state_values() {
        assert_eq!(STATE_PENDING, 0);
        assert_eq!(STATE_FULFILLED, 1);
        assert_eq!(STATE_REJECTED, 2);
        assert_eq!(TAG_PROMISE, 8);
    }
}
