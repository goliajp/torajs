//! `__torajs_any_index_method_call` — `recv[key](args…)` where the
//! receiver types as `any` and the key is a runtime value (RFC
//! 20260728-gen-forof-yieldstar F0b).
//!
//! ES §13.3.6.2 EvaluateCall: when the callee is a property
//! reference, thisValue is the reference's base — `o[k]()` is a
//! METHOD call on `o`, exactly like `o.m()`. Pre-entry the lowering
//! read the property as a value and bare-called it: a builtin
//! reified cell hit its this-undefined TypeError and a user objlit
//! method ran with no receiver (`this.x` → cannot read properties).
//!
//! Dispatch by the runtime key shape:
//! - **Str cell / ShortStr** — the §7.1.19 string key: intern the
//!   name through the shared mid table and re-enter the by-name
//!   method dispatch ([`crate::method_call::any_method_call_inner`]),
//!   which owns every receiver arm (builtin switch, dynobj
//!   user-method probe with its this plumbing, wrapper expandos,
//!   proto-patch consult).
//! - **Symbol cell** — probe the receiver's symbol-keyed face
//!   ([`crate::member_get_symbol::symbol_key_pair`], which ends in
//!   the F0 `@@iterator` builtin reify). A reified builtin cell
//!   re-dispatches its mid with THIS receiver; a user closure
//!   bare-calls its boxed entry (recorded residue: a user
//!   symbol-keyed method that reads `this` still sees undefined —
//!   the dynobj this-plumbing is name-keyed today); anything else
//!   is the standard not-callable TypeError.
//! - **everything else** (numbers, bool, nullish) — ToString per
//!   §7.1.19 step 3, then the string lane.
//!
//! The result follows the method-call convention (fresh owned
//! AnyValue); argv is borrowed.

use core::ffi::c_void;

use crate::method_call::{any_method_call_inner, not_callable};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_short_str};
use crate::nanbox_ffi_materialize::{drop_materialized_str, materialize_short_str};
use torajs_rc::Tag;

/// `AnySlotTag::Heap` in the member-pair protocol.
const TAG_HEAP: u64 = 4;

/// §13.3.6.2 `recv[key](args…)`.
///
/// # Safety
/// `recv` / `key` are live AnyValues; `argv` points at `argc` live
/// AnyValue slots (borrowed); `recv_slot` is NULL or the receiver's
/// variable slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_index_method_call(
    recv: AnyValue,
    key: AnyValue,
    recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    // Symbol key — §7.1.19 step 2, no coercion.
    if is_cell(key) {
        // SAFETY: is_cell guarantees a live header.
        let kt = unsafe { (as_void_ptr(key).cast::<u8>().add(4) as *const u16).read() };
        if kt == Tag::Symbol as u16 {
            return unsafe { symbol_keyed_call(recv, as_void_ptr(key), argv, argc) };
        }
        if kt == Tag::Str as u16 {
            let r = unsafe {
                str_keyed_call(recv, as_void_ptr(key) as *const u8, recv_slot, argv, argc)
            };
            return unsafe { settle_str_lane(r, recv, key, argv, argc) };
        }
    }
    if is_short_str(key) {
        // SAFETY: materializes a fresh rc=1 Str; dropped below.
        let s = unsafe { materialize_short_str(key) };
        let r = unsafe { str_keyed_call(recv, s as *const u8, recv_slot, argv, argc) };
        unsafe { drop_materialized_str(s) };
        return unsafe { settle_str_lane(r, recv, key, argv, argc) };
    }
    // Numeric / bool / nullish key — ToString (§7.1.19 step 3),
    // then the string lane. Fresh owned Str, released after.
    let s = unsafe { crate::nanbox_ffi::__torajs_anyv_to_str(key) };
    if s.is_null() {
        return unsafe { not_callable() };
    }
    let r = unsafe { str_keyed_call(recv, s as *const u8, recv_slot, argv, argc) };
    // SAFETY: to_str minted the cell for us.
    unsafe { crate::__torajs_str_drop(s as *mut c_void) };
    unsafe { settle_str_lane(r, recv, key, argv, argc) }
}

/// The string-key leg — intern the name and re-enter the by-name
/// dispatch. A mid-miss gets the builtin-proto patch consult (the
/// named form's §10.1.9.2 chain step); a full miss floats
/// `ANY_METHOD_NO_SUCH` back so the entry runs the value-call
/// fallback with the ORIGINAL key.
unsafe fn str_keyed_call(
    recv: AnyValue,
    name_cell: *const u8,
    recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    let mid = unsafe { crate::method_value::key_method_id(name_cell as *const c_void) };
    let r = unsafe { any_method_call_inner(recv, mid, name_cell, recv_slot, argv, argc) };
    if r == crate::method_call::ANY_METHOD_NO_SUCH
        && let Some(out) = unsafe {
            crate::method_call_proto_patch::builtin_proto_patch_method(
                recv, mid, name_cell, argv, argc,
            )
        }
    {
        return out;
    }
    r
}

/// Resolve a string-lane `ANY_METHOD_NO_SUCH` float: properties the
/// method-dispatch surface cannot see (an Array element under a
/// canonical-index key, a string-keyed slot only the indexed read
/// serves) get the §13.3.6.2 fallback — keyed property read, then the
/// call on the value with THE BASE AS RECEIVER (`arr[0](x)` is a
/// method call on `arr`; a non-callable read keeps the standard
/// TypeError).
///
/// The receiver half was missing while every other leg had it: a
/// dynobj property (`o[0]()`), an array's named expando (`arr.m()`)
/// and a string-keyed read (`o[k]()`) all answered the base, and only
/// the ARRAY ELEMENT — the one property this fallback exists for —
/// bare-called its value, so `this` read `undefined`. Same shape as
/// the symbol leg below: a receiver-first closure (a promoted fn-expr
/// body that says `this`) carries FLAG_CLOSURE_RECV_FIRST, and
/// `invoke_with_this` is what puts the receiver in argv[0]; a closure
/// without the flag ignores it, so nothing else moves.
unsafe fn settle_str_lane(
    r: AnyValue,
    recv: AnyValue,
    key: AnyValue,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    if r != crate::method_call::ANY_METHOD_NO_SUCH {
        return r;
    }
    // Owned read (+1 for cells); the call borrows the callee.
    let v = unsafe { crate::index_any_keyed::__torajs_any_index_get_keyed(recv, key) };
    let out = unsafe { call_with_base(v, recv, argv, argc) };
    unsafe { crate::nanbox_ffi::__torajs_anyv_rc_dec(v) };
    out
}

/// Call `v` with `base` as its `this` — the §13.3.6.2 method-call
/// convention for a property-reference callee.
///
/// Only a user closure carrying a boxed entry can be handed a receiver
/// through this path; everything else (builtin reified cells,
/// non-callables) keeps the bare-call answer it had, which is where
/// their own dispatch decides the receiver.
unsafe fn call_with_base(v: AnyValue, base: AnyValue, argv: *const u64, argc: i64) -> AnyValue {
    if is_cell(v) {
        let cell = as_void_ptr(v);
        // SAFETY: is_cell guarantees a live header.
        let ct = unsafe { (cell.cast::<u8>().add(4) as *const u16).read() };
        if ct == Tag::Closure as u16
            && unsafe { crate::method_value::builtin_method_mid(cell) }.is_none()
        {
            // SAFETY: the closure layout carries the boxed dual entry.
            let entry =
                unsafe { crate::method_call_closure_dispatch::boxed_entry_of(cell.cast::<u8>()) };
            if entry != 0 {
                let raw = unsafe {
                    crate::method_call_closure_dispatch::invoke_with_this(
                        cell, entry, base, argv, argc,
                    )
                };
                return if raw == crate::method_call::ANY_METHOD_NO_SUCH {
                    VALUE_UNDEFINED
                } else {
                    raw
                };
            }
        }
    }
    unsafe { crate::method_call_closure_dispatch::__torajs_any_call(v, argv, argc) }
}

/// The symbol-key leg — resolve through the symbol face (dict /
/// monkey-patch / F0 builtin reify), then invoke with the §13.3.6.2
/// receiver semantics a reified cell can honor today.
unsafe fn symbol_keyed_call(
    recv: AnyValue,
    key: *const c_void,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    // A Get, so an accessor-shaped method entry runs its getter: the
    // pair alone answers the sentinel, which this arm read as "not a
    // heap value" and turned into `not_callable` (RFC knife 2b).
    let (tag, value, owned) = unsafe { crate::member_get_symbol::symbol_key_get(recv, key) };
    if unsafe { __torajs_throw_check() } != 0 {
        return VALUE_UNDEFINED;
    }
    if tag != TAG_HEAP || value == 0 {
        // A non-heap answer carries no cell, so `owned` needs no release.
        if let Some(out) = unsafe { obj_symbol_iterator_call(recv, key) } {
            return out;
        }
        return unsafe { not_callable() };
    }
    let cell = value as *mut c_void;
    // The call has five exits below; a getter-produced cell has to
    // outlive all of them and be released once, so the dispatch moves
    // into its own fn and this one owns the release.
    let out = unsafe { call_resolved_symbol_method(recv, cell, argv, argc) };
    if owned {
        unsafe { __torajs_value_drop_heap(cell) };
    }
    out
}

/// The dispatch half of [`symbol_keyed_call`], split out so the caller
/// can release a getter-produced cell across every exit.
///
/// # Safety
/// `cell` is a live method cell; `recv` is a live AnyValue; `argv`
/// points at `argc` slots.
unsafe fn call_resolved_symbol_method(
    recv: AnyValue,
    cell: *mut c_void,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    // SAFETY: pair protocol hands out live cells.
    let ct = unsafe { (cell.cast::<u8>().add(4) as *const u16).read() };
    if ct != Tag::Closure as u16 {
        return unsafe { not_callable() };
    }
    // A reified builtin re-dispatches its mid against THIS receiver
    // (the `.call` short-circuit shape) — own-property resolution is
    // already done, so the expando-skip flavor is correct.
    if let Some(mid) = unsafe { crate::method_value::builtin_method_mid(cell) } {
        return unsafe { crate::method_call::any_method_redispatch(recv, mid, argv, argc) };
    }
    // User closure — boxed-entry call through the uniform dispatch
    // helpers (they pad argv into the 8-slot undefined buffer the
    // adapters read unconditionally). A receiver-first closure (a
    // promoted fn-expr body that says `this`) carries
    // FLAG_CLOSURE_RECV_FIRST on its env header — §13.3.6.2 method
    // semantics: this symbol-keyed store's receiver rides argv[0]
    // onto the `__this` param, user args shift up by one.
    // SAFETY: the closure layout carries the boxed dual entry.
    let entry = unsafe { crate::method_call_closure_dispatch::boxed_entry_of(cell.cast::<u8>()) };
    if entry == 0 {
        return unsafe { not_callable() };
    }
    let raw = unsafe {
        crate::method_call_closure_dispatch::invoke_with_this(cell, entry, recv, argv, argc)
    };
    if raw == crate::method_call::ANY_METHOD_NO_SUCH {
        return VALUE_UNDEFINED;
    }
    raw
}

/// `class_tag` u32 offset inside a `Tag::Obj` instance — mirror of
/// torajs-core `ssa_lower::OBJ_CLASS_TAG_OFF` (the iter_any mirror).
const OBJ_CLASS_TAG_OFF: usize = 8;

unsafe extern "C" {
    /// torajs-structmeta — class-methods dispatch table lookups
    /// (same pair the iter_any Obj lane uses).
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_method_find(
        layout: *const c_void,
        name: *const u8,
        name_len: u32,
    ) -> *const c_void;
    fn __torajs_rc_inc(p: *mut c_void);
    /// torajs-throw — did the symbol getter leave a pending throw?
    fn __torajs_throw_check() -> i64;
    /// torajs-rc — release a getter-produced method cell.
    fn __torajs_value_drop_heap(p: *mut c_void);
}

/// `src[Symbol.iterator]()` on a class instance — the symbol dict
/// walk misses (a class member keyed `[Symbol.iterator]` is folded to
/// the mangled vtable name; knife 4 of 20260725-getiterator-getmethod
/// retires the fold). Two arms, mirroring the for-of Obj lane:
/// a declared `[Symbol.iterator]()` invokes through the vtable with
/// THIS receiver; a generator object (no declared `@@iterator`, but a
/// vtable `next`) answers itself per §27.1.3.1
/// %IteratorPrototype%[@@iterator]. `None` = not this shape.
unsafe fn obj_symbol_iterator_call(recv: AnyValue, key: *const c_void) -> Option<AnyValue> {
    if key != crate::method_value::symbol_static::well_known_singleton(5) {
        return None;
    }
    if !is_cell(recv) {
        return None;
    }
    let obj = as_void_ptr(recv);
    // SAFETY: is_cell guarantees a live header.
    if unsafe { (obj.cast::<u8>().add(4) as *const u16).read() } != Tag::Obj as u16 {
        return None;
    }
    match unsafe { crate::iter_any::call_obj_method_0(obj, crate::iter_any::SYM_ITERATOR_METHOD) } {
        crate::iter_any::MethodOutcome::Ok(v) => Some(v),
        // The user body's throw is in flight — the caller's throw
        // check propagates it; the sentinel must not be read.
        crate::iter_any::MethodOutcome::Threw => Some(VALUE_UNDEFINED),
        crate::iter_any::MethodOutcome::Missing => {
            let class_tag = unsafe { obj.cast::<u8>().add(OBJ_CLASS_TAG_OFF).cast::<u32>().read() };
            let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
            if layout.is_null() {
                return None;
            }
            let next = unsafe { __torajs_struct_method_find(layout, c"next".as_ptr().cast(), 4) };
            if next.is_null() {
                return None;
            }
            // Owned result convention — the self-answer takes a stake.
            unsafe { __torajs_rc_inc(obj) };
            Some(recv)
        }
    }
}
