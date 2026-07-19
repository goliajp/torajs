//! Namespace-static dispatch table + kernel externs (RFC
//! 20260719-ns-static-value-reify) — split from
//! `method_value/ns_static.rs` under the 500-line file rule. Holds
//! the `Disp` shape enum, the id-indexed `DISPATCH` table
//! (index-lockstep with `torajs_rc::ns_static::NS_STATIC_TABLE`,
//! asserted in the parent's unit tests), and the cross-staticlib
//! kernel declarations the arms delegate to.
//!
//! MAINTENANCE: every extern added here needs a matching no-op stub
//! in `lib.rs`'s `#[cfg(test)] mod tests` — the table is
//! test-reachable, so `-dead_strip` keeps this module and the test
//! binary link fails on any unstubbed symbol (bitten twice: the
//! inspect print chain, then the num parse pair).

use core::ffi::c_void;

unsafe extern "C" {
    pub(super) fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-throw — 1 when a pending throw is recorded (a poisoned
    /// valueOf during ToNumber aborts the remaining coercions).
    pub(super) fn __torajs_throw_check() -> i64;
    fn __torajs_math_sqrt(x: f64) -> f64;
    fn __torajs_math_abs(x: f64) -> f64;
    fn __torajs_math_floor(x: f64) -> f64;
    fn __torajs_math_ceil(x: f64) -> f64;
    fn __torajs_math_log(x: f64) -> f64;
    fn __torajs_math_exp(x: f64) -> f64;
    fn __torajs_math_sign(x: f64) -> f64;
    fn __torajs_math_round(x: f64) -> f64;
    fn __torajs_math_trunc(x: f64) -> f64;
    fn __torajs_math_sin(x: f64) -> f64;
    fn __torajs_math_cos(x: f64) -> f64;
    fn __torajs_math_tan(x: f64) -> f64;
    fn __torajs_math_asin(x: f64) -> f64;
    fn __torajs_math_acos(x: f64) -> f64;
    fn __torajs_math_atan(x: f64) -> f64;
    fn __torajs_math_log2(x: f64) -> f64;
    fn __torajs_math_log10(x: f64) -> f64;
    fn __torajs_math_cbrt(x: f64) -> f64;
    fn __torajs_math_sinh(x: f64) -> f64;
    fn __torajs_math_cosh(x: f64) -> f64;
    fn __torajs_math_tanh(x: f64) -> f64;
    fn __torajs_math_asinh(x: f64) -> f64;
    fn __torajs_math_acosh(x: f64) -> f64;
    fn __torajs_math_atanh(x: f64) -> f64;
    fn __torajs_math_expm1(x: f64) -> f64;
    fn __torajs_math_log1p(x: f64) -> f64;
    fn __torajs_math_fround(x: f64) -> f64;
    fn __torajs_math_f16round(x: f64) -> f64;
    fn __torajs_math_pow(x: f64, y: f64) -> f64;
    pub(super) fn __torajs_math_min(x: f64, y: f64) -> f64;
    pub(super) fn __torajs_math_max(x: f64, y: f64) -> f64;
    fn __torajs_math_atan2(y: f64, x: f64) -> f64;
    fn __torajs_math_imul(a: i64, b: i64) -> i64;
    fn __torajs_math_clz32(x: i64) -> i64;
    fn __torajs_math_random() -> f64;
    /// torajs-num — the typed tier's §19.2.5/.4 parse kernels
    /// (Str cell in, auto-detect radix on 0).
    pub(super) fn __torajs_num_parse_int(s: *const u8, radix: i64) -> f64;
    pub(super) fn __torajs_num_parse_float(s: *const u8) -> f64;
    /// torajs-str — release the owned coercion temp.
    pub(super) fn __torajs_str_drop(s: *mut c_void);
}

/// Per-id dispatch shape. Index-lockstep with
/// [`torajs_rc::ns_static::NS_STATIC_TABLE`].
pub(super) enum Disp {
    /// f64 → f64 unary (argc 0 coerces undefined → NaN).
    F(unsafe extern "C" fn(f64) -> f64),
    /// f64 × f64 → f64 binary (missing args coerce to NaN).
    Ff(unsafe extern "C" fn(f64, f64) -> f64),
    /// §21.3.2.24/25 variadic reduction (empty → ±Infinity).
    MinMax { is_max: bool },
    /// ToInt32 pair → i32 result (imul).
    I32Pair(unsafe extern "C" fn(i64, i64) -> i64),
    /// ToUint32 unary → i32-ranged result (clz32).
    I32One(unsafe extern "C" fn(i64) -> i64),
    /// () → f64 (random).
    Nullary(unsafe extern "C" fn() -> f64),
    /// WHATWG console stdout logger — per-arg tag-aware inline
    /// print + `' '` separators + `'\n'` (the chunk-808 multiarg
    /// phase-2 sequence; args are already evaluated in argv).
    ConsoleLog,
    /// §19.2.5 parseInt — ToString(arg0) + ToInt32(radix) into the
    /// typed tier's parse kernel.
    ParseInt,
    /// §19.2.4 parseFloat — ToString(arg0) into the parse kernel.
    ParseFloat,
    /// §21.1.2 Number predicate family — computed inline on the
    /// NaN-box (spec: non-number input answers false, NO coercion).
    NumPredicate(NumPred),
    /// §23.1.2.2 Array.isArray — heap-tag probe.
    ArrayIsArray,
    /// §20.1.2.14 Object.is — the §7.2.10 same-value kernel.
    ObjectIs,
}

/// The four `Number.is*` predicates (shared dispatch shape).
pub(super) enum NumPred {
    Integer,
    Nan,
    Finite,
    SafeInteger,
}

pub(super) static DISPATCH: &[Disp] = &[
    Disp::F(__torajs_math_sqrt),
    Disp::F(__torajs_math_abs),
    Disp::F(__torajs_math_floor),
    Disp::F(__torajs_math_ceil),
    Disp::F(__torajs_math_log),
    Disp::F(__torajs_math_exp),
    Disp::F(__torajs_math_sign),
    Disp::F(__torajs_math_round),
    Disp::F(__torajs_math_trunc),
    Disp::F(__torajs_math_sin),
    Disp::F(__torajs_math_cos),
    Disp::F(__torajs_math_tan),
    Disp::F(__torajs_math_asin),
    Disp::F(__torajs_math_acos),
    Disp::F(__torajs_math_atan),
    Disp::F(__torajs_math_log2),
    Disp::F(__torajs_math_log10),
    Disp::F(__torajs_math_cbrt),
    Disp::F(__torajs_math_sinh),
    Disp::F(__torajs_math_cosh),
    Disp::F(__torajs_math_tanh),
    Disp::F(__torajs_math_asinh),
    Disp::F(__torajs_math_acosh),
    Disp::F(__torajs_math_atanh),
    Disp::F(__torajs_math_expm1),
    Disp::F(__torajs_math_log1p),
    Disp::F(__torajs_math_fround),
    Disp::F(__torajs_math_f16round),
    Disp::Ff(__torajs_math_pow),
    Disp::MinMax { is_max: false },
    Disp::MinMax { is_max: true },
    Disp::Ff(__torajs_math_atan2),
    Disp::I32Pair(__torajs_math_imul),
    Disp::I32One(__torajs_math_clz32),
    Disp::Nullary(__torajs_math_random),
    Disp::ConsoleLog, // console.log
    Disp::ConsoleLog, // console.info — same stream per §1.1.2/.4
    Disp::ConsoleLog, // console.debug
    Disp::ParseInt,
    Disp::ParseFloat,
    Disp::NumPredicate(NumPred::Integer),
    Disp::NumPredicate(NumPred::Nan),
    Disp::NumPredicate(NumPred::Finite),
    Disp::NumPredicate(NumPred::SafeInteger),
    Disp::ArrayIsArray,
    Disp::ObjectIs,
];
