//! `Object.fromEntries(entries)` with a runtime entries array
//! (chunk 693, T-09) — the dynamic complement to the compile-time
//! folds (static pairs literal → ObjectLit, chunk 690; struct-
//! annotated let fast-path, T-09.c). Walks the pairs array and
//! builds a dynobj per ES §20.1.2.7 (insertion order, last
//! duplicate key wins).
//!
//! Slot reads ride `__torajs_arr_get_any_boxed` — kind-aware (a
//! typed 8-byte block reboxes per the header's elem kind, an
//! FLAG_ARR_ANY block reads the NaN-box slot directly) and
//! OOB-safe (`[["a"]]` answers `{ a: undefined }`), so every
//! entries shape (`Arr<Arr<Any>>`, `Arr<Any>`, `Arr<Arr<Str>>`,
//! …) takes the same walk.
//!
//! A Map source (chunk 694) rides the caller-managed-cursor
//! `__torajs_map_iter_next` walk over the hash storage (insertion
//! order, tombstone-skipping — the same walk `forEach` takes); a Map
//! entry is a `(k, v)` pair by construction.
//!
//! Object entries (chunks 739 / 744) — ES §20.1.2.7
//! AddEntriesFromIterable requires only `Type(entry) is Object` and
//! reads `Get(entry, "0")` / `Get(entry, "1")`, so `{0: "a", 1: 1}`
//! is a legal entry, read through the member dispatcher whether it is
//! a dynobj or a struct. An accessor at "0"/"1" runs its getter
//! (chunk 746 — ES [[Get]] semantics, the §7.3.24 precedent from
//! `obj_own_keys::entry_value_pair`); the OWNED answer transfers to
//! the built slot (value side) or drops after ToPropertyKey (key
//! side) instead of taking the borrowed-read inc.
//!
//! Any other iterable (a Set, a user iterator object, a generator, a
//! helper) walks the iteration protocol instead (`iterable`).
//!
//! The array lane takes two passes: validate first (non-object
//! entries throw a catchable TypeError BEFORE anything allocates),
//! then construct; an entry's getter is the one thing that can throw
//! there, and the half-built dynobj is released when it does. The
//! "0" / "1" key str temps mint before pass 1 and drop on every
//! return path.

use core::ffi::{c_char, c_void};

mod iterable;

use crate::reflect::{
    TAG_OBJ, VALUE_NULL_IMM, VALUE_UNDEFINED_IMM, alloc_str_key, heap_type_tag, is_cell_imm,
};

const TAG_ARR: u16 = 2;
const TAG_DYNOBJ: u16 = 14;
const TAG_MAP: u16 = 15;
const ANY_HEAP: u64 = 4;
/// `dynobj_get_tag` accessor sentinel (mirrors
/// `torajs_dynobj::layout::ANY_ACCESSOR`).
const ANY_ACCESSOR: u64 = 6;

unsafe extern "C" {
    /// torajs-anyvalue — a borrowed payload made owned (an inline split
    /// view is copied out rather than shared).
    fn __torajs_anyv_unbox_value_owned(v: u64) -> i64;
    fn __torajs_throw_type_error(msg: *const c_char);
    fn __torajs_throw_check() -> i64;
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_set(dst: *mut *mut c_void, key: *const u8, tag: u64, value: u64);
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_arr_get_any_boxed(arr: *const c_void, i: u64) -> u64;
    fn __torajs_arr_get_any_tag(arr: *const c_void, i: u64) -> u64;
    fn __torajs_arr_get_any_value(arr: *const c_void, i: u64) -> u64;
    fn __torajs_anyv_to_str(v: u64) -> *mut c_void;
    fn __torajs_anyv_to_str_pair(tag: i64, value: i64) -> *mut c_void;
    fn __torajs_any_member_get_tag(recv: u64, key: *const c_void) -> u64;
    fn __torajs_any_member_get_value(recv: u64, key: *const c_void) -> u64;
    fn __torajs_any_accessor_get(recv: u64, key: *const c_void, pair_bits: u64) -> u64;
    fn __torajs_str_drop(s: *mut u8);
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_map_iter_next(
        p: *const c_void,
        cursor: *mut i64,
        out_k_tag: *mut i64,
        out_k_payload: *mut i64,
        out_v_tag: *mut i64,
        out_v_payload: *mut i64,
    ) -> i64;
}

const ARR_LEN_OFF: usize = 8;

/// The receiver's live Arr cell pointer, or `None` for anything
/// that isn't one (immediates, other heap tags).
unsafe fn arr_cell(v: u64) -> Option<*const c_void> {
    if !is_cell_imm(v) {
        return None;
    }
    let p = v as *const c_void;
    if unsafe { heap_type_tag(p) } == TAG_ARR {
        Some(p)
    } else {
        None
    }
}

/// Is the entry a dynobj or an anon/named struct cell? Either is an
/// object whose "0" / "1" the member dispatcher reads (chunk 744 — an
/// inline `{0:"a",1:9} as any` literal boxes as a `Tag::Obj`
/// anon-struct, a legal ES entry).
unsafe fn object_entry(v: u64) -> bool {
    if !is_cell_imm(v) {
        return false;
    }
    let tag = unsafe { heap_type_tag(v as *const c_void) };
    tag == TAG_DYNOBJ || tag == TAG_OBJ
}

/// Release the "0" / "1" key str temps (every return path of a
/// dynobj-capable lane).
unsafe fn drop_pair_keys(k0: *mut u8, k1: *mut u8) {
    unsafe { __torajs_str_drop(k0) };
    unsafe { __torajs_str_drop(k1) };
}

/// `Object.fromEntries(entries)` — builds a fresh dynobj from a
/// runtime pairs array, returned as a cell-encoded AnyValue (+1 rc
/// from `dynobj_alloc`, transferred to the caller).
///
/// # Safety
///
/// `entries` carries a valid AnyValue bit pattern (a raw Arr
/// pointer from a statically typed receiver satisfies the cell
/// encoding).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_from_entries(entries: u64) -> u64 {
    if entries == VALUE_UNDEFINED_IMM || entries == VALUE_NULL_IMM {
        // SAFETY: NUL-terminated static C string.
        unsafe {
            __torajs_throw_type_error(c"Object.fromEntries requires an iterable argument".as_ptr());
        }
        return VALUE_UNDEFINED_IMM;
    }
    let Some(outer) = (unsafe { arr_cell(entries) }) else {
        // a Map receiver takes the hash-storage walk instead
        if is_cell_imm(entries) && unsafe { heap_type_tag(entries as *const c_void) } == TAG_MAP {
            return unsafe { from_map(entries as *const c_void) };
        }
        // any other iterable (a Set included) walks the iteration
        // protocol; a value that is not iterable throws from
        // GetIterator there
        let k0 = unsafe { alloc_str_key(b"0") };
        let k1 = unsafe { alloc_str_key(b"1") };
        let obj = unsafe { iterable::from_iterable(entries, k0, k1) };
        unsafe { drop_pair_keys(k0, k1) };
        return obj;
    };
    // SAFETY: live Arr cell; len lives at ARR_LEN_OFF per layout.
    let len = unsafe { (outer.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read() };
    // "0" / "1" key temps for the dynobj-entry Get probe; dropped on
    // every return path below.
    let k0 = unsafe { alloc_str_key(b"0") };
    let k1 = unsafe { alloc_str_key(b"1") };
    // Pass 1 — validate every entry is an object (array / dynobj /
    // struct cell) BEFORE any dynobj allocation, so the reject path
    // leaves nothing half-built. Accessor slots pass — their getter
    // runs at the construct walk (chunk 746).
    for i in 0..len {
        let entry = unsafe { __torajs_arr_get_any_boxed(outer, i) };
        if unsafe { arr_cell(entry) }.is_some() || unsafe { object_entry(entry) } {
            continue;
        }
        unsafe { drop_pair_keys(k0, k1) };
        // SAFETY: NUL-terminated static C string.
        unsafe {
            __torajs_throw_type_error(c"Object.fromEntries entry is not an object".as_ptr());
        }
        return VALUE_UNDEFINED_IMM;
    }
    // Pass 2 — construct; only an entry's getter can throw here.
    let mut obj = unsafe { __torajs_dynobj_alloc() };
    for i in 0..len {
        let entry = unsafe { __torajs_arr_get_any_boxed(outer, i) };
        if let Some(inner) = unsafe { arr_cell(entry) } {
            unsafe { set_prop_from_pair_arr(&mut obj, inner) };
        } else {
            // Pass 1 proved the only remaining shape is an object entry.
            unsafe { set_prop_from_object_entry(&mut obj, entry, k0, k1) };
        }
        // an entry's getter threw: the half-built object goes
        if unsafe { __torajs_throw_check() } != 0 {
            unsafe { drop_pair_keys(k0, k1) };
            unsafe { __torajs_value_drop_heap(obj) };
            return VALUE_UNDEFINED_IMM;
        }
    }
    unsafe { drop_pair_keys(k0, k1) };
    obj as u64
}

/// Define one property from a `[k, v]` pair array (borrowed reads;
/// OOB-safe so `[["a"]]` answers `{ a: undefined }`).
///
/// # Safety
///
/// `inner` is a live Arr cell.
unsafe fn set_prop_from_pair_arr(obj: &mut *mut c_void, inner: *const c_void) {
    let k_av = unsafe { __torajs_arr_get_any_boxed(inner, 0) };
    let v_tag = unsafe { __torajs_arr_get_any_tag(inner, 1) };
    let v_val = unsafe { __torajs_arr_get_any_value(inner, 1) };
    // ToPropertyKey → string (owned temp this walk drops).
    let k_str = unsafe { __torajs_anyv_to_str(k_av) };
    unsafe { set_prop(obj, k_str, v_tag, v_val) };
}

/// Define one property from a dynobj or struct entry
/// (`{0: k, 1: v}`) — ES §20.1.2.7 AddEntriesFromIterable reads
/// `Get(entry, "0")` / `Get(entry, "1")` through the member
/// dispatcher; an absent property answers undefined (the key side
/// then stringifies to "undefined" via ToPropertyKey).
///
/// # Safety
///
/// `entry` is a live cell-encoded dynobj or `Tag::Obj` AnyValue;
/// `k0` / `k1` are live Str cells.
unsafe fn set_prop_from_object_entry(
    obj: &mut *mut c_void,
    entry: u64,
    k0: *const u8,
    k1: *const u8,
) {
    let (k_tag, k_val, k_owned) = unsafe { entry_get(entry, k0) };
    // an abrupt Get(entry, "0") ends the entry before "1" is read
    if unsafe { __torajs_throw_check() } != 0 {
        return;
    }
    // ToPropertyKey → string (owned temp this walk drops).
    let k_str = unsafe { __torajs_anyv_to_str_pair(k_tag as i64, k_val as i64) };
    if k_owned && k_tag == ANY_HEAP && k_val != 0 {
        unsafe { __torajs_value_drop_heap(k_val as *mut c_void) };
    }
    let (v_tag, v_val, v_owned) = unsafe { entry_get(entry, k1) };
    unsafe { set_prop_with(obj, k_str, v_tag, v_val, v_owned) };
}

/// ES `Get(entry, k)`: a data property answers the borrowed probe
/// pair; an accessor (`{ get "0"() {…} }`, chunk 746) runs its getter
/// and answers the OWNED result, flagged so the caller transfers the
/// share (value side) or drops it after ToPropertyKey (key side)
/// instead of inc'ing a borrow.
unsafe fn entry_get(entry: u64, k: *const u8) -> (u64, u64, bool) {
    let key = k as *const c_void;
    let t = unsafe { __torajs_any_member_get_tag(entry, key) };
    let v = unsafe { __torajs_any_member_get_value(entry, key) };
    if t == ANY_ACCESSOR {
        let g = unsafe { __torajs_any_accessor_get(entry, key, v) };
        return (
            unsafe { __torajs_anyv_unbox_tag(g) } as u64,
            unsafe { __torajs_anyv_unbox_value(g) } as u64,
            true,
        );
    }
    (t, v, false)
}

/// Shared define tail — the dynobj slot owns its share of a heap
/// value (gOPD descriptor shape); the key string temp is dropped.
///
/// # Safety
///
/// `k_str` is a live owned Str cell; an ANY_HEAP `v_val` holds a
/// valid heap pointer.
unsafe fn set_prop(obj: &mut *mut c_void, k_str: *mut c_void, v_tag: u64, v_val: u64) {
    unsafe { set_prop_with(obj, k_str, v_tag, v_val, false) };
}

/// [`set_prop`] with an ownership flag — a borrowed value incs a
/// fresh share for the slot; an OWNED value (accessor getter answer,
/// chunk 746) transfers its share verbatim.
///
/// # Safety
///
/// As [`set_prop`]; an owned ANY_HEAP `v_val` carries a +1 the slot
/// takes over.
unsafe fn set_prop_with(
    obj: &mut *mut c_void,
    k_str: *mut c_void,
    v_tag: u64,
    v_val: u64,
    v_owned: bool,
) {
    let v_val = if !v_owned && v_tag == ANY_HEAP && v_val != 0 {
        // A split product's view is copied out rather than shared.
        unsafe { __torajs_anyv_unbox_value_owned(v_val) as u64 }
    } else {
        v_val
    };
    unsafe { __torajs_dynobj_set(obj, k_str as *const u8, v_tag, v_val) };
    unsafe { __torajs_str_drop(k_str as *mut u8) };
}

/// Map receiver — every entry is a `(k, v)` pair by construction,
/// so there is no validate pass (no throw path → nothing
/// half-built can leak). Insertion order rides the entries[]
/// prefix walk; keys take ToPropertyKey like the array lane.
///
/// # Safety
///
/// `map` is a live Map cell.
unsafe fn from_map(map: *const c_void) -> u64 {
    let mut obj = unsafe { __torajs_dynobj_alloc() };
    let mut cursor: i64 = -1;
    let (mut kt, mut kp, mut vt, mut vp) = (0i64, 0i64, 0i64, 0i64);
    while unsafe { __torajs_map_iter_next(map, &mut cursor, &mut kt, &mut kp, &mut vt, &mut vp) }
        == 1
    {
        let k_str = unsafe { __torajs_anyv_to_str_pair(kt, kp) };
        unsafe { set_prop(&mut obj, k_str, vt as u64, vp as u64) };
    }
    obj as u64
}
