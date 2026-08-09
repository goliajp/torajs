//! `Promise.allKeyed` / `Promise.allSettledKeyed` — the
//! await-dictionary proposal's keyed combinators over a dynamic
//! (any-boxed) OBJECT argument.
//!
//! Shape: a non-object argument REJECTS with TypeError (the proposal
//! validates before any iteration — unlike the iterable combinators,
//! whose GetIterator throw covers it); the object's own enumerable
//! string keys walk in §10.1.11.1 order; each value Gets through the
//! any lane; and the values array delegates to the matching sync
//! kernel (`all` / `allSettled` — fan-in, first-rejection-wins and
//! microtask order all come with it). A native finish job then zips
//! the keys with the result array into a NULL-PROTOTYPE object (the
//! proposal's result carries no prototype) and settles the outer
//! promise; a rejection forwards its reason unchanged.
//!
//! Recorded subset (lands with the ns-static/receiver faces, not
//! here): enumerable SYMBOL keys (the proposal includes them; the
//! own-keys walk is string-keyed) and a patched `Promise.resolve`
//! (§27.2.4.1.2 reads it dynamically; the sync kernels bake the
//! builtin resolve).

use core::ffi::c_void;

use crate::combinator_dyn::reject_with_pending_throw;
use crate::layout::{ARR_DATA_PTR_OFF, ARR_HEAD_OFF, REPR_ANY, STATE_REJECTED, as_promise};
use crate::pool::__torajs_promise_drop;
use crate::state::{
    __torajs_promise_attach_then, __torajs_promise_reject, __torajs_promise_resolve,
};

unsafe extern "C" {
    #[link_name = "__torajs_libc_malloc"]
    fn malloc(n: usize) -> *mut c_void;
    #[link_name = "__torajs_libc_free"]
    fn free(p: *mut c_void);
    #[link_name = "__torajs_memcpy"]
    fn memcpy(dst: *mut c_void, src: *const c_void, n: usize) -> *mut c_void;
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    fn __torajs_anyv_rc_dec(v: u64);
    /// torajs-meta — own enumerable string keys, §10.1.11.1 order
    /// (fresh typed arr of owned Str cells).
    fn __torajs_anyv_own_keys(v: u64, include_nonenum: i64) -> *mut c_void;
    /// torajs-anyvalue — Get(recv, key) through the any lane (key is
    /// a borrowed AnyValue; the answer is owned).
    fn __torajs_any_index_get_keyed(recv: u64, key: u64) -> u64;
    fn __torajs_arr_alloc_any_filled(n: u64) -> *mut u8;
    /// torajs-dynobj — the result object and its null-proto mark.
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_mark_null_proto(obj: *mut c_void);
    fn __torajs_dynobj_set(obj_slot: *mut *mut c_void, key: *mut c_void, tag: u64, value: u64);
}

/// The finish job's shared block — one per keyed-combinator call.
/// The job owns one stake on each field and frees the block after
/// settling.
struct KeyedBlock {
    /// The pending promise the caller holds.
    outer: *mut c_void,
    /// The delegated all/allSettled promise the job reads.
    source: *mut c_void,
    /// The Str-cell key array (typed arr), index-aligned with the
    /// source's result array.
    keys: *mut c_void,
    /// Non-zero for the allSettledKeyed flavor: the source's result
    /// slots are the anonymous settled records, translated into real
    /// `{status, value|reason}` dynobjs (the anonymous struct carries
    /// no layout the any lane can read — the recorded record-identity
    /// defect; the translation sidesteps it for the keyed face).
    settled: i64,
}

/// Typed-arr slot walk (the keys array): 8-byte slots at
/// `data + (head + i) * 8` — same layout the any-shape walk uses,
/// different slot meaning (raw Str ptr, not a NaN box).
unsafe fn arr_slot_raw(arr: *mut c_void, i: u64) -> i64 {
    unsafe {
        let head = *((arr as *mut u8).add(ARR_HEAD_OFF) as *const u32) as u64;
        let data = *((arr as *mut u8).add(ARR_DATA_PTR_OFF) as *const *mut u8);
        *(data.add(((head + i) * 8) as usize) as *const i64)
    }
}

unsafe fn arr_len_of(arr: *mut c_void) -> u64 {
    unsafe { *((arr as *mut u8).add(crate::layout::ARR_LEN_OFF) as *const u64) }
}

/// A ShortStr immediate's top 16 bits (SSO layout — the tag shim
/// reports it Heap, so it gates before the tag read).
const SHORT_STR_TOP16: u64 = 0x0001;
/// Heap tags that are PRIMITIVES, not objects (torajs-rc `Tag`):
/// Str = 0, Symbol = 7, BigInt = 10. Every other heap cell — dynobj,
/// struct, arr, closure, regexp, date, map/set, promise, wrappers —
/// is an object per §7.2 Type(x).
const TAG_STR: u16 = 0;
const TAG_SYMBOL: u16 = 7;
const TAG_BIGINT: u16 = 10;

/// `true` when the boxed value is an object per §7.2 Type(x).
/// Immediates (numbers / bools / null / undefined / ShortStr) and
/// the primitive-carrying heap cells are not.
unsafe fn is_object_arg(v: u64) -> bool {
    unsafe {
        if v >> 48 == SHORT_STR_TOP16 {
            return false;
        }
        if __torajs_anyv_unbox_tag(v) != 4 {
            return false;
        }
        let p = __torajs_anyv_unbox_value(v) as *const u8;
        if p.is_null() {
            return false;
        }
        let tag = (p.add(4) as *const u16).read();
        tag != TAG_STR && tag != TAG_SYMBOL && tag != TAG_BIGINT
    }
}

/// The shared drive: validate → keys → values → delegate → attach
/// the keyed finish. `sync` is the borrowed-input sync kernel
/// (`all_sync_untargeted` shape).
unsafe fn keyed_dyn(
    v: u64,
    sync: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    settled: i64,
) -> *mut c_void {
    unsafe {
        if !is_object_arg(v) {
            __torajs_throw_type_error(c"Promise.allKeyed requires an object argument".as_ptr());
            return reject_with_pending_throw();
        }
        let keys = __torajs_anyv_own_keys(v, 0);
        if __torajs_throw_check() != 0 {
            __torajs_value_drop_heap(keys);
            return reject_with_pending_throw();
        }
        let len = arr_len_of(keys);
        // Values collect — owned AnyValue bits transferred into the
        // prefilled any-shape array (the collect_iterable pattern).
        let values = __torajs_arr_alloc_any_filled(len);
        let head = *(values.add(ARR_HEAD_OFF) as *const u32) as u64;
        let data = *(values.add(ARR_DATA_PTR_OFF) as *const *mut u8);
        for i in 0..len {
            let key = arr_slot_raw(keys, i) as *mut c_void;
            // The key box takes its own stake (pair ABI transfers
            // one rc for tag 4); the keys array keeps the original.
            __torajs_rc_inc(key);
            let key_box = __torajs_anyv_box_from_pair(4, key as i64);
            let val = __torajs_any_index_get_keyed(v, key_box);
            __torajs_anyv_rc_dec(key_box);
            if __torajs_throw_check() != 0 {
                __torajs_value_drop_heap(values as *mut c_void);
                __torajs_value_drop_heap(keys);
                return reject_with_pending_throw();
            }
            *(data.add(((head + i) * 8) as usize) as *mut u64) = val;
        }
        let source = sync(values as *mut c_void);
        __torajs_value_drop_heap(values as *mut c_void);
        let outer = crate::pool::__torajs_promise_alloc_pending();
        // Pre-stamp at create (the fan-in's posture): the any-lane
        // `.then` bridge refuses an UNSTAMPED receiver at attach
        // time, and this outer always fulfills boxed (the null-proto
        // object rides REPR_ANY); a rejection overwrites with the
        // source's own form before settling.
        (*as_promise(outer)).value_repr = REPR_ANY;
        (*as_promise(outer)).value_is_heap = 1;
        let b = malloc(core::mem::size_of::<KeyedBlock>()) as *mut KeyedBlock;
        // The block takes its OWN stake on the outer and never
        // releases it — deliberately, in both halves. Which then-lane
        // holds a source stake varies by receiver shape (the typed
        // closure lane incs, others read through the binding alone),
        // so the finish cannot prove the cell outlives it without a
        // stake of its own; and releasing that stake IN the finish
        // recycled the cell under a later-dispatched handler (the
        // keyed-4 probe read the NEXT call's result object). The
        // retained stake matches the sibling combinators' recorded
        // result-promise leak posture (see the allsettled module's
        // "still leaking" note) at +1.
        __torajs_rc_inc(outer);
        (*b).outer = outer;
        (*b).source = source;
        (*b).keys = keys;
        (*b).settled = settled;
        __torajs_promise_attach_then(source, Some(keyed_finish), b as i64);
        outer
    }
}

/// The finish job: zip keys with the source's result array into a
/// null-proto object (fulfilled), or forward the rejection reason.
/// Runs as a microtask once the source settles.
unsafe extern "C" fn keyed_finish(arg: i64) {
    unsafe {
        let b = arg as *mut KeyedBlock;
        let sp = as_promise((*b).source);
        let outer = (*b).outer;
        let rp = as_promise(outer);
        if (*sp).state == STATE_REJECTED {
            // Forward the element's own settlement with a fresh
            // stake (the settle_from posture).
            (*rp).value_is_heap = (*sp).value_is_heap;
            (*rp).value_repr = (*sp).value_repr;
            let value = (*sp).value;
            if (*sp).value_is_heap != 0 && value != 0 {
                __torajs_rc_inc(value as *mut c_void);
            }
            __torajs_promise_reject(outer, value);
        } else {
            // Fulfilled — the sync kernels settle REPR_HEAP with the
            // raw result-array pointer, slots = AnyValue bits.
            let arr = (*sp).value as *mut c_void;
            let mut obj = __torajs_dynobj_alloc();
            __torajs_dynobj_mark_null_proto(obj);
            let n = arr_len_of(arr);
            let rec_keys = if (*b).settled != 0 {
                Some(RecordKeys::mint())
            } else {
                None
            };
            for i in 0..n {
                let key = arr_slot_raw((*b).keys, i) as *mut c_void;
                let bits = arr_slot_raw(arr, i) as u64;
                if let Some(rk) = &rec_keys
                    && let Some(rec_obj) = settled_record_to_dynobj(bits, rk)
                {
                    // The fresh record dynobj's stake transfers into
                    // the entry pair.
                    __torajs_dynobj_set(&mut obj, key, 4, rec_obj as u64);
                    continue;
                }
                let tag = __torajs_anyv_unbox_tag(bits);
                let value = __torajs_anyv_unbox_value(bits);
                // The entry's pair takes a stake; the result array
                // keeps its own (ShortStr materializes owned from
                // the unbox shim, so Heap covers it either way).
                if tag == 4 && value != 0 {
                    __torajs_rc_inc(value as *mut c_void);
                }
                __torajs_dynobj_set(&mut obj, key, tag as u64, value as u64);
            }
            if let Some(rk) = rec_keys {
                rk.release();
            }
            let boxed = __torajs_anyv_box_from_pair(4, obj as i64);
            (*rp).value_is_heap = 1;
            (*rp).value_repr = REPR_ANY;
            __torajs_promise_resolve(outer, boxed as i64);
        }
        __torajs_value_drop_heap((*b).keys);
        __torajs_promise_drop((*b).source);
        free(b as *mut c_void);
    }
}

/// The record dynobjs' three key cells, minted once per finish run.
/// `dynobj_set`'s insert incs the key it stores, so one mint serves
/// every record; release settles the mint's own stake.
struct RecordKeys {
    status: *mut c_void,
    value: *mut c_void,
    reason: *mut c_void,
}

impl RecordKeys {
    unsafe fn mint() -> Self {
        unsafe {
            RecordKeys {
                status: mint_str(b"status"),
                value: mint_str(b"value"),
                reason: mint_str(b"reason"),
            }
        }
    }
    unsafe fn release(self) {
        unsafe {
            __torajs_value_drop_heap(self.status);
            __torajs_value_drop_heap(self.value);
            __torajs_value_drop_heap(self.reason);
        }
    }
}

unsafe fn mint_str(literal: &[u8]) -> *mut c_void {
    unsafe {
        let s = __torajs_str_alloc_pooled(literal.len() as u64);
        memcpy(
            s.add(crate::layout::STR_HDR_SIZE) as *mut c_void,
            literal.as_ptr() as *const c_void,
            literal.len(),
        );
        s as *mut c_void
    }
}

/// One anonymous settled record → a real `{status, value|reason}`
/// dynobj (ordinary prototype — the proposal's records are ordinary
/// objects; only the enclosing result is null-proto). Reads the
/// record cell directly: status Str at +32, the boxed value at +40
/// (the any-lane record layout — `alloc_settled_struct`). `None`
/// when the slot is not a cell (the sync kernel's undefined
/// fallback), which stores verbatim instead.
unsafe fn settled_record_to_dynobj(rec_bits: u64, rk: &RecordKeys) -> Option<*mut c_void> {
    unsafe {
        if __torajs_anyv_unbox_tag(rec_bits) != 4 {
            return None;
        }
        let rec = __torajs_anyv_unbox_value(rec_bits) as *mut u8;
        if rec.is_null() {
            return None;
        }
        let status = *(rec.add(32) as *const *mut c_void);
        let first = *(status as *const u8).add(crate::layout::STR_HDR_SIZE);
        let value_bits = *(rec.add(40) as *const u64);
        let mut obj = __torajs_dynobj_alloc();
        __torajs_rc_inc(status);
        __torajs_dynobj_set(&mut obj, rk.status, 4, status as u64);
        let vt = __torajs_anyv_unbox_tag(value_bits);
        let vv = __torajs_anyv_unbox_value(value_bits);
        if vt == 4 && vv != 0 {
            __torajs_rc_inc(vv as *mut c_void);
        }
        let field = if first == b'f' { rk.value } else { rk.reason };
        __torajs_dynobj_set(&mut obj, field, vt as u64, vv as u64);
        Some(obj)
    }
}

/// # Safety
/// `v` is a live any-boxed value the caller owns for the duration of
/// the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_all_keyed_dyn(v: u64) -> *mut c_void {
    unsafe { keyed_dyn(v, crate::combinator_dyn::all_sync_untargeted, 0) }
}

/// # Safety
/// See [`__torajs_promise_all_keyed_dyn`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_allsettled_keyed_dyn(v: u64) -> *mut c_void {
    unsafe { keyed_dyn(v, crate::combinator_dyn::allsettled_sync_untagged, 1) }
}
