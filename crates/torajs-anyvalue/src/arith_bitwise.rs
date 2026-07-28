//! Bitwise (`&` / `|` / `^` / `<<` / `>>` / `>>>`) and `~`
//! dispatch on Any-tagged operands (ES §13.12 / §13.5.6) — RFC
//! 20260716 刀 7's family, moved out of [`crate::arith`] verbatim
//! when the rotation-241 BigInt lanes pushed that file past the
//! 500-line cap. The Number lane runs at 32-bit width per spec;
//! the BigInt leg (full-width) lives in [`crate::arith_bigint`].

use crate::coerce::any_to_number;
use crate::nanbox::{AnyValue, box_double, box_int32};

/// Bitwise op code decoded from the SSA-emitted wire (RFC 20260716
/// 刀 7). Mirror of the checker `check_bitwise` accept-set plus
/// `UShr` — six ops in total: 0=BitAnd, 1=BitOr, 2=BitXor, 3=Shl,
/// 4=Shr, 5=UShr. Kept aligned with the SSA lower's op-code emit
/// in `ssa_lower_binop_inner_any_arith::try_lower`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BitwiseOp {
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    UShr,
}

impl BitwiseOp {
    fn from_i64(op: i64) -> Option<BitwiseOp> {
        match op {
            0 => Some(BitwiseOp::BitAnd),
            1 => Some(BitwiseOp::BitOr),
            2 => Some(BitwiseOp::BitXor),
            3 => Some(BitwiseOp::Shl),
            4 => Some(BitwiseOp::Shr),
            5 => Some(BitwiseOp::UShr),
            _ => None,
        }
    }
}

/// ES spec §7.1.6 ToInt32 — NaN / ±0 / ±Infinity → 0; otherwise
/// truncate towards zero, then modulo 2^32, then signed
/// interpretation of the low 32 bits.
#[inline]
fn to_int32(n: f64) -> i32 {
    if !n.is_finite() {
        return 0;
    }
    let n_trunc = n.trunc();
    let n_mod = n_trunc.rem_euclid(4294967296.0);
    n_mod as u32 as i32
}

/// ES spec §7.1.7 ToUint32 — same as ToInt32 but returns the low
/// 32 bits interpreted as unsigned.
#[inline]
fn to_uint32(n: f64) -> u32 {
    if !n.is_finite() {
        return 0;
    }
    let n_trunc = n.trunc();
    n_trunc.rem_euclid(4294967296.0) as u32
}

/// Bitwise dispatch (`&`, `|`, `^`, `<<`, `>>`, `>>>`) on two
/// Any-tagged `(tag, value)` pairs per ES §13.12. Both operands
/// route through `ToNumber` (via [`any_to_number`], which unwraps
/// primitive wrappers per RFC 20260716 刀 1-6) and then `ToInt32`
/// (`ToUint32` for `>>>`'s LHS). The op runs at 32-bit width; the
/// result is boxed as I32 (fits in i32) or F64 (`>>>` results in
/// `[2^31, 2^32)` promote to f64 so the value stays non-negative
/// per spec).
///
/// Out-of-range `op` → NaN-boxed (defensive; IR should never emit
/// this).
///
/// # Safety
///
/// If either `tag` is Heap, the corresponding `value` must be
/// null or a valid `*mut HeapHeader` — propagated through
/// [`any_to_number`].
pub(crate) unsafe fn any_bitwise(op: i64, lt: i64, lv: i64, rt: i64, rv: i64) -> AnyValue {
    let Some(bit_op) = BitwiseOp::from_i64(op) else {
        return box_double(f64::NAN);
    };
    // §13.12 ToNumeric dispatch — BigInt pair rides the bigint
    // bitwise/shift kernels at full width, a mixed pair throws,
    // before ToNumber's 32-bit path (see arith_bigint).
    // SAFETY: caller invariant — propagated.
    if let Some(v) = unsafe { crate::arith_bigint::try_bitwise_bigint(bit_op, lt, lv, rt, rv) } {
        return v;
    }
    // SAFETY: caller invariant — propagated.
    let l = unsafe { any_to_number(lt, lv) };
    let r = unsafe { any_to_number(rt, rv) };
    let shift_count = to_uint32(r) & 31;
    match bit_op {
        BitwiseOp::BitAnd => box_int32(to_int32(l) & to_int32(r)),
        BitwiseOp::BitOr => box_int32(to_int32(l) | to_int32(r)),
        BitwiseOp::BitXor => box_int32(to_int32(l) ^ to_int32(r)),
        BitwiseOp::Shl => box_int32((to_int32(l).wrapping_shl(shift_count)) as i32),
        BitwiseOp::Shr => box_int32(to_int32(l) >> shift_count),
        BitwiseOp::UShr => {
            // Spec §13.12 `>>>` — LHS ToUint32, result ∈ [0, 2^32).
            // Values ≥ 2^31 do not fit in i32; promote to f64 so
            // the sign stays non-negative per spec.
            let result = to_uint32(l) >> shift_count;
            if result <= i32::MAX as u32 {
                box_int32(result as i32)
            } else {
                box_double(result as f64)
            }
        }
    }
}

/// Unary `~` on an Any-tagged `(tag, value)` pair per ES §13.5.6.
/// Operand routes through `ToNumber` → `ToInt32` → bitwise-not.
/// Result always fits in i32 (`~` preserves sign-extended-32).
///
/// # Safety
///
/// If `tag` is Heap, `value` must be null or a valid
/// `*mut HeapHeader` — propagated through [`any_to_number`].
pub(crate) unsafe fn any_bitnot(tag: i64, value: i64) -> AnyValue {
    // §6.1.6.2.2 — `~` on a BigInt operand rides the bigint kernel
    // (ToNumber's BigInt arm is the mixed-types throw).
    // SAFETY: caller invariant — propagated.
    if let Some(v) = unsafe { crate::arith_bigint::try_bitnot_bigint(tag, value) } {
        return v;
    }
    // SAFETY: caller invariant — propagated.
    let n = unsafe { any_to_number(tag, value) };
    box_int32(!to_int32(n))
}
