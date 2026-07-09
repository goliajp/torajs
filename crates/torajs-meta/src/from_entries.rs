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
//! Map / Set sources (chunk 694) ride the caller-managed-cursor
//! `__torajs_map_iter_next` walk over the shared hash storage
//! (insertion order, tombstone-skipping — the same walk `forEach`
//! takes). A Map entry is a `(k, v)` pair by construction; a Set
//! element is the entry itself and must be a pair array (ES
//! §20.1.2.7 iterates the Set's values — primitives throw).
//!
//! Dynobj array-like entries (chunk 739) — ES §20.1.2.7
//! AddEntriesFromIterable requires only `Type(entry) is Object` and
//! reads `Get(entry, "0")` / `Get(entry, "1")`, so `{0: "a", 1: 1}`
//! is a legal entry. An accessor at "0"/"1" runs its getter
//! (chunk 746 — ES [[Get]] semantics, the §7.3.24 precedent from
//! `obj_own_keys::entry_value_pair`); the OWNED answer transfers to
//! the built slot (value side) or drops after ToPropertyKey (key
//! side) instead of taking the borrowed-read inc.
//!
//! Two passes: validate first (undefined / null / non-iterable
//! receivers and non-object entries throw a catchable TypeError
//! BEFORE anything allocates), then construct (no throw path — no
//! half-built dynobj leaks under the pending-throw model; the "0" /
//! "1" key str temps mint before pass 1 and drop on every return
//! path — a key temp is not half-built state).

use core::ffi::{c_char, c_void};

use crate::reflect::{
    TAG_OBJ, VALUE_NULL_IMM, VALUE_UNDEFINED_IMM, alloc_str_key, heap_type_tag, is_cell_imm,
};

const TAG_ARR: u16 = 2;
const TAG_DYNOBJ: u16 = 14;
const TAG_MAP: u16 = 15;
const TAG_SET: u16 = 19;
const ANY_HEAP: u64 = 4;
/// `dynobj_get_tag` accessor sentinel (mirrors
/// `torajs_dynobj::layout::ANY_ACCESSOR`).
const ANY_ACCESSOR: u64 = 6;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const c_char);
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_set(dst: *mut *mut c_void, key: *const u8, tag: u64, value: u64);
    fn __torajs_accessor_invoke_getter(pair: *const c_void) -> u64;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_arr_get_any_boxed(arr: *const c_void, i: u64) -> u64;
    fn __torajs_arr_get_any_tag(arr: *const c_void, i: u64) -> u64;
    fn __torajs_arr_get_any_value(arr: *const c_void, i: u64) -> u64;
    fn __torajs_anyv_to_str(v: u64) -> *mut c_void;
    fn __torajs_anyv_to_str_pair(tag: i64, value: i64) -> *mut c_void;
    fn __torajs_dynobj_get_tag(dynobj: *const c_void, key: *const u8) -> u64;
    fn __torajs_dynobj_get_value(dynobj: *const c_void, key: *const u8) -> u64;
    fn __torajs_any_member_get_tag(recv: u64, key: *const c_void) -> u64;
    fn __torajs_any_member_get_value(recv: u64, key: *const c_void) -> u64;
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

/// The entry's live DynObj cell pointer, or `None` (mirror of
/// [`arr_cell`]).
unsafe fn dynobj_cell(v: u64) -> Option<*const c_void> {
    if !is_cell_imm(v) {
        return None;
    }
    let p = v as *const c_void;
    if unsafe { heap_type_tag(p) } == TAG_DYNOBJ {
        Some(p)
    } else {
        None
    }
}

/// The entry's live anon/named struct cell pointer, or `None`
/// (chunk 744 — an inline `{0:"a",1:9} as any` literal boxes as a
/// `Tag::Obj` anon-struct, a legal ES entry; field reads take the
/// `__torajs_any_member_get_*` class-layout reflection probe).
unsafe fn struct_cell(v: u64) -> Option<*const c_void> {
    if !is_cell_imm(v) {
        return None;
    }
    let p = v as *const c_void;
    if unsafe { heap_type_tag(p) } == TAG_OBJ {
        Some(p)
    } else {
        None
    }
}

/// ES `Get(entry, k)` on a dynobj entry — a data slot answers a
/// borrowed `(tag, value)` pair; an accessor slot runs its getter
/// (chunk 746, ES §7.3.24 [[Get]]) and answers the OWNED result,
/// flagged so the caller transfers the share (value side) or drops
/// it after ToPropertyKey (key side) instead of inc'ing a borrow.
unsafe fn dynobj_entry_get(entry: *const c_void, k: *const u8) -> (u64, u64, bool) {
    let t = unsafe { __torajs_dynobj_get_tag(entry, k) };
    if t == ANY_ACCESSOR {
        let p = unsafe { __torajs_dynobj_get_value(entry, k) } as *const c_void;
        let g = unsafe { __torajs_accessor_invoke_getter(p) };
        return (
            unsafe { __torajs_anyv_unbox_tag(g) } as u64,
            unsafe { __torajs_anyv_unbox_value(g) } as u64,
            true,
        );
    }
    (t, unsafe { __torajs_dynobj_get_value(entry, k) }, false)
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
        // Map / Set receivers take the hash-storage walk instead.
        if is_cell_imm(entries) {
            let p = entries as *const c_void;
            let tag = unsafe { heap_type_tag(p) };
            if tag == TAG_MAP {
                return unsafe { from_map(p) };
            }
            if tag == TAG_SET {
                return unsafe { from_set(p) };
            }
        }
        // SAFETY: NUL-terminated static C string.
        unsafe {
            __torajs_throw_type_error(c"Object.fromEntries argument is not iterable".as_ptr());
        }
        return VALUE_UNDEFINED_IMM;
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
        if unsafe { arr_cell(entry) }.is_some()
            || unsafe { dynobj_cell(entry) }.is_some()
            || unsafe { struct_cell(entry) }.is_some()
        {
            continue;
        }
        unsafe { drop_pair_keys(k0, k1) };
        // SAFETY: NUL-terminated static C string.
        unsafe {
            __torajs_throw_type_error(c"Object.fromEntries entry is not an object".as_ptr());
        }
        return VALUE_UNDEFINED_IMM;
    }
    // Pass 2 — construct. No throw path from here on.
    let mut obj = unsafe { __torajs_dynobj_alloc() };
    for i in 0..len {
        let entry = unsafe { __torajs_arr_get_any_boxed(outer, i) };
        if let Some(inner) = unsafe { arr_cell(entry) } {
            unsafe { set_prop_from_pair_arr(&mut obj, inner) };
        } else if unsafe { dynobj_cell(entry) }.is_some() {
            unsafe { set_prop_from_dynobj_entry(&mut obj, entry as *const c_void, k0, k1) };
        } else {
            // Pass 1 proved the only remaining shape is a struct cell.
            unsafe { set_prop_from_struct_entry(&mut obj, entry, k0, k1) };
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

/// Define one property from a dynobj array-like entry
/// (`{0: k, 1: v}`) — ES §20.1.2.7 AddEntriesFromIterable reads
/// `Get(entry, "0")` / `Get(entry, "1")`; an absent slot answers
/// undefined (the key side then stringifies to "undefined" via
/// ToPropertyKey). Data slots read borrowed (the entry's bucket
/// keeps its share, `set_prop` incs a fresh one); accessor slots
/// answer OWNED (chunk 746) — the value side transfers the share
/// to the built slot, the key side drops it after ToPropertyKey.
///
/// # Safety
///
/// `entry` is a live DynObj cell; `k0` / `k1` are live Str cells.
unsafe fn set_prop_from_dynobj_entry(
    obj: &mut *mut c_void,
    entry: *const c_void,
    k0: *const u8,
    k1: *const u8,
) {
    let (k_tag, k_val, k_owned) = unsafe { dynobj_entry_get(entry, k0) };
    // ToPropertyKey → string (owned temp this walk drops).
    let k_str = unsafe { __torajs_anyv_to_str_pair(k_tag as i64, k_val as i64) };
    if k_owned && k_tag == ANY_HEAP && k_val != 0 {
        // The getter's key answer served ToPropertyKey; release it.
        unsafe { __torajs_value_drop_heap(k_val as *mut c_void) };
    }
    let (v_tag, v_val, v_owned) = unsafe { dynobj_entry_get(entry, k1) };
    unsafe { set_prop_with(obj, k_str, v_tag, v_val, v_owned) };
}

/// Define one property from a `Tag::Obj` struct entry — same ES
/// `Get(entry, "0")` / `Get(entry, "1")` reads as the dynobj arm,
/// through the `__torajs_any_member_get_*` class-layout reflection
/// probe (borrowed pair; an absent field answers `(ANY_UNDEF, 0)`,
/// so the key side stringifies to "undefined" via ToPropertyKey).
///
/// # Safety
///
/// `entry` is a live cell-encoded `Tag::Obj` AnyValue; `k0` / `k1`
/// are live Str cells.
unsafe fn set_prop_from_struct_entry(
    obj: &mut *mut c_void,
    entry: u64,
    k0: *const u8,
    k1: *const u8,
) {
    let k_tag = unsafe { __torajs_any_member_get_tag(entry, k0 as *const c_void) };
    let k_val = unsafe { __torajs_any_member_get_value(entry, k0 as *const c_void) };
    // ToPropertyKey → string (owned temp this walk drops).
    let k_str = unsafe { __torajs_anyv_to_str_pair(k_tag as i64, k_val as i64) };
    let v_tag = unsafe { __torajs_any_member_get_tag(entry, k1 as *const c_void) };
    let v_val = unsafe { __torajs_any_member_get_value(entry, k1 as *const c_void) };
    unsafe { set_prop(obj, k_str, v_tag, v_val) };
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
    if !v_owned && v_tag == ANY_HEAP && v_val != 0 {
        // SAFETY: ANY_HEAP slot holds a valid heap pointer.
        unsafe { __torajs_rc_inc(v_val as *mut c_void) };
    }
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

/// Set receiver — the iterated value is the element itself, which
/// must be an object entry (`new Set([["a", 1]])` is a legal entries
/// iterable; a primitive element throws per ES §20.1.2.7). Same
/// two-pass model as the array lane: validate before anything
/// allocates; accessor slots run their getter at the construct walk.
///
/// # Safety
///
/// `set` is a live Set cell (shares the Map storage layout).
unsafe fn from_set(set: *const c_void) -> u64 {
    let mut cursor: i64 = -1;
    let (mut kt, mut kp, mut vt, mut vp) = (0i64, 0i64, 0i64, 0i64);
    // "0" / "1" key temps for the dynobj-entry probe; dropped on
    // every return path.
    let k0 = unsafe { alloc_str_key(b"0") };
    let k1 = unsafe { alloc_str_key(b"1") };
    // Pass 1 — every element must be an Arr cell or a data-prop
    // dynobj cell.
    while unsafe { __torajs_map_iter_next(set, &mut cursor, &mut kt, &mut kp, &mut vt, &mut vp) }
        == 1
    {
        let tag = if kt == ANY_HEAP as i64 && kp != 0 {
            unsafe { heap_type_tag(kp as *const c_void) }
        } else {
            u16::MAX
        };
        if tag == TAG_ARR || tag == TAG_DYNOBJ || tag == TAG_OBJ {
            // Accessor slots pass — their getter runs at the
            // construct walk (chunk 746).
            continue;
        }
        unsafe { drop_pair_keys(k0, k1) };
        // SAFETY: NUL-terminated static C string.
        unsafe {
            __torajs_throw_type_error(c"Object.fromEntries entry is not an object".as_ptr());
        }
        return VALUE_UNDEFINED_IMM;
    }
    // Pass 2 — construct. No throw path from here on.
    let mut obj = unsafe { __torajs_dynobj_alloc() };
    cursor = -1;
    while unsafe { __torajs_map_iter_next(set, &mut cursor, &mut kt, &mut kp, &mut vt, &mut vp) }
        == 1
    {
        // Pass 1 proved every element is an Arr / dynobj / struct cell.
        let etag = unsafe { heap_type_tag(kp as *const c_void) };
        if etag == TAG_ARR {
            unsafe { set_prop_from_pair_arr(&mut obj, kp as *const c_void) };
        } else if etag == TAG_DYNOBJ {
            unsafe { set_prop_from_dynobj_entry(&mut obj, kp as *const c_void, k0, k1) };
        } else {
            unsafe { set_prop_from_struct_entry(&mut obj, kp as u64, k0, k1) };
        }
    }
    unsafe { drop_pair_keys(k0, k1) };
    obj as u64
}
