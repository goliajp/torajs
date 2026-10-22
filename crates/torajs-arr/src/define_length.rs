//! §10.4.2.4 ArraySetLength via `Object.defineProperty(arr,
//! "length", desc)` — split out of `define.rs` (file-size hard
//! limit; RFC 20260713-defprop-residual-cluster chunk C pushed the
//! shared file over 500). Dispatch stays in
//! [`crate::define::__torajs_arr_define`].

use core::ffi::c_void;

use torajs_rc::{
    FLAG_ARR_EXOTIC_INDEX, FLAG_ARR_LENGTH_RO, FLAG_FROZEN, FLAG_NON_EXTENSIBLE, FLAG_SEALED,
};

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
///
/// Answers 1 on success, 0 on a §10.1.6.3 refusal — recorded as a
/// TypeError only for the throwing flavor (rotation 267 刀 R5a; the
/// ToNumber throw and the RangeError are conversions, not refusals,
/// and record for either flavor).
pub(crate) unsafe fn define_length(
    arr: *mut c_void,
    tag: u64,
    value: u64,
    flags_byte: u64,
    throw_on_refusal: bool,
) -> i64 {
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
            return 0;
        }
        let n2 = unsafe { __torajs_anyv_to_number(anyv) };
        unsafe { drop_owned(tag, value) };
        if unsafe { __torajs_throw_check() } != 0 {
            return 0;
        }
        let new_len = to_uint32(n1);
        if new_len as f64 != n2 {
            unsafe { __torajs_throw_range_error(c"Invalid array length".as_ptr() as *const u8) };
            return 0;
        }
        converted = Some(new_len as i64);
    }
    let ro = unsafe { header_flags(arr) } & FLAG_ARR_LENGTH_RO != 0;
    if flags_byte & P_CONFIGURABLE != 0 && flags_byte & F_CONFIGURABLE != 0 {
        return refuse_length(
            throw_on_refusal,
            c"Attempting to change configurable attribute of unconfigurable property.".as_ptr(),
        );
    }
    if flags_byte & P_ENUMERABLE != 0 && flags_byte & F_ENUMERABLE != 0 {
        return refuse_length(
            throw_on_refusal,
            c"Attempting to change enumerable attribute of unconfigurable property.".as_ptr(),
        );
    }
    if ro && flags_byte & P_WRITABLE != 0 && flags_byte & F_WRITABLE != 0 {
        return refuse_length(
            throw_on_refusal,
            c"Attempting to change writable attribute of unconfigurable property.".as_ptr(),
        );
    }
    if let Some(n) = converted {
        let cur_len =
            unsafe { (arr.cast::<u8>().add(crate::layout::ARR_LEN_OFF) as *const u64).read() };
        // §10.4.2.4 steps 11-12 — this is a DEFINE, and a non-writable
        // `length` (a lock, or a freeze) is what OrdinaryDefineOwn-
        // Property validates it against: the same value is admitted,
        // a changed one refused. The Set kernel below refuses every
        // write on a non-writable length (§10.1.9.2), which is the
        // wrong question here — it turned a same-value redefine into
        // a TypeError.
        if ro || unsafe { header_flags(arr) } & FLAG_FROZEN != 0 {
            if n as u64 != cur_len {
                return refuse_length(
                    throw_on_refusal,
                    c"Attempting to change value of a readonly property.".as_ptr(),
                );
            }
        } else if unsafe { define_length_value(arr, n, cur_len, flags_byte, throw_on_refusal) } == 0
            || unsafe { __torajs_throw_check() } != 0
        {
            return 0;
        }
    }
    if flags_byte & P_WRITABLE != 0 && flags_byte & F_WRITABLE == 0 {
        unsafe { crate::exotic_seam::__torajs_arr_flag_length_ro(arr) };
    }
    1
}

/// The value half of a `length` define on a receiver whose length is
/// writable — the shrink walk, then the resize kernel (which carries
/// the seal's tail-delete refusal). Answers 0 when the walk refused
/// (flavored), 1 otherwise; the kernel's own refusal is pending.
///
/// # Safety
/// `arr` is a live array heap block; `n` is a validated length.
unsafe fn define_length_value(
    arr: *mut c_void,
    n: i64,
    cur_len: u64,
    flags_byte: u64,
    throw_on_refusal: bool,
) -> i64 {
    {
        // §10.4.2.4 steps 15-19 — shrinking deletes down from
        // oldLen-1; a NON-CONFIGURABLE index stops the walk, length
        // lands at that index + 1, and the whole define throws
        // TypeError. Zero cost for ordinary arrays: the walk only
        // runs when the exotic-index header bit says a shadow
        // attribute table exists.
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
                    // applies before the refusal (§10.4.2.4 answers
                    // false here; §20.1.2.4 turns that into the
                    // TypeError).
                    if flags_byte & P_WRITABLE != 0 && flags_byte & F_WRITABLE == 0 {
                        unsafe { crate::exotic_seam::__torajs_arr_flag_length_ro(arr) };
                    }
                    return refuse_length(throw_on_refusal, c"Unable to delete property.".as_ptr());
                }
            }
        }
        // The resize kernel carries the seal's shrink refusal; a
        // pending throw short-circuits the caller's writable drop per
        // the spec's fail-fast order.
        unsafe { __torajs_arr_set_length_any(arr, 2, n) };
        1
    }
}

/// Refusal answer for the `length` arm — no [[Value]] stake to
/// release here (every leg runs after the conversion block already
/// consumed it, or carries none).
fn refuse_length(throw_on_refusal: bool, msg: *const core::ffi::c_char) -> i64 {
    if throw_on_refusal {
        unsafe { __torajs_throw_type_error(msg) };
    }
    0
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
/// `push` / `unshift` add a NEW index, so §10.1.9 CreateDataProperty
/// fails when the receiver's `[[Extensible]]` slot is false — that is
/// a TypeError, not a silent append. Returns 1 when it was recorded.
///
/// The predicate differs from [`__torajs_arr_len_write_guard`]'s on
/// purpose, and it was read off node rather than assumed: `push` and
/// `unshift` throw under `Object.preventExtensions` as well as under
/// `seal` and `freeze`, while `pop` throws under `seal`/`freeze` but
/// NOT under bare `preventExtensions` (its element must be
/// configurable to be deleted, and prevent-only leaves it so). Hence
/// two guards, each naming its own reason.
///
/// Six of the nine array mutators had NO guard at all and silently
/// mutated a frozen array: `Object.freeze([3,1,2])` then `.push(9)`
/// answered `[3,1,2,9]` where every engine throws — while `pop`,
/// `shift` and `splice`, whose guards landed in RFC 20260713 blade 4
/// and 刀 6 G9a, threw on the very same receiver. The callers must
/// release any transferred rc on the reject path, the way each
/// function's existing loud rejects already do.
///
/// # Safety
/// `arr` is a live array heap block pointer.
/// `sort` / `reverse` / `fill` / `copyWithin` write EXISTING slots,
/// so the bit they answer to is §10.1.9's [[Writable]] — which only a
/// freeze clears. A sealed or preventExtensions receiver keeps
/// writable elements and MUST still mutate (node: `Object.seal([3,1,2])
/// .sort()` answers `[1,2,3]`), so this is the third guard, on
/// `FLAG_FROZEN` alone, next to the length guard (`FROZEN |
/// LENGTH_RO`) and the extend guard (`FROZEN | SEALED |
/// NON_EXTENSIBLE`) rather than a reuse of either. Returns 1 when the
/// TypeError was recorded; callers store nothing after a 1.
///
/// Four of the nine mutators wrote straight through a freeze —
/// `Object.freeze([3,1,2]).sort()` answered `[1,2,3]` — after push /
/// unshift had been closed under the extend guard.
///
/// # Safety
/// `arr` is a live array heap block pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_frozen_guard(arr: *const c_void) -> i64 {
    if unsafe { header_flags(arr as *mut c_void) } & FLAG_FROZEN == 0 {
        return 0;
    }
    unsafe {
        __torajs_throw_type_error(c"Attempted to assign to readonly property.".as_ptr());
    }
    1
}

/// # Safety
/// `arr` is a live Arr cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_extend_guard(arr: *const c_void) -> i64 {
    // A `defineProperty` lock on `length` refuses the append too —
    // §23.1.3.23 step 5's `Set(length)` fails after the element
    // landed; refused at entry here like the other locks (before this
    // the push went through and the length moved).
    let locked = unsafe { header_flags(arr as *mut c_void) }
        & (FLAG_FROZEN | FLAG_SEALED | FLAG_NON_EXTENSIBLE | FLAG_ARR_LENGTH_RO);
    if locked == 0 {
        return 0;
    }
    unsafe {
        __torajs_throw_type_error(c"Attempted to assign to readonly property.".as_ptr());
    }
    1
}

/// Deleting the LAST element — `pop`, or an ArraySetLength that
/// shrinks — needs that element to be configurable, and a seal clears
/// that too: node/bun throw on `Object.seal([3,1,2]).pop()` and on
/// `.length = 1`, with the array untouched, while `preventExtensions`
/// alone leaves both legal and a seal still lets `length` GROW. So this
/// is the fourth guard, `FROZEN | LENGTH_RO | SEALED`, for the two
/// tail-deleting shapes; `__torajs_arr_len_write_guard` stays what the
/// grow path and shift/splice's length write answer to. Returns 1 when
/// the TypeError was recorded.
///
/// # Safety
/// `arr` is a live array heap block pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_delete_tail_guard(arr: *const c_void) -> i64 {
    let flags = unsafe { header_flags(arr as *mut c_void) };
    let len = unsafe { *(arr.cast::<u8>().add(crate::layout::ARR_LEN_OFF) as *const u64) };
    // §23.1.3.22 pop, in order: an empty array only writes `length`
    // (step 3.b, the same value), which a non-writable length refuses
    // and a seal does not — before this `Object.seal([]).pop()` threw.
    if len == 0 {
        if flags & (FLAG_ARR_LENGTH_RO | FLAG_FROZEN) == 0 {
            return 0;
        }
        unsafe {
            __torajs_throw_type_error(c"Attempted to assign to readonly property.".as_ptr());
        }
        return 1;
    }
    // Step 4.d deletes the last element first: a sealed or frozen one
    // is non-configurable. Only then (step 4.e) does the length write
    // meet a `defineProperty` lock.
    if flags & (FLAG_FROZEN | FLAG_SEALED) != 0 {
        unsafe {
            __torajs_throw_type_error(c"Unable to delete property.".as_ptr());
        }
        return 1;
    }
    if flags & FLAG_ARR_LENGTH_RO != 0 {
        unsafe {
            __torajs_throw_type_error(c"Attempted to assign to readonly property.".as_ptr());
        }
        return 1;
    }
    // Step 4.d deletes the popped index, and with it any shadow entry:
    // a typed hole's mark left past the new length printed as a
    // `"3": …` expando, and the next push landed in a hole. The typed
    // lowering routes `FLAG_ARR_EXOTIC_INDEX` here for this.
    if flags & FLAG_ARR_EXOTIC_INDEX != 0
        && unsafe { crate::define_hole::shrink_purge_shadows(arr as *mut c_void, len - 1, len) }
            != len - 1
    {
        unsafe { __torajs_throw_type_error(c"Unable to delete property.".as_ptr()) };
        return 1;
    }
    0
}

/// # Safety
/// `arr` is a live Arr cell.
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

/// `arr.length = v` validator (ES §9.4.2.4: throw RangeError if `v`
/// doesn't ToUint32-round-trip).
///
/// Tora's typed pack:
/// - tag 0 = null/other → ToNumber=0 → valid (early return)
/// - tag 1 = Bool 0/1 → valid (early return)
/// - tag 2 = I64 → interpret raw int as length candidate
/// - tag 3 = F64-bits → reinterpret raw bits as f64
/// - other = heap / undefined → record RangeError + return
///
/// Range check: `n` must be a non-negative integer in `[0, 2^32 - 1]`.
/// NaN, Infinity, fractional, negative, and overflow all fail.
///
/// After every `__torajs_throw_range_error` call we `return` so the
/// caller's `emit_throw_check` sees the pending throw immediately (the
/// throw is non-local via TLS, not via stack unwind — see fn-level
/// extern doc).
///
/// # Safety
/// Takes no pointer arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_set_length_validate(tag: i64, value: i64) {
    let n: f64 = match tag {
        0 | 1 => return,
        2 => value as f64,
        3 => f64::from_bits(value as u64),
        _ => {
            unsafe {
                __torajs_throw_range_error(c"Invalid array length".as_ptr().cast());
            }
            return;
        }
    };
    if n.is_nan() || n < 0.0 || n > 4_294_967_295.0 || n != (n as i64) as f64 {
        unsafe {
            __torajs_throw_range_error(c"Invalid array length".as_ptr().cast());
        }
    }
}
