//! `Object.defineProperty(arr, key, desc)` — the Array exotic
//! DefineOwnProperty kernel (RFC 20260712-arr-exotic-define chunk B,
//! spec §10.4.2.1 + §10.1.6.3 data subset).
//!
//! Pre-fix `defineProperty(arr, "0", {...})` mis-routed to the
//! expando `arrprops_set` (values landed in the props dynobj where
//! index reads never look — a silent no-op), and the runtime Any
//! path handed the Arr cell straight to `dynobj_define`, which read
//! the Arr header (`len` @8) as a dynobj header (count/cap @8).
//!
//! ## Dispatch (`__torajs_arr_define`)
//! - `"length"` → §10.4.2.4 ArraySetLength: a present `[[Value]]`
//!   routes through `arr_set_length_any` (ToUint32 validate + real
//!   resize). The `writable: false` length lock is chunk D.
//! - canonical array index → [`define_index`].
//! - anything else → ordinary define on the expando props dynobj
//!   (full attribute flags — the plain-assign `arrprops_set` default
//!   stays for `arr.x = v`).
//!
//! ## Per-index attributes
//! The element value always lives in element storage; only the
//! *flags* of a defineProperty'd index live as a shadow entry
//! (canonical index key, value slot dead `undefined`) in the props
//! dynobj. `FLAG_ARR_EXOTIC_INDEX` on the header gates every reader
//! so arrays that never meet defineProperty pay one predictable
//! branch at most. Fresh defines complete absent flags to `false`
//! (spec §10.1.6.2), so a defineProperty'd index is almost always
//! exotic.

use core::ffi::c_void;

use torajs_rc::{FLAG_ARR_EXOTIC_INDEX, FLAG_ARR_LENGTH_RO, FLAG_NON_EXTENSIBLE};

use crate::layout::ARR_LEN_OFF;

unsafe extern "C" {
    /// torajs-dynobj — ordinary define on the expando dynobj
    /// (validate + apply; lazily allocated via the slot).
    fn __torajs_dynobj_define(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_has(dynobj: *const c_void, key: *const c_void) -> bool;
    fn __torajs_dynobj_get_flags(dynobj: *const c_void, key: *const c_void) -> u64;
    /// torajs-dynobj — raw flags upsert for shadow entries (no
    /// §10.1.6.3 validation; this kernel already validated). A hit
    /// also revives a hole entry (value slot resets to live).
    fn __torajs_dynobj_set_entry_flags(obj_slot: *mut *mut c_void, key: *mut c_void, flags: u64);
    /// torajs-dynobj — HOLE sentinel probe (chunk C; the upsert
    /// lives in `define_hole.rs`).
    fn __torajs_dynobj_entry_is_hole(dynobj: *const c_void, key: *const c_void) -> i32;
    /// torajs-str — canonical index key mint for the flags probe.
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_arr_set_length_any(arr: *mut c_void, tag: i64, value: i64);
    /// torajs-anyvalue — NaN-box + spec ToNumber (the §10.4.2.4
    /// convert-once step; a heap operand's valueOf runs here).
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_to_number(v: u64) -> f64;
    fn __torajs_throw_range_error(msg: *const u8);
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-throw — non-destructive pending-throw probe.
    fn __torajs_throw_check() -> i64;
    fn __torajs_value_drop_heap(p: *mut c_void);
}

// flags_byte layout mirror (torajs-dynobj::layout DEFINE_*): low 3
// bits = flag value, bits 3-5 = flag present, bit 6 = value present.
pub(crate) const F_WRITABLE: u64 = 1 << 0;
pub(crate) const F_ENUMERABLE: u64 = 1 << 1;
pub(crate) const F_CONFIGURABLE: u64 = 1 << 2;
const P_WRITABLE: u64 = 1 << 3;
const P_ENUMERABLE: u64 = 1 << 4;
const P_CONFIGURABLE: u64 = 1 << 5;
const P_VALUE: u64 = 1 << 6;
/// All three attribute bits — the implicit flags of a plain element.
pub(crate) const FLAGS_DEFAULT: u64 = F_WRITABLE | F_ENUMERABLE | F_CONFIGURABLE;
/// `arr_index_flags` RESULT bit (not a descriptor `flags_byte` bit):
/// the index was deleted — element storage stays dense but the index
/// is not an own property (RFC 20260713-defprop-tpd-cluster chunk C).
/// Every consumer treats a hole as absent: reads answer undefined,
/// has/gOPD answer absent, enumeration skips, writes re-create.
pub const F_HOLE: u64 = 1 << 3;

/// Key Str layout mirror — len u32 at +8, payload at +16.
const STR_LEN_OFF: usize = 8;
pub(crate) const STR_DATA_OFF: usize = 16;

/// `AnySlotTag` mirrors.
const ANY_HEAP: u64 = 4;
pub(crate) const ANY_UNDEF: u64 = 5;

unsafe fn key_bytes<'a>(key: *const c_void) -> &'a [u8] {
    let len = unsafe { key.cast::<u8>().add(STR_LEN_OFF).cast::<u32>().read() };
    unsafe { core::slice::from_raw_parts(key.cast::<u8>().add(STR_DATA_OFF), len as usize) }
}

/// Canonical array-index parse — `arr_reflect.rs` (torajs-meta) twin.
fn canonical_index(bytes: &[u8]) -> Option<u64> {
    if bytes.is_empty() || bytes.len() > 10 {
        return None;
    }
    if bytes == b"0" {
        return Some(0);
    }
    if bytes[0] == b'0' || !bytes.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut v: u64 = 0;
    for &b in bytes {
        v = v * 10 + (b - b'0') as u64;
    }
    if v < u32::MAX as u64 { Some(v) } else { None }
}

/// Release a transferred `tag == ANY_HEAP` rc on a rejected define.
unsafe fn drop_owned(tag: u64, value: u64) {
    if tag == ANY_HEAP && value != 0 {
        unsafe { __torajs_value_drop_heap(value as *mut c_void) };
    }
}

/// Props-dynobj slot pointer (offset mirror of `crate::props`).
#[inline]
pub(crate) unsafe fn props_slot(arr: *mut c_void) -> *mut *mut c_void {
    unsafe { (arr as *mut u8).add(crate::layout::ARR_PROPS_OFF) as *mut *mut c_void }
}

/// Header flags word (u16 @6).
#[inline]
unsafe fn header_flags(arr: *const c_void) -> u16 {
    unsafe { (arr.cast::<u8>().add(6) as *const u16).read() }
}

/// Current attribute flags of index `idx` — the shadow entry when one
/// exists, the implicit defaults otherwise. `key` is the caller's
/// index Str (avoids a re-mint).
pub(crate) unsafe fn index_flags_with_key(arr: *const c_void, key: *const c_void) -> u64 {
    if unsafe { header_flags(arr) } & FLAG_ARR_EXOTIC_INDEX == 0 {
        return FLAGS_DEFAULT;
    }
    let props = unsafe { *props_slot(arr as *mut c_void) };
    if props.is_null() || !unsafe { __torajs_dynobj_has(props, key) } {
        return FLAGS_DEFAULT;
    }
    if unsafe { __torajs_dynobj_entry_is_hole(props, key) } != 0 {
        return F_HOLE;
    }
    unsafe { __torajs_dynobj_get_flags(props, key) }
}

/// `Object.getOwnPropertyDescriptor` / element-write flags probe —
/// mint the canonical index key, read the shadow entry (or defaults).
/// Fast path: exotic bit clear → defaults with zero allocation.
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_index_flags(arr: *const c_void, idx: u64) -> u64 {
    if unsafe { header_flags(arr) } & FLAG_ARR_EXOTIC_INDEX == 0 {
        return FLAGS_DEFAULT;
    }
    let mut buf = [0u8; 20];
    let mut n = buf.len();
    let mut v = idx;
    loop {
        n -= 1;
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    let digits = &buf[n..];
    let key = unsafe { __torajs_str_alloc_pooled(digits.len() as u64) };
    unsafe { core::ptr::copy_nonoverlapping(digits.as_ptr(), key.add(STR_DATA_OFF), digits.len()) };
    let flags = unsafe { index_flags_with_key(arr, key as *const c_void) };
    unsafe { __torajs_str_drop(key as *mut c_void) };
    flags
}

/// §10.4.2.1 ArrayDefineOwnProperty entry — see module doc. `tag` /
/// `value` are honored only with `P_VALUE` set; an `ANY_HEAP` value
/// transfers one rc (consumed on store, released on rejection).
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer; `key` a live Str. Caller
/// must check for a pending throw after return.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_define(
    arr: *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
    flags_byte: u64,
) {
    let bytes = unsafe { key_bytes(key) };
    if bytes == b"length" {
        unsafe { define_length(arr, tag, value, flags_byte) };
        return;
    }
    if let Some(idx) = canonical_index(bytes) {
        unsafe { define_index(arr, key, idx, tag, value, flags_byte) };
        return;
    }
    // Ordinary key — full-flags define on the expando props dynobj
    // (lazily allocated; dynobj_define's null-obj guard would no-op).
    let slot = unsafe { props_slot(arr) };
    if unsafe { (*slot).is_null() } {
        unsafe { *slot = __torajs_dynobj_alloc() };
    }
    unsafe { __torajs_dynobj_define(slot, key, tag, value, flags_byte) };
}

/// Validate + apply for a canonical index per §10.1.6.3 (data
/// subset; SameValue approximated by exact `(tag, value)` match —
/// F64 bit equality distinguishes ±0 and unifies NaN, the same
/// heuristic `dynobj_define` uses).
unsafe fn define_index(
    arr: *mut c_void,
    key: *mut c_void,
    idx: u64,
    tag: u64,
    value: u64,
    flags_byte: u64,
) {
    let len = unsafe { (arr.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read() };
    let has_value = flags_byte & P_VALUE != 0;

    if idx < len {
        let cur_flags = unsafe { index_flags_with_key(arr, key as *const c_void) };
        // A hole is an absent property — the define is a fresh
        // CreateDataProperty (extensible check, no current-flags
        // validation) that revives the index (chunk C).
        if cur_flags & F_HOLE != 0 {
            if unsafe { header_flags(arr) } & FLAG_NON_EXTENSIBLE != 0 {
                unsafe { drop_owned(tag, value) };
                unsafe {
                    __torajs_throw_type_error(
                        c"Attempting to define property on object that is not extensible.".as_ptr(),
                    )
                };
                return;
            }
            if has_value {
                unsafe { crate::index_any::__torajs_arr_index_set(arr, idx as i64, tag, value) };
            }
            // Unconditional shadow write — the flags upsert also
            // clears the hole sentinel, so a defaults-flags define
            // must still land.
            unsafe { store_shadow(arr, key, flags_byte & FLAGS_DEFAULT) };
            return;
        }
        let cur_w = cur_flags & F_WRITABLE != 0;
        let cur_e = cur_flags & F_ENUMERABLE != 0;
        let cur_c = cur_flags & F_CONFIGURABLE != 0;
        if !cur_c {
            if flags_byte & P_CONFIGURABLE != 0 && flags_byte & F_CONFIGURABLE != 0 {
                unsafe { drop_owned(tag, value) };
                unsafe {
                    __torajs_throw_type_error(
                        c"Attempting to change configurable attribute of unconfigurable property."
                            .as_ptr(),
                    )
                };
                return;
            }
            if flags_byte & P_ENUMERABLE != 0 && (flags_byte & F_ENUMERABLE != 0) != cur_e {
                unsafe { drop_owned(tag, value) };
                unsafe {
                    __torajs_throw_type_error(
                        c"Attempting to change enumerable attribute of unconfigurable property."
                            .as_ptr(),
                    )
                };
                return;
            }
            if !cur_w {
                if flags_byte & P_WRITABLE != 0 && flags_byte & F_WRITABLE != 0 {
                    unsafe { drop_owned(tag, value) };
                    unsafe {
                        __torajs_throw_type_error(
                            c"Attempting to change writable attribute of unconfigurable property."
                                .as_ptr(),
                        )
                    };
                    return;
                }
                if has_value {
                    let cur_tag = unsafe { crate::any::__torajs_arr_get_any_tag(arr, idx) };
                    let cur_val = unsafe { crate::any::__torajs_arr_get_any_value(arr, idx) };
                    if tag != cur_tag || value != cur_val {
                        unsafe { drop_owned(tag, value) };
                        unsafe {
                            __torajs_throw_type_error(
                                c"Attempting to change value of a readonly property.".as_ptr(),
                            )
                        };
                        return;
                    }
                }
            }
        }
        if has_value {
            // index_set consumes the transferred rc (drop-old +
            // store-new, kind-aware).
            unsafe { crate::index_any::__torajs_arr_index_set(arr, idx as i64, tag, value) };
        }
        let new_flags = fold_flags(cur_flags, flags_byte);
        if new_flags != cur_flags {
            unsafe { store_shadow(arr, key, new_flags) };
        }
        return;
    }

    // Fresh index — §10.4.2.1 step 2: a locked length (chunk D)
    // rejects the implicit length bump before the extensible check.
    if unsafe { header_flags(arr) } & FLAG_ARR_LENGTH_RO != 0 {
        unsafe { drop_owned(tag, value) };
        unsafe {
            __torajs_throw_type_error(
                c"Attempting to define property beyond a non-writable array length.".as_ptr(),
            )
        };
        return;
    }
    if unsafe { header_flags(arr) } & FLAG_NON_EXTENSIBLE != 0 {
        unsafe { drop_owned(tag, value) };
        unsafe {
            __torajs_throw_type_error(
                c"Attempting to define property on object that is not extensible.".as_ptr(),
            )
        };
        return;
    }
    // Dense model: fill the gap with undefined elements, then append
    // the defined value. (Recorded divergence: the fill positions
    // read as own properties; spec makes them holes.)
    let mut cursor = len;
    while cursor < idx {
        unsafe { crate::any::__torajs_arr_push_any(arr, ANY_UNDEF, 0) };
        cursor += 1;
    }
    let (init_tag, init_value) = if has_value {
        (tag, value)
    } else {
        (ANY_UNDEF, 0)
    };
    unsafe { crate::any::__torajs_arr_push_any(arr, init_tag, init_value) };
    // Fresh define completes absent flags to false (§10.1.6.2).
    let new_flags = flags_byte & FLAGS_DEFAULT;
    if new_flags != FLAGS_DEFAULT {
        unsafe { store_shadow(arr, key, new_flags) };
    }
}

/// Spec §7.1.7 ToUint32 over an already-ToNumber'd f64 — NaN / ±inf
/// map to 0, everything else truncates and wraps modulo 2^32.
fn to_uint32(n: f64) -> u32 {
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
unsafe fn define_length(arr: *mut c_void, tag: u64, value: u64, flags_byte: u64) {
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

/// Per-flag fold: present → descriptor value, absent → current.
fn fold_flags(cur: u64, flags_byte: u64) -> u64 {
    let pick = |present: u64, val: u64, cur_bit: u64| -> u64 {
        if flags_byte & present != 0 {
            if flags_byte & val != 0 { val } else { 0 }
        } else {
            cur & cur_bit
        }
    };
    pick(P_WRITABLE, F_WRITABLE, F_WRITABLE)
        | pick(P_ENUMERABLE, F_ENUMERABLE, F_ENUMERABLE)
        | pick(P_CONFIGURABLE, F_CONFIGURABLE, F_CONFIGURABLE)
}

/// Write the shadow flags entry + raise the header exotic bit.
pub(crate) unsafe fn store_shadow(arr: *mut c_void, key: *mut c_void, flags: u64) {
    let slot = unsafe { props_slot(arr) };
    if unsafe { (*slot).is_null() } {
        unsafe { *slot = __torajs_dynobj_alloc() };
    }
    unsafe { __torajs_dynobj_set_entry_flags(slot, key, flags) };
    let p = unsafe { (arr as *mut u8).add(6) as *mut u16 };
    unsafe { p.write(p.read() | FLAG_ARR_EXOTIC_INDEX) };
}
