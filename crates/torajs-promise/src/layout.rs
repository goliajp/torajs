//! Promise heap-block layout + state constants + callback record.
//!
//! Mirrors `runtime_promise.c`'s 1:1 — every byte offset matches so
//! ABI-compat is preserved at the `value_drop_heap` dispatch (TAG=8)
//! and at every C-side caller (e.g. ssa_lower's emitted dispatcher
//! functions read `pp->state` / `pp->value` directly).
//!
//! ## Promise struct (64 bytes)
//!
//! ```text
//!   +0..7   : universal heap header (refcount u32 + type_tag u16 + flags u16)
//!   +8      : state u8 (PENDING=0, FULFILLED=1, REJECTED=2)
//!   +9      : value_is_heap u8 — set on heap-typed Promise<Heap T>
//!   +10     : has_handler u8 — set by attach_then / get_value (P10.5-A3-a);
//!             read by HPRT-check microtask to decide unhandled-rejection
//!             reporting per spec §27.2.1.9 HostPromiseRejectionTracker
//!   +11     : value_repr u8 — storage form of the value slot
//!   +12     : deferred_state u8 — a combinator result's target state
//!             while its settle microtask is in flight
//!   +13     : rx_kind u8 — inline reaction kind (0 = none)
//!   +14     : rx_ret_repr u8 — inline reaction handler's return repr
//!   +15     : rx_param_repr u8 — inline reaction handler's param repr
//!   +16..23 : i64 value (primitive bits or heap-ptr cast)
//!   +24..31 : callback list head (NULL when no reaction attached)
//!   +32..39 : *mut props expando dynobj (NULL until the first
//!             `Object.defineProperty(promise, …)` — the Closure /
//!             class-instance lazy-expando shape; §27.2.4.1.3's
//!             per-element `then` GET/INVOKE observation reads it)
//!   +40..47 : rx_next — this cell's link in its SOURCE's callback list
//!   +48..55 : rx_source — the promise this cell reacts to (+1 stake)
//!   +56..63 : rx_cb — the handler: fn pointer, or closure env (+1 stake)
//! ```
//!
//! ## Inline reaction (`rx_*`)
//!
//! `p.then(f)` / `p.catch(f)` mint a derived promise and park the
//! reaction INSIDE it, the way JSC's `JSPromise` keeps a single
//! handler inline: the derived cell is pending, so its `value` slot
//! is idle and its three trailing words hold the list link, the
//! source and the handler. One allocation per link of a chain where
//! the arg block + list node form needed three. The dispatcher
//! (`reaction::inline_reaction_dispatch`) clears the `rx_*` words
//! before running the handler.
//!
//! ## Callback list
//!
//! `callbacks` heads a singly-linked list whose links are TAGGED
//! words ([`CB_INLINE_TAG`]): low bit set = the link points at a
//! derived promise carrying an inline reaction (its own `rx_next`
//! continues the list); low bit clear = a heap [`PromiseCb`] record
//! whose `next` continues it. The two forms interleave freely, so a
//! second `then` on a pending source head-pushes without spilling
//! the first into a node. Every cell is 8-aligned, so the bit is
//! free; the pool's parked-cell freelist reuses `callbacks` as an
//! untagged link.
//!
//! ## Callback chain (PromiseCb)
//!
//! Singly-linked list head-pushed onto `Promise.callbacks` by
//! `attach_then`. Each node carries:
//!
//! ```text
//!   +0      : invoke (extern "C" fn(int64_t)) — the dispatcher
//!   +8      : arg (i64) — packed dispatcher state
//!   +16     : next ptr (NULL = end of list)
//!   +24     : ops — NULL, or how the runtime sees into `arg`
//!             (`reaction::record`)
//! ```
//!
//! On `resolve` / `reject`, `drain_callbacks` enqueues each node's
//! `(invoke, arg)` and frees the node.

use core::ffi::c_void;

/// `type_tag` value for Promise heap blocks (matches
/// `runtime_promise.c::__TORAJS_TAG_PROMISE = 8` + the
/// `runtime_str.c::value_drop_heap` dispatch case).
pub const TAG_PROMISE: u16 = 8;

/// Promise lifecycle states (matches `runtime_promise.c` macros).
pub const STATE_PENDING: u8 = 0;
pub const STATE_FULFILLED: u8 = 1;
pub const STATE_REJECTED: u8 = 2;

/// `sizeof(Promise)` — 64 bytes: the 40-byte settled face plus the
/// three inline-reaction words. mmalloc serves 40 and 64 from the
/// same size class, so the growth costs no memory per cell.
pub const PROMISE_SIZE: usize = 64;

/// Low bit of a callback-list link that marks an inline reaction
/// (the link, with the bit cleared, is a derived `Promise`).
pub const CB_INLINE_TAG: usize = 1;

/// `rx_kind` codes. Bits 0-1 select the handler's interest, bit 2
/// says `rx_cb` is a closure env (owned, released after the call),
/// bit 3 says the handler's first parameter is `any` (the settled
/// value is boxed per the source's repr before the call).
pub const RX_NONE: u8 = 0;
/// `.then(onFulfilled)` — fires on FULFILLED, forwards REJECTED.
pub const RX_THEN: u8 = 1;
/// `.catch(onRejected)` — fires on REJECTED, forwards FULFILLED.
pub const RX_CATCH: u8 = 2;
/// See [`RX_NONE`].
pub const RX_INTEREST_MASK: u8 = 3;
/// See [`RX_NONE`].
pub const RX_CLOSURE: u8 = 4;
/// See [`RX_NONE`].
pub const RX_PARAM_ANY: u8 = 8;

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
    /// A combinator result's target state while its deferred settle
    /// microtask is in flight (`combinator_settle::defer_settle`);
    /// 0 otherwise.
    pub deferred_state: u8,
    /// Inline reaction kind — [`RX_NONE`] when this cell carries no
    /// unfired reaction; see the module doc.
    pub rx_kind: u8,
    /// Inline reaction: the handler call site's return repr.
    pub rx_ret_repr: u8,
    /// Inline reaction: the handler's declared first-parameter repr
    /// (0 when the parameter is `any` or has no repr code).
    pub rx_param_repr: u8,
    pub value: i64,
    /// Tagged list head — see the module doc's "Callback list".
    pub callbacks: *mut PromiseCb,
    /// Lazy own-property expando dynobj — NULL until the first
    /// `Object.defineProperty(promise, …)` lands (the dynobj
    /// `define_apply` Promise arm allocates it, the Closure-arm
    /// shape). A user `then` override stored here is what the
    /// combinators' per-element §27.2.4.1.3 step 6.q-s observation
    /// and the any-lane method dispatch consult before the builtin.
    pub props: *mut c_void,
    /// Inline reaction: this cell's (tagged) link in its source's
    /// callback list.
    pub rx_next: *mut PromiseCb,
    /// Inline reaction: the source promise; the cell holds one
    /// reference on it until the reaction fires.
    pub rx_source: *mut c_void,
    /// Inline reaction: the handler — a `ThenCbI64` fn pointer, or
    /// (with [`RX_CLOSURE`]) an owned closure env cell.
    pub rx_cb: *mut c_void,
}

/// Tag `derived` as an inline-reaction link.
#[inline]
pub fn inline_link(derived: *mut Promise) -> *mut PromiseCb {
    (derived as usize | CB_INLINE_TAG) as *mut PromiseCb
}

/// The derived promise an inline-reaction link names, or NULL when
/// `link` is a heap record (or the list end).
#[inline]
pub fn inline_link_derived(link: *mut PromiseCb) -> *mut Promise {
    if link as usize & CB_INLINE_TAG != 0 {
        (link as usize & !CB_INLINE_TAG) as *mut Promise
    } else {
        core::ptr::null_mut()
    }
}

/// Promise callback record: a dispatcher and the packed state it runs
/// on. `arg` is opaque unless `ops` describes it.
#[repr(C)]
pub struct PromiseCb {
    pub invoke: MicrotaskFn,
    pub arg: i64,
    pub next: *mut PromiseCb,
    pub ops: *const crate::reaction::record::ReactionOps,
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
        assert_eq!(core::mem::offset_of!(Promise, deferred_state), 12);
        assert_eq!(core::mem::offset_of!(Promise, rx_kind), 13);
        assert_eq!(core::mem::offset_of!(Promise, rx_ret_repr), 14);
        assert_eq!(core::mem::offset_of!(Promise, rx_param_repr), 15);
        assert_eq!(core::mem::offset_of!(Promise, value), 16);
        assert_eq!(core::mem::offset_of!(Promise, callbacks), 24);
        assert_eq!(core::mem::offset_of!(Promise, props), 32);
        assert_eq!(core::mem::offset_of!(Promise, rx_next), 40);
        assert_eq!(core::mem::offset_of!(Promise, rx_source), 48);
        assert_eq!(core::mem::offset_of!(Promise, rx_cb), 56);
    }

    #[test]
    fn inline_link_round_trips_and_leaves_records_alone() {
        let d = 0x1000 as *mut Promise;
        let link = inline_link(d);
        assert_eq!(link as usize, 0x1001);
        assert_eq!(inline_link_derived(link), d);
        assert!(inline_link_derived(0x2000 as *mut PromiseCb).is_null());
        assert!(inline_link_derived(core::ptr::null_mut()).is_null());
    }

    #[test]
    fn cb_struct_layout() {
        assert_eq!(core::mem::size_of::<PromiseCb>(), 32);
        assert_eq!(core::mem::align_of::<PromiseCb>(), 8);
        assert_eq!(core::mem::offset_of!(PromiseCb, invoke), 0);
        assert_eq!(core::mem::offset_of!(PromiseCb, arg), 8);
        assert_eq!(core::mem::offset_of!(PromiseCb, next), 16);
        assert_eq!(core::mem::offset_of!(PromiseCb, ops), 24);
    }

    #[test]
    fn state_values() {
        assert_eq!(STATE_PENDING, 0);
        assert_eq!(STATE_FULFILLED, 1);
        assert_eq!(STATE_REJECTED, 2);
        assert_eq!(TAG_PROMISE, 8);
    }
}
