//! `Map.groupBy(items, callbackFn)` per ES §24.2.2.4.
//!
//! Sister to [`crate::object_group_by`] — same Array items walk +
//! any-call cb dispatch, but the accumulator is a Map (SameValueZero
//! key equality; keys retain their runtime type instead of coercing
//! to a property-key string). Every returned key is used as-is; a
//! same-key hit (by SameValueZero) pushes into the shared bucket,
//! a fresh key mints a new Array<Any>.
//!
//! Iterable-only receivers (non-Array with @@iterator) are L3b —
//! spec step 3 GroupBy(items, cb, ~collection~) requires the full
//! IteratorRecord walk.
//!
//! Ownership contract mirrors the sibling object_group_by:
//! - `arr_get_any_boxed` slots are BORROWED; `unbox_value_owned`
//!   mints a fresh stake before `arr_push_any` transfer.
//! - `any_call` argv is BORROWED; return key is OWNED — the walk
//!   drops it after storing it in the map (map_set consumes; on the
//!   already-present arm the key just drops).
//! - Bucket cell pointers stored in Map entries stay valid across
//!   `arr_push_any` grows (data buffer may realloc but the cell
//!   itself never moves).

use core::ffi::{c_char, c_void};

use crate::reflect::{VALUE_NULL_IMM, VALUE_UNDEFINED_IMM, heap_type_tag, is_cell_imm};

const ANY_HEAP: i64 = 4;
const ANY_I64: i64 = 2;
const TAG_ARR: u16 = 2;
const ARR_LEN_OFF: usize = 8;

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const c_char);
    fn __torajs_throw_check() -> i64;
    fn __torajs_map_create() -> *mut c_void;
    fn __torajs_map_get(
        p: *const c_void,
        key_tag: i64,
        key_payload: i64,
        out_tag: *mut i64,
        out_payload: *mut i64,
    );
    fn __torajs_map_set(
        p: *mut c_void,
        key_tag: i64,
        key_payload: i64,
        value_tag: i64,
        value_payload: i64,
    );
    fn __torajs_arr_get_any_boxed(arr: *const c_void, i: u64) -> u64;
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    fn __torajs_anyv_unbox_value_owned(v: u64) -> i64;
    fn __torajs_any_call(recv: u64, argv: *const u64, argc: i64) -> u64;
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_rc_inc(p: *mut c_void);
}

/// Route raw AnyValue bits to a live Array cell, or `None` (nullish
/// / non-cell / other-tag). Same predicate as
/// [`crate::object_group_by::arr_cell`].
unsafe fn arr_cell(v: u64) -> Option<*const c_void> {
    if v == VALUE_UNDEFINED_IMM || v == VALUE_NULL_IMM {
        return None;
    }
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

/// Release an OWNED AnyValue payload (heap cell → rc_dec; inline
/// tags → no-op). Mirrors the release path the ssa-lower emits
/// after an any-call; same shape as
/// [`crate::object_group_by::drop_owned_any`].
unsafe fn drop_owned_any(v: u64) {
    let t = unsafe { __torajs_anyv_unbox_tag(v) };
    if t == ANY_HEAP {
        let p = unsafe { __torajs_anyv_unbox_value(v) };
        if p != 0 {
            unsafe { __torajs_value_drop_heap(p as *mut c_void) };
        }
    }
}

/// `Map.groupBy(items, cb)` — Array items lane.
///
/// Returns a freshly minted Map (refcount = 1, transferred to
/// caller). On any throw path the accumulator + pending key
/// release, and the return is `VALUE_UNDEFINED_IMM` — the caller's
/// throw check unwinds before the value is consumed.
///
/// # Safety
/// `items` and `cb` carry valid AnyValue bit patterns; caller must
/// check for a pending throw after return.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_map_group_by(items: u64, cb: u64) -> u64 {
    let Some(arr) = (unsafe { arr_cell(items) }) else {
        unsafe {
            __torajs_throw_type_error(c"Map.groupBy requires an iterable argument".as_ptr());
        }
        return VALUE_UNDEFINED_IMM;
    };
    // SAFETY: live Arr cell — length lives at ARR_LEN_OFF per layout.
    let len = unsafe { (arr.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read() };
    let result = unsafe { __torajs_map_create() };
    for i in 0..len {
        // Borrowed item — arr keeps its own share.
        let item = unsafe { __torajs_arr_get_any_boxed(arr, i) };
        let idx = unsafe { __torajs_anyv_box_from_pair(ANY_I64, i as i64) };
        let argv = [item, idx];
        // cb(item, i) — argv is borrowed; key is OWNED.
        let key = unsafe { __torajs_any_call(cb, argv.as_ptr(), 2) };
        if unsafe { __torajs_throw_check() } != 0 {
            unsafe { drop_owned_any(key) };
            unsafe { __torajs_value_drop_heap(result) };
            return VALUE_UNDEFINED_IMM;
        }
        // SameValueZero lookup — key keeps its runtime type (unlike
        // Object.groupBy's ToPropertyKey coercion).
        let key_tag = unsafe { __torajs_anyv_unbox_tag(key) };
        let key_val = unsafe { __torajs_anyv_unbox_value(key) };
        // map_get borrows the key then RELEASES it per its contract
        // ("callers pass keys with an rc_inc already applied") — hand
        // it its own stake. Without this a RUNTIME heap key (the
        // fixtures' static-literal keys are immortal and masked it)
        // was freed right here: a ShortStr materialization at rc=1
        // died inside map_get and the fresh-insert arm adopted the
        // freed pointer.
        if key_tag == ANY_HEAP && key_val != 0 {
            unsafe { __torajs_rc_inc(key_val as *mut c_void) };
        }
        let mut existing_tag: i64 = 0;
        let mut existing_val: i64 = 0;
        unsafe {
            __torajs_map_get(
                result,
                key_tag,
                key_val,
                &mut existing_tag,
                &mut existing_val,
            );
        }
        // Push item — unbox_value_owned mints the +1 the bucket
        // slot keeps.
        let t = unsafe { __torajs_anyv_unbox_tag(item) };
        let val = unsafe { __torajs_anyv_unbox_value_owned(item) };
        if existing_tag == ANY_HEAP && existing_val != 0 {
            // Bucket exists — push in place (cell doesn't move).
            // map_get hands back an OWNED ref on the value ("caller
            // becomes the new owner of the returned reference") — the
            // map keeps its own share, so release ours after the
            // push. Unreleased, every same-key hit stranded one
            // bucket ref and the whole bucket (elements included)
            // outlived the map: 150k-round numeric churn leaked one
            // bucket per round, ~21MB per key against 1.7MB flat.
            let bucket = existing_val as *mut c_void;
            let _ = unsafe { __torajs_arr_push_any(bucket, t as u64, val as u64) };
            unsafe { __torajs_value_drop_heap(bucket) };
            // Key is redundant — the map already owns its share via
            // the earlier insert. Release the query pair we hold: for
            // a ShortStr key, `key_val` IS the materialization the
            // unbox above minted — `drop_owned_any(key)` would mint
            // and free a FRESH one and leak this one.
            if key_tag == ANY_HEAP && key_val != 0 {
                unsafe { __torajs_value_drop_heap(key_val as *mut c_void) };
            }
        } else {
            // Fresh key — mint a bucket and insert. map_set consumes
            // the key/value pair (owned); the key we hold IS owned,
            // and the bucket pointer is fresh rc=1 so both transfer.
            let fresh = unsafe { __torajs_arr_alloc_any(4) } as *mut c_void;
            let _ = unsafe { __torajs_arr_push_any(fresh, t as u64, val as u64) };
            // Consume the owned key by unbox_value (already own the
            // share; map_set takes it verbatim, no rc traffic).
            unsafe {
                __torajs_map_set(result, key_tag, key_val, ANY_HEAP, fresh as i64);
            }
        }
    }
    result as u64
}
