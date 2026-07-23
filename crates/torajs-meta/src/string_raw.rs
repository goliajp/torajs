//! `String.raw(template, ...substitutions)` per ES §22.1.2.4.
//!
//! Concatenates `template.raw[0] + subs[0] + template.raw[1] + subs[1]
//! + ... + template.raw[n-1]`. Every value coerces through ToString.
//! The runtime call path is the direct-fn shape
//! (`String.raw({raw:['a','b']}, 1)`); the tagged-template literal
//! surface (`String.raw`...``) is a separate parser/emitter substrate
//! item and not covered here.
//!
//! Contract:
//! - `template` is a live AnyValue holding an object with an own
//!   `raw` property; the runtime reads it through the shape-blind
//!   `__torajs_any_member_get_{tag,value}` path (dynobj / struct /
//!   `Object.create(null)` all take the same walk).
//! - `raw` must resolve to an array; a nullish template or a missing
//!   `raw` throws `TypeError` per §22.1.2.4 step 4 (ToObject +
//!   Get(raw) + LengthOfArrayLike).
//! - `argv` / `argc` describe the substitutions; missing subs (fewer
//!   than `raw.length - 1`) default to the empty string.
//!
//! Ownership: the accumulator is a freshly owned Str whose refcount
//! transfers to the caller on return. Every intermediate part / sub
//! Str comes from `__torajs_anyv_to_str` (already-owned) and is
//! dropped after its concat. The keys ("raw", numeric-index probes)
//! and the raw-array borrow track the same borrow contract the
//! sibling meta walks (obj_assign / from_entries) already use.

use core::ffi::{c_char, c_void};

use crate::reflect::{VALUE_NULL_IMM, VALUE_UNDEFINED_IMM, alloc_str_key};

/// AnyValue tag encoding for a heap-cell payload.
const ANY_HEAP: u64 = 4;
/// torajs-arr fixed cell layout (`layout.rs`) — length lives at
/// `+8` from the cell header. Same mirror the sibling meta walks
/// carry (obj_assign / obj_own_keys).
const ARR_LEN_OFF: usize = 8;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const c_char);
    fn __torajs_throw_check() -> i64;
    fn __torajs_any_member_get_tag(recv: u64, key: *const c_void) -> u64;
    fn __torajs_any_member_get_value(recv: u64, key: *const c_void) -> u64;
    fn __torajs_arr_get_any_boxed(arr: *const c_void, i: u64) -> u64;
    fn __torajs_anyv_to_str(v: u64) -> *mut c_void;
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_concat(a: *const u8, b: *const u8) -> *mut u8;
    fn __torajs_str_drop(s: *mut u8);
}

/// `String.raw(template, ...substitutions)` — returns a freshly owned
/// Str (refcount = 1) the caller drops. On any error a pending
/// TypeError is recorded and the return is a live empty Str (the
/// caller's throw check unwinds before the value is consumed).
///
/// # Safety
/// `template` is a live AnyValue bit pattern. `argv` points at
/// `argc` live AnyValue bits (may be null when `argc == 0`). The
/// caller checks for a pending throw after return.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_string_raw(
    template: u64,
    argv: *const u64,
    argc: i64,
) -> *mut c_void {
    if template == VALUE_NULL_IMM || template == VALUE_UNDEFINED_IMM {
        unsafe {
            __torajs_throw_type_error(c"Cannot convert undefined or null to object".as_ptr());
        }
        return unsafe { __torajs_str_alloc(b"".as_ptr(), 0) } as *mut c_void;
    }
    // Fetch `template.raw` through the shape-blind member get. A
    // dynobj / struct / class receiver all take the same probe; the
    // "raw" key is a fresh pooled Str, dropped once at the tail.
    let raw_key = unsafe { alloc_str_key(b"raw") };
    let raw_tag = unsafe { __torajs_any_member_get_tag(template, raw_key as *const c_void) };
    let raw_val = unsafe { __torajs_any_member_get_value(template, raw_key as *const c_void) };
    if raw_tag != ANY_HEAP || raw_val == 0 {
        unsafe {
            __torajs_str_drop(raw_key);
            __torajs_throw_type_error(
                c"String.raw requires a template with a `raw` array".as_ptr(),
            );
        }
        return unsafe { __torajs_str_alloc(b"".as_ptr(), 0) } as *mut c_void;
    }
    let raw_arr = raw_val as *const c_void;
    let raw_len = unsafe { (raw_arr.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read() };
    let mut acc = unsafe { __torajs_str_alloc(b"".as_ptr(), 0) };
    for i in 0..raw_len {
        // raw[i] → owned Str (ToString on whatever the slot holds;
        // an array literal `['a','b']` slots hold Str cells, but
        // String.raw is spec-driven to coerce any shape).
        let part_boxed = unsafe { __torajs_arr_get_any_boxed(raw_arr, i) };
        let part_str = unsafe { __torajs_anyv_to_str(part_boxed) };
        // A ToString throw poisons the walk; the accumulator we've
        // built so far still needs releasing before we bail.
        if unsafe { __torajs_throw_check() } != 0 {
            unsafe {
                __torajs_str_drop(part_str as *mut u8);
                __torajs_str_drop(acc);
                __torajs_str_drop(raw_key);
            }
            return unsafe { __torajs_str_alloc(b"".as_ptr(), 0) } as *mut c_void;
        }
        let new_acc = unsafe { __torajs_str_concat(acc, part_str as *const u8) };
        unsafe {
            __torajs_str_drop(acc);
            __torajs_str_drop(part_str as *mut u8);
        }
        acc = new_acc;
        // Interleave the substitution — one per raw slot except the
        // last. Missing subs (argc < raw.length - 1) contribute
        // nothing per §22.1.2.4 step 10.d.iii ("undefined" would be
        // spec-strict, but bun matches every major engine in
        // treating them as absent).
        if i + 1 < raw_len && (i as i64) < argc {
            let sub_boxed = unsafe { *argv.add(i as usize) };
            let sub_str = unsafe { __torajs_anyv_to_str(sub_boxed) };
            if unsafe { __torajs_throw_check() } != 0 {
                unsafe {
                    __torajs_str_drop(sub_str as *mut u8);
                    __torajs_str_drop(acc);
                    __torajs_str_drop(raw_key);
                }
                return unsafe { __torajs_str_alloc(b"".as_ptr(), 0) } as *mut c_void;
            }
            let new_acc = unsafe { __torajs_str_concat(acc, sub_str as *const u8) };
            unsafe {
                __torajs_str_drop(acc);
                __torajs_str_drop(sub_str as *mut u8);
            }
            acc = new_acc;
        }
    }
    unsafe { __torajs_str_drop(raw_key) };
    acc as *mut c_void
}
