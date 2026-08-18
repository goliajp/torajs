//! The Math variadic reductions of the ns-static dispatch (§21.3.2
//! min / max / hypot) — split from `ns_static.rs` when the hypot arm
//! took it over the 500-line cap. The parent decides WHICH id runs;
//! this file holds the folds that walk `argv` themselves instead of
//! taking a fixed arity.

use crate::nanbox::{VALUE_UNDEFINED, box_double};

use super::ns_static::arg_num;
use super::ns_static_table::{__torajs_math_max, __torajs_math_min, __torajs_math_sqrt};

/// §21.3.2.24/25 Math.min / Math.max arm — coerce every arg in
/// source order, fold pairwise through the typed-tier kernel (NaN
/// propagation and ±0 ordering live there). A coercion throw
/// answers undefined with the pending throw recorded.
pub(super) unsafe fn min_max_fold(is_max: bool, argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let mut acc = if is_max {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
        for i in 0..argc {
            let Ok(x) = arg_num(argv, argc, i) else {
                return VALUE_UNDEFINED;
            };
            acc = if is_max {
                __torajs_math_max(acc, x)
            } else {
                __torajs_math_min(acc, x)
            };
        }
        box_double(acc)
    }
}

/// §21.3.2.18 Math.hypot arm — ToNumber every argument first (an
/// abrupt coercion propagates before any math), then steps 3-4: any
/// infinite coerced value answers +Infinity even when another is
/// NaN, which a plain `sum += x²` cannot express (∞² + NaN² sums to
/// NaN). Empty call answers +0 through `sqrt(0)`.
pub(super) unsafe fn hypot_fold(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let mut sum = 0.0f64;
        let mut has_inf = false;
        let mut has_nan = false;
        for i in 0..argc {
            let Ok(x) = arg_num(argv, argc, i) else {
                return VALUE_UNDEFINED;
            };
            has_inf |= x.is_infinite();
            has_nan |= x.is_nan();
            sum += x * x;
        }
        let r = if has_inf {
            f64::INFINITY
        } else if has_nan {
            f64::NAN
        } else {
            __torajs_math_sqrt(sum)
        };
        box_double(r)
    }
}
