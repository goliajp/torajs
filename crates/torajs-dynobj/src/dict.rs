//! The keyed read and write the lowering emits for a binding the
//! checker typed as a dictionary of numbers (`Record<string, number>`).
//!
//! The type does not change what the access answers: a key may name an
//! inherited member (§10.1.8.1 OrdinaryGet walks the chain on a miss)
//! and an `any` alias may have stored any value. It only says which
//! outcome to try first. Each entry is one call that takes and answers
//! NaN-box values, so the call site is the call alone:
//!
//! - the read answers an own entry holding a number verbatim (a number
//!   owns nothing and is never an accessor) and every other outcome
//!   exactly as the generic pair read followed by the accessor-or-data
//!   result does;
//! - the write stores into a writable own data entry whose current
//!   value is an immediate (nothing to release, no setter, no global
//!   binding behind it) and hands every other case to the member-set
//!   kernel the generic write calls.

use core::ffi::c_void;

use crate::get::type_tag;
use crate::layout::{ANY_ACCESSOR, BUCKET_FLAG_WRITABLE, DYNOBJ_HDR_FLAG_MODULE_NS, TAG_DYNOBJ};
use crate::nanbox;
use crate::probe::{Entry, bucket_flags};
use crate::probe_str::{cached, walk_str};

unsafe extern "C" {
    /// torajs-anyvalue — both channels of the any-lane member read.
    fn __torajs_any_member_get_pair(recv: u64, key: *const c_void, value_out: *mut u64) -> u64;
    /// torajs-anyvalue — runs the getter behind an accessor answer.
    fn __torajs_any_accessor_get(recv: u64, key: *const c_void, pair_bits: u64) -> u64;
    /// torajs-anyvalue — the caller's own stake on a borrowed box.
    fn __torajs_anyv_retain(v: u64) -> u64;
    /// torajs-rc / torajs-str — a share of a key cell, and its release.
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-anyvalue — the member-set kernel, strict and sloppy.
    fn __torajs_any_member_set(
        recv_slot: *mut u64,
        key: *mut c_void,
        tag: u64,
        value: u64,
        hint: i64,
    );
    fn __torajs_any_member_set_soft(
        recv_slot: *mut u64,
        key: *mut c_void,
        tag: u64,
        value: u64,
        hint: i64,
    ) -> i64;
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
/// `obj` is a live dynobj; `key` is a live Str cell.
#[inline]
unsafe fn own_number(obj: *mut c_void, key: *const c_void) -> Option<u64> {
    let v = unsafe { (*walk_str(obj, key)?).value_anyv };
    nanbox::is_number(v).then_some(v)
}

/// True when a write may overwrite the entry `e` in place: a writable
/// data entry holding an immediate (nothing to release, no setter, no
/// global binding behind it).
///
/// # Safety
/// `e` is a live entry.
#[inline(always)]
unsafe fn plain(e: *mut Entry) -> bool {
    unsafe {
        bucket_flags((*e).key_ptr_tagged) & BUCKET_FLAG_WRITABLE != 0
            && !nanbox::is_cell((*e).value_anyv)
    }
}

/// True for a module namespace, which refuses every write (§10.4.6.9)
/// whatever its entries say.
///
/// # Safety
/// `obj` is a live dynobj.
#[inline(always)]
unsafe fn module_ns(obj: *mut c_void) -> bool {
    unsafe { (obj.cast::<u8>().add(6) as *const u16).read() & DYNOBJ_HDR_FLAG_MODULE_NS != 0 }
}

/// The own entry of `obj` that a write to `key` may overwrite in
/// place, found by the walk: one that is [`plain`]. The caller has
/// ruled out a module namespace.
///
/// # Safety
/// `obj` is a live dynobj; `key` is a live Str cell.
#[inline]
unsafe fn plain_own_entry(obj: *mut c_void, key: *const c_void) -> Option<*mut Entry> {
    unsafe {
        let e = walk_str(obj, key)?;
        plain(e).then_some(e)
    }
}

/// `recv[key]` as an owned box.
///
/// # Safety
/// `recv` is a live box; `key` is a live Str cell. The caller checks
/// for a pending throw (a nullish receiver, a throwing getter).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_dict_get(recv: u64, key: *const c_void) -> u64 {
    let Some(obj) = dynobj_cell(recv) else {
        return unsafe { get_generic(recv, key) };
    };
    if let Some(e) = unsafe { cached(obj, key) } {
        let v = unsafe { (*e).value_anyv };
        if nanbox::is_number(v) {
            return v;
        }
    }
    unsafe { get_walk(obj, key) }
}

/// [`__torajs_dynobj_dict_get`] on a dynobj whose cached entry did not
/// answer: the walk, then the generic read.
///
/// # Safety
/// `obj` is a live dynobj; otherwise as [`__torajs_dynobj_dict_get`].
#[inline(never)]
unsafe fn get_walk(obj: *mut c_void, key: *const c_void) -> u64 {
    match unsafe { own_number(obj, key) } {
        Some(v) => v,
        None => unsafe { get_generic(obj as u64, key) },
    }
}

// the walks and the generic halves of both entries stay out of line:
// the cached paths above them then call nothing but a tail call and
// need no stack frame

/// The generic read: the pair read, then the getter on an accessor
/// answer, else the retained box.
///
/// The caller may have borrowed `key` from a binding or a field that a
/// getter or a proxy trap run from here can rewrite, and the proxy
/// path reads the key again after its trap returns, so the read holds
/// a share of the key cell until it is done.
///
/// # Safety
/// As [`__torajs_dynobj_dict_get`].
#[inline(never)]
unsafe fn get_generic(recv: u64, key: *const c_void) -> u64 {
    unsafe {
        __torajs_rc_inc(key as *mut c_void);
        let mut value = 0;
        let tag = __torajs_any_member_get_pair(recv, key, &mut value);
        let out = if tag == ANY_ACCESSOR {
            __torajs_any_accessor_get(recv, key, value)
        } else {
            __torajs_anyv_retain(nanbox::box_from_pair(tag as i64, value as i64))
        };
        __torajs_str_drop(key as *mut c_void);
        out
    }
}

/// `*recv_slot[key] = v`, `v` carrying the caller's stake on a heap
/// payload. `throw_on_refusal` is the strict goal's §13.15.2 TypeError
/// (sloppy code discards the refusal).
///
/// # Safety
/// `recv_slot` points at the receiver's live box; `key` is a live Str
/// cell; `v` is a live box. The caller checks for a pending throw.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_dict_set(
    recv_slot: *mut u64,
    key: *mut c_void,
    v: u64,
    throw_on_refusal: bool,
) {
    unsafe {
        let Some(obj) = dynobj_cell(*recv_slot) else {
            return set_generic(recv_slot, key, v, throw_on_refusal);
        };
        if module_ns(obj) {
            return set_generic(recv_slot, key, v, throw_on_refusal);
        }
        // the read of `d[k] = (d[k] ?? 0) + 1` found this entry just
        // now; a number in it is an immediate (the walk takes the
        // other immediates)
        if let Some(e) = cached(obj, key)
            && bucket_flags((*e).key_ptr_tagged) & BUCKET_FLAG_WRITABLE != 0
            && nanbox::is_number((*e).value_anyv)
        {
            store_own(e, v);
            return;
        }
        set_walk(recv_slot, key, v, throw_on_refusal, obj);
    }
}

/// The in-place write into the entry `e` that [`plain_own_entry`]
/// answers.
///
/// It owes the builtin-prototype own-write note nothing: the note
/// marks a name the first time an own entry is created under it on a
/// `<Ctor>.prototype` singleton, the mark is never cleared, and every
/// kernel that creates an entry notes it, so the entry overwritten
/// here was noted when it was made. (The installs that run before a
/// singleton is published add no writable entry holding an immediate.)
///
/// # Safety
/// `e` is a plain own entry; `v` is a live box.
#[inline(always)]
unsafe fn store_own(e: *mut Entry, v: u64) {
    unsafe { (*e).value_anyv = v };
}

/// [`__torajs_dynobj_dict_set`] on the dynobj `obj` held in
/// `*recv_slot`, no module namespace, whose cached entry did not take
/// the write: the walk,
/// then the generic write. `obj` comes last so the other arguments
/// stay in the registers the entry received them in.
///
/// # Safety
/// `obj` is the live dynobj `*recv_slot` holds, no module namespace;
/// otherwise as
/// [`__torajs_dynobj_dict_set`].
#[inline(never)]
unsafe fn set_walk(
    recv_slot: *mut u64,
    key: *mut c_void,
    v: u64,
    throw_on_refusal: bool,
    obj: *mut c_void,
) {
    unsafe {
        match plain_own_entry(obj, key) {
            Some(e) => store_own(e, v),
            None => set_generic(recv_slot, key, v, throw_on_refusal),
        }
    }
}

/// The generic write: the member-set kernel on the decoded box.
///
/// # Safety
/// As [`__torajs_dynobj_dict_set`].
#[inline(never)]
unsafe fn set_generic(recv_slot: *mut u64, key: *mut c_void, v: u64, throw_on_refusal: bool) {
    unsafe {
        let (tag, value) = (nanbox::unbox_tag(v) as u64, nanbox::unbox_value(v) as u64);
        if throw_on_refusal {
            __torajs_any_member_set(recv_slot, key, tag, value, -1);
        } else {
            __torajs_any_member_set_soft(recv_slot, key, tag, value, -1);
        }
    }
}

// The generic read and the kernel write live in torajs-anyvalue, whose
// archive the unit tests do not link: the tests stay on the own-entry
// paths, so any call reaching these is a test bug.
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

    /// # Safety
    /// Never called; any call panics.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __torajs_any_member_set(
        _: *mut u64,
        _: *mut c_void,
        _: u64,
        _: u64,
        _: i64,
    ) {
        panic!("torajs-dynobj unit-test stub: the dictionary tests stay on own entries")
    }

    /// # Safety
    /// Never called; any call panics.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __torajs_any_member_set_soft(
        _: *mut u64,
        _: *mut c_void,
        _: u64,
        _: u64,
        _: i64,
    ) -> i64 {
        panic!("torajs-dynobj unit-test stub: the dictionary tests stay on own entries")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alloc::{__torajs_dynobj_alloc, free_dynobj_blocks};
    use crate::layout::{
        BUCKET_FLAG_CONFIGURABLE, BUCKET_FLAGS_DEFAULT, STR_DATA_OFF, STR_LEN_OFF,
    };
    use crate::probe::{
        KEY_STR_FLAG_IS_LATIN1, bucket_make_key_tagged, count, entries, entries_len, index_ptr,
        probe, set_count, set_entries_len,
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

    /// A writable own entry holding an immediate takes the box as is,
    /// in place, with its flags.
    #[test]
    fn set_stores_the_box_into_a_writable_immediate_entry() {
        let k = make_str("k");
        unsafe {
            let obj = __torajs_dynobj_alloc();
            let kp = k.as_ptr() as *mut c_void;
            insert(obj, kp, BUCKET_FLAGS_DEFAULT, av::box_int32(1));
            let mut recv = obj as u64;
            let v = av::box_double(2.5);
            __torajs_dynobj_dict_set(&mut recv, kp, v, true);
            assert_eq!((*plain_own_entry(obj, kp).unwrap()).value_anyv, v);
            assert_eq!(recv, obj as u64);
            let flags = bucket_flags((*entries(obj)).key_ptr_tagged);
            assert_eq!(flags, BUCKET_FLAGS_DEFAULT);
            free_dynobj_blocks(obj);
        }
    }

    /// Only a writable own entry holding an immediate is written in
    /// place: a non-writable entry, one holding a heap cell (a value to
    /// release, an accessor, a global binding), a miss and a module
    /// namespace all go to the kernel.
    #[test]
    fn only_a_writable_immediate_entry_is_written_in_place() {
        let (ka, kb, kc, kd) = (make_str("a"), make_str("b"), make_str("c"), make_str("d"));
        let cell = [0u64; 4];
        unsafe {
            let obj = __torajs_dynobj_alloc();
            let a = ka.as_ptr() as *mut c_void;
            let b = kb.as_ptr() as *mut c_void;
            let c = kc.as_ptr() as *mut c_void;
            insert(obj, a, BUCKET_FLAGS_DEFAULT, av::VALUE_UNDEFINED);
            insert(obj, b, BUCKET_FLAG_CONFIGURABLE, av::box_int32(3));
            insert(obj, c, BUCKET_FLAGS_DEFAULT, cell.as_ptr() as u64);
            assert!(plain_own_entry(obj, a).is_some());
            assert!(plain_own_entry(obj, b).is_none());
            assert!(plain_own_entry(obj, c).is_none());
            assert!(plain_own_entry(obj, kd.as_ptr() as *const c_void).is_none());
            assert!(!module_ns(obj));
            *(obj.cast::<u8>().add(6) as *mut u16) |= DYNOBJ_HDR_FLAG_MODULE_NS;
            assert!(module_ns(obj));
            *(obj.cast::<u8>().add(6) as *mut u16) &= !DYNOBJ_HDR_FLAG_MODULE_NS;
            free_dynobj_blocks(obj);
        }
    }
}
