//! Typed `number` value lane of a `Map<K, number>`.
//!
//! A Map the compiler created for a `Map<K, number>` binding carries
//! [`FLAG_MAP_VALUES_NUMBER`] from birth. Its values are then read as
//! an f64 slot (`get` answers a number or the undefined sentinel, no
//! tag dispatch at the consumer), which holds only while no other kind
//! of value ever lands in it. The typed `set` lane stores numbers by
//! construction; every other writer goes through
//! [`crate::mutate::__torajs_map_set`] or the `set_str` hint path, and
//! both refuse a non-number value on a marked Map with a TypeError —
//! the Map twin of the `number[]` push backstop (`torajs-arr`
//! `typed_push_pair`). The refusal sits at the write, so the read
//! kernels below never see a foreign tag (RFC 20260706 settled that a
//! read-point throw is the worse half: the message cannot say who
//! wrote).

use core::ffi::c_void;

use crate::layout::{ANY_F64, ANY_I64, ANY_UNDEF, FLAG_MAP_VALUES_NUMBER, Map};
use crate::probe::map_lookup_slot;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// Mirror of `torajs_rc::F64_UNDEF_SENTINEL_BITS` (this crate does not
/// depend on torajs-rc; same replication as torajs-meta's copy).
const F64_UNDEF_SENTINEL_BITS: u64 = 0x7FFC_0000_0000_000A;

/// Mark a freshly created Map as the store of a `Map<K, number>`
/// binding.
///
/// # Safety
/// `p` is a live Map with no entries: the mark admits what is already
/// stored without looking at it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_mark_num(p: *mut c_void) {
    let m = p as *mut Map;
    unsafe {
        debug_assert_eq!((*m).n_used, 0, "value-kind mark on a non-empty Map");
        (*m).header.flags |= FLAG_MAP_VALUES_NUMBER;
    }
}

/// True when `m` may take a value tagged `tag`; false raises the
/// TypeError, and the caller releases what it owns and returns.
///
/// # Safety
/// `m` is a live Map.
#[inline]
pub(crate) unsafe fn admits_value(m: *const Map, tag: u8) -> bool {
    if unsafe { (*m).header.flags } & FLAG_MAP_VALUES_NUMBER == 0
        || matches!(tag, ANY_I64 | ANY_F64 | ANY_UNDEF)
    {
        return true;
    }
    unsafe {
        __torajs_throw_type_error(
            c"set through an any view would change the typed Map's value kind".as_ptr(),
        );
    }
    false
}

/// A stored value read as a `number` slot: undefined answers the
/// sentinel.
#[inline]
fn value_f64(v_anyv: u64) -> f64 {
    match crate::key_decode::stored_pair(v_anyv) {
        (ANY_I64, v) => v as i64 as f64,
        (ANY_F64, b) => f64::from_bits(b),
        (tag, _) => {
            // the write-point refusal keeps every other tag out
            debug_assert_eq!(tag, ANY_UNDEF, "typed Map value lane saw tag {tag}");
            f64::from_bits(F64_UNDEF_SENTINEL_BITS)
        }
    }
}

/// `m.get(s)` on the string-key lane: entry `idx` from
/// `__torajs_map_find_str`, or the sentinel for a miss (`idx < 0`).
///
/// # Safety
/// `p` is a marked live Map when `idx >= 0`, and `idx` is a live entry
/// index answered by `__torajs_map_find_str` with no write since.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_entry_get_f64(p: *const c_void, idx: i64) -> f64 {
    if idx < 0 {
        return f64::from_bits(F64_UNDEF_SENTINEL_BITS);
    }
    let m = p as *const Map;
    value_f64(unsafe { (*(*m).entries.add(idx as usize)).value_anyv })
}

/// `m.get(k)` on the `(tag, payload)` key lane. The key stays owned by
/// the caller.
///
/// # Safety
/// `p` is a marked live Map.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_get_f64(
    p: *const c_void,
    key_tag: i64,
    key_payload: i64,
) -> f64 {
    let m = p as *const Map;
    let lr = unsafe { map_lookup_slot(m, key_tag as u8, key_payload as u64) };
    if !lr.found {
        return f64::from_bits(F64_UNDEF_SENTINEL_BITS);
    }
    value_f64(unsafe { (*(*m).entries.add(lr.entry_idx as usize)).value_anyv })
}
