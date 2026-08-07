//! Pass 0 `declare_intrinsic` group: Math.* runtime intrinsics.
//!
//! chunk 132 continuation of the Pass 0 multi-sub-sibling split
//! (chunks 110-131). 35 declarations covering the entire Math.*
//! namespace surface that the lowerer routes from `Math.*`
//! call-site dispatch.
//!
//! Most take/return F64 (libc-backed via thin wrappers in each
//! backend: sqrt / fabs / floor / ceil / log / exp / pow / sin /
//! cos / tan / asin / acos / atan / atan2 / log2 / log10 / cbrt /
//! sinh / cosh / tanh / asinh / acosh / atanh / expm1 / log1p +
//! ES specials sign / round / trunc / min / max / fround / f16round
//! / random). Exceptions:
//! - `math_imul(i64, i64) -> i64` — Math.imul 32×32→32 with i64
//!   carriage.
//! - `math_clz32(i64) -> i64` — Math.clz32 count-leading-zeros on
//!   the low 32 bits.
//! - `math_sum_precise(ptr) -> f64` + `_i64(ptr) -> f64` — TC39
//!   Math.sumPrecise proposal; takes an Array<F64>/Array<I64>
//!   pointer and returns the lossless sum.
//! - `math_random() -> f64` — Math.random, [0, 1).

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct MathIds {
    pub math_sqrt: FuncId,
    pub math_abs: FuncId,
    pub math_floor: FuncId,
    pub math_ceil: FuncId,
    pub math_log: FuncId,
    pub math_exp: FuncId,
    pub math_sign: FuncId,
    pub math_round: FuncId,
    pub math_trunc: FuncId,
    pub math_pow: FuncId,
    pub math_min: FuncId,
    pub math_max: FuncId,
    pub math_sin: FuncId,
    pub math_cos: FuncId,
    pub math_tan: FuncId,
    pub math_asin: FuncId,
    pub math_acos: FuncId,
    pub math_atan: FuncId,
    pub math_atan2: FuncId,
    pub math_log2: FuncId,
    pub math_log10: FuncId,
    pub math_cbrt: FuncId,
    pub math_sinh: FuncId,
    pub math_cosh: FuncId,
    pub math_tanh: FuncId,
    pub math_asinh: FuncId,
    pub math_acosh: FuncId,
    pub math_atanh: FuncId,
    pub math_expm1: FuncId,
    pub math_log1p: FuncId,
    pub math_imul: FuncId,
    pub math_clz32: FuncId,
    pub math_fround: FuncId,
    pub math_sum_precise: FuncId,
    pub math_sum_precise_i64: FuncId,
    pub math_f16round: FuncId,
    pub math_random: FuncId,
    /// RFC 20260801-ns-object-value — the Math namespace object as a
    /// first-class value (interned immortal singleton, Any).
    pub ns_object_math: FuncId,
    /// RFC 20260807-global-object G2 — the globalThis singleton as a
    /// first-class value (same immortal pre-filled dynobj lane).
    pub globalthis_object: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> MathIds {
    // Helper closures for the common shapes.
    let f_to_f = |m: &mut Module, t: &mut HashMap<String, FuncId>, name: &'static str| -> FuncId {
        declare_intrinsic(m, t, name, &[Type::F64], Type::F64)
    };
    let ff_to_f = |m: &mut Module, t: &mut HashMap<String, FuncId>, name: &'static str| -> FuncId {
        declare_intrinsic(m, t, name, &[Type::F64, Type::F64], Type::F64)
    };
    MathIds {
        math_sqrt: f_to_f(module, fn_table, "__torajs_math_sqrt"),
        math_abs: f_to_f(module, fn_table, "__torajs_math_abs"),
        math_floor: f_to_f(module, fn_table, "__torajs_math_floor"),
        math_ceil: f_to_f(module, fn_table, "__torajs_math_ceil"),
        math_log: f_to_f(module, fn_table, "__torajs_math_log"),
        math_exp: f_to_f(module, fn_table, "__torajs_math_exp"),
        math_sign: f_to_f(module, fn_table, "__torajs_math_sign"),
        math_round: f_to_f(module, fn_table, "__torajs_math_round"),
        math_trunc: f_to_f(module, fn_table, "__torajs_math_trunc"),
        math_pow: ff_to_f(module, fn_table, "__torajs_math_pow"),
        math_min: ff_to_f(module, fn_table, "__torajs_math_min"),
        math_max: ff_to_f(module, fn_table, "__torajs_math_max"),
        math_sin: f_to_f(module, fn_table, "__torajs_math_sin"),
        math_cos: f_to_f(module, fn_table, "__torajs_math_cos"),
        math_tan: f_to_f(module, fn_table, "__torajs_math_tan"),
        math_asin: f_to_f(module, fn_table, "__torajs_math_asin"),
        math_acos: f_to_f(module, fn_table, "__torajs_math_acos"),
        math_atan: f_to_f(module, fn_table, "__torajs_math_atan"),
        math_atan2: ff_to_f(module, fn_table, "__torajs_math_atan2"),
        math_log2: f_to_f(module, fn_table, "__torajs_math_log2"),
        math_log10: f_to_f(module, fn_table, "__torajs_math_log10"),
        math_cbrt: f_to_f(module, fn_table, "__torajs_math_cbrt"),
        math_sinh: f_to_f(module, fn_table, "__torajs_math_sinh"),
        math_cosh: f_to_f(module, fn_table, "__torajs_math_cosh"),
        math_tanh: f_to_f(module, fn_table, "__torajs_math_tanh"),
        math_asinh: f_to_f(module, fn_table, "__torajs_math_asinh"),
        math_acosh: f_to_f(module, fn_table, "__torajs_math_acosh"),
        math_atanh: f_to_f(module, fn_table, "__torajs_math_atanh"),
        math_expm1: f_to_f(module, fn_table, "__torajs_math_expm1"),
        math_log1p: f_to_f(module, fn_table, "__torajs_math_log1p"),
        math_imul: declare_intrinsic(
            module,
            fn_table,
            "__torajs_math_imul",
            &[Type::I64, Type::I64],
            Type::I64,
        ),
        math_clz32: declare_intrinsic(
            module,
            fn_table,
            "__torajs_math_clz32",
            &[Type::I64],
            Type::I64,
        ),
        math_fround: f_to_f(module, fn_table, "__torajs_math_fround"),
        math_sum_precise: declare_intrinsic(
            module,
            fn_table,
            "__torajs_math_sum_precise",
            &[Type::Ptr],
            Type::F64,
        ),
        math_sum_precise_i64: declare_intrinsic(
            module,
            fn_table,
            "__torajs_math_sum_precise_i64",
            &[Type::Ptr],
            Type::F64,
        ),
        math_f16round: f_to_f(module, fn_table, "__torajs_math_f16round"),
        math_random: declare_intrinsic(module, fn_table, "__torajs_math_random", &[], Type::F64),
        ns_object_math: declare_intrinsic(
            module,
            fn_table,
            "__torajs_ns_object_math",
            &[],
            Type::Any,
        ),
        globalthis_object: declare_intrinsic(
            module,
            fn_table,
            "__torajs_globalthis_object",
            &[],
            Type::Any,
        ),
    }
}
