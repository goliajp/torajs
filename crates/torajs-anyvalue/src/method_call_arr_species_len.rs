//! §23.1.3's ArraySpeciesCreate length argument, per family method
//! (species key 2, rotation 339) — split from
//! `method_call_arr_species.rs` under the 500-line file rule. Body
//! verbatim.

use core::ffi::c_void;

use crate::index_any::MIRROR_ARR_LEN_OFF;

/// §23.1.3's ArraySpeciesCreate length argument, per family method:
/// concat / filter / flat / flatMap seed an EMPTY product (their
/// steps pass 0 — elements append afterwards); map passes the source
/// len (§23.1.3.20 step 5); slice passes count (steps 3-7's k/final
/// clamp); splice passes actualDeleteCount (§23.1.3.31 steps 3-7).
/// The receiver-len shortcut this replaces handed every method the
/// source length — filter's create-species.js asserts the ctor sees
/// 0 (probed rotation 339). Infinities clamp before the i64 cast so
/// the relative-index math cannot overflow.
///
/// # Safety
/// `arr` is a live array heap block pointer; `argv` holds `argc`
/// borrowed AnyValues.
pub(crate) unsafe fn species_ctor_len(
    arr: *mut c_void,
    mid: i64,
    argv: *const u64,
    argc: i64,
) -> i64 {
    let len = unsafe { *((arr as *const u8).add(MIRROR_ARR_LEN_OFF) as *const u64) as i64 };
    let int_arg = |k: i64, default: i64| -> i64 {
        if k >= argc {
            return default;
        }
        let av = unsafe { *argv.add(k as usize) };
        let t = crate::nanbox_encode::__torajs_anyv_unbox_tag(av);
        if t == 5 {
            return default;
        }
        let v = crate::nanbox_encode::__torajs_anyv_unbox_value(av);
        let n = unsafe { crate::coerce::any_to_number(t, v) };
        // any_to_number only borrows; a ShortStr argument
        // materialized an owned rc=1 Str in the unbox above (546-02
        // M1 family) — release it or every `slice("1")` leaks.
        if crate::nanbox::is_short_str(av) && v != 0 {
            unsafe { crate::__torajs_value_drop_heap(v as *mut c_void) };
        }
        if n.is_nan() {
            0
        } else {
            n.clamp(-9.0e15, 9.0e15).trunc() as i64
        }
    };
    if mid == torajs_rc::ANY_METHOD_MAP {
        len
    } else if mid == torajs_rc::ANY_METHOD_SLICE {
        let rel_start = int_arg(0, 0);
        let k = if rel_start < 0 {
            (len + rel_start).max(0)
        } else {
            rel_start.min(len)
        };
        let rel_end = int_arg(1, len);
        let fin = if rel_end < 0 {
            (len + rel_end).max(0)
        } else {
            rel_end.min(len)
        };
        (fin - k).max(0)
    } else if mid == torajs_rc::ANY_METHOD_SPLICE {
        let rel_start = int_arg(0, 0);
        let actual_start = if rel_start < 0 {
            (len + rel_start).max(0)
        } else {
            rel_start.min(len)
        };
        if argc == 0 {
            0
        } else if argc == 1 {
            len - actual_start
        } else {
            int_arg(1, 0).clamp(0, len - actual_start)
        }
    } else {
        0
    }
}
