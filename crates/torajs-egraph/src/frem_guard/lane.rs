//! How one `frem` operand reaches the integer remainder, and what has
//! to be checked at run time before it may.
//!
//! The fast arm wants `a` in `[0, 2^53)` and `b` in `[1, 2^53]`, both
//! exact integers. Inside those windows `fmod` and the i64 remainder
//! agree bit for bit and the remainder converts back without rounding;
//! `a ≥ 0` keeps the `-0` of `-6 % 3` out, `b ≥ 1` keeps out the NaN
//! of `x % 0`. Each check is spelled as an i64 that is zero exactly
//! when the operand is inside its window, so the head ORs them all and
//! takes one branch.

use std::collections::HashMap;

use torajs_core::ssa::{BinOp, InstKind, Operand, Type, ValueId};

use crate::interval::NumFact;

const F64_EXACT: i128 = 1 << 53;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Role {
    /// window `[0, 2^53)`
    Dividend,
    /// window `[1, 2^53]`
    Divisor,
}

impl Role {
    fn lo(self) -> i128 {
        match self {
            Role::Dividend => 0,
            Role::Divisor => 1,
        }
    }
    fn fits(self, f: &NumFact) -> bool {
        f.lo >= self.lo() && f.hi < F64_EXACT + self.lo()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Lane {
    /// an integral constant inside the window — nothing to check
    Const(i64),
    /// `sitofp x` of an i64; `checked` when the facts leave the window
    /// open
    Int { x: ValueId, checked: bool },
    /// an f64 with no integer def: truncate, then check the roundtrip
    /// and the window — or, when the facts already prove an integer
    /// inside the window, only what they cannot say: the sign bit of
    /// a dividend (a fact does not exclude `-0`)
    Float { a: ValueId, proven: bool },
    /// already lowered elsewhere (see `hoist`): the integer form and
    /// its check
    Ready { t: Operand, check: Option<Operand> },
}

/// `None`: the operand is never inside its window (a constant outside
/// it), so the site keeps its `frem`.
pub(super) fn classify(
    op: &Operand,
    role: Role,
    defs: &HashMap<ValueId, InstKind>,
    facts: &HashMap<ValueId, NumFact>,
    stable: &dyn Fn(ValueId, ValueId) -> bool,
) -> Option<Lane> {
    let in_window = |c: f64| {
        let lo = role.lo() as f64;
        c >= lo && c < (F64_EXACT + role.lo()) as f64 && c.fract() == 0.0
    };
    match op {
        Operand::ConstF64(c) => (in_window(*c) && !(*c == 0.0 && c.is_sign_negative()))
            .then_some(Lane::Const(*c as i64)),
        Operand::Value(v) => match defs.get(v) {
            Some(InstKind::Copy(Type::F64, inner @ Operand::ConstF64(_))) => {
                classify(inner, role, defs, facts, stable)
            }
            Some(InstKind::SiToFp(Operand::ConstI64(c))) => {
                in_window(*c as f64).then_some(Lane::Const(*c))
            }
            // `x` must still hold what the `sitofp` read (a loop cell
            // may be redefined in between)
            Some(InstKind::SiToFp(Operand::Value(x))) if stable(*v, *x) => Some(Lane::Int {
                x: *x,
                checked: !facts.get(x).is_some_and(|f| role.fits(f)),
            }),
            _ => Some(Lane::Float {
                a: *v,
                proven: facts.get(v).is_some_and(|f| role.fits(f)),
            }),
        },
        _ => None,
    }
}

impl Lane {
    /// Nothing about this operand needs a run-time check.
    pub(super) fn free(&self, role: Role) -> bool {
        match self {
            Lane::Const(_) => true,
            Lane::Int { checked, .. } => !checked,
            Lane::Float { proven, .. } => *proven && role == Role::Divisor,
            Lane::Ready { check, .. } => check.is_none(),
        }
    }

    /// Emit the operand's integer form and, unless [`Lane::free`], an
    /// i64 that is zero exactly when it is inside the window.
    pub(super) fn lower(
        &self,
        role: Role,
        emit: &mut impl FnMut(Type, InstKind) -> Operand,
    ) -> (Operand, Option<Operand>) {
        // `t >> 53` is zero for `t ∈ [0, 2^53)`; the divisor shifts its
        // window down by one first, so 0 and every negative wrap high
        let window = |emit: &mut dyn FnMut(Type, InstKind) -> Operand, t: Operand| {
            let base = match role {
                Role::Dividend => t,
                Role::Divisor => emit(
                    Type::I64,
                    InstKind::BinOp(BinOp::Sub, t, Operand::ConstI64(1)),
                ),
            };
            emit(
                Type::I64,
                InstKind::BinOp(BinOp::LShr, base, Operand::ConstI64(53)),
            )
        };
        match *self {
            Lane::Const(c) => (Operand::ConstI64(c), None),
            Lane::Ready { t, check } => (t, check),
            Lane::Int { x, checked } => {
                let t = Operand::Value(x);
                (t, checked.then(|| window(&mut *emit, t)))
            }
            Lane::Float { a, proven } => {
                let a = Operand::Value(a);
                let t = emit(Type::I64, InstKind::FpToSi(a));
                if proven {
                    let check = (role == Role::Dividend).then(|| {
                        let bits = emit(Type::I64, InstKind::BitCastF64ToI64(a));
                        emit(
                            Type::I64,
                            InstKind::BinOp(BinOp::LShr, bits, Operand::ConstI64(63)),
                        )
                    });
                    return (t, check);
                }
                // NaN, ±0's sign, a fraction, an infinity and anything
                // past 2^63 all come back with other bits; 2^63 itself
                // saturates to a value the window rejects
                let back = emit(Type::F64, InstKind::SiToFp(t));
                let bits = emit(Type::I64, InstKind::BitCastF64ToI64(a));
                let back_bits = emit(Type::I64, InstKind::BitCastF64ToI64(back));
                let diff = emit(Type::I64, InstKind::BinOp(BinOp::Xor, bits, back_bits));
                let win = window(&mut *emit, t);
                let bad = emit(Type::I64, InstKind::BinOp(BinOp::Or, diff, win));
                (t, Some(bad))
            }
        }
    }
}
