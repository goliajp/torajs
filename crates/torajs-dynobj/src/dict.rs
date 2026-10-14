//! The keyed read the lowering emits for a binding the checker typed
//! as a dictionary of numbers (`Record<string, number>`).
//!
//! The type does not change what the access answers: a key may name an
//! inherited member (§10.1.8.1 OrdinaryGet walks the chain on a miss)
//! and an `any` alias may have stored any value. It only says which
//! outcome to try first. The read is one call that takes and answers
//! NaN-box values, so the call site is the call alone: an own entry
//! holding a number is the answer verbatim (a number owns nothing and
//! is never an accessor), and every other outcome is what the generic
//! pair read followed by the accessor-or-data result answers.

use core::ffi::c_void;

use crate::get::type_tag;
use crate::layout::{ANY_ACCESSOR, TAG_DYNOBJ};
use crate::nanbox;
use crate::probe::{entries, probe};

unsafe extern "C" {
    /// torajs-anyvalue — both channels of the any-lane member read.
    fn __torajs_any_member_get_pair(recv: u64, key: *const c_void, value_out: *mut u64) -> u64;
    /// torajs-anyvalue — runs the getter behind an accessor answer.
    fn __torajs_any_accessor_get(recv: u64, key: *const c_void, pair_bits: u64) -> u64;
    /// torajs-anyvalue — the caller's own stake on a borrowed box.
    fn __torajs_anyv_retain(v: u64) -> u64;
}

/// `recv` when it boxes a dynamic object, else `None`.
#[inline]
fn dynobj_cell(recv: u64) -> Option<*mut c_void> {
    let obj = recv as *mut c_void;
    (nanbox::is_cell(recv) && unsafe { type_tag(obj) } == TAG_DYNOBJ).then_some(obj)
}

/// The own entry's NaN-box when `obj` owns `key` and the entry holds
/// a number.
///
/// # Safety
/// `obj` is a live dynobj; `key` is a live Str or Symbol cell.
#[inline]
unsafe fn own_number(obj: *mut c_void, key: *const c_void) -> Option<u64> {
    let p = unsafe { probe(obj, key) };
    if !p.found {
        return None;
    }
    let v = unsafe { (*entries(obj).add(p.entry as usize)).value_anyv };
    nanbox::is_number(v).then_some(v)
}

/// `recv[key]` as an owned box.
///
/// # Safety
/// `recv` is a live box; `key` is a live Str cell. The caller checks
/// for a pending throw (a nullish receiver, a throwing getter).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_dict_get(recv: u64, key: *const c_void) -> u64 {
    if let Some(obj) = dynobj_cell(recv)
        && let Some(v) = unsafe { own_number(obj, key) }
    {
        return v;
    }
    let mut value = 0;
    let tag = unsafe { __torajs_any_member_get_pair(recv, key, &mut value) };
    if tag == ANY_ACCESSOR {
        return unsafe { __torajs_any_accessor_get(recv, key, value) };
    }
    unsafe { __torajs_anyv_retain(nanbox::box_from_pair(tag as i64, value as i64)) }
}

// The generic read lives in torajs-anyvalue, whose archive the unit
// tests do not link: the tests stay on own number entries, so any call
// reaching these is a test bug.
#[cfg(test)]
mod generic_stubs {
    use core::ffi::c_void;

    /// # Safety
    /// Never called; any call panics.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __torajs_any_member_get_pair(
        _: u64,
        _: *const c_void,
        _: *mut u64,
    ) -> u64 {
        panic!("torajs-dynobj unit-test stub: the dictionary tests stay on own entries")
    }

    /// # Safety
    /// Never called; any call panics.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __torajs_anyv_retain(_: u64) -> u64 {
        panic!("torajs-dynobj unit-test stub: the dictionary tests stay on own entries")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alloc::{__torajs_dynobj_alloc, free_dynobj_blocks};
    use crate::layout::{BUCKET_FLAGS_DEFAULT, STR_DATA_OFF, STR_LEN_OFF};
    use crate::probe::{
        Entry, KEY_STR_FLAG_IS_LATIN1, bucket_make_key_tagged, count, entries_len, index_ptr,
        set_count, set_entries_len,
    };
    use torajs_anyvalue::nanbox as av;

    /// A Latin-1 Str cell, as the probe reads it.
    fn make_str(s: &str) -> Vec<u64> {
        let mut v = vec![0u64; (STR_DATA_OFF + s.len()).div_ceil(8)];
        unsafe {
            let p = v.as_mut_ptr() as *mut u8;
            *(p.add(6) as *mut u16) = KEY_STR_FLAG_IS_LATIN1;
            *(p.add(STR_LEN_OFF) as *mut u64) = s.len() as u64;
            core::ptr::copy_nonoverlapping(s.as_ptr(), p.add(STR_DATA_OFF), s.len());
        }
        v
    }

    /// A fresh entry without the key retain (the tests own their keys).
    unsafe fn insert(obj: *mut c_void, key: *mut c_void, flags: u64, value_anyv: u64) {
        unsafe {
            let pr = probe(obj, key);
            let e_idx = entries_len(obj);
            *entries(obj).add(e_idx as usize) = Entry {
                key_ptr_tagged: bucket_make_key_tagged(key, flags),
                value_anyv,
            };
            *index_ptr(obj).add(pr.slot as usize) = e_idx;
            set_entries_len(obj, e_idx + 1);
            set_count(obj, count(obj) + 1);
        }
    }

    /// An own int32 or double entry is the answer, box for box.
    #[test]
    fn get_answers_an_own_number_verbatim() {
        let ka = make_str("a");
        let kb = make_str("b");
        let (one, half) = (av::box_int32(-7), av::box_double(0.5));
        unsafe {
            let obj = __torajs_dynobj_alloc();
            let a = ka.as_ptr() as *mut c_void;
            let b = kb.as_ptr() as *mut c_void;
            insert(obj, a, BUCKET_FLAGS_DEFAULT, one);
            insert(obj, b, BUCKET_FLAGS_DEFAULT, half);
            assert_eq!(own_number(obj, a), Some(one));
            assert_eq!(own_number(obj, b), Some(half));
            assert_eq!(__torajs_dynobj_dict_get(obj as u64, a), one);
            assert_eq!(__torajs_dynobj_dict_get(obj as u64, b), half);
            free_dynobj_blocks(obj);
        }
    }

    /// A miss and an entry holding anything but a number are not the
    /// own-number answer (they take the generic read).
    #[test]
    fn only_a_number_entry_is_an_own_number() {
        let ka = make_str("a");
        let kb = make_str("b");
        let kc = make_str("c");
        unsafe {
            let obj = __torajs_dynobj_alloc();
            let a = ka.as_ptr() as *mut c_void;
            let b = kb.as_ptr() as *mut c_void;
            insert(obj, a, BUCKET_FLAGS_DEFAULT, av::VALUE_UNDEFINED);
            insert(
                obj,
                b,
                BUCKET_FLAGS_DEFAULT,
                av::try_box_short_str(b"s").unwrap(),
            );
            assert_eq!(own_number(obj, a), None);
            assert_eq!(own_number(obj, b), None);
            assert_eq!(own_number(obj, kc.as_ptr() as *const c_void), None);
            assert_eq!(dynobj_cell(av::box_int32(1)), None);
            assert_eq!(dynobj_cell(obj as u64), Some(obj));
            free_dynobj_blocks(obj);
        }
    }
}
