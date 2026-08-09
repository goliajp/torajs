//! `Object.seal` per-entry configurable walk.
//!
//! Spec §10.1.4 / §20.1.2.20: `Object.seal(O)` sets `[[Extensible]] =
//! false` AND every own property's `[[Configurable]] = false`. The
//! header-flag part is in `torajs-rc::extensible`; this module owns
//! the dynobj-entry walk that clears each entry's
//! [`crate::layout::BUCKET_FLAG_CONFIGURABLE`] bit (and the symmetric
//! "is everything already non-configurable?" predicate that
//! `Object.isSealed` reads).
//!
//! Pure structural ops over the dense entry array — no rc traffic, no
//! cross-tier externs, no allocation. NULL / non-DynObj input answers
//! the safe default (no-op set / vacuously-true read).

use core::ffi::c_void;

use crate::get::type_tag;
use crate::layout::{
    BUCKET_FLAG_CONFIGURABLE, BUCKET_FLAG_ENUMERABLE, BUCKET_FLAG_WRITABLE, DYNOBJ_KEY_HOLE,
    TAG_DYNOBJ,
};
use crate::probe::{bucket_key_ptr, entries, entries_len, key_str_bytes};

/// `__torajs_dynobj_seal_entries(obj)` — clear
/// `BUCKET_FLAG_CONFIGURABLE` (bit 2 of the `key_ptr_tagged` word) on
/// every live entry. The accompanying `[[Extensible]] = false` flip on
/// the heap header lives in `torajs-rc::extensible`; this is the
/// per-entry half of `Object.seal`.
///
/// Holes (`key_ptr_tagged == DYNOBJ_KEY_HOLE`) are skipped — `delete`
/// already retired their slot.
///
/// # Safety
/// `obj` is null or a live heap pointer with a universal header.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_seal_entries(obj: *mut c_void) {
    if obj.is_null() {
        return;
    }
    if unsafe { type_tag(obj) } != TAG_DYNOBJ {
        return;
    }
    let n = unsafe { entries_len(obj) };
    let base = unsafe { entries(obj) };
    for i in 0..n as usize {
        let e = unsafe { base.add(i) };
        let kp = unsafe { (*e).key_ptr_tagged };
        if kp == DYNOBJ_KEY_HOLE {
            continue;
        }
        unsafe { (*e).key_ptr_tagged = kp & !BUCKET_FLAG_CONFIGURABLE };
    }
}

/// `__torajs_dynobj_freeze_entries(obj)` — clear BOTH
/// [`crate::layout::BUCKET_FLAG_WRITABLE`] and
/// [`crate::layout::BUCKET_FLAG_CONFIGURABLE`] on every live entry, per
/// ES §7.3.14 SetIntegrityLevel with "frozen":
/// - data properties get `{writable: false, configurable: false}`
/// - accessor properties get `{configurable: false}` (writable is
///   meaningless for accessors and the bit is unread on their arm; the
///   symmetric clear is harmless and keeps the walk shape uniform with
///   the seal sibling above)
///
/// Enumerable is preserved. Holes (`key_ptr_tagged == DYNOBJ_KEY_HOLE`)
/// are skipped. The header-flag part (`FLAG_FROZEN` +
/// `FLAG_NON_EXTENSIBLE` + `FLAG_SEALED`) lives in `torajs-rc`; this is
/// the per-entry half.
///
/// # Safety
/// `obj` is null or a live heap pointer with a universal header.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_freeze_entries(obj: *mut c_void) {
    if obj.is_null() {
        return;
    }
    if unsafe { type_tag(obj) } != TAG_DYNOBJ {
        return;
    }
    let n = unsafe { entries_len(obj) };
    let base = unsafe { entries(obj) };
    let clear_mask = !(BUCKET_FLAG_CONFIGURABLE | BUCKET_FLAG_WRITABLE);
    for i in 0..n as usize {
        let e = unsafe { base.add(i) };
        let kp = unsafe { (*e).key_ptr_tagged };
        if kp == DYNOBJ_KEY_HOLE {
            continue;
        }
        unsafe { (*e).key_ptr_tagged = kp & clear_mask };
    }
}

/// `__torajs_dynobj_lock_builtin_fn_class_slots(obj)` — apply the
/// ECMAScript §17 built-in Function attribute pattern to a class-object
/// dynobj (the one that reads via `<ClassName>.<slot>`):
///
/// - `name` and `length` become `{writable: false, enumerable: false,
///   configurable: true}` (§17: "the name/length property of a built-in
///   Function object has the attributes `{[[Writable]]: false,
///   [[Enumerable]]: false, [[Configurable]]: true}`").
/// - `prototype` becomes `{writable: false, enumerable: false,
///   configurable: false}` (§17 for built-in Function; ES §10.2.3
///   MakeConstructor sets the same shape on user classes).
///
/// Called once per class from `__torajs_anyv_class_register` at module
/// init; uniform behaviour across built-in NativeError and user
/// classes because the spec attribute set is the same. Value + key
/// storage are untouched — only the `key_ptr_tagged` low-3 flag bits
/// are masked.
///
/// Walk cost is O(entries), and each class dynobj only carries a handful
/// of slots, so this is negligible.
///
/// # Safety
/// `obj` is null or a live heap pointer with a universal header.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_lock_builtin_fn_class_slots(obj: *mut c_void) {
    if obj.is_null() {
        return;
    }
    if unsafe { type_tag(obj) } != TAG_DYNOBJ {
        return;
    }
    let n = unsafe { entries_len(obj) };
    let base = unsafe { entries(obj) };
    let clear_name_len = BUCKET_FLAG_WRITABLE | BUCKET_FLAG_ENUMERABLE;
    let clear_proto = BUCKET_FLAG_WRITABLE | BUCKET_FLAG_ENUMERABLE | BUCKET_FLAG_CONFIGURABLE;
    for i in 0..n as usize {
        let e = unsafe { base.add(i) };
        let kp = unsafe { (*e).key_ptr_tagged };
        if kp == DYNOBJ_KEY_HOLE {
            continue;
        }
        let key_ptr = bucket_key_ptr(kp);
        if key_ptr.is_null() {
            continue;
        }
        let clear_mask = match key_kind(key_ptr) {
            Some(KeyKind::NameOrLength) => clear_name_len,
            Some(KeyKind::Prototype) => clear_proto,
            None => continue,
        };
        unsafe { (*e).key_ptr_tagged = kp & !clear_mask };
    }
}

enum KeyKind {
    NameOrLength,
    Prototype,
}

/// Byte-compare the Str payload at `key` against the three built-in
/// class-object slot names. A Symbol key names none of them, and
/// answers `None` before its cell is walked as a Str.
fn key_kind(key: *const c_void) -> Option<KeyKind> {
    unsafe {
        let (data, len) = key_str_bytes(key)?;
        let slice = core::slice::from_raw_parts(data, len as usize);
        match slice {
            b"name" | b"length" => Some(KeyKind::NameOrLength),
            b"prototype" => Some(KeyKind::Prototype),
            _ => None,
        }
    }
}

/// `__torajs_dynobj_all_entries_non_configurable(obj) -> bool` — true
/// iff every live entry has its configurable flag cleared (or the
/// object has no live entries at all — spec's vacuous `[[OwnProperty]]
/// = ∅` case).
///
/// NULL or non-DynObj cells answer `true`; this matches the
/// `Object.isSealed` caller, which already has the `[[Extensible]] =
/// false` gate from the header check and is asking "do all own props
/// satisfy non-configurable?". For non-dynobj cells (typed-Obj / Arr /
/// Map / Set / …) there is no observable own-property table in tora,
/// so the AND-with-`[[Extensible]]` reduces to just the header bit.
///
/// # Safety
/// `obj` is null or a live heap pointer with a universal header.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_all_entries_non_configurable(obj: *const c_void) -> bool {
    if obj.is_null() {
        return true;
    }
    if unsafe { type_tag(obj) } != TAG_DYNOBJ {
        return true;
    }
    let n = unsafe { entries_len(obj) };
    let base = unsafe { entries(obj) };
    for i in 0..n as usize {
        let e = unsafe { base.add(i) };
        let kp = unsafe { (*e).key_ptr_tagged };
        if kp == DYNOBJ_KEY_HOLE {
            continue;
        }
        if kp & BUCKET_FLAG_CONFIGURABLE != 0 {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alloc::__torajs_dynobj_alloc;
    use crate::layout::{
        BUCKET_FLAG_ENUMERABLE, BUCKET_FLAG_WRITABLE, BUCKET_FLAGS_DEFAULT, STR_DATA_OFF,
        STR_LEN_OFF,
    };
    use crate::probe::{Entry, bucket_make_key_tagged, entries, set_entries_len};

    fn make_str(s: &str) -> Vec<u64> {
        let bytes = STR_DATA_OFF + s.len();
        let mut v = vec![0u64; bytes.div_ceil(8)];
        unsafe {
            let p = v.as_mut_ptr() as *mut u8;
            *(p.add(STR_LEN_OFF) as *mut u64) = s.len() as u64;
            core::ptr::copy_nonoverlapping(s.as_ptr(), p.add(STR_DATA_OFF), s.len());
        }
        v
    }

    unsafe fn raw_append(obj: *mut c_void, i: u32, key: *const c_void, flags: u64) {
        unsafe {
            *entries(obj).add(i as usize) = Entry {
                key_ptr_tagged: bucket_make_key_tagged(key as *mut c_void, flags),
                value_anyv: 0,
            };
            set_entries_len(obj, i + 1);
        }
    }

    /// Empty dynobj — vacuously sealed; seal walk is a no-op.
    #[test]
    fn empty_is_vacuously_sealed() {
        unsafe {
            let obj = __torajs_dynobj_alloc();
            assert!(__torajs_dynobj_all_entries_non_configurable(obj));
            __torajs_dynobj_seal_entries(obj);
            assert!(__torajs_dynobj_all_entries_non_configurable(obj));
            crate::alloc::free_dynobj_blocks(obj);
        }
    }

    /// Default-flag insert reports configurable; post-seal the bit is
    /// cleared on every entry while writable / enumerable persist.
    #[test]
    fn seal_clears_configurable_keeps_other_flags() {
        let ka = make_str("alpha");
        let kb = make_str("beta");
        unsafe {
            let obj = __torajs_dynobj_alloc();
            raw_append(obj, 0, ka.as_ptr() as *const c_void, BUCKET_FLAGS_DEFAULT);
            raw_append(obj, 1, kb.as_ptr() as *const c_void, BUCKET_FLAGS_DEFAULT);
            assert!(!__torajs_dynobj_all_entries_non_configurable(obj));

            __torajs_dynobj_seal_entries(obj);
            assert!(__torajs_dynobj_all_entries_non_configurable(obj));

            // W / E stay; C is gone.
            let kept = BUCKET_FLAG_WRITABLE | BUCKET_FLAG_ENUMERABLE;
            for i in 0..2 {
                let kp = (*entries(obj).add(i)).key_ptr_tagged & (BUCKET_FLAGS_DEFAULT);
                assert_eq!(kp, kept);
            }
            crate::alloc::free_dynobj_blocks(obj);
        }
    }

    /// A hole between two live entries does not flip back to
    /// configurable and does not break the all-non-configurable read.
    #[test]
    fn hole_is_skipped_on_walk() {
        let ka = make_str("a");
        let kb = make_str("b");
        unsafe {
            let obj = __torajs_dynobj_alloc();
            raw_append(obj, 0, ka.as_ptr() as *const c_void, BUCKET_FLAGS_DEFAULT);
            // entry index 1 left as a hole (zeroed by calloc, so
            // key_ptr_tagged == DYNOBJ_KEY_HOLE).
            set_entries_len(obj, 2);
            raw_append(obj, 2, kb.as_ptr() as *const c_void, BUCKET_FLAGS_DEFAULT);
            __torajs_dynobj_seal_entries(obj);
            assert!(__torajs_dynobj_all_entries_non_configurable(obj));
            // The hole's word is still DYNOBJ_KEY_HOLE — seal must
            // not flip it.
            assert_eq!((*entries(obj).add(1)).key_ptr_tagged, DYNOBJ_KEY_HOLE);
            crate::alloc::free_dynobj_blocks(obj);
        }
    }

    /// NULL / non-DynObj input is the safe default.
    #[test]
    fn null_and_foreign_inputs_are_vacuous() {
        unsafe {
            __torajs_dynobj_seal_entries(core::ptr::null_mut());
            assert!(__torajs_dynobj_all_entries_non_configurable(
                core::ptr::null()
            ));
            // Str-shaped block reads vacuous + walk is no-op.
            let s = make_str("imposter");
            let sp = s.as_ptr() as *mut c_void;
            __torajs_dynobj_seal_entries(sp);
            assert!(__torajs_dynobj_all_entries_non_configurable(sp));
        }
    }
}
