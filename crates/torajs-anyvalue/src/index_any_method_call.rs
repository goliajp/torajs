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

/// Closure-env boxed-entry slot — `method_value::CLOSURE_BOXED_ENTRY_OFF`
/// mirror (the universal closure layout).
const CLOSURE_BOXED_ENTRY_OFF: usize = 32;

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
            return unsafe {
                str_keyed_call(recv, as_void_ptr(key) as *const u8, recv_slot, argv, argc)
            };
        }
    }
    if is_short_str(key) {
        // SAFETY: materializes a fresh rc=1 Str; dropped below.
        let s = unsafe { materialize_short_str(key) };
        let out = unsafe { str_keyed_call(recv, s as *const u8, recv_slot, argv, argc) };
        unsafe { drop_materialized_str(s) };
        return out;
    }
    // Numeric / bool / nullish key — ToString (§7.1.19 step 3),
    // then the string lane. Fresh owned Str, released after.
    let s = unsafe { crate::nanbox_ffi::__torajs_anyv_to_str(key) };
    if s.is_null() {
        return unsafe { not_callable() };
    }
    let out = unsafe { str_keyed_call(recv, s as *const u8, recv_slot, argv, argc) };
    // SAFETY: to_str minted the cell for us.
    unsafe { crate::__torajs_str_drop(s as *mut c_void) };
    out
}

/// The string-key leg — intern the name and re-enter the by-name
/// dispatch. A mid-miss gets the builtin-proto patch consult (the
/// named form's §10.1.9.2 chain step) before the standard
/// TypeError, so `o["m"]()` and `o.m()` resolve identically.
unsafe fn str_keyed_call(
    recv: AnyValue,
    name_cell: *const u8,
    recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    let mid = unsafe { crate::method_value::key_method_id(name_cell as *const c_void) };
    let r = unsafe { any_method_call_inner(recv, mid, name_cell, recv_slot, argv, argc) };
    if r == crate::method_call::ANY_METHOD_NO_SUCH {
        if let Some(out) = unsafe {
            crate::method_call_proto_patch::builtin_proto_patch_method(
                recv, mid, name_cell, argv, argc,
            )
        } {
            return out;
        }
        return unsafe { not_callable() };
    }
    r
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
    let (tag, value) = unsafe { crate::member_get_symbol::symbol_key_pair(recv, key) };
    if tag != TAG_HEAP || value == 0 {
        return unsafe { not_callable() };
    }
    let cell = value as *mut c_void;
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
    // User closure — boxed-entry call (recorded residue: symbol-keyed
    // user methods have no this-plumbing yet; `this`-free bodies work).
    // SAFETY: the closure layout carries the boxed dual entry.
    let entry = unsafe { *(cell.cast::<u8>().add(CLOSURE_BOXED_ENTRY_OFF) as *const u64) };
    if entry == 0 {
        return unsafe { not_callable() };
    }
    let f: unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64 =
        // SAFETY: non-zero boxed entries are function addresses by
        // the closure-layout contract.
        unsafe { core::mem::transmute(entry) };
    let raw = unsafe { f(cell, argv, argc) };
    if raw == crate::method_call::ANY_METHOD_NO_SUCH {
        return VALUE_UNDEFINED;
    }
    raw
}
