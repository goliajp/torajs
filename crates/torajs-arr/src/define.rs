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

use torajs_rc::{FLAG_ARR_EXOTIC_INDEX, FLAG_ARR_LENGTH_RO, FLAG_NON_EXTENSIBLE, HeapHeader, Tag};

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
    fn __torajs_dynobj_has(dynobj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_dynobj_get_flags(dynobj: *const c_void, key: *const c_void) -> u64;
    /// torajs-dynobj — raw flags upsert for shadow entries (no
    /// §10.1.6.3 validation; this kernel already validated). A hit
    /// also revives a hole entry (value slot resets to live).
    fn __torajs_dynobj_set_entry_flags(obj_slot: *mut *mut c_void, key: *mut c_void, flags: u64);
    /// torajs-dynobj — HOLE sentinel probe (chunk C; the upsert
    /// lives in `define_hole.rs`).
    fn __torajs_dynobj_entry_is_hole(dynobj: *const c_void, key: *const c_void) -> i32;
    /// torajs-dynobj — entry removal (drops key + value; the
    /// accessor→data transition deletes the pair-owning shadow entry).
    fn __torajs_dynobj_delete(dynobj: *mut c_void, key: *const c_void) -> i32;
    /// torajs-str — canonical index key mint for the flags probe.
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-str — view-aware content equality (SameValue on Str).
    fn __torajs_str_eq(a: *const u8, b: *const u8) -> i64;
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
pub(crate) const P_WRITABLE: u64 = 1 << 3;
pub(crate) const P_ENUMERABLE: u64 = 1 << 4;
pub(crate) const P_CONFIGURABLE: u64 = 1 << 5;
pub(crate) const P_VALUE: u64 = 1 << 6;
/// Accessor-face present bits (DEFINE_PRESENT_GET / _SET mirrors) —
/// route to the chunk-C accessor arm.
const P_GET: u64 = 1 << 7;
const P_SET: u64 = 1 << 8;
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
pub(crate) unsafe fn drop_owned(tag: u64, value: u64) {
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
pub(crate) unsafe fn header_flags(arr: *const c_void) -> u16 {
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
    if props.is_null() || unsafe { __torajs_dynobj_has(props, key) } == 0 {
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
    let key = unsafe { mint_index_key(idx) };
    let flags = unsafe { index_flags_with_key(arr, key as *const c_void) };
    unsafe { __torajs_str_drop(key as *mut c_void) };
    flags
}

/// Mint a pooled Str carrying the canonical decimal digits of `idx`
/// (the shadow-entry key shape). Caller owns the returned Str.
pub(crate) unsafe fn mint_index_key(idx: u64) -> *mut u8 {
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
    key
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
        if flags_byte & (P_GET | P_SET) != 0 {
            // §10.4.2.4 — length is a data property; an accessor
            // redefine of a non-configurable property throws.
            unsafe { drop_owned(tag, value) };
            unsafe {
                __torajs_throw_type_error(
                    c"Attempting to change configurable attribute of unconfigurable property."
                        .as_ptr(),
                )
            };
            return;
        }
        unsafe { crate::define_length::define_length(arr, tag, value, flags_byte) };
        return;
    }
    if let Some(idx) = canonical_index(bytes) {
        if flags_byte & (P_GET | P_SET) != 0 {
            unsafe {
                crate::define_accessor::define_index_accessor(arr, key, idx, tag, value, flags_byte)
            };
            return;
        }
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

/// §7.2.10 SameValue on unboxed `(tag, value)` pairs. Bit equality
/// covers immediates (F64 bits distinguish ±0 and unify NaN) and
/// heap identity; equal-content Str cells at different addresses
/// compare by content (view-aware `str_eq`), and a mixed-width
/// number pair (I64 5 vs F64 5.0) is the same Number value.
unsafe fn same_value_pair(a_tag: u64, a_val: u64, b_tag: u64, b_val: u64) -> bool {
    if a_tag == b_tag && a_val == b_val {
        return true;
    }
    const ANY_I64: u64 = 2;
    const ANY_F64: u64 = 3;
    if a_tag == ANY_I64 && b_tag == ANY_F64 {
        // bits-equal keeps ±0 apart (i as f64 is always +0); the
        // round-trip check rejects a lossy i64 → f64 conversion.
        let (i, d) = (a_val as i64, f64::from_bits(b_val));
        return d.to_bits() == (i as f64).to_bits() && d as i64 == i;
    }
    if a_tag == ANY_F64 && b_tag == ANY_I64 {
        return unsafe { same_value_pair(b_tag, b_val, a_tag, a_val) };
    }
    if a_tag != ANY_HEAP || b_tag != ANY_HEAP || a_val == 0 || b_val == 0 {
        return false;
    }
    let a_tt = unsafe { (*(a_val as *const HeapHeader)).type_tag };
    let b_tt = unsafe { (*(b_val as *const HeapHeader)).type_tag };
    if a_tt != Tag::Str as u16 || b_tt != Tag::Str as u16 {
        return false;
    }
    unsafe { __torajs_str_eq(a_val as *const u8, b_val as *const u8) != 0 }
}

/// Validate + apply for a canonical index per §10.1.6.3 (data
/// subset; the readonly value check runs §7.2.10 SameValue via
/// [`same_value_pair`]).
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
        // Accessor → data transition (chunk C): a data descriptor over
        // an accessor index needs the configurable gate, then the
        // shadow entry (owning the pair) is deleted and the define
        // lands as a data property — absent `writable` completes to
        // false, e/c keep the current values (§10.1.6.3 step 4).
        let pair = unsafe { crate::define_accessor::__torajs_arr_index_accessor(arr, idx) };
        if !pair.is_null() {
            let cur_e = cur_flags & F_ENUMERABLE != 0;
            let cur_c = cur_flags & F_CONFIGURABLE != 0;
            if !cur_c {
                unsafe { drop_owned(tag, value) };
                unsafe {
                    __torajs_throw_type_error(
                        c"Attempting to change configurable attribute of unconfigurable property."
                            .as_ptr(),
                    )
                };
                return;
            }
            let props = unsafe { *props_slot(arr) };
            unsafe { __torajs_dynobj_delete(props, key as *const c_void) };
            if has_value {
                unsafe { crate::index_any::__torajs_arr_index_set(arr, idx as i64, tag, value) };
            }
            let mut new_flags = 0u64;
            if flags_byte & P_WRITABLE != 0 && flags_byte & F_WRITABLE != 0 {
                new_flags |= F_WRITABLE;
            }
            let e = if flags_byte & P_ENUMERABLE != 0 {
                flags_byte & F_ENUMERABLE != 0
            } else {
                cur_e
            };
            let c = if flags_byte & P_CONFIGURABLE != 0 {
                flags_byte & F_CONFIGURABLE != 0
            } else {
                cur_c
            };
            if e {
                new_flags |= F_ENUMERABLE;
            }
            if c {
                new_flags |= F_CONFIGURABLE;
            }
            unsafe { store_shadow(arr, key, new_flags) };
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
                    if !unsafe { same_value_pair(tag, value, cur_tag, cur_val) } {
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
