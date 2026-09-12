//! RFC 20260911-global-object-record — the three operators that can
//! reach a bare name with no static lane, asked of the global object.
//!
//! The sibling next door answers "how is the singleton built and
//! filled"; this file answers "what does an unresolvable name get from
//! it". §9.1.1 gives global code an environment record with two
//! halves, and only the declarative half is decidable when tr
//! compiles — whether the OBJECT half carries a name is a fact about
//! the run, because `this.p = 1` in global code puts it there.
//!
//! The three differ only in what a miss means: a read throws
//! (§9.1.1.4 GetBindingValue, `S` = false), `typeof` answers
//! `"undefined"` (§13.5.1.1), and a write creates the property
//! (§9.1.1.4.6 SetMutableBinding).
//!
//! Only the write mints. See [`super::globalthis_object::
//! globalthis_bag_ptr`] for why that split exists.

use core::ffi::c_void;
use core::sync::atomic::Ordering;

use crate::AnyValue;

use super::globalthis_object::{GLOBALTHIS_OBJECT, globalthis_bag_ptr};

unsafe extern "C" {
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_set_soft(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
    ) -> i64;
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_rc_dec(p: *mut c_void) -> i32;
    fn __torajs_throw_reference_error_name(name: *mut c_void);
    fn __torajs_str_alloc(p: *const u8, len: i64) -> *mut u8;
    fn __torajs_anyv_unbox_tag(v: AnyValue) -> i64;
    fn __torajs_anyv_unbox_value(v: AnyValue) -> i64;
    fn __torajs_accessor_invoke_getter(pair: *const c_void, recv_anyv: u64) -> u64;
}

/// The `__torajs_anyv_box_from_pair` tag for a heap payload — the one
/// tag whose value the dynobj bag hands out as a BORROW.
const PAIR_TAG_HEAP: u64 = 4;

/// The `__torajs_anyv_box_from_pair` tag for `undefined`.
const PAIR_TAG_UNDEFINED: u64 = 5;

/// The `get_tag` sentinel for an accessor entry: the value is the
/// `AccessorPair` cell, not a value.
const PAIR_TAG_ACCESSOR: u64 = 6;

/// §9.1.1.4 GetBindingValue on the OBJECT half of the global
/// environment record, with `S` = false.
///
/// A bare name that resolves to no binding is not automatically a
/// ReferenceError under the script goal: the global environment
/// record's object record is consulted first, and only a name the
/// global object does not carry throws. `this.p = 1` in global code
/// writes THROUGH that object (`ast::toplevel_this` binds top-level
/// `this` to this singleton under the sloppy goal), so `p` has to read
/// it back.
///
/// The caller is `ssa_lower_ident::try_undeclared_read_throw`, and the
/// name it passes is the checker's own verdict — an ExprId recorded in
/// `undeclared_reads` (RFC 20260730). That record is exact by
/// construction, which is the whole reason the lookup lives here
/// rather than in an AST pass guessing at the same question ahead of
/// the checker.
///
/// The throw is RECORDED, not raised: `__torajs_throw_reference_error_
/// name` parks a pending throw and returns, and the caller's
/// `emit_throw_check` propagates it. The `undefined` handed back on
/// that path is never used.
///
/// # Safety
///
/// `key` must be a valid Str block pointer, borrowed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_global_ident_get(key: *mut c_void) -> AnyValue {
    // Read the singleton, do NOT mint it. Minting drags in the whole
    // ns-static world (the `Math` / `JSON` / console singletons and
    // the interned ctor cells the fill list needs), and the linker
    // strips that world from a program with no `globalThis` in it —
    // so minting HERE, from a site every undeclared read reaches,
    // answered `namespace-static world stripped (dispatch judgment
    // bug)` instead of a ReferenceError. Not minting also happens to
    // be the right semantics: a program that never wrote through the
    // global object has nothing there for a bare name to find, and
    // every name that IS pre-filled (`Array`, `Math`, …) has a static
    // lane and never reaches this kernel.
    let obj = GLOBALTHIS_OBJECT.load(Ordering::Acquire) as *mut c_void;
    // SAFETY: `obj` is the singleton or null (checked); `key` is a Str
    // block per the caller invariant, and every callee below borrows
    // both.
    unsafe {
        if !obj.is_null() && __torajs_dynobj_has(obj, key) != 0 {
            let tag = __torajs_dynobj_get_tag(obj, key);
            let value = __torajs_dynobj_get_value(obj, key);
            // §7.3.2 Get: an accessor's getter runs, with the global
            // object as `this` (borrowed); its result is owned.
            if tag == PAIR_TAG_ACCESSOR {
                return __torajs_accessor_invoke_getter(
                    value as *const c_void,
                    crate::nanbox::box_void_ptr(obj),
                );
            }
            // The bag hands out a borrow; the value escapes into the
            // program, so a heap payload needs a stake of its own.
            if tag == PAIR_TAG_HEAP {
                __torajs_rc_inc(value as *mut c_void);
            }
            return crate::nanbox_encode::__torajs_anyv_box_from_pair(tag as i64, value as i64);
        }
        __torajs_throw_reference_error_name(key);
        crate::nanbox_encode::__torajs_anyv_box_from_pair(PAIR_TAG_UNDEFINED as i64, 0)
    }
}

/// §13.5.1.1 — `typeof` on a name that resolves to no binding answers
/// `"undefined"` instead of throwing, so it asks the object half of
/// the record the same question [`__torajs_global_ident_get`] does and
/// falls back to the string rather than the throw.
///
/// Folding it to the constant at compile time was right while nothing
/// could write through the global object; it stops being right the
/// moment `this.p = 1` in global code can, because then `typeof p` has
/// to answer `"number"`.
///
/// Does not mint, for the reason the getter does not — see there.
///
/// # Safety
///
/// `key` must be a valid Str block pointer, borrowed. The returned Str
/// is owned by the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_global_ident_typeof(key: *mut c_void) -> *mut u8 {
    let obj = GLOBALTHIS_OBJECT.load(Ordering::Acquire) as *mut c_void;
    // SAFETY: `obj` is the singleton or null (checked); `key` is a Str
    // block per the caller invariant, borrowed by every callee.
    unsafe {
        if !obj.is_null() && __torajs_dynobj_has(obj, key) != 0 {
            let tag = __torajs_dynobj_get_tag(obj, key);
            let value = __torajs_dynobj_get_value(obj, key);
            let boxed = crate::nanbox_encode::__torajs_anyv_box_from_pair(tag as i64, value as i64);
            return crate::inspect::typeof_any::__torajs_anyv_typeof(boxed);
        }
        __torajs_str_alloc(b"undefined".as_ptr(), 9)
    }
}

/// §9.1.1.4.6 SetMutableBinding with `S` = false — an assignment to a
/// name that resolves to no binding CREATES the property on the global
/// object, at run time.
///
/// tr answered that statically until now, by synthesizing a hoisted
/// `var` per never-declared assignment target. That is unconditional
/// where the write is not, and `with (o) { foo = 42 }` creates `foo`
/// only when the object does not already carry it — which no pass
/// ahead of the run can know. So the write lands here instead.
///
/// This one DOES mint (unlike the getter and the typeof): it has
/// somewhere to put the value, which a null singleton does not
/// provide. That is also why the read side can stay cheap — nothing
/// can be there to find unless a write like this put it there.
///
/// An ordinary [[Set]], not a define: §9.1.1.4.6 creates the property
/// with the default attributes (writable / enumerable / configurable
/// all true), which is what [[Set]] on a fresh key already does, and
/// what node's script goal answers for `p = 3`.
///
/// The define may MOVE the object as it grows — the slot is how the
/// callee hands the new address back — so the singleton is restored
/// from the slot afterwards.
///
/// # Safety
///
/// `key` must be a valid Str block pointer, borrowed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_global_ident_set(key: *mut c_void, v: AnyValue) -> AnyValue {
    let mut obj = globalthis_bag_ptr();
    // SAFETY: `obj` is the minted singleton, `key` a Str block per the
    // caller invariant; the set borrows the key and takes a stake on a
    // heap payload, which is minted just above it.
    unsafe {
        let tag = __torajs_anyv_unbox_tag(v) as u64;
        let value = __torajs_anyv_unbox_value(v) as u64;
        // The slot keeps the value, so a heap payload gets a stake of
        // its own — the argument stays the caller's.
        if tag == PAIR_TAG_HEAP {
            __torajs_rc_inc(value as *mut c_void);
        }
        // Only SLOPPY code reaches this kernel (a strict unresolvable
        // write is the ReferenceError at the site), and §6.2.5.6
        // PutValue with `S` = false discards a refused [[Set]] — a
        // non-extensible global object, a read-only property — rather
        // than throwing. A refusal leaves the stake with nobody, so it
        // is released here.
        if __torajs_dynobj_set_soft(&mut obj, key, tag, value) == 0 && tag == PAIR_TAG_HEAP {
            __torajs_rc_dec(value as *mut c_void);
        }
    }
    GLOBALTHIS_OBJECT.store(obj as u64, Ordering::Release);
    v
}
