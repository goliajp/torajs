//! Transfer functions for the integer-interval lattice — one
//! `eval_inst` per SSA instruction, mapping operand facts to a result
//! fact. All arithmetic runs in i128 with infinity sentinels far
//! outside the i64 range, so bound computation never wraps and
//! saturation is explicit.
//!
//! Soundness contract: a `Fact` claims the runtime value is always an
//! integer-valued number inside `[lo, hi]`. For f64-typed values the
//! integer-valued claim is the load-bearing part (float demotion);
//! every f64 transfer therefore widens any bound at or beyond 2^53 to
//! a sentinel — inside (−2^53, 2^53) f64 integer arithmetic is exact,
//! outside it rounds and only the integer-valuedness (ulp ≥ 1)
//! survives.

use torajs_core::ssa::{BinOp, InstKind, Operand, Type};

/// Negative / positive infinity sentinels. Far enough outside i64
/// that interval addition of two sentinels still fits in i128, and
/// any computed bound beyond ±2^63 clamps to them.
pub const NEG_INF: i128 = -(1i128 << 70);
pub const POS_INF: i128 = 1i128 << 70;

const I64_MIN: i128 = i64::MIN as i128;
const I64_MAX: i128 = i64::MAX as i128;
const F64_EXACT: i128 = 1i128 << 53;

/// Integer-valued interval fact: the value is always an integer
/// (exact for i64-typed values, integer-valued for f64-typed ones)
/// and lies in `[lo, hi]`. Sentinel bounds mean unbounded on that
/// side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumFact {
    pub lo: i128,
    pub hi: i128,
}

impl NumFact {
    pub fn new(lo: i128, hi: i128) -> Self {
        Self { lo, hi }
    }

    pub fn point(v: i128) -> Self {
        Self { lo: v, hi: v }
    }

    pub fn full_i64() -> Self {
        Self {
            lo: I64_MIN,
            hi: I64_MAX,
        }
    }

    pub fn join(self, other: Self) -> Self {
        Self {
            lo: self.lo.min(other.lo),
            hi: self.hi.max(other.hi),
        }
    }

    /// Intersect with a branch constraint. `None` if empty (the
    /// constrained path is dead — caller keeps the unrefined fact).
    pub fn meet(self, other: Self) -> Option<Self> {
        let lo = self.lo.max(other.lo);
        let hi = self.hi.min(other.hi);
        (lo <= hi).then_some(Self { lo, hi })
    }

    pub fn contains(self, other: Self) -> bool {
        self.lo <= other.lo && self.hi >= other.hi
    }

    /// The value provably equals its own low-32 sign extension.
    pub fn is_sext32(self) -> bool {
        self.lo >= i32::MIN as i128 && self.hi <= i32::MAX as i128
    }
}

/// Three-point lattice around `NumFact`: `Bottom` = not yet computed
/// (fixpoint seed), `Top` = unknown / possibly non-integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbsVal {
    Bottom,
    Fact(NumFact),
    Top,
}

impl AbsVal {
    pub fn join(self, other: Self) -> Self {
        match (self, other) {
            (AbsVal::Bottom, x) | (x, AbsVal::Bottom) => x,
            (AbsVal::Top, _) | (_, AbsVal::Top) => AbsVal::Top,
            (AbsVal::Fact(a), AbsVal::Fact(b)) => AbsVal::Fact(a.join(b)),
        }
    }
}

fn clamp(v: i128) -> i128 {
    if v < I64_MIN {
        NEG_INF
    } else if v > I64_MAX {
        POS_INF
    } else {
        v
    }
}

fn clamp_fact(f: NumFact) -> NumFact {
    NumFact::new(clamp(f.lo), clamp(f.hi))
}

/// f64-op result: any bound at or beyond 2^53 widens to a sentinel —
/// past that the op rounds, so only integer-valuedness is claimed.
fn clamp53(f: NumFact) -> NumFact {
    NumFact::new(
        if f.lo <= -F64_EXACT { NEG_INF } else { f.lo },
        if f.hi >= F64_EXACT { POS_INF } else { f.hi },
    )
}

fn add_iv(a: NumFact, b: NumFact) -> NumFact {
    clamp_fact(NumFact::new(a.lo + b.lo, a.hi + b.hi))
}

fn sub_iv(a: NumFact, b: NumFact) -> NumFact {
    clamp_fact(NumFact::new(a.lo - b.hi, a.hi - b.lo))
}

fn mul_corner(a: i128, b: i128) -> i128 {
    match a.checked_mul(b) {
        Some(v) => clamp(v),
        None => {
            if (a > 0) == (b > 0) {
                POS_INF
            } else {
                NEG_INF
            }
        }
    }
}

fn mul_iv(a: NumFact, b: NumFact) -> NumFact {
    let c = [
        mul_corner(a.lo, b.lo),
        mul_corner(a.lo, b.hi),
        mul_corner(a.hi, b.lo),
        mul_corner(a.hi, b.hi),
    ];
    NumFact::new(*c.iter().min().unwrap(), *c.iter().max().unwrap())
}

/// Truncating division of an interval by a nonzero constant.
fn div_iv(a: NumFact, d: i128) -> NumFact {
    let c = [a.lo / d, a.hi / d];
    clamp_fact(NumFact::new(
        *c.iter().min().unwrap(),
        *c.iter().max().unwrap(),
    ))
}

/// Truncated remainder by a constant: |result| < |d|, sign follows
/// the dividend (both i64 srem and f64 frem/fmod share this shape).
fn rem_iv(a: NumFact, d: i128) -> NumFact {
    let m = d.abs() - 1;
    NumFact::new(
        if a.lo >= 0 { 0 } else { -m },
        if a.hi <= 0 { 0 } else { m },
    )
}

fn shl_iv(a: NumFact, k: i128) -> NumFact {
    if !(0..64).contains(&k) {
        return NumFact::full_i64();
    }
    mul_iv(a, NumFact::point(1i128 << k))
}

fn ashr_iv(a: NumFact, k: i128) -> NumFact {
    if !(0..64).contains(&k) {
        return NumFact::full_i64();
    }
    clamp_fact(NumFact::new(a.lo >> k, a.hi >> k))
}

/// Resolve an operand constant to its integer value when it has one.
pub fn op_const_int(op: &Operand) -> Option<i128> {
    match op {
        Operand::ConstI64(c) => Some(*c as i128),
        Operand::ConstI32(c) => Some(*c as i128),
        Operand::ConstF64(f) => {
            (f.is_finite() && f.fract() == 0.0 && f.abs() < (1u64 << 63) as f64).then(|| *f as i128)
        }
        _ => None,
    }
}

/// Evaluate one instruction. `env` resolves an operand to its current
/// abstract value (constants included); `is_even` answers whether the
/// operand is provably even in the current block context (frem-based
/// branch refinement — makes `fdiv x, 2` exact).
pub fn eval_inst(
    kind: &InstKind,
    result_ty: Type,
    env: &dyn Fn(&Operand) -> AbsVal,
    is_even: &dyn Fn(&Operand) -> bool,
) -> AbsVal {
    // opaque producers (calls, loads, params…): an i64-typed result
    // is still a bounded machine integer; anything else is unknown.
    let opaque = || match result_ty {
        Type::I64 => AbsVal::Fact(NumFact::full_i64()),
        _ => AbsVal::Top,
    };
    let binary = |a: &Operand, b: &Operand, f: &dyn Fn(NumFact, NumFact) -> NumFact| match (
        env(a),
        env(b),
    ) {
        (AbsVal::Bottom, _) | (_, AbsVal::Bottom) => AbsVal::Bottom,
        (AbsVal::Fact(fa), AbsVal::Fact(fb)) => AbsVal::Fact(f(fa, fb)),
        _ => opaque(),
    };
    match kind {
        InstKind::BinOp(op, a, b) => match op {
            BinOp::Add => binary(a, b, &add_iv),
            BinOp::Sub => binary(a, b, &sub_iv),
            BinOp::Mul => binary(a, b, &mul_iv),
            BinOp::SDiv | BinOp::SRem => match (env(a), op_const_int(b)) {
                (AbsVal::Bottom, _) => AbsVal::Bottom,
                (AbsVal::Fact(fa), Some(d)) if d != 0 => AbsVal::Fact(match op {
                    BinOp::SDiv => div_iv(fa, d),
                    _ => rem_iv(fa, d),
                }),
                _ => opaque(),
            },
            BinOp::And => match (env(a), env(b)) {
                (AbsVal::Bottom, _) | (_, AbsVal::Bottom) => AbsVal::Bottom,
                (av, bv) => {
                    // bitwise-and only clears bits: with any provably
                    // non-negative operand the result is [0, that hi].
                    let nonneg_hi = |v: AbsVal| match v {
                        AbsVal::Fact(f) if f.lo >= 0 => Some(f.hi),
                        _ => None,
                    };
                    match (nonneg_hi(av), nonneg_hi(bv)) {
                        (Some(x), Some(y)) => AbsVal::Fact(NumFact::new(0, x.min(y))),
                        (Some(x), None) | (None, Some(x)) => AbsVal::Fact(NumFact::new(0, x)),
                        (None, None) => AbsVal::Fact(NumFact::full_i64()),
                    }
                }
            },
            BinOp::Or | BinOp::Xor => match (env(a), env(b)) {
                (AbsVal::Bottom, _) | (_, AbsVal::Bottom) => AbsVal::Bottom,
                (AbsVal::Fact(fa), AbsVal::Fact(fb)) if fa.lo >= 0 && fb.lo >= 0 => {
                    // or/xor of non-negatives stays under the next
                    // power of two covering both his.
                    let m = fa.hi.max(fb.hi);
                    let bound = if m <= 0 {
                        0
                    } else {
                        (1i128 << (128 - m.leading_zeros())) - 1
                    };
                    AbsVal::Fact(clamp_fact(NumFact::new(0, bound)))
                }
                _ => AbsVal::Fact(NumFact::full_i64()),
            },
            BinOp::Shl => match (env(a), op_const_int(b)) {
                (AbsVal::Bottom, _) => AbsVal::Bottom,
                (AbsVal::Fact(fa), Some(k)) => AbsVal::Fact(shl_iv(fa, k)),
                _ => opaque(),
            },
            BinOp::AShr => match (env(a), op_const_int(b)) {
                (AbsVal::Bottom, _) => AbsVal::Bottom,
                (AbsVal::Fact(fa), Some(k)) => AbsVal::Fact(ashr_iv(fa, k)),
                _ => opaque(),
            },
            BinOp::LShr => match (env(a), op_const_int(b)) {
                (AbsVal::Bottom, _) => AbsVal::Bottom,
                (AbsVal::Fact(fa), Some(k)) if fa.lo >= 0 && (0..64).contains(&k) => {
                    AbsVal::Fact(ashr_iv(fa, k))
                }
                _ => opaque(),
            },
            BinOp::FAdd => binary(a, b, &|x, y| clamp53(add_iv(x, y))),
            BinOp::FSub => binary(a, b, &|x, y| clamp53(sub_iv(x, y))),
            BinOp::FMul => binary(a, b, &|x, y| clamp53(mul_iv(x, y))),
            BinOp::FDiv => match (env(a), op_const_int(b)) {
                (AbsVal::Bottom, _) => AbsVal::Bottom,
                // exact only when the dividend is provably even and
                // the divisor is 2 — the parity-guarded hot-loop shape.
                (AbsVal::Fact(fa), Some(2)) if is_even(a) => AbsVal::Fact(div_iv(fa, 2)),
                _ => AbsVal::Top,
            },
            BinOp::FRem => match (env(a), op_const_int(b)) {
                (AbsVal::Bottom, _) => AbsVal::Bottom,
                (AbsVal::Fact(fa), Some(d)) if d != 0 => AbsVal::Fact(rem_iv(fa, d)),
                // W3 C3 — variable divisor: a non-negative dividend
                // and a divisor range excluding zero (branch-refined,
                // the `while (b !== 0)` gcd shape) bound the result in
                // [0, max|b|-1]. The fact's presence doubles as the
                // nonzero-divisor certificate fit::def_exact consumes
                // (same channel as the FDiv evenness certificate).
                (AbsVal::Fact(fa), None) if fa.lo >= 0 => match env(b) {
                    AbsVal::Fact(fb)
                        if (fb.lo >= 1 || fb.hi <= -1) && fb.lo > NEG_INF && fb.hi < POS_INF =>
                    {
                        let m = fb.lo.abs().max(fb.hi.abs());
                        AbsVal::Fact(NumFact::new(0, m - 1))
                    }
                    _ => AbsVal::Top,
                },
                _ => AbsVal::Top,
            },
        },
        InstKind::Copy(_, a) | InstKind::Identity(a) => env(a),
        InstKind::SiToFp(a) => match env(a) {
            AbsVal::Fact(f) => AbsVal::Fact(clamp53(f)),
            x => x,
        },
        InstKind::FpToSi(a) => match env(a) {
            // exact when the float fact is inside i64; otherwise the
            // saturation/UB edge makes the bounds unusable.
            AbsVal::Fact(f) if f.lo > I64_MIN && f.hi < I64_MAX => AbsVal::Fact(f),
            AbsVal::Bottom => AbsVal::Bottom,
            _ => opaque(),
        },
        InstKind::ZExtBoolToI64(_) => AbsVal::Fact(NumFact::new(0, 1)),
        InstKind::ZExtI32ToI64(_) => AbsVal::Fact(NumFact::new(0, u32::MAX as i128)),
        _ => opaque(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_arith_corners() {
        let a = NumFact::new(-3, 5);
        let b = NumFact::new(2, 4);
        assert_eq!(add_iv(a, b), NumFact::new(-1, 9));
        assert_eq!(sub_iv(a, b), NumFact::new(-7, 3));
        assert_eq!(mul_iv(a, b), NumFact::new(-12, 20));
        assert_eq!(
            div_iv(NumFact::new(1, 1_000_000), 2),
            NumFact::new(0, 500_000)
        );
        assert_eq!(rem_iv(NumFact::new(0, 100), 2), NumFact::new(0, 1));
        assert_eq!(rem_iv(NumFact::new(-100, 100), 2), NumFact::new(-1, 1));
    }

    #[test]
    fn sentinel_arithmetic_saturates() {
        let unb = NumFact::new(0, POS_INF);
        assert_eq!(mul_iv(unb, NumFact::point(3)).hi, POS_INF);
        assert_eq!(add_iv(unb, NumFact::point(1)).hi, POS_INF);
        assert_eq!(add_iv(unb, NumFact::point(1)).lo, 1);
        // widened bound stays a sentinel after clamping
        assert_eq!(clamp(POS_INF * 3), POS_INF);
    }

    #[test]
    fn clamp53_widens_rounding_range() {
        let f = NumFact::new(0, F64_EXACT + 10);
        assert_eq!(clamp53(f), NumFact::new(0, POS_INF));
        let g = NumFact::new(0, F64_EXACT - 1);
        assert_eq!(clamp53(g), g);
    }

    #[test]
    fn and_with_nonneg_operand_bounds_result() {
        let env = |op: &Operand| match op {
            Operand::Value(v) if v.0 == 0 => AbsVal::Fact(NumFact::new(0, 9_999_999)),
            Operand::Value(_) => AbsVal::Fact(NumFact::new(-1, 9_999_998)),
            _ => AbsVal::Top,
        };
        let kind = InstKind::BinOp(
            BinOp::And,
            Operand::Value(torajs_core::ssa::ValueId(0)),
            Operand::Value(torajs_core::ssa::ValueId(1)),
        );
        let r = eval_inst(&kind, Type::I64, &env, &|_| false);
        assert_eq!(r, AbsVal::Fact(NumFact::new(0, 9_999_999)));
    }

    #[test]
    fn fdiv_needs_even_proof() {
        let env = |_: &Operand| AbsVal::Fact(NumFact::new(1, 1_000_000));
        let kind = InstKind::BinOp(
            BinOp::FDiv,
            Operand::Value(torajs_core::ssa::ValueId(0)),
            Operand::ConstF64(2.0),
        );
        assert_eq!(eval_inst(&kind, Type::F64, &env, &|_| false), AbsVal::Top);
        assert_eq!(
            eval_inst(&kind, Type::F64, &env, &|_| true),
            AbsVal::Fact(NumFact::new(0, 500_000))
        );
    }

    #[test]
    fn const_operand_forms_resolve() {
        assert_eq!(op_const_int(&Operand::ConstI64(2)), Some(2));
        assert_eq!(op_const_int(&Operand::ConstF64(2.0)), Some(2));
        assert_eq!(op_const_int(&Operand::ConstF64(0.5)), None);
        assert_eq!(op_const_int(&Operand::ConstI32(7)), Some(7));
    }
}
