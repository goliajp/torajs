//! The F64 spelling of `undefined` — the runtime's one copy of the
//! sentinel the compiler mints (`ssa_lower_undef_f64_source`).

/// The NaN payload a `number[]` read past the end answers, and a
/// generator step's completion value (RFC 20260805). Mirror of the
/// compiler's `F64_UNDEF_SENTINEL_BITS`; the two must agree. A typed
/// `number` slot crossing into the any world by REFLECTION (a struct
/// field read through an `any` receiver) normalizes it to `undefined`
/// here, the way the compiled crossing (`box_f64_or_undef`) does.
pub const F64_UNDEF_SENTINEL_BITS: u64 = 0x7FFC_0000_0000_000A;
