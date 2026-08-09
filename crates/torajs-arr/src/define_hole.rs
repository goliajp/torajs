//! `delete arr[i]` hole semantics — RFC 20260713-defprop-tpd-cluster
//! chunk C, split out of `define.rs` (file-size hard limit).
//!
//! Element storage stays dense: a deleted canonical index drops its
//! element value (slot reads `undefined`) and gains a HOLE shadow
//! entry in the expando props dynobj (sentinel value slot — see
//! torajs-dynobj `set_entry_hole`). Every own-property consumer
//! treats the hole as absent through the `arr_index_flags` result's
//! [`crate::define::F_HOLE`] bit; a plain write or a fresh define
//! revives the index (the flags upsert clears the sentinel).

use core::ffi::c_void;

use torajs_rc::FLAG_ARR_EXOTIC_INDEX;

use crate::define::{
    ANY_UNDEF, F_CONFIGURABLE, F_HOLE, FLAGS_DEFAULT, STR_DATA_OFF, index_flags_with_key,
    props_slot, store_shadow,
};
use crate::layout::ARR_LEN_OFF;

unsafe extern "C" {
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_has(dynobj: *const c_void, key: *const c_void) -> i32;
    /// torajs-dynobj — HOLE sentinel upsert / probe (chunk C).
    fn __torajs_dynobj_set_entry_hole(obj_slot: *mut *mut c_void, key: *mut c_void);
    fn __torajs_dynobj_entry_is_hole(dynobj: *const c_void, key: *const c_void) -> i32;
    /// torajs-dynobj — entry removal (drops key + value, incl. an
    /// accessor pair cell).
    fn __torajs_dynobj_delete(dynobj: *mut c_void, key: *const c_void) -> i32;
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
}

/// Re-create a deleted index as a default data property — the plain
/// `arr[i] = v` write path calls this after the element store when
/// the index was a hole (§10.1.5.1 CreateDataProperty completes to
/// w/e/c all true, and the flags upsert clears the hole sentinel).
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer; `key` a live Str.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_index_revive(arr: *mut c_void, key: *mut c_void) {
    unsafe { store_shadow(arr, key, FLAGS_DEFAULT) };
}

/// Cross-tier entry over [`revive_index_if_hole`] — the static
/// typed lane's post-store revive (RFC 20260721-typed-grow-on-write
/// chunk B: an in-bounds `a[i] = v` into a HOLE index re-creates it
/// as a default data property, §10.1.5.1). The emit gates on
/// `FLAG_ARR_EXOTIC_INDEX` inline, so the plain-array hot path
/// never pays the call.
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer; `idx` was bounds-checked
/// by the caller's guard (negative never reaches the store arm).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_index_revive_idx(arr: *mut c_void, idx: i64) {
    unsafe { revive_index_if_hole(arr, idx as u64) };
}

/// Numeric-index twin of [`__torajs_arr_index_revive`] for the
/// any-tier element write lanes (`arr[i] = v` with a number index
/// never mints a key Str) — mint, revive iff the index is currently
/// a hole, drop. Callers gate on `FLAG_ARR_EXOTIC_INDEX` so the
/// plain-array hot path never reaches here.
pub(crate) unsafe fn revive_index_if_hole(arr: *mut c_void, idx: u64) {
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
    let props = unsafe { *props_slot(arr) };
    if !props.is_null()
        && unsafe { __torajs_dynobj_has(props, key as *const c_void) } != 0
        && unsafe { __torajs_dynobj_entry_is_hole(props, key as *const c_void) } != 0
    {
        unsafe { store_shadow(arr, key as *mut c_void, FLAGS_DEFAULT) };
    }
    unsafe { __torajs_str_drop(key as *mut c_void) };
}

/// Mark every index in `[from, to)` a HOLE — the §10.4.2.5
/// length-grow tail (RFC 20260721 刀 5 G3: grown slots are not own
/// properties). Same shadow-entry representation as `delete`, so
/// every existing consumer (has / gOPD / enumeration / reads)
/// answers absent for free.
pub(crate) unsafe fn mark_hole_range(arr: *mut c_void, from: u64, to: u64) {
    if from >= to {
        return;
    }
    unsafe {
        let slot = props_slot(arr);
        if (*slot).is_null() {
            *slot = __torajs_dynobj_alloc();
        }
        for i in from..to {
            let key = crate::define::mint_index_key(i);
            __torajs_dynobj_set_entry_hole(slot, key as *mut c_void);
            __torajs_str_drop(key as *mut c_void);
        }
        let p = (arr as *mut u8).add(6) as *mut u16;
        p.write(p.read() | FLAG_ARR_EXOTIC_INDEX);
    }
}

/// Mark the LAST pushed slot of a freshly-built array literal a
/// HOLE (§13.2.4 elision — the slot reads undefined but is not an
/// own property: `1 in [0,,2]` is false, indexOf skips it). Called
/// by the array-literal lowering right after the elision slot's
/// push, so `len - 1` is the elision index; the fresh slot carries
/// default attributes, so the delete below cannot refuse.
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_mark_last_hole(arr: *mut c_void) {
    let len = unsafe { (arr.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read() };
    if len == 0 {
        return;
    }
    let idx = len - 1;
    let key = unsafe { crate::define::mint_index_key(idx) };
    unsafe {
        __torajs_arr_delete_index(arr, key as *mut c_void, idx);
        __torajs_str_drop(key as *mut c_void);
    }
}

/// RFC 20260721 刀 12 G16 — splice moved the tail elements but the
/// exotic-index HOLE shadows are keyed by index string, so a hole
/// must travel with its slot (ES §14.7.5: an unvisited deleted index
/// is never enumerated — sm/splice-suppresses-unvisited-indexes).
/// Snapshot the affected range's holes, revive every stale shadow,
/// re-mark each tail hole at its shifted position (dropped when it
/// lands outside the new length). Non-hole attribute shadows stay
/// put (recorded boundary — the G7/G14 exotic-flag design owns full
/// attribute motion).
pub(crate) unsafe fn splice_remap_holes(
    arr: *mut c_void,
    start: i64,
    deleted: i64,
    inserted: i64,
    old_len: i64,
) {
    if unsafe { crate::define::header_flags(arr) } & FLAG_ARR_EXOTIC_INDEX == 0 {
        return;
    }
    let delta = inserted - deleted;
    let new_len = old_len + delta;
    let mut moved: Vec<i64> = Vec::new();
    for idx in start.max(0)..old_len {
        let key = unsafe { crate::define::mint_index_key(idx as u64) };
        let is_hole = unsafe { index_flags_with_key(arr, key as *const c_void) } & F_HOLE != 0;
        if is_hole {
            // Revive the stale shadow — the kernel already moved /
            // overwrote the slot itself.
            unsafe { store_shadow(arr, key as *mut c_void, FLAGS_DEFAULT) };
            let nidx = idx + delta;
            if idx >= start + deleted && (0..new_len).contains(&nidx) {
                moved.push(nidx);
            }
        }
        unsafe { __torajs_str_drop(key as *mut c_void) };
    }
    if moved.is_empty() {
        return;
    }
    unsafe {
        let slot = props_slot(arr);
        if (*slot).is_null() {
            *slot = __torajs_dynobj_alloc();
        }
        for nidx in moved {
            let key = crate::define::mint_index_key(nidx as u64);
            __torajs_dynobj_set_entry_hole(slot, key as *mut c_void);
            __torajs_str_drop(key as *mut c_void);
        }
    }
}

/// §10.4.2 [[Delete]] on a canonical index — `delete arr[i]`.
/// Answers 1 (deleted / already absent) or 0 (refused: the index is
/// non-configurable — caller throws per §13.5.1.2 strict semantics).
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer; `key` a live Str whose
/// bytes already parsed as canonical index `idx`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_delete_index(
    arr: *mut c_void,
    key: *mut c_void,
    idx: u64,
) -> i32 {
    let len = unsafe { (arr.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read() };
    if idx >= len {
        return 1;
    }
    let cur_flags = unsafe { index_flags_with_key(arr, key as *const c_void) };
    if cur_flags & F_HOLE != 0 {
        return 1;
    }
    if cur_flags & F_CONFIGURABLE == 0 {
        return 0;
    }
    // Drop the element's value (kind-aware store of undefined keeps
    // the dense model; reads through the hole answer undefined
    // regardless).
    unsafe { crate::index_any::__torajs_arr_index_set(arr, idx as i64, ANY_UNDEF, 0) };
    let slot = unsafe { props_slot(arr) };
    if unsafe { (*slot).is_null() } {
        unsafe { *slot = __torajs_dynobj_alloc() };
    }
    unsafe { __torajs_dynobj_set_entry_hole(slot, key) };
    let p = unsafe { (arr as *mut u8).add(6) as *mut u16 };
    unsafe { p.write(p.read() | FLAG_ARR_EXOTIC_INDEX) };
    1
}

/// §10.4.2.4 steps 15-19 shadow sweep (RFC 20260721 刀 13d) — a
/// length shrink walks DOWN from `old_len-1` deleting each truncated
/// index's shadow entry (accessor pair / hole sentinel / attribute
/// shadow) alongside the element storage the caller drops; without
/// this the stale accessor shadow survives the shrink and a later
/// grow-back answers `i in arr` true for an index the spec deleted
/// (test262 reverse/get_if_present_with_delete). A live
/// NON-CONFIGURABLE entry stops the walk and the shrink lands at
/// `stop + 1` (the silent assignment-path semantics; the
/// defineProperty path pre-validates and throws before reaching
/// here). Returns the landed length.
///
/// # Safety
/// `arr` is a live `Tag::Arr` heap pointer with a shrink in
/// progress (`new_len < old_len`).
pub(crate) unsafe fn shrink_purge_shadows(arr: *mut c_void, new_len: u64, old_len: u64) -> u64 {
    let slot = unsafe { props_slot(arr) };
    if unsafe { (*slot).is_null() } {
        return new_len;
    }
    let props = unsafe { *slot };
    // Shadow entries only ever exist below the materialized extent —
    // an implicit sparse tail (RFC 20260810-arr-sparse-grow) never
    // minted any, so the sweep skips it instead of probing 0..4e9
    // keys one by one.
    let mut i = core::cmp::min(old_len, unsafe {
        crate::layout::arr_live_extent(arr as *const u8)
    });
    while i > new_len {
        i -= 1;
        let key = unsafe { crate::define::mint_index_key(i) };
        if unsafe { __torajs_dynobj_has(props, key as *const c_void) } != 0 {
            let f = unsafe { index_flags_with_key(arr, key as *const c_void) };
            if f & F_HOLE == 0 && f & F_CONFIGURABLE == 0 {
                unsafe { __torajs_str_drop(key as *mut c_void) };
                return i + 1;
            }
            unsafe { __torajs_dynobj_delete(props, key as *const c_void) };
        }
        unsafe { __torajs_str_drop(key as *mut c_void) };
    }
    new_len
}

unsafe extern "C" {
    /// torajs-anyvalue — §7.3.11 HasProperty walk (live index + hole
    /// shadows + expando + prototype chain).
    fn __torajs_arr_forin_key_live(arr: *mut c_void, key: *const c_void) -> i64;
}

/// §7.3.11 HasProperty for a numeric key on an Array receiver — the
/// static `in` lane's kernel (RFC 20260721 刀 13d). Pre-fix the lane
/// was a raw `0 <= key < len` bounds check, blind to hole shadows
/// (`delete` / length-grow / elision) and to prototype digit keys —
/// `i in arr` answered true for a hole and false for an
/// Array.prototype-supplied index. A clean in-bounds index answers 1
/// with one flag test; everything else continues through the full
/// walk.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_has_index(arr: *mut c_void, idx: i64) -> i64 {
    unsafe {
        if arr.is_null() || idx < 0 {
            return 0;
        }
        let len = (arr.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read();
        let hflags = crate::define::header_flags(arr);
        let exotic = hflags & FLAG_ARR_EXOTIC_INDEX != 0;
        // Sparse tail (RFC 20260810) — implicit holes are absent as
        // own properties: fall through to the walk (shadow probe
        // finds nothing, the chain consults prototype digit keys).
        let in_tail = hflags & torajs_rc::FLAG_ARR_SPARSE_TAIL != 0
            && (idx as u64) >= crate::layout::arr_live_extent(arr as *const u8);
        if (idx as u64) < len && !exotic && !in_tail {
            return 1;
        }
        // Exotic (hole shadows possible) or OOB (prototype digit
        // keys) — mint the canonical key and walk.
        let key = crate::define::mint_index_key(idx as u64);
        let live = __torajs_arr_forin_key_live(arr, key as *const c_void);
        __torajs_str_drop(key as *mut c_void);
        live
    }
}
