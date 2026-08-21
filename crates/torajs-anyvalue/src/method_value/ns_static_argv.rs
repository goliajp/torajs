//! Namespace statics that walk `argv` themselves — the arms of
//! [`super::ns_static`]'s dispatch whose argument handling is more
//! than a fixed-arity unbox.
//!
//! `JSON.parse` and `JSON.stringify` each pick a different kernel by
//! how many slots arrived; `Iterator.concat` packs the whole tail into
//! one array; `String.raw` reads slot 0 as the template and treats
//! everything after it as substitutions. A fixed `Disp::F` / `Disp::Ff`
//! shape cannot say any of that, so each gets a hand-written adapter
//! here rather than a row in the shape table.
//!
//! Split out of the parent when it reached the 500-line limit
//! (rotation 452 registered the watch; `String.raw` is the arrival
//! that spent it).

use core::ffi::c_void;

use super::ns_static::arg_at;
use super::ns_static_table::__torajs_str_drop;

/// §25.5.1 JSON.parse arm — parse + optional reviver walk. The
/// reviver kernel gates IsCallable itself (non-callable →
/// unfiltered root), so the split here is only an argv bounds
/// guard.
pub(super) unsafe fn json_parse_value(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        if argc >= 2 {
            crate::json_reviver::__torajs_json_parse_reviver(
                arg_at(argv, argc, 0),
                arg_at(argv, argc, 1),
            )
        } else {
            crate::json_any::__torajs_json_parse_any(arg_at(argv, argc, 0))
        }
    }
}

/// §25.5.2 JSON.stringify arm — value + replacer + space through the
/// full kernel. The walk answers an owned Str (or the undefined-Str
/// sentinel), which the slot box turns back into the §25.5.2
/// undefined answer.
pub(super) unsafe fn json_stringify_value(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let v = arg_at(argv, argc, 0);
        let s = if argc >= 2 {
            // Slot 2 non-callable is the spec's own ignore (step 4),
            // and slot 3 absent normalizes to the empty gap — so the
            // one kernel serves every arity past the bare form.
            let gap = crate::json_stringify::gap::__torajs_anyv_json_gap_str(arg_at(argv, argc, 2));
            let out = crate::json_stringify::replacer::__torajs_anyv_json_stringify_full(
                v,
                arg_at(argv, argc, 1),
                gap.cast_const(),
                0,
            );
            __torajs_str_drop(gap.cast::<c_void>());
            out
        } else {
            crate::json_stringify::__torajs_anyv_json_stringify(v)
        };
        crate::nanbox_encode::__torajs_anyv_box_str_slot(s.cast::<c_void>())
    }
}

unsafe extern "C" {
    /// torajs-arr — the fresh `Array<Any>` pack Iterator.concat's
    /// kernel takes ownership of (mirrors the wedge lowering).
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
}

/// `Iterator.concat(...items)` through the value cell — pack the
/// borrowed argv into a fresh rc-1 `Array<Any>` (each slot takes its
/// own +1 stake) and hand it to the kernel, which owns it from there.
pub(super) unsafe fn iterator_concat_pack(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let n = argc.max(0);
        let mut items = __torajs_arr_alloc_any(n as u64);
        for i in 0..n {
            let v = arg_at(argv, argc, i);
            let t = crate::__torajs_anyv_unbox_tag(v);
            let p = crate::__torajs_anyv_unbox_value(v);
            crate::payload_rc_inc(t, p);
            items = __torajs_arr_push_any(items as *mut c_void, t as u64, p as u64);
        }
        crate::iter_concat::__torajs_iterator_concat(items as *mut c_void)
    }
}

unsafe extern "C" {
    /// torajs-meta — §22.1.2.4's own walk (`template.raw` through the
    /// shape-blind member get, LengthOfArrayLike, ToString on every
    /// part and substitution). Answers an owned Str.
    fn __torajs_string_raw(template: u64, argv: *const u64, argc: i64) -> *mut c_void;
}

/// `String.raw(template, ...substitutions)` through the value cell.
/// Slot 0 is the template; the substitutions are the tail, which the
/// kernel indexes from 0 -- so the pointer advances past slot 0 rather
/// than the kernel re-deriving the split. A bare `String.raw()` has no
/// template at all, and the kernel's step 2 raises the spec TypeError
/// on the undefined it gets.
pub(super) unsafe fn string_raw_value(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let template = arg_at(argv, argc, 0);
        let subs = if argc > 1 { argv.add(1) } else { argv };
        let s = __torajs_string_raw(template, subs, (argc - 1).max(0));
        crate::nanbox_encode::__torajs_anyv_box_str_slot(s)
    }
}
