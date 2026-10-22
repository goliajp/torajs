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
    fn __torajs_dynobj_has(dynobj: *const c_void, key: *const c_void) -> i32;
    /// torajs-dynobj — HOLE sentinel upsert / probe (chunk C).
    fn __torajs_dynobj_set_entry_hole(obj_slot: *mut *mut c_void, key: *mut c_void);
    fn __torajs_dynobj_entry_is_hole(dynobj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_dynobj_set_entry_flags(obj_slot: *mut *mut c_void, key: *mut c_void, flags: u64);
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
        // the bag is there and every caller gated on the exotic bit,
        // so neither needs its writer: the attach and the flag entry
        // stay out of a typed store's reach
        unsafe {
            __torajs_dynobj_set_entry_flags(props_slot(arr), key as *mut c_void, FLAGS_DEFAULT)
        };
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
        let slot = crate::exotic_seam::__torajs_arr_props_attach_index(arr);
        for i in from..to {
            let key = crate::define::mint_index_key(i);
            __torajs_dynobj_set_entry_hole(slot, key as *mut c_void);
            __torajs_str_drop(key as *mut c_void);
        }
    }
}

/// Carry the holes of `src[from .. from + n]` onto `dst[at .. at + n]`
/// — the copy kernels that keep holes (§23.1.3.28 slice, §23.1.3.1
/// concat) move slot words in bulk, and a typed slot's word does not say
/// absent; the shadow does. Callers screen on `needs_index_mop(src)`.
///
/// Recorded boundary: a hole a prototype digit key fills is present to
/// HasProperty, and the spec copies that key's value; this marks it a
/// hole (the same boundary as the typed callback-method copy).
pub(crate) unsafe fn copy_hole_marks(src: *const u8, from: u64, dst: *mut u8, at: u64, n: u64) {
    for k in 0..n {
        let hole = unsafe {
            crate::define_index_flags::__torajs_arr_index_flags(src as *const c_void, from + k)
        } & F_HOLE
            != 0;
        if hole {
            unsafe { mark_hole_range(dst as *mut c_void, at + k, at + k + 1) };
        }
    }
}

/// Overwrite with `fill` each slot of `dst[at .. at + n]` whose source
/// index is a hole — `src[from + k]`, or `src[from + n - 1 - k]` when
/// `reverse`. The copy kernels that read through [[Get]] (Array.from,
/// spread, toReversed, with, toSpliced, toSorted's clone) answer
/// `undefined` for a hole, but a heap slot's hole word is `0`, which
/// reads as `null`. `fill` is the target kind's `undefined` (the Str
/// sentinel cell); `0` keeps the copied word. Callers screen on
/// `needs_index_mop(src)`.
pub(crate) unsafe fn fill_hole_slots(
    src: *const u8,
    from: u64,
    dst: *mut u8,
    at: u64,
    n: u64,
    reverse: bool,
    fill: u64,
) {
    if fill == 0 {
        return;
    }
    let base = unsafe { crate::ops::data_ptr(dst) };
    for k in 0..n {
        let i = if reverse { from + n - 1 - k } else { from + k };
        let hole =
            unsafe { crate::define_index_flags::__torajs_arr_index_flags(src as *const c_void, i) }
                & F_HOLE
                != 0;
        if hole {
            unsafe { *(base.add(((at + k) as usize) * 8) as *mut u64) = fill };
        }
    }
}

/// Mark the LAST pushed slot a HOLE (§13.2.4 elision — the slot
/// reads undefined but is not an own property: `1 in [0,,2]` is
/// false, indexOf skips it). Every caller pushes a slot and marks it
/// immediately, so `len - 1` is the index and the fresh slot carries
/// default attributes, which is why the delete below cannot refuse.
/// Four of them: the array-literal elision lowering, and `map`'s
/// hole arm on the static lane, the Arr kernel and the array-like
/// walker (§23.1.3.21 step 6.c — the product keeps the source's
/// length, so a skipped index arrives as a hole instead of shortening
/// the result).
///
/// An Any cell's hole is the slot value `0` under `FLAG_ARR_ANY_HOLEY`
/// — the `new Array(n)` representation — instead of a shadow entry
/// keyed by a minted index string. The pushed slot holds the
/// `undefined` immediate, which owns nothing, so overwriting it
/// releases nothing. A typed cell (`map`'s typed product) keeps the
/// shadow: its caller pushed the element type's hole word (the F64
/// sentinel, a boolean 2, a NULL pointer), so only the mark is left.
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
    if unsafe { crate::define::header_flags(arr) } & torajs_rc::FLAG_ARR_ANY != 0 {
        unsafe {
            let p = arr.cast::<u8>();
            *(crate::layout::arr_data(p).add(idx as usize * 8) as *mut u64) = 0;
            crate::exotic_seam::__torajs_arr_flag_any_holey(arr);
        }
        return;
    }
    unsafe { mark_hole_range(arr, idx, idx + 1) };
}

/// RFC 20260721 刀 12 G16 — splice moved the tail elements but the
/// exotic-index HOLE shadows are keyed by index string, so a hole
/// must travel with its slot (ES §14.7.5: an unvisited deleted index
/// is never enumerated — sm/splice-suppresses-unvisited-indexes).
/// Each tail hole is re-marked at its shifted position (dropped when
/// it lands outside the new length). Non-hole attribute shadows stay
/// put (recorded boundary — the G7/G14 exotic-flag design owns full
/// attribute motion).
pub(crate) unsafe fn splice_remap_holes(
    arr: *mut c_void,
    start: i64,
    deleted: i64,
    inserted: i64,
    old_len: i64,
) {
    let delta = inserted - deleted;
    let new_len = old_len + delta;
    for idx in unsafe { take_holes(arr, start.max(0) as u64, old_len as u64) } {
        let (idx, nidx) = (idx as i64, idx as i64 + delta);
        if idx >= start + deleted && (0..new_len).contains(&nidx) {
            unsafe { mark_hole_range(arr, nidx as u64, nidx as u64 + 1) };
        }
    }
}

/// Carry the holes below `old_len` through an in-place move that put
/// the slot at `i` at `to(i)` (`None`: the move dropped it). The
/// kernels move slot words, and a typed slot's word does not say
/// absent — the shadow does (§23.1.3.26 reverse, .27 shift, .30
/// unshift, .7 fill: a hole moves as DeletePropertyOrThrow at the
/// slot's new index). Callers screen on `needs_index_mop`.
pub(crate) unsafe fn move_holes(
    arr: *mut c_void,
    old_len: u64,
    new_len: u64,
    to: impl Fn(u64) -> Option<u64>,
) {
    for h in unsafe { take_holes(arr, 0, old_len) } {
        if let Some(n) = to(h)
            && n < new_len
        {
            unsafe { mark_hole_range(arr, n, n + 1) };
        }
    }
}

/// The holes of `arr[from .. old_len)`, each shadow entry dropped so
/// the caller can re-mark it where its move put the slot. Dropping
/// (not reviving to default attributes) leaves no entry at an index
/// the move carried past the new length — a revived one printed as a
/// `"3": undefined` expando after `splice(1, 2)`. Shadow entries only
/// exist below the materialized extent, which a sparse tail bounds by
/// the capacity.
pub(crate) unsafe fn take_holes(arr: *mut c_void, from: u64, old_len: u64) -> Vec<u64> {
    let mut holes = Vec::new();
    let flags = unsafe { crate::define::header_flags(arr) };
    if flags & FLAG_ARR_EXOTIC_INDEX == 0 {
        return holes;
    }
    let mut bound = old_len;
    if flags & torajs_rc::FLAG_ARR_SPARSE_TAIL != 0 {
        let cap = unsafe { *(arr.cast::<u8>().add(crate::layout::ARR_CAP_OFF) as *const u32) };
        bound = bound.min(cap as u64);
    }
    let props = unsafe { *props_slot(arr) };
    for idx in from..bound {
        let key = unsafe { crate::define::mint_index_key(idx) };
        if unsafe { index_flags_with_key(arr, key as *const c_void, idx) } & F_HOLE != 0 {
            holes.push(idx);
            if !props.is_null() {
                unsafe { __torajs_dynobj_delete(props, key as *const c_void) };
            }
        }
        unsafe { __torajs_str_drop(key as *mut c_void) };
    }
    holes
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
    if unsafe { crate::arguments_index_bag::in_bag(arr, idx, len) } {
        // No such property: deleting it succeeds.
        if unsafe { crate::props::__torajs_arrprops_has(arr, key) } == 0 {
            return 1;
        }
        return unsafe { crate::props::__torajs_arrprops_delete(arr, key) };
    }
    if idx >= len {
        return 1;
    }
    let cur_flags = unsafe { index_flags_with_key(arr, key as *const c_void, idx) };
    if cur_flags & F_HOLE != 0 {
        return 1;
    }
    if cur_flags & F_CONFIGURABLE == 0 {
        return 0;
    }
    // Drop the element's value (kind-aware store of undefined keeps
    // the dense model; reads through the hole answer undefined
    // regardless).
    //
    // §10.4.2 [[Delete]] REMOVES a property; it never writes to one.
    // On an accessor index the ordinary store runs the setter, and a
    // getter-only pair answers the readonly TypeError — so
    // `Object.defineProperty(arr, "1", {get})` followed by
    // `delete arr[1]` threw instead of deleting. The shadow entry the
    // hole sentinel replaces below is what the delete actually
    // removes, so the store has nothing to do on that index.
    if unsafe { crate::define_accessor::__torajs_arr_index_accessor(arr, idx) }.is_null()
        && !unsafe { crate::index_any::clear_heap_slot(arr, idx) }
    {
        unsafe { crate::index_any::__torajs_arr_index_set(arr, idx as i64, ANY_UNDEF, 0) };
    }
    let slot = unsafe { crate::exotic_seam::__torajs_arr_props_attach_index(arr) };
    unsafe { __torajs_dynobj_set_entry_hole(slot, key) };
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
            let f = unsafe { index_flags_with_key(arr, key as *const c_void, i) };
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

/// §7.3.11 HasProperty for a numeric key on an Array receiver — the
/// static `in` lane's kernel (RFC 20260721 刀 13d) and the §23.1.3
/// present gate of the array callback methods. Own: an index below
/// `length` whose attribute flags carry no HOLE — the same answer the
/// any tier's own probe gives an `Arr` receiver (`any_prop_has`), and
/// `__torajs_arr_index_flags` already folds the sparse tail, the slot
/// hole and the shadow hole into that bit (and answers a plain array
/// with one flag test). Absent own: the chain ([`proto_index_present`]).
///
/// Both halves stay inside this crate on purpose. The own half used to
/// go through the any tier's generic HasProperty walk and the prototype
/// half through the minting prototype getter; either one linked the
/// whole builtin-prototype machinery into every program that asks
/// about an index (≈800 KB of a release `forEach` product).
///
/// # Safety
/// `arr` is null or a live Arr cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_has_index(arr: *mut c_void, idx: i64) -> i64 {
    unsafe {
        if arr.is_null() || idx < 0 {
            return 0;
        }
        let len = (arr.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read();
        if (idx as u64) < len
            && crate::define_index_flags::__torajs_arr_index_flags(arr, idx as u64) & F_HOLE == 0
        {
            return 1;
        }
        // §7.3.11 does not stop at own: a hole is absent from the
        // receiver, and HasProperty then asks the chain.
        proto_index_present(idx as u64) as i64
    }
}

/// The chain half of HasProperty for an index key on an Array: the
/// digit keys of %Array.prototype% (an `Arr` cell of its own —
/// `Array.prototype[0] = v`, or an accessor pair from
/// `Object.defineProperty`) and of %Object.prototype% above it. The one
/// home for this walk: `in` / the §23.1.3 present gate
/// ([`__torajs_arr_has_index`]) and the indexOf / lastIndexOf skip
/// (`method_any_search`) all ask it. Six test262 cases turn on the
/// first level (`[, 1].forEach(cb)` with a setter at
/// `Array.prototype[0]` calls the callback for index 0);
/// `arr-has-property-proto-chain-001` on the second.
///
/// Peek, never mint: a digit key can only be written onto a prototype
/// through a cell somebody already fetched, so an empty slot means
/// there is none to find. Minting here installed the whole builtin
/// prototype set on first use and linked that machinery into every
/// program that asks about an index. HasProperty never runs a getter,
/// so presence alone answers for accessor entries.
pub(crate) unsafe fn proto_index_present(idx: u64) -> bool {
    use torajs_rc::builtin_proto::{
        __torajs_peek_builtin_prototype, ARRAY_PROTO_TAG, OBJECT_PROTO_TAG,
    };
    unsafe {
        let ap = __torajs_peek_builtin_prototype(ARRAY_PROTO_TAG as i64);
        if !ap.is_null() {
            let len = (ap.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read();
            if idx < len
                && crate::define_index_flags::__torajs_arr_index_flags(ap, idx) & F_HOLE == 0
            {
                return true;
            }
        }
        let op = __torajs_peek_builtin_prototype(OBJECT_PROTO_TAG as i64);
        if op.is_null() {
            return false;
        }
        let key = crate::define::mint_index_key(idx);
        // A deleted proto index leaves a hole tombstone — raw presence
        // alone would resurrect it.
        let r = __torajs_dynobj_has(op, key as *const c_void) != 0
            && __torajs_dynobj_entry_is_hole(op, key as *const c_void) == 0;
        __torajs_str_drop(key as *mut c_void);
        r
    }
}
