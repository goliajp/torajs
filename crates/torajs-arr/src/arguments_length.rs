//! The materialized `arguments` object's `"length"` face — §10.4.4
//! gives an arguments exotic object a PLAIN data `length`
//! ({writable: true, enumerable: false, **configurable: true**}),
//! where an Array's length is the §10.4.2 non-configurable exotic
//! slot. Both desugar lanes mint the `__torajs_arguments` local as a
//! `Tag::Arr` cell, so the distinction lives in
//! [`FLAG_ARR_ARGUMENTS`], stamped by the lowering right after the
//! mint; the keyed readers (gOPD / delete / hasOwnProperty /
//! member-get) gate on it.
//!
//! A delete leaves a HOLE shadow entry under the `"length"` key in
//! the expando props dynobj — the element-domain tombstone
//! representation (RFC 20260712 chunk C), so every enumerator and
//! bag reader already treats it as absent. A later
//! `defineProperty(args, "length", …)` restore is a recorded face
//! (L3b) — the t262 verifyProperty probe deletes without the restore
//! option.

use core::ffi::c_void;

use torajs_rc::{FLAG_ARR_ARGUMENTS, arr_flags_are_arguments};

use crate::define::{header_flags, props_slot};

unsafe extern "C" {
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_has(dynobj: *const c_void, key: *const c_void) -> i32;
    /// torajs-dynobj — HOLE sentinel upsert / probe (chunk C). The
    /// upsert is a no-op on a NULL bag — the delete below allocates
    /// the lazy expando first (the `store_shadow` idiom).
    fn __torajs_dynobj_set_entry_hole(obj_slot: *mut *mut c_void, key: *mut c_void);
    fn __torajs_dynobj_entry_is_hole(dynobj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_dynobj_delete(dynobj: *mut c_void, key: *const c_void) -> i32;
    /// torajs-dynobj — §10.1.6.3 [[DefineOwnProperty]] (throwing
    /// flavor); the value's stake transfers under `P_VALUE`.
    fn __torajs_dynobj_define(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// NaN-box `undefined` immediate (torajs-anyvalue encoding mirror —
/// `join.rs` keeps the same local constant).
const VALUE_UNDEFINED_IMM: u64 = 0x0000_0000_0000_000A;

/// §10.4.4.6 step 21 — the strict `arguments.callee` read: the
/// %ThrowTypeError% getter. The desugar rewrites the direct
/// `arguments.callee` spelling to this no-arg call; the escaped
/// keyed read answers the accessor descriptor through the gOPD
/// arguments arm instead (torajs-meta `arr_reflect.rs`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arguments_callee() -> u64 {
    unsafe {
        __torajs_throw_type_error(
            c"'callee' may not be accessed on an arguments object in strict mode".as_ptr(),
        )
    };
    VALUE_UNDEFINED_IMM
}

/// Stamp the freshly minted `__torajs_arguments` cell — called once
/// per materialization by both lowering lanes (the argv-form
/// expansion and the literal-form mark statement). Idempotent.
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer (or NULL, a no-op).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_mark_arguments(arr: *mut c_void) {
    if arr.is_null() {
        return;
    }
    let fp = unsafe { (arr as *mut u8).add(6) }.cast::<u16>();
    let flags = unsafe { fp.read() };
    // The identity is only read together with FLAG_ARR_ANY. Every mint
    // with elements is an `any[]` literal; a zero-argument call mints
    // `[]`, which comes out as a slot-less cell with no element kind —
    // nothing to reinterpret, so it simply becomes an Any cell.
    let len = unsafe { (arr.cast::<u8>().add(crate::layout::ARR_LEN_OFF) as *const u64).read() };
    debug_assert!(
        flags & torajs_rc::FLAG_ARR_ANY != 0 || len == 0,
        "arguments mint with elements that is not an Any cell"
    );
    let any = if len == 0 { torajs_rc::FLAG_ARR_ANY } else { 0 };
    unsafe { fp.write(flags | any | FLAG_ARR_ARGUMENTS) };
}

/// Bare FLAG_ARR_ARGUMENTS probe — the `callee` descriptor arm and
/// any other keyed reader that needs the identity without the
/// length-tombstone question.
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_is_arguments(arr: *const c_void) -> i64 {
    unsafe { arr_flags_are_arguments(header_flags(arr)) as i64 }
}

/// The `"length"` face state of an Arr cell: 0 = plain array (the
/// §10.4.2 non-configurable length), 1 = arguments materialization
/// with its length intact, 2 = arguments materialization whose
/// length was deleted (the hole tombstone is present), 3 = arguments
/// materialization whose length was written (the value is a live entry
/// in the props bag; see [`__torajs_arr_arguments_length_set`]).
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer; `key` is the caller's
/// live `"length"` Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_arguments_length_state(
    arr: *const c_void,
    key: *const c_void,
) -> i64 {
    unsafe {
        if !arr_flags_are_arguments(header_flags(arr)) {
            return 0;
        }
        let props = *props_slot(arr as *mut c_void);
        if props.is_null() || __torajs_dynobj_has(props, key) == 0 {
            return 1;
        }
        if __torajs_dynobj_entry_is_hole(props, key) != 0 {
            2
        } else {
            3
        }
    }
}

/// `Set(args, "length", v)` on an arguments materialization — §10.4.4
/// gives it an ORDINARY data `length`, so the write stores `v` as it
/// is; ArraySetLength (ToUint32, RangeError on a non-length value,
/// element truncation) is the Array's rule, not this object's. The
/// value moves into the props bag and the state becomes 3, where every
/// keyed reader delegates to the bag entry. The element count the cell
/// itself carries is untouched. Answers 0 for a plain array (the caller
/// runs ArraySetLength); 1 when handled, with the value's stake
/// transferred. A refusal (a length redefined non-writable) records
/// the TypeError the dynobj define raises.
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer; `key` is a live `"length"`
/// Str cell; `(tag, value)` carries the caller's +1 on a heap payload.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_arguments_length_set(
    arr: *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
) -> i64 {
    use crate::define::{F_CONFIGURABLE, F_ENUMERABLE, F_WRITABLE, P_CONFIGURABLE};
    use crate::define::{P_ENUMERABLE, P_VALUE, P_WRITABLE};
    unsafe {
        let flags_byte = match __torajs_arr_arguments_length_state(arr, key) {
            0 => return 0,
            // The mint's own entry: {writable, !enumerable, configurable}.
            1 => P_VALUE | P_WRITABLE | F_WRITABLE | P_ENUMERABLE | P_CONFIGURABLE | F_CONFIGURABLE,
            // Deleted: OrdinarySet creates a fresh all-true property.
            2 => {
                P_VALUE
                    | P_WRITABLE
                    | F_WRITABLE
                    | P_ENUMERABLE
                    | F_ENUMERABLE
                    | P_CONFIGURABLE
                    | F_CONFIGURABLE
            }
            // Already in the bag: a value-only define keeps its attributes
            // and refuses a non-writable one.
            _ => P_VALUE,
        };
        let slot = props_slot(arr);
        if (*slot).is_null() {
            *slot = __torajs_dynobj_alloc();
        } else if flags_byte & F_ENUMERABLE != 0 {
            // State 2: the tombstone is an absence, not an entry the
            // fresh create should merge with.
            __torajs_dynobj_delete(*slot, key as *const c_void);
        }
        __torajs_dynobj_define(slot, key, tag, value, flags_byte);
        1
    }
}

/// RFC 20260810-sloppy-goal-arguments S2 — the `"callee"` face state
/// of an arguments materialization: 0 = no bag entry (a strict mint —
/// the poisoned accessor / non-configurable own; also any plain
/// array), 1 = live sloppy entry (the mint's defineProperty seed,
/// possibly rewritten), 2 = deleted (hole tombstone).
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer; `key` is the caller's
/// live `"callee"` Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_arguments_callee_state(
    arr: *const c_void,
    key: *const c_void,
) -> i64 {
    unsafe {
        if !arr_flags_are_arguments(header_flags(arr)) {
            return 0;
        }
        let props = *props_slot(arr as *mut c_void);
        if props.is_null() || __torajs_dynobj_has(props, key) == 0 {
            return 0;
        }
        if __torajs_dynobj_entry_is_hole(props, key) != 0 {
            return 2;
        }
        1
    }
}

/// `delete args.callee` under the sloppy goal — a configurable data
/// property deletes, leaving the hole tombstone so the keyed readers
/// (has / gOPD / member-get) can tell "deleted" from "strict mint".
/// Answers 1 when handled (deleted now, or already a tombstone);
/// -1 when not applicable — a strict mint or plain array, where the
/// caller keeps its existing path.
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer; `key` is a live
/// `"callee"` Str cell (borrowed — the hole upsert takes its own
/// stake through the dynobj entry mint).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_arguments_callee_delete(
    arr: *mut c_void,
    key: *mut c_void,
) -> i64 {
    unsafe {
        match __torajs_arr_arguments_callee_state(arr as *const c_void, key as *const c_void) {
            1 => {
                __torajs_dynobj_set_entry_hole(props_slot(arr), key);
                1
            }
            2 => 1,
            _ => -1,
        }
    }
}

/// `delete args.length` — a configurable data property deletes
/// (§10.4.4 has no length special-case), leaving the hole tombstone.
/// Answers 1 when the cell is an arguments materialization; 0 for a
/// plain array (the caller keeps the §10.4.2 refusal).
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer; `key` is a live
/// `"length"` Str cell (borrowed — the hole upsert takes its own
/// stake through the dynobj entry mint).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_arguments_length_delete(
    arr: *mut c_void,
    key: *mut c_void,
) -> i64 {
    unsafe {
        if !arr_flags_are_arguments(header_flags(arr as *const c_void)) {
            return 0;
        }
        let slot = props_slot(arr);
        if (*slot).is_null() {
            *slot = __torajs_dynobj_alloc();
        }
        __torajs_dynobj_set_entry_hole(slot, key);
        1
    }
}
