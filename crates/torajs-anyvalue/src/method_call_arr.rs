//! `Tag::Arr` receiver arm for `__torajs_any_method_call` (split out
//! of `method_call.rs` at the 500-line boundary, chunk 710 — the
//! dispatcher stays the tag-switch, the torajs-arr glue id-switch
//! lives here; same shape as the str / num / date siblings).
//!
//! Argument ledger: identical to the dispatcher — argv slots are
//! BORROWED; growth-relocating methods (push / unshift) write the
//! possibly-moved receiver back through `recv_slot` themselves.

use core::ffi::c_void;

use torajs_rc::{
    ANY_METHOD_CONCAT, ANY_METHOD_COPY_WITHIN, ANY_METHOD_ENTRIES, ANY_METHOD_EVERY,
    ANY_METHOD_FILL, ANY_METHOD_FILTER, ANY_METHOD_FIND, ANY_METHOD_FIND_INDEX,
    ANY_METHOD_FOR_EACH, ANY_METHOD_INCLUDES, ANY_METHOD_INDEX_OF, ANY_METHOD_JOIN,
    ANY_METHOD_KEYS, ANY_METHOD_LAST_INDEX_OF, ANY_METHOD_MAP, ANY_METHOD_POP, ANY_METHOD_PUSH,
    ANY_METHOD_REDUCE, ANY_METHOD_REDUCE_RIGHT, ANY_METHOD_REVERSE, ANY_METHOD_SHIFT,
    ANY_METHOD_SLICE, ANY_METHOD_SOME, ANY_METHOD_SORT, ANY_METHOD_SPLICE, ANY_METHOD_UNSHIFT,
    ANY_METHOD_VALUES,
};

use crate::index_any::MIRROR_ARR_LEN_OFF;
use crate::method_call::{closure_boxed_entry, not_callable, to_index};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, is_undefined};
use crate::nanbox_encode::{
    __torajs_anyv_box_from_pair, __torajs_anyv_box_i64, __torajs_anyv_box_pointer,
    __torajs_anyv_unbox_tag, __torajs_anyv_unbox_value,
};
use crate::nanbox_ffi::__torajs_anyv_to_str;

unsafe extern "C" {
    /// torajs-arr — variadic push glue; returns the new length or
    /// the u64::MAX throw sentinel. Chases grow relocation and
    /// writes the fresh pointer back through `recv_slot` itself.
    fn __torajs_arr_any_push(
        arr: *mut c_void,
        argv: *const u64,
        argc: i64,
        recv_slot: *mut u64,
    ) -> u64;
    /// torajs-arr — pop glue (boxed last element, len shrink).
    fn __torajs_arr_any_pop(arr: *mut c_void) -> u64;
    /// Cross-tier — universal NaN-box-safe heap dropper (the fill
    /// arm's ShortStr-materialization release).
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-arr — shift glue (boxed first element, forward move).
    fn __torajs_arr_any_shift(arr: *mut c_void) -> u64;
    /// torajs-arr — variadic unshift glue; new length or the
    /// u64::MAX throw sentinel; relocations via `recv_slot`.
    fn __torajs_arr_any_unshift(
        arr: *mut c_void,
        argv: *const u64,
        argc: i64,
        recv_slot: *mut u64,
    ) -> u64;
    /// torajs-throw — pending-throw probe (fromIndex valueOf may
    /// throw; the scan must not run its getter side effects then).
    fn __torajs_throw_check() -> i64;
    /// torajs-arr — strict-eq index scan (found index or -1).
    fn __torajs_arr_any_index_of(arr: *const c_void, needle: u64, from: i64) -> i64;
    /// torajs-arr — backwards strict-eq scan (§23.1.3.20).
    fn __torajs_arr_any_last_index_of(arr: *const c_void, needle: u64, from: i64) -> i64;
    /// torajs-arr — in-place 8-byte-slot swap (element-type-agnostic
    /// — FLAG_ARR_ANY slots are 8-byte NaN-box immediates too);
    /// answers the same pointer for chaining.
    fn __torajs_arr_reverse(arr: *mut u8) -> *mut u8;
    /// torajs-arr — SameValueZero scan (1/0).
    fn __torajs_arr_any_includes(arr: *const c_void, needle: u64, from: i64) -> i64;
    /// torajs-arr — element-kind-dispatched join (fresh Str).
    fn __torajs_arr_any_join(arr: *const u8, sep: *const u8) -> u64;
    /// torajs-arr — kind-aware slice for any receivers (C4+); the
    /// returned array is fresh +1 rc, same slot layout as the source.
    fn __torajs_arr_any_slice(arr: *const u8, start: i64, end: i64) -> *mut u8;
    /// torajs-arr — variadic kind-aware concat (fresh +1 rc).
    fn __torajs_arr_any_concat(arr: *const u8, argv: *const u64, argc: i64) -> *mut u8;
    /// torajs-arr — kind-aware fill (`(tag, value)` pair form);
    /// answers the receiver.
    fn __torajs_arr_fill_any(
        arr: *mut c_void,
        tag: u64,
        value: u64,
        start: i64,
        end: i64,
    ) -> *mut u8;
    /// torajs-arr — in-place move with the heap-slot rc ledger
    /// (raw relative indices — the kernel wraps + clamps).
    fn __torajs_arr_any_copy_within(arr: *mut u8, target: i64, start: i64, end: i64) -> *mut u8;
    /// torajs-arr — remove + variadic insert; answers the fresh
    /// (+1 rc) removed array. Start / delete arrive normalized.
    fn __torajs_arr_any_splice(
        arr: *mut u8,
        actual_start: i64,
        actual_delete: i64,
        items: *const u64,
        item_count: i64,
    ) -> *mut u8;
    /// torajs-arr — HO loops over the boxed dual-entry callback ABI.
    fn __torajs_arr_any_map(
        arr: *const c_void,
        cb_env: *mut c_void,
        cb_entry: u64,
        this_arg: u64,
    ) -> u64;
    fn __torajs_arr_any_filter(
        arr: *const c_void,
        cb_env: *mut c_void,
        cb_entry: u64,
        this_arg: u64,
    ) -> u64;
    fn __torajs_arr_any_for_each(
        arr: *const c_void,
        cb_env: *mut c_void,
        cb_entry: u64,
        this_arg: u64,
    ) -> u64;
    /// torajs-arr — early-exit predicate loops (chunk 3).
    fn __torajs_arr_any_every(
        arr: *const c_void,
        cb_env: *mut c_void,
        cb_entry: u64,
        this_arg: u64,
    ) -> u64;
    fn __torajs_arr_any_some(
        arr: *const c_void,
        cb_env: *mut c_void,
        cb_entry: u64,
        this_arg: u64,
    ) -> u64;
    fn __torajs_arr_any_find(
        arr: *const c_void,
        cb_env: *mut c_void,
        cb_entry: u64,
        this_arg: u64,
    ) -> u64;
    fn __torajs_arr_any_find_index(
        arr: *const c_void,
        cb_env: *mut c_void,
        cb_entry: u64,
        this_arg: u64,
    ) -> u64;
    /// torajs-arr — accumulator fold; `init` is borrowed when
    /// `has_init != 0`, the returned accumulator is owned.
    fn __torajs_arr_any_reduce(
        arr: *const c_void,
        cb_env: *mut c_void,
        cb_entry: u64,
        init: u64,
        has_init: i64,
        right: i64,
    ) -> u64;
    /// torajs-arr — in-place stable merge sort (chunk 4); boxed
    /// comparator when `has_cb != 0`, else the §23.1.3.30.2
    /// ToString default. Answers the receiver.
    fn __torajs_arr_any_sort(
        arr: *mut u8,
        cb_env: *mut c_void,
        cb_entry: u64,
        has_cb: i64,
    ) -> *mut u8;
    /// torajs-arr — ArrIter mint (fresh +1 rc cells; the kind-aware
    /// step reads both receiver shapes).
    fn __torajs_arr_iter_create_keys(arr: *mut c_void) -> *mut c_void;
    fn __torajs_arr_iter_create_values(arr: *mut c_void) -> *mut c_void;
    fn __torajs_arr_iter_create_entries(arr: *mut c_void) -> *mut c_void;
    /// torajs-str — allocate a fresh Str from raw bytes (the
    /// `join()` default "," separator).
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    /// torajs-str — release a heap Str/Substr reference.
    fn __torajs_str_drop(s: *mut c_void);
}

/// Throw sentinel the torajs-arr glue floats — mirror of
/// `method_call.rs::ANY_METHOD_THREW`.
const ANY_METHOD_THREW: u64 = u64::MAX;

/// `Tag::Arr` arm — the §23.1.3 species pre-gate around the
/// id-switch (RFC 20260808 B3): `Early` is the gate's own answer
/// (throw / concat derive), `TransplantInto` runs the default
/// kernel below and moves its product's elements into the
/// species-constructed one, `Proceed` is the ordinary derive.
pub(crate) unsafe fn arr_method(
    arr: *mut c_void,
    mid: i64,
    recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    use crate::method_call_arr_species::{
        SpeciesGate, family_sets_length, species_family_pregate, transplant_product,
    };
    unsafe {
        match species_family_pregate(arr, mid, argv, argc) {
            SpeciesGate::Early(v) => v,
            SpeciesGate::TransplantInto(product) => {
                let d = arr_method_inner(arr, mid, recv_slot, argv, argc);
                transplant_product(product, d, family_sets_length(mid))
            }
            SpeciesGate::Proceed => arr_method_inner(arr, mid, recv_slot, argv, argc),
        }
    }
}

/// The id-switch onto the torajs-arr glue.
unsafe fn arr_method_inner(
    arr: *mut c_void,
    mid: i64,
    recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    let arg_at = |i: i64| -> u64 {
        if i < argc {
            unsafe { *argv.add(i as usize) }
        } else {
            VALUE_UNDEFINED
        }
    };
    unsafe {
        match mid {
            m if m == ANY_METHOD_PUSH => {
                let new_len = __torajs_arr_any_push(arr, argv, argc, recv_slot);
                if new_len == ANY_METHOD_THREW {
                    return VALUE_UNDEFINED;
                }
                __torajs_anyv_box_i64(new_len as i64)
            }
            m if m == ANY_METHOD_POP => __torajs_arr_any_pop(arr),
            m if m == ANY_METHOD_SHIFT => __torajs_arr_any_shift(arr),
            m if m == ANY_METHOD_UNSHIFT => {
                let new_len = __torajs_arr_any_unshift(arr, argv, argc, recv_slot);
                if new_len == ANY_METHOD_THREW {
                    return VALUE_UNDEFINED;
                }
                __torajs_anyv_box_i64(new_len as i64)
            }
            m if m == ANY_METHOD_INDEX_OF => {
                let from = to_index(arg_at(1), 0);
                // ToIntegerOrInfinity(fromIndex) valueOf may throw —
                // the scan's getter side effects must not run then.
                if __torajs_throw_check() != 0 {
                    return VALUE_UNDEFINED;
                }
                __torajs_anyv_box_i64(__torajs_arr_any_index_of(arr, arg_at(0), from))
            }
            m if m == ANY_METHOD_LAST_INDEX_OF => {
                // §23.1.3.20 step 4: absent fromIndex starts at the
                // last slot (i64::MAX rides the kernel clamp); a
                // PRESENT undefined is ToIntegerOrInfinity(undefined)
                // = 0 — only slot 0 is scanned.
                let from = if argc >= 2 {
                    to_index(arg_at(1), 0)
                } else {
                    i64::MAX
                };
                if __torajs_throw_check() != 0 {
                    return VALUE_UNDEFINED;
                }
                __torajs_anyv_box_i64(__torajs_arr_any_last_index_of(arr, arg_at(0), from))
            }
            m if m == ANY_METHOD_REVERSE => {
                let p = __torajs_arr_reverse(arr as *mut u8);
                // The receiver is the return value (chaining) — the
                // owned return protocol takes a fresh stake.
                torajs_rc::__torajs_rc_inc(p as *mut c_void);
                __torajs_anyv_box_pointer(p as *mut c_void)
            }
            m if m == ANY_METHOD_CONCAT => {
                let p = __torajs_arr_any_concat(arr as *const u8, argv, argc);
                __torajs_anyv_box_pointer(p as *mut c_void)
            }
            m if m == ANY_METHOD_FILL => {
                // §23.1.3.7 — relative start / end wrap from the end
                // here (the kernel's clamp is [0, len] only, matching
                // its SSA callers which pre-normalize); a missing end
                // is len (i64::MAX stays non-negative → kernel clamp).
                let av = arg_at(0);
                let len = *((arr as *const u8).add(MIRROR_ARR_LEN_OFF) as *const u64) as i64;
                let wrap = |v: i64| if v < 0 { v + len } else { v };
                let start = wrap(to_index(arg_at(1), 0));
                let end = wrap(to_index(arg_at(2), i64::MAX));
                let vp = __torajs_anyv_unbox_value(av);
                let p = __torajs_arr_fill_any(
                    arr,
                    __torajs_anyv_unbox_tag(av) as u64,
                    vp as u64,
                    start,
                    end,
                );
                // The fill kernel BORROWS the value (per-slot inc);
                // a ShortStr arg materialized an owned rc=1 Str in
                // the unbox above — release it or every
                // `fill("short")` call leaks the materialization.
                if crate::nanbox::is_short_str(av) && vp != 0 {
                    __torajs_value_drop_heap(vp as *mut c_void);
                }
                torajs_rc::__torajs_rc_inc(p as *mut c_void);
                __torajs_anyv_box_pointer(p as *mut c_void)
            }
            m if m == ANY_METHOD_COPY_WITHIN => {
                // Raw relative indices — the kernel owns §23.1.3.4's
                // wrap + clamp; a missing end is len.
                let target = to_index(arg_at(0), 0);
                let start = to_index(arg_at(1), 0);
                let end = to_index(arg_at(2), i64::MAX);
                let p = __torajs_arr_any_copy_within(arr as *mut u8, target, start, end);
                torajs_rc::__torajs_rc_inc(p as *mut c_void);
                __torajs_anyv_box_pointer(p as *mut c_void)
            }
            m if m == ANY_METHOD_SPLICE => {
                // §23.1.3.31 steps 3-7 — argc decides the delete
                // count: absent start deletes nothing, a lone start
                // deletes through the end.
                let len = *((arr as *const u8).add(MIRROR_ARR_LEN_OFF) as *const u64) as i64;
                let rel = to_index(arg_at(0), 0);
                let actual_start = if rel < 0 {
                    (rel + len).max(0)
                } else {
                    rel.min(len)
                };
                let actual_delete = if argc == 0 {
                    0
                } else if argc == 1 {
                    len - actual_start
                } else {
                    to_index(arg_at(1), 0).clamp(0, len - actual_start)
                };
                let (items, item_count) = if argc > 2 {
                    (argv.add(2), argc - 2)
                } else {
                    (core::ptr::null(), 0)
                };
                let p = __torajs_arr_any_splice(
                    arr as *mut u8,
                    actual_start,
                    actual_delete,
                    items,
                    item_count,
                );
                __torajs_anyv_box_pointer(p as *mut c_void)
            }
            m if m == ANY_METHOD_INCLUDES => {
                let from = to_index(arg_at(1), 0);
                __torajs_anyv_box_from_pair(1, __torajs_arr_any_includes(arr, arg_at(0), from))
            }
            m if is_callback_method(m) => match arr_method_callback(arr, m, argv, argc) {
                Some(v) => v,
                None => return not_callable(),
            },
            m if m == ANY_METHOD_KEYS || m == ANY_METHOD_VALUES || m == ANY_METHOD_ENTRIES => {
                // Fresh ArrIter cell (rc=1) — the owned return
                // protocol takes it as-is.
                let it = if m == ANY_METHOD_KEYS {
                    __torajs_arr_iter_create_keys(arr)
                } else if m == ANY_METHOD_VALUES {
                    __torajs_arr_iter_create_values(arr)
                } else {
                    __torajs_arr_iter_create_entries(arr)
                };
                __torajs_anyv_box_pointer(it)
            }
            m if m == ANY_METHOD_JOIN => {
                // ES §23.1.3.18 step 2: missing sep means ",".
                let sep_av = arg_at(0);
                let sep = if is_undefined(sep_av) {
                    __torajs_str_alloc(c",".as_ptr() as *const u8, 1)
                } else {
                    __torajs_anyv_to_str(sep_av) as *mut u8
                };
                let out = __torajs_arr_any_join(arr as *const u8, sep as *const u8);
                __torajs_str_drop(sep as *mut c_void);
                out
            }
            m if m == ANY_METHOD_SLICE => {
                // Missing end rides the kernel clamp (same idiom as
                // the str arm's slice).
                let start = to_index(arg_at(0), 0);
                let end = to_index(arg_at(1), i64::MAX);
                let p = __torajs_arr_any_slice(arr as *const u8, start, end);
                __torajs_anyv_box_pointer(p as *mut c_void)
            }
            // Copy-family / reflection extension arms (chunk 1, RFC
            // 20260712-array-generic-receiver) — the sibling floats
            // no-such for a genuine miss.
            _ => crate::method_call_arr_copy::arr_method_ext(arr, mid, argv, argc),
        }
    }
}

/// `true` iff `mid` is a callback-shaped method (`arg_at(0)` is a
/// callable / comparator whose entry pair the [`arr_method_callback`]
/// helper reads via [`closure_boxed_entry`]).
fn is_callback_method(mid: i64) -> bool {
    mid == ANY_METHOD_MAP
        || mid == ANY_METHOD_FILTER
        || mid == ANY_METHOD_FOR_EACH
        || mid == ANY_METHOD_EVERY
        || mid == ANY_METHOD_SOME
        || mid == ANY_METHOD_FIND
        || mid == ANY_METHOD_FIND_INDEX
        || mid == ANY_METHOD_REDUCE
        || mid == ANY_METHOD_REDUCE_RIGHT
        || mid == ANY_METHOD_SORT
}

/// Callback-arg method dispatch — 10 methods share the closure-boxed-
/// entry preamble (§23.1.3 callback presence check; a non-callable
/// arg[0] returns `None` and the caller floats a TypeError). Split
/// into three sub-arms per return shape: HO loops (map / filter /
/// for_each), predicate loops (every / some / find / find_index),
/// accumulator fold (reduce / reduce_right), and in-place sort. `sort`
/// accepts an undefined comparator (ToString default) but rejects a
/// present non-callable via the shared not_callable path.
unsafe fn arr_method_callback(
    arr: *mut c_void,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> Option<AnyValue> {
    let arg_at = |i: i64| -> u64 {
        if i < argc {
            unsafe { *argv.add(i as usize) }
        } else {
            VALUE_UNDEFINED
        }
    };
    unsafe {
        if mid == ANY_METHOD_SORT {
            // §23.1.3.30 step 1 — an undefined comparator is the
            // ToString default; a present non-callable throws.
            let (cb_env, cb_entry, has_cb) = if is_undefined(arg_at(0)) {
                (core::ptr::null_mut(), 0u64, 0i64)
            } else {
                let (e, en) = closure_boxed_entry(arg_at(0))?;
                (e, en, 1)
            };
            let p = __torajs_arr_any_sort(arr as *mut u8, cb_env, cb_entry, has_cb);
            // The receiver is the return value (chaining).
            torajs_rc::__torajs_rc_inc(p as *mut c_void);
            return Some(__torajs_anyv_box_pointer(p as *mut c_void));
        }
        let (cb_env, cb_entry) = closure_boxed_entry(arg_at(0))?;
        let raw = if mid == ANY_METHOD_MAP {
            __torajs_arr_any_map(arr, cb_env, cb_entry, arg_at(1))
        } else if mid == ANY_METHOD_FILTER {
            __torajs_arr_any_filter(arr, cb_env, cb_entry, arg_at(1))
        } else if mid == ANY_METHOD_FOR_EACH {
            __torajs_arr_any_for_each(arr, cb_env, cb_entry, arg_at(1))
        } else if mid == ANY_METHOD_EVERY {
            __torajs_arr_any_every(arr, cb_env, cb_entry, arg_at(1))
        } else if mid == ANY_METHOD_SOME {
            __torajs_arr_any_some(arr, cb_env, cb_entry, arg_at(1))
        } else if mid == ANY_METHOD_FIND {
            __torajs_arr_any_find(arr, cb_env, cb_entry, arg_at(1))
        } else if mid == ANY_METHOD_FIND_INDEX {
            __torajs_arr_any_find_index(arr, cb_env, cb_entry, arg_at(1))
        } else {
            // §23.1.3.24 step 4 — initialValue presence is an
            // argc question, not an undefined question.
            let has_init = (argc >= 2) as i64;
            let right = (mid == ANY_METHOD_REDUCE_RIGHT) as i64;
            __torajs_arr_any_reduce(arr, cb_env, cb_entry, arg_at(1), has_init, right)
        };
        Some(raw)
    }
}
