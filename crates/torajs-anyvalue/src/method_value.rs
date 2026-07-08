//! Builtin method reification (chunk 711) — `s.toUpperCase` read as
//! a VALUE off an `any` builtin receiver answers a real function
//! object instead of undefined (the chunk 521 recorded boundary).
//!
//! ES semantics: extracting a builtin method yields the UNBOUND
//! `Function.prototype` member — `typeof f` is "function",
//! `f === s.toUpperCase` is true (same function object), a bare
//! `f()` runs with `this = undefined` and throws (every builtin
//! brand-checks its receiver), and `f.call(recv, …)` /
//! `f.apply(recv, list)` re-binds the receiver.
//!
//! The projection: one interned immortal cell per method id —
//!
//! - Layout is a capture-less closure env (universal header +
//!   fn_addr + drop_fn + props + boxed_entry + one capture slot
//!   holding the method id), so every existing callable probe
//!   (`typeof` → "function", `closure_boxed_entry`, HOF callbacks,
//!   expando reads/writes, strict-eq pointer identity) works
//!   unchanged.
//! - The header carries [`FLAG_STATIC_LITERAL`] — rc traffic
//!   no-ops, the cycle collector skips it, the cell never drops
//!   (CPython-immortal shape). The borrow-shaped member-get pair
//!   hands it out without any ledger.
//! - `boxed_entry` points at [`bare_entry`] — a bare call (direct,
//!   HOF callback, any-call) is the ES `this = undefined` TypeError.
//! - `fn_addr` points at [`native_entry`] — an any→typed fn-slot
//!   cast that direct-calls the native entry throws instead of
//!   jumping to 0 (recorded boundary: the typed-tier result value
//!   is garbage until the pending throw propagates).
//! - `f.call` / `f.apply` on the cell short-circuit in
//!   `method_call_closure` via [`builtin_method_mid`] and
//!   re-dispatch the ORIGINAL method id with the thisArg as the
//!   receiver.
//!
//! [`builtin_method_supported`] is the exact per-receiver-shape
//! support table (mirrors each `method_call_*` arm's id-switch) —
//! a wrong-arm read (`(42 as any).slice`) stays undefined like bun,
//! never an optimistic function that would TypeError on call.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use torajs_rc::{
    ANY_METHOD_ADD, ANY_METHOD_APPLY, ANY_METHOD_BIND, ANY_METHOD_CALL, ANY_METHOD_CHAR_AT,
    ANY_METHOD_CLEAR, ANY_METHOD_DELETE, ANY_METHOD_ENDS_WITH, ANY_METHOD_ENTRIES, ANY_METHOD_EXEC,
    ANY_METHOD_FILTER, ANY_METHOD_FOR_EACH, ANY_METHOD_GET, ANY_METHOD_HAS, ANY_METHOD_INCLUDES,
    ANY_METHOD_INDEX_OF, ANY_METHOD_JOIN, ANY_METHOD_KEYS, ANY_METHOD_MAP, ANY_METHOD_MATCH,
    ANY_METHOD_NEXT, ANY_METHOD_POP, ANY_METHOD_PUSH, ANY_METHOD_REPLACE, ANY_METHOD_REPLACE_ALL,
    ANY_METHOD_SET, ANY_METHOD_SHIFT, ANY_METHOD_SLICE, ANY_METHOD_SPLIT, ANY_METHOD_STARTS_WITH,
    ANY_METHOD_TEST, ANY_METHOD_TO_EXPONENTIAL, ANY_METHOD_TO_FIXED, ANY_METHOD_TO_LOCALE_STRING,
    ANY_METHOD_TO_LOWER_CASE, ANY_METHOD_TO_PRECISION, ANY_METHOD_TO_STRING,
    ANY_METHOD_TO_UPPER_CASE, ANY_METHOD_TRIM, ANY_METHOD_TRIM_END, ANY_METHOD_TRIM_START,
    ANY_METHOD_UNKNOWN, ANY_METHOD_UNSHIFT, ANY_METHOD_VALUE_OF, ANY_METHOD_VALUES, any_method_id,
};
use torajs_rc::{FLAG_STATIC_LITERAL, Tag};

use crate::nanbox::{
    AnyValue, VALUE_UNDEFINED, as_void_ptr, is_bool, is_cell, is_double, is_int32, is_short_str,
};

unsafe extern "C" {
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// Cell layout offsets — mirror of torajs-core `ssa_lower.rs`
/// closure-env constants.
const CLOSURE_FN_ADDR_OFF: usize = 8;
const CLOSURE_DROP_FN_OFF: usize = 16;
const CLOSURE_PROPS_OFF: usize = 24;
const CLOSURE_BOXED_ENTRY_OFF: usize = 32;
const CLOSURE_CAP_BASE_OFF: usize = 40;
const CELL_SIZE: usize = 48;

/// Interned name Str layout — mirror of torajs-str
/// `layout::{STR_LEN_OFF, STR_DATA_OFF}`.
const STR_LEN_OFF: usize = 8;
const STR_DATA_OFF: usize = 16;

/// Method-id intern table span (ids are append-only; headroom
/// beyond the current max keeps future ids table-hits).
const TABLE_SIZE: usize = 128;

/// Per-mid interned cells. Atomic-static, NOT `thread_local!` —
/// std's lazy TLS machinery is unavailable inside the baked
/// staticlib runtime (same constraint as torajs-cycle / torajs-weak,
/// which use the AtomicPtr-static pattern). Relaxed is exact today
/// (single-threaded runtime) and a benign double-alloc race later —
/// both winners are immortal.
static METHOD_CELLS: [AtomicU64; TABLE_SIZE] = [const { AtomicU64::new(0) }; TABLE_SIZE];

/// Boxed dual entry of every reified method cell — a bare call is
/// the ES `this = undefined` TypeError.
unsafe extern "C" fn bare_entry(_env: *mut c_void, _argv: *const u64, _argc: i64) -> u64 {
    unsafe {
        __torajs_throw_type_error(
            c"builtin method called without a receiver (this is undefined)".as_ptr(),
        );
    }
    VALUE_UNDEFINED
}

/// Native entry — an any→typed fn-slot cast direct-calls this
/// instead of jumping to 0. Arguments are ignored (safe under the C
/// calling convention); the pending throw propagates at the callee
/// boundary.
pub(crate) unsafe extern "C" fn native_entry() -> u64 {
    unsafe {
        __torajs_throw_type_error(
            c"builtin method called without a receiver (this is undefined)".as_ptr(),
        );
    }
    0
}

/// The interned cell for a method id — lazily allocated, immortal.
pub(crate) fn builtin_method_cell(mid: i64) -> *mut u8 {
    let slot = &METHOD_CELLS[mid as usize];
    let p = slot.load(Ordering::Relaxed);
    if p != 0 {
        return p as *mut u8;
    }
    // SAFETY: fresh 48-byte allocation, fully initialized below.
    unsafe {
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        let cell = std::alloc::alloc_zeroed(layout);
        // Universal header: refcount 1 (never reaches 0 — rc
        // traffic no-ops on the static flag), Tag::Closure,
        // FLAG_STATIC_LITERAL.
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::Closure as u16;
        *(cell.add(6) as *mut u16) = FLAG_STATIC_LITERAL;
        *(cell.add(CLOSURE_FN_ADDR_OFF) as *mut u64) = native_entry as *const () as u64;
        *(cell.add(CLOSURE_DROP_FN_OFF) as *mut u64) = 0;
        *(cell.add(CLOSURE_PROPS_OFF) as *mut u64) = 0;
        *(cell.add(CLOSURE_BOXED_ENTRY_OFF) as *mut u64) = bare_entry as *const () as u64;
        *(cell.add(CLOSURE_CAP_BASE_OFF) as *mut u64) = mid as u64;
        slot.store(cell as u64, Ordering::Relaxed);
        cell
    }
}

/// The method id a reified cell carries — `None` for ordinary
/// closures (discriminated by the boxed entry's address).
pub(crate) unsafe fn builtin_method_mid(ptr: *mut c_void) -> Option<i64> {
    unsafe {
        let entry = *(ptr.cast::<u8>().add(CLOSURE_BOXED_ENTRY_OFF) as *const u64);
        if entry == bare_entry as *const () as u64 {
            Some(*(ptr.cast::<u8>().add(CLOSURE_CAP_BASE_OFF) as *const u64) as i64)
        } else {
            None
        }
    }
}

/// Member-name → interned method cell, `None` when the name is not
/// a method the receiver's dispatch arm supports (the member read
/// stays undefined, matching bun's wrong-arm answer).
///
/// # Safety
/// `key` is NULL or a live Str cell.
pub(crate) unsafe fn builtin_method_lookup(recv: AnyValue, key: *const c_void) -> Option<*mut u8> {
    let mid = unsafe { key_method_id(key) };
    if mid == ANY_METHOD_UNKNOWN || (mid as usize) >= TABLE_SIZE {
        return None;
    }
    if !builtin_method_supported(recv, mid) {
        return None;
    }
    Some(builtin_method_cell(mid))
}

/// Read the key Str's bytes and intern them through the shared
/// compile-time table.
unsafe fn key_method_id(key: *const c_void) -> i64 {
    if key.is_null() {
        return ANY_METHOD_UNKNOWN;
    }
    unsafe {
        let len = (key.cast::<u8>().add(STR_LEN_OFF) as *const u32).read() as usize;
        let bytes = core::slice::from_raw_parts(key.cast::<u8>().add(STR_DATA_OFF), len);
        match core::str::from_utf8(bytes) {
            Ok(s) => any_method_id(s),
            Err(_) => ANY_METHOD_UNKNOWN,
        }
    }
}

/// Exact per-receiver-shape support table — one arm per
/// `method_call_*` dispatch module, listing the ids that arm
/// resolves (extend together when an arm grows a method).
pub(crate) fn builtin_method_supported(recv: AnyValue, mid: i64) -> bool {
    if is_short_str(recv) {
        return str_supports(mid);
    }
    if is_int32(recv) || is_double(recv) {
        return num_supports(mid);
    }
    if is_bool(recv) {
        return mid == ANY_METHOD_TO_STRING;
    }
    if !is_cell(recv) {
        return false;
    }
    let ptr = as_void_ptr(recv);
    // SAFETY: is_cell guarantees a live heap pointer.
    let tag = unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() };
    match tag {
        t if t == Tag::Str as u16 => str_supports(mid),
        t if t == Tag::Arr as u16 => matches!(
            mid,
            ANY_METHOD_PUSH
                | ANY_METHOD_POP
                | ANY_METHOD_SHIFT
                | ANY_METHOD_UNSHIFT
                | ANY_METHOD_INDEX_OF
                | ANY_METHOD_INCLUDES
                | ANY_METHOD_JOIN
                | ANY_METHOD_SLICE
                | ANY_METHOD_MAP
                | ANY_METHOD_FILTER
                | ANY_METHOD_FOR_EACH
        ),
        t if t == Tag::Map as u16 => matches!(
            mid,
            ANY_METHOD_GET
                | ANY_METHOD_SET
                | ANY_METHOD_HAS
                | ANY_METHOD_DELETE
                | ANY_METHOD_CLEAR
                | ANY_METHOD_FOR_EACH
                | ANY_METHOD_KEYS
                | ANY_METHOD_VALUES
                | ANY_METHOD_ENTRIES
        ),
        t if t == Tag::Set as u16 => matches!(
            mid,
            ANY_METHOD_ADD
                | ANY_METHOD_HAS
                | ANY_METHOD_DELETE
                | ANY_METHOD_CLEAR
                | ANY_METHOD_FOR_EACH
                | ANY_METHOD_KEYS
                | ANY_METHOD_VALUES
                | ANY_METHOD_ENTRIES
        ),
        t if t == Tag::MapIter as u16 => mid == ANY_METHOD_NEXT,
        t if t == Tag::Date as u16 => date_supports(mid),
        t if t == Tag::RegExp as u16 => {
            matches!(
                mid,
                ANY_METHOD_TEST | ANY_METHOD_EXEC | ANY_METHOD_TO_STRING
            )
        }
        t if t == Tag::WeakMap as u16 => matches!(
            mid,
            ANY_METHOD_GET | ANY_METHOD_SET | ANY_METHOD_HAS | ANY_METHOD_DELETE
        ),
        t if t == Tag::WeakSet as u16 => {
            matches!(mid, ANY_METHOD_ADD | ANY_METHOD_HAS | ANY_METHOD_DELETE)
        }
        t if t == Tag::Closure as u16 => {
            matches!(mid, ANY_METHOD_CALL | ANY_METHOD_APPLY | ANY_METHOD_BIND)
        }
        _ => false,
    }
}

/// `method_call_str` arm ids (+ the dispatcher's toString identity).
fn str_supports(mid: i64) -> bool {
    matches!(
        mid,
        ANY_METHOD_TO_STRING
            | ANY_METHOD_CHAR_AT
            | ANY_METHOD_TO_UPPER_CASE
            | ANY_METHOD_TO_LOWER_CASE
            | ANY_METHOD_INDEX_OF
            | ANY_METHOD_INCLUDES
            | ANY_METHOD_SLICE
            | ANY_METHOD_SPLIT
            | ANY_METHOD_TRIM
            | ANY_METHOD_TRIM_START
            | ANY_METHOD_TRIM_END
            | ANY_METHOD_MATCH
            | ANY_METHOD_REPLACE
            | ANY_METHOD_REPLACE_ALL
            | ANY_METHOD_STARTS_WITH
            | ANY_METHOD_ENDS_WITH
    )
}

/// `method_call_num` arm ids.
fn num_supports(mid: i64) -> bool {
    matches!(
        mid,
        ANY_METHOD_TO_STRING
            | ANY_METHOD_TO_FIXED
            | ANY_METHOD_TO_EXPONENTIAL
            | ANY_METHOD_TO_PRECISION
            | ANY_METHOD_TO_LOCALE_STRING
            | ANY_METHOD_VALUE_OF
    )
}

/// `method_call_date` arm ids — the getter / setter / to*String
/// table (ids 25-62 with the annexB aliases).
fn date_supports(mid: i64) -> bool {
    use torajs_rc::{
        ANY_METHOD_GET_DATE, ANY_METHOD_GET_DAY, ANY_METHOD_GET_FULL_YEAR, ANY_METHOD_GET_HOURS,
        ANY_METHOD_GET_MILLISECONDS, ANY_METHOD_GET_MINUTES, ANY_METHOD_GET_MONTH,
        ANY_METHOD_GET_SECONDS, ANY_METHOD_GET_TIME, ANY_METHOD_GET_TIMEZONE_OFFSET,
        ANY_METHOD_GET_UTC_DATE, ANY_METHOD_GET_UTC_DAY, ANY_METHOD_GET_UTC_FULL_YEAR,
        ANY_METHOD_GET_UTC_HOURS, ANY_METHOD_GET_UTC_MILLISECONDS, ANY_METHOD_GET_UTC_MINUTES,
        ANY_METHOD_GET_UTC_MONTH, ANY_METHOD_GET_UTC_SECONDS, ANY_METHOD_GET_YEAR,
        ANY_METHOD_SET_DATE, ANY_METHOD_SET_FULL_YEAR, ANY_METHOD_SET_HOURS,
        ANY_METHOD_SET_MILLISECONDS, ANY_METHOD_SET_MINUTES, ANY_METHOD_SET_MONTH,
        ANY_METHOD_SET_SECONDS, ANY_METHOD_SET_TIME, ANY_METHOD_SET_YEAR,
        ANY_METHOD_TO_DATE_STRING, ANY_METHOD_TO_GMT_STRING, ANY_METHOD_TO_ISO_STRING,
        ANY_METHOD_TO_JSON, ANY_METHOD_TO_LOCALE_DATE_STRING, ANY_METHOD_TO_LOCALE_TIME_STRING,
        ANY_METHOD_TO_UTC_STRING,
    };
    matches!(
        mid,
        ANY_METHOD_GET_TIME
            | ANY_METHOD_VALUE_OF
            | ANY_METHOD_TO_STRING
            | ANY_METHOD_TO_ISO_STRING
            | ANY_METHOD_TO_JSON
            | ANY_METHOD_TO_LOCALE_STRING
            | ANY_METHOD_GET_FULL_YEAR
            | ANY_METHOD_GET_UTC_FULL_YEAR
            | ANY_METHOD_GET_MONTH
            | ANY_METHOD_GET_UTC_MONTH
            | ANY_METHOD_GET_DATE
            | ANY_METHOD_GET_UTC_DATE
            | ANY_METHOD_GET_HOURS
            | ANY_METHOD_GET_UTC_HOURS
            | ANY_METHOD_GET_MINUTES
            | ANY_METHOD_GET_UTC_MINUTES
            | ANY_METHOD_GET_SECONDS
            | ANY_METHOD_GET_UTC_SECONDS
            | ANY_METHOD_GET_MILLISECONDS
            | ANY_METHOD_GET_UTC_MILLISECONDS
            | ANY_METHOD_GET_DAY
            | ANY_METHOD_GET_UTC_DAY
            | ANY_METHOD_GET_TIMEZONE_OFFSET
            | ANY_METHOD_GET_YEAR
            | ANY_METHOD_SET_TIME
            | ANY_METHOD_SET_YEAR
            | ANY_METHOD_SET_FULL_YEAR
            | ANY_METHOD_SET_MONTH
            | ANY_METHOD_SET_DATE
            | ANY_METHOD_SET_HOURS
            | ANY_METHOD_SET_MINUTES
            | ANY_METHOD_SET_SECONDS
            | ANY_METHOD_SET_MILLISECONDS
            | ANY_METHOD_TO_UTC_STRING
            | ANY_METHOD_TO_GMT_STRING
            | ANY_METHOD_TO_DATE_STRING
            | ANY_METHOD_TO_LOCALE_DATE_STRING
            | ANY_METHOD_TO_LOCALE_TIME_STRING
    )
}
