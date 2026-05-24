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
//!   +10..15 : _pad[6] (alignment)
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

/// `__torajs_str_alloc_pooled` returns a pointer offset HDR_SIZE
/// past the Str header; allsettled's status field copy uses this
/// offset to memcpy the literal into the data segment. Mirrors
/// `__TORAJS_STR_HDR_SIZE = 16` in runtime_str.c.
pub const STR_HDR_SIZE: usize = 16;

/// Array<T> layout offsets used by Promise.all / .race / .allSettled
/// / .any walks. Re-declared here so torajs-promise doesn't pull in
/// a torajs-arr dep — same independent-layout pattern the original
/// runtime_promise.c used.
pub const ARR_HDR_SIZE: usize = 24;
pub const ARR_LEN_OFF: usize = 8;
pub const ARR_HEAD_OFF: usize = 20;

/// allsettled inner-struct constants. The MVP packs every settled
/// outcome into the same `{status: string, value: number}` Obj
/// (matches the C code's `__TORAJS_OBJ_HEADER_SIZE_AS = 24`).
pub const ALLSETTLED_OBJ_TAG: u16 = 1;
pub const ALLSETTLED_OBJ_HEADER_SIZE: usize = 24;

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
    pub _pad: [u8; 6],
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
        assert_eq!(core::mem::offset_of!(Promise, _pad), 10);
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
