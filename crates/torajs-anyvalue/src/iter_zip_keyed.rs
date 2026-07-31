//! `Iterator.zipKeyed(obj, options)` — RFC 20260730-iterator-global
//! 刀 5c (proposal-joint-iteration, stage 3).
//!
//! The keyed sibling of zip: the columns come from `obj`'s own
//! enumerable string-keyed properties (each value opens through
//! GetIteratorFlattenable with REJECT-STRINGS), longest-mode padding
//! reads `Get(paddingOption, key)` per key instead of iterating, and
//! every row materializes as a fresh object carrying the same keys.
//!
//! The minted cell is the SAME kind-ZIP_KEYED IterHelper shape zip
//! uses — the keys snapshot (an owned `Array<Str>`, elem-kind marked
//! HEAP so the drop glue releases the key cells) parks in the
//! otherwise-unused inner slot, and the shared row step serves both
//! kinds through `RowSink`.

use core::ffi::c_void;

use crate::iter_from::derive_flattenable;
use crate::iter_helper::{
    COUNTER_OFF, FN_OFF, INNER_OFF, ITER_HELPER_ZIP_KEYED, UNDERLYING_OFF, iter_helper_cell_alloc,
};
use crate::iter_zip_shared::{
    ARR_DATA_PTR_OFF, ARR_LEN_OFF, TAG_HEAP, TAG_UNDEF, ZIP_MODE_LONGEST, av_is_object,
    close_open_iters, member_pair_cell, zip_parse_options,
};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
    fn __torajs_arr_drop_any(arr: *mut c_void);
    /// torajs-meta — own enumerable string keys as a +1-rc
    /// `Array<Str>` (§10.1.11.1 order).
    fn __torajs_anyv_own_keys(v: u64, include_nonenum: i64) -> *mut c_void;
    /// torajs-arr — stamp the keys array's elem kind (HEAP) so its
    /// drop releases the key Str cells (own_keys leaves it UNSET).
    fn __torajs_arr_mark_kind(arr: *mut c_void, chain: u64);
    fn __torajs_arr_drop_heap(arr: *mut c_void);
}

/// `ARR_KIND_HEAP` chain level (mirror of `torajs_rc`).
const KIND_CHAIN_HEAP: u64 = 4;

/// `Iterator.zipKeyed(obj, options)` kernel. Undefined with a
/// pending throw on any refusal (open columns close per
/// IfAbruptCloseIterators).
///
/// # Safety
/// `obj` / `options` carry valid AnyValue bit patterns (borrowed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_iterator_zip_keyed(obj: AnyValue, options: AnyValue) -> AnyValue {
    unsafe {
        // Step 1 — obj must be an Object.
        if !av_is_object(obj) {
            __torajs_throw_type_error(c"Iterator.zipKeyed argument is not an object".as_ptr());
            return VALUE_UNDEFINED;
        }
        // Steps 2-6 — options / mode / padding option (shared).
        let Some((mode, padding_opt)) = zip_parse_options(options) else {
            return VALUE_UNDEFINED;
        };
        // Step 7 — the keys snapshot (own enumerable string keys).
        let keys = __torajs_anyv_own_keys(obj, 0);
        __torajs_arr_mark_kind(keys, KIND_CHAIN_HEAP);
        let count = (keys.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read();
        let key_data = ((keys as *const u8).add(ARR_DATA_PTR_OFF) as *const *const u8).read();
        // Step 8 — open one column per key.
        let mut iters = __torajs_arr_alloc_any(count) as *mut c_void;
        for i in 0..count {
            let key = (key_data.add(i as usize * 8) as *const *mut c_void).read();
            let (t, p) = member_pair_cell(obj, key);
            let value = crate::nanbox_encode::__torajs_anyv_box_from_pair(t as i64, p as i64);
            match derive_flattenable(value, false) {
                Some(it) => {
                    iters = __torajs_arr_push_any(iters, TAG_HEAP, it) as *mut c_void;
                }
                None => {
                    let n = (iters.cast::<u8>().add(ARR_LEN_OFF) as *const u64).read();
                    close_open_iters(iters, n, u64::MAX);
                    __torajs_arr_drop_any(iters);
                    __torajs_arr_drop_heap(keys);
                    return VALUE_UNDEFINED;
                }
            }
        }
        // Step 9 — longest-mode padding: Get(paddingOption, key) per
        // key (an absent key pads undefined; no iteration).
        let mut padding_arr: *mut c_void = core::ptr::null_mut();
        if mode == ZIP_MODE_LONGEST {
            padding_arr = __torajs_arr_alloc_any(count) as *mut c_void;
            for i in 0..count {
                let (t, p) = if padding_opt != VALUE_UNDEFINED {
                    let key = (key_data.add(i as usize * 8) as *const *mut c_void).read();
                    member_pair_cell(padding_opt, key)
                } else {
                    (TAG_UNDEF, 0)
                };
                if __torajs_throw_check() != 0 {
                    close_open_iters(iters, count, u64::MAX);
                    __torajs_arr_drop_any(iters);
                    __torajs_arr_drop_any(padding_arr);
                    __torajs_arr_drop_heap(keys);
                    return VALUE_UNDEFINED;
                }
                // Borrowed pair — the slot takes its own stake.
                crate::payload_rc_inc(t as i64, p as i64);
                padding_arr = __torajs_arr_push_any(padding_arr, t, p) as *mut c_void;
            }
        }
        // Step 10 — mint; the keys snapshot transfers to inner.
        let cell = iter_helper_cell_alloc(ITER_HELPER_ZIP_KEYED);
        *(cell.add(UNDERLYING_OFF) as *mut u64) =
            crate::nanbox_encode::__torajs_anyv_box_pointer(iters);
        if !padding_arr.is_null() {
            *(cell.add(FN_OFF) as *mut u64) =
                crate::nanbox_encode::__torajs_anyv_box_pointer(padding_arr);
        }
        *(cell.add(COUNTER_OFF) as *mut u64) = mode;
        *(cell.add(INNER_OFF) as *mut u64) =
            crate::nanbox_encode::__torajs_anyv_box_pointer(keys as *mut c_void);
        crate::nanbox_encode::__torajs_anyv_box_pointer(cell as *mut c_void)
    }
}
