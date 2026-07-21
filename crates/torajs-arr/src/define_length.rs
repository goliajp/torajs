//! §10.4.2.4 ArraySetLength via `Object.defineProperty(arr,
//! "length", desc)` — split out of `define.rs` (file-size hard
//! limit; RFC 20260713-defprop-residual-cluster chunk C pushed the
//! shared file over 500). Dispatch stays in
//! [`crate::define::__torajs_arr_define`].

use core::ffi::c_void;

use torajs_rc::{FLAG_ARR_LENGTH_RO, FLAG_FROZEN};

use crate::define::{
    F_CONFIGURABLE, F_ENUMERABLE, F_WRITABLE, P_CONFIGURABLE, P_ENUMERABLE, P_VALUE, P_WRITABLE,
    drop_owned, header_flags,
};

unsafe extern "C" {
    fn __torajs_arr_set_length_any(arr: *mut c_void, tag: i64, value: i64);
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_to_number(v: u64) -> f64;
    fn __torajs_throw_range_error(msg: *const u8);
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
}

/// Spec §7.1.7 ToUint32 over an already-ToNumber'd f64 — NaN / ±inf
/// map to 0, everything else truncates and wraps modulo 2^32.
/// Shared with the assignment-path twin in `grow.rs` (刀 6 G9c).
pub(crate) fn to_uint32(n: f64) -> u32 {
    if n.is_nan() || n.is_infinite() {
        return 0;
    }
    (n.trunc() as i64 as u64 & 0xFFFF_FFFF) as u32
}

/// §10.4.2.4 ArraySetLength via defineProperty — length is
/// permanently `{enumerable: false, configurable: false}`; a present
/// `[[Value]]` resizes through the shared validate helper (which
/// also carries the writable lock — a locked length rejects any
/// change but admits the same value); `writable: false` arms the
/// lock AFTER the resize (spec applies the value first, then the
/// writability drop). Step order: ToUint32 of the [[Value]] runs
/// FIRST (steps 3-4 — a throwing valueOf wins over every attribute
/// rejection and runs exactly once), then the e/c/w validation, then
/// the apply with the already-converted number.
pub(crate) unsafe fn define_length(arr: *mut c_void, tag: u64, value: u64, flags_byte: u64) {
    // §10.4.2.4 steps 3-5 — `newLen = ToUint32(v)` then `numberLen =
    // ToNumber(v)` (the spec really runs a side-effecting valueOf
    // TWICE), RangeError when they disagree. Conversion precedes the
    // attribute validation, and a throwing valueOf wins outright.
    let mut converted: Option<i64> = None;
    if flags_byte & P_VALUE != 0 {
        let anyv = unsafe { __torajs_anyv_box_from_pair(tag as i64, value as i64) };
        let n1 = unsafe { __torajs_anyv_to_number(anyv) };
        if unsafe { __torajs_throw_check() } != 0 {
            unsafe { drop_owned(tag, value) };
            return;
        }
        let n2 = unsafe { __torajs_anyv_to_number(anyv) };
        unsafe { drop_owned(tag, value) };
        if unsafe { __torajs_throw_check() } != 0 {
            return;
        }
        let new_len = to_uint32(n1);
        if new_len as f64 != n2 {
            unsafe { __torajs_throw_range_error(c"Invalid array length".as_ptr() as *const u8) };
            return;
        }
        converted = Some(new_len as i64);
    }
    let ro = unsafe { header_flags(arr) } & FLAG_ARR_LENGTH_RO != 0;
    if flags_byte & P_CONFIGURABLE != 0 && flags_byte & F_CONFIGURABLE != 0 {
        unsafe {
            __torajs_throw_type_error(
                c"Attempting to change configurable attribute of unconfigurable property.".as_ptr(),
            )
        };
        return;
    }
    if flags_byte & P_ENUMERABLE != 0 && flags_byte & F_ENUMERABLE != 0 {
        unsafe {
            __torajs_throw_type_error(
                c"Attempting to change enumerable attribute of unconfigurable property.".as_ptr(),
            )
        };
        return;
    }
    if ro && flags_byte & P_WRITABLE != 0 && flags_byte & F_WRITABLE != 0 {
        unsafe {
            __torajs_throw_type_error(
                c"Attempting to change writable attribute of unconfigurable property.".as_ptr(),
            )
        };
        return;
    }
    if let Some(n) = converted {
        // §10.4.2.4 steps 15-19 — shrinking deletes down from
        // oldLen-1; a NON-CONFIGURABLE index stops the walk, length
        // lands at that index + 1, and the whole define throws
        // TypeError. Zero cost for ordinary arrays: the walk only
        // runs when the exotic-index header bit says a shadow
        // attribute table exists.
        let cur_len =
            unsafe { (arr.cast::<u8>().add(crate::layout::ARR_LEN_OFF) as *const u64).read() };
        if (n as u64) < cur_len
            && unsafe { crate::define::header_flags(arr) } & torajs_rc::FLAG_ARR_EXOTIC_INDEX != 0
        {
            let mut i = cur_len;
            while i > n as u64 {
                i -= 1;
                let f = unsafe { crate::define::__torajs_arr_index_flags(arr, i) };
                if f & crate::define::F_HOLE == 0 && f & F_CONFIGURABLE == 0 {
                    unsafe { __torajs_arr_set_length_any(arr, 2, (i + 1) as i64) };
                    // Step 19.b-c — a requested writable:false still
                    // applies before the throw.
                    if flags_byte & P_WRITABLE != 0 && flags_byte & F_WRITABLE == 0 {
                        let p = unsafe { (arr as *mut u8).add(6) as *mut u16 };
                        unsafe { p.write(p.read() | FLAG_ARR_LENGTH_RO) };
                    }
                    unsafe { __torajs_throw_type_error(c"Unable to delete property.".as_ptr()) };
                    return;
                }
            }
        }
        // The resize helper carries the lock (rejects a changed
        // value, admits the same one) — pending throw short-circuits
        // the writable drop below per the spec's fail-fast order.
        unsafe { __torajs_arr_set_length_any(arr, 2, n) };
        if unsafe { __torajs_throw_check() } != 0 {
            return;
        }
    }
    if flags_byte & P_WRITABLE != 0 && flags_byte & F_WRITABLE == 0 {
        let p = unsafe { (arr as *mut u8).add(6) as *mut u16 };
        unsafe { p.write(p.read() | FLAG_ARR_LENGTH_RO) };
    }
}

/// §23.1.3.{20,29} pop/shift step 4.f / 5.g — `Set(O, "length", …,
/// true)` on a frozen array or a non-writable `length` throws
/// TypeError (OrdinarySetWithOwnDescriptor step 2.a + Set's Throw
/// flag). Fired at the head of every pop/shift lane — the EMPTY
/// receiver also writes length 0 per step 3.b, so the guard
/// precedes the empty short-circuit (RFC 20260713-array-proto-
/// residual blade 4). Returns 1 when the TypeError was recorded.
///
/// # Safety
/// `arr` is a live array heap block pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_len_write_guard(arr: *const c_void) -> i64 {
    let locked = unsafe { header_flags(arr as *mut c_void) } & (FLAG_ARR_LENGTH_RO | FLAG_FROZEN);
    if locked == 0 {
        return 0;
    }
    unsafe {
        __torajs_throw_type_error(c"Attempted to assign to readonly property.".as_ptr());
    }
    1
}
