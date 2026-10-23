//! Whether a loop's copy could pay beyond its integer exit tests: the
//! question asked before the copy is made, of the loop as it is.
//!
//! The copy gives the passes after it one thing the loop lacks — facts
//! for the counter compared with the bound and for the values the copy
//! reads as the image of an integer. Beyond the tests themselves it
//! pays only where some instruction of the loop is one those passes
//! rewrite from such a fact. So the values the facts reach are followed
//! through the loop: a copy, a constant step, scale or shift, and the
//! counter's `sitofp` carry them on (the image of a bounded integer is
//! a bounded f64). A constant non-negative mask, a call argument, a
//! boxed or stored value use them without asking anything of them, and
//! the bound's own comparisons are what the exit test serves. So does a
//! remainder by a constant when the facts bound the counter from above
//! only, and f64 arithmetic with a value no fact can bound. Every other
//! use — arithmetic or a comparison in either domain, a division, a sum
//! into a cell of the loop — is one a fact could inform.
//!
//! A constant shift is carried, not counted as a use: the int32 window
//! it would ask for lets sign-extension elision drop a pair per trip,
//! and on the bench's loops that saved at most two instructions an
//! iteration for a copy that cost the build more than it saved.

use crate::id_hash::HashSet;

use torajs_core::ssa::{BinOp, BlockId, Function, IPred, InstKind, Operand, Type, ValueId};

use super::BoundUse;

use crate::float_demote::cfg::term_operand_value;
use crate::rc_peephole::visit_value_operands;

/// Some instruction of `region`, other than the `matched` comparisons
/// of `uses`, does more with the counters and `values` than carry them
/// on or hand them over. When the facts the copy adds bound them from
/// above only — counters compared below the bound, no value read as an
/// image — a remainder by a constant gets the same fact in either loop,
/// its range set by the divisor and the dividend's sign.
pub(super) fn informs(
    func: &Function,
    region: &HashSet<BlockId>,
    uses: &[BoundUse],
    values: &[ValueId],
    matched: &HashSet<(BlockId, usize)>,
) -> bool {
    let from_above = values.is_empty()
        && uses
            .iter()
            .all(|u| matches!(u.pred, IPred::Slt | IPred::Sle));
    let mut reached: HashSet<ValueId> = values.iter().copied().collect();
    reached.extend(uses.iter().filter_map(|u| match u.counter {
        Operand::Value(c) => Some(c),
        _ => None,
    }));
    let mut ask = Ask {
        reached,
        unbounded: unbounded_f64s(func, region),
        from_above,
    };
    loop {
        let before = ask.reached.len();
        for b in region {
            let block = &func.blocks[b.0 as usize];
            for (i, inst) in block.insts.iter().enumerate() {
                if matched.contains(&(*b, i)) {
                    continue;
                }
                let mut reads = false;
                visit_value_operands(&inst.kind, |v| reads |= ask.reached.contains(&v));
                if !reads {
                    continue;
                }
                match ask.use_of(&inst.kind) {
                    Use::Carries => ask.reached.extend(inst.result),
                    Use::Ends => {}
                    Use::Informed => return true,
                }
            }
            if term_operand_value(&block.term).is_some_and(|v| ask.reached.contains(&v)) {
                return true;
            }
        }
        if ask.reached.len() == before {
            return false;
        }
    }
}

enum Use {
    /// the result is bounded where the operand is
    Carries,
    /// nothing a fact could change
    Ends,
    /// a fact could rewrite it
    Informed,
}

struct Ask {
    reached: HashSet<ValueId>,
    unbounded: HashSet<ValueId>,
    from_above: bool,
}

impl Ask {
    /// What `kind`, reading a reached value, does with it.
    fn use_of(&self, kind: &InstKind) -> Use {
        let constant = |o: &Operand| !matches!(o, Operand::Value(_));
        let opaque = |o: &Operand| matches!(o, Operand::Value(v) if self.unbounded.contains(v));
        let reached = |o: &Operand| matches!(o, Operand::Value(v) if self.reached.contains(v));
        match kind {
            InstKind::Copy(..) | InstKind::SiToFp(_) => Use::Carries,
            InstKind::BinOp(
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Shl | BinOp::AShr | BinOp::LShr,
                a,
                c,
            ) if constant(a) || constant(c) => Use::Carries,
            InstKind::BinOp(BinOp::FAdd | BinOp::FSub | BinOp::FMul, a, c)
                if opaque(a) || opaque(c) =>
            {
                Use::Ends
            }
            InstKind::BinOp(BinOp::SRem, x @ Operand::Value(_), Operand::ConstI64(k))
                if self.from_above && *k != 0 && reached(x) =>
            {
                Use::Ends
            }
            InstKind::BinOp(BinOp::And, _, Operand::ConstI64(k))
            | InstKind::BinOp(BinOp::And, Operand::ConstI64(k), _)
                if *k >= 0 =>
            {
                Use::Ends
            }
            InstKind::Call(..)
            | InstKind::CallIndirect(..)
            | InstKind::BitCastF64ToI64(_)
            | InstKind::BoxF64(_)
            | InstKind::BoxInt32(_)
            | InstKind::BoxAny(..) => Use::Ends,
            InstKind::Store(v, addr, _) if reached(v) && !reached(addr) => Use::Ends,
            _ => Use::Informed,
        }
    }
}

/// The f64 values of the loop no fact bounds in either copy: a load, a
/// call's answer, a conversion from a dynamic value, a cell the loop
/// also sets to a NaN, an infinity or a fraction, and the arithmetic
/// and cells reading one.
fn unbounded_f64s(func: &Function, region: &HashSet<BlockId>) -> HashSet<ValueId> {
    let f64_value = |v: ValueId| func.values[v.0 as usize].ty == Type::F64;
    let non_integer = |o: &Operand| match o {
        Operand::ConstF64(x) => !x.is_finite() || x.fract() != 0.0,
        _ => false,
    };
    let mut out: HashSet<ValueId> = HashSet::default();
    loop {
        let before = out.len();
        for b in region {
            for inst in &func.blocks[b.0 as usize].insts {
                let Some(r) = inst.result.filter(|r| f64_value(*r) && !out.contains(r)) else {
                    continue;
                };
                let held = |o: &Operand| matches!(o, Operand::Value(v) if out.contains(v));
                let unbounded = match &inst.kind {
                    InstKind::Load(..)
                    | InstKind::LoadDyn(..)
                    | InstKind::LoadDynScaled8(..)
                    | InstKind::Call(..)
                    | InstKind::CallIndirect(..)
                    | InstKind::AnyToF64(..)
                    | InstKind::BitCastI64ToF64(_) => true,
                    InstKind::Copy(_, o) => non_integer(o) || held(o),
                    InstKind::BinOp(
                        BinOp::FAdd | BinOp::FSub | BinOp::FMul | BinOp::FDiv | BinOp::FRem,
                        a,
                        c,
                    ) => held(a) || held(c),
                    InstKind::Select(_, _, a, c) => held(a) || held(c),
                    _ => false,
                };
                if unbounded {
                    out.insert(r);
                }
            }
        }
        if out.len() == before {
            return out;
        }
    }
}
