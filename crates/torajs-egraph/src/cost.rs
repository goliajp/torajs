//! Apple Silicon M1/M2 (aarch64) cycle-cost model — the table the
//! elaboration phase queries to decide inline / hoist / extract.
//!
//! Day-one latency-aware (RFC 20260609-torajs-codegen-optimizer §9 Q3:
//! "indication-count-only is reject"). Per-opcode cycle counts derive
//! from the Apple Silicon Software Optimization Guide (Apple M1) §3
//! Integer/FP execution-unit latencies, cross-checked against Cranelift's
//! `cranelift/codegen/src/egraph/cost.rs` numeric ordering (where the
//! design rationale is shared even though the numeric units differ).
//!
//! `Cost` is a saturating `u32` — saturates well below `u32::MAX` so
//! cumulative cost over a function never wraps (Cranelift's same
//! invariant). Loop-nest scaling multiplies by depth in elaboration:
//! a body at depth `d` is roughly `BR_PRED_FACTOR^d` more expensive
//! than the same body at depth 0 (modelling expected iteration count).

use torajs_core::ssa::{BinOp, InstKind, Terminator};

/// Cost units. Saturating `u32`. Numeric value is "abstract cycles" — the
/// Apple Silicon M1 cycle estimate normalised so simple integer-ALU
/// operations cost 1. Saturation cap at `Cost::HUGE` (`u32::MAX / 16`)
/// keeps headroom for loop-depth multiplication (BR_PRED_FACTOR^d) without
/// hitting u32::MAX even at unreasonable nesting depths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cost(u32);

impl Cost {
    /// Cost-zero — for ops with no machine-code emission cost (Alloca
    /// is folded into frame prologue, type casts that vanish after
    /// regalloc, etc.).
    pub const ZERO: Cost = Cost(0);
    /// Cap. `add` and `mul` saturate here so subsequent operations stay
    /// non-overflowing even with high loop-nest weighting.
    pub const HUGE: Cost = Cost(u32::MAX / 16);

    /// Construct a cost from a raw cycle count. Clamped to `HUGE`.
    pub const fn new(cycles: u32) -> Self {
        if cycles >= Self::HUGE.0 {
            Self::HUGE
        } else {
            Self(cycles)
        }
    }

    /// Raw cycle estimate.
    pub const fn as_u32(self) -> u32 {
        self.0
    }

    /// Saturating add. Both arguments and the result stay ≤ `HUGE`.
    pub const fn add(self, other: Cost) -> Cost {
        let sum = self.0.saturating_add(other.0);
        if sum >= Self::HUGE.0 {
            Self::HUGE
        } else {
            Self(sum)
        }
    }

    /// Saturating multiply by a small factor (typically a loop iteration
    /// estimate). Result clamped to `HUGE`.
    pub const fn mul(self, factor: u32) -> Cost {
        let prod = self.0.saturating_mul(factor);
        if prod >= Self::HUGE.0 {
            Self::HUGE
        } else {
            Self(prod)
        }
    }

    /// Scale by loop-nest depth — models the expected-iteration-count
    /// multiplier elaboration uses to decide LICM (hoist if hoisted
    /// position is cheaper). Factor BR_PRED_FACTOR per level mirrors
    /// Cranelift's same heuristic; tuned via bench (BR_PRED_FACTOR = 10
    /// matches "loop body runs 10x preheader" rule of thumb).
    pub const fn scale_for_depth(self, depth: u32) -> Cost {
        let mut acc = self;
        let mut i = 0;
        while i < depth {
            acc = acc.mul(BR_PRED_FACTOR);
            i += 1;
        }
        acc
    }
}

/// Loop-iteration multiplier per nest level — see `Cost::scale_for_depth`.
const BR_PRED_FACTOR: u32 = 10;

// Per-opcode Apple Silicon M1/M2 cycle estimates (Apple Silicon Software
// Optimization Guide §3 + measured pipeline depth for in-order arrival).

const ALU_COST: Cost = Cost::new(1); // add/sub/and/or/xor/shift/cmp/cset/mov
const MUL_COST: Cost = Cost::new(3); // imul i32/i64 — 3-cycle M1 multiplier
const DIV_COST: Cost = Cost::new(10); // sdiv/udiv — variable 6-13, midpoint
const LOAD_COST: Cost = Cost::new(4); // L1 hit; L2/L3 misses go beyond model
const STORE_COST: Cost = Cost::new(1); // store-buffer write, retires later
const BRANCH_COST: Cost = Cost::new(1); // predicted taken/not-taken
const CALL_COST: Cost = Cost::new(2); // bl/blr frame overhead — excludes callee
const FALU_COST: Cost = Cost::new(2); // fadd/fsub/fcmp
const FMUL_COST: Cost = Cost::new(4); // fmul d_d_d
const FDIV_COST: Cost = Cost::new(15); // fdiv d_d_d — measured 13-22
const CAST_COST: Cost = Cost::new(4); // sitofp/fptosi cross-pipeline cost
const ALLOCA_COST: Cost = Cost::ZERO; // folded into frame prologue at codegen

/// Static cost of executing one `InstKind` (excludes callee body for
/// `Call`; the elaboration / inliner combines that separately based on
/// estimated callee size).
pub fn cost_of_kind(kind: &InstKind) -> Cost {
    match kind {
        // Integer ALU family. Mul / Div are pulled out because they
        // occupy different execution units with very different latency.
        InstKind::BinOp(op, _, _) => match op {
            BinOp::Mul => MUL_COST,
            BinOp::SDiv | BinOp::SRem => DIV_COST,
            BinOp::FAdd | BinOp::FSub => FALU_COST,
            BinOp::FMul => FMUL_COST,
            BinOp::FDiv | BinOp::FRem => FDIV_COST,
            // Everything else integer-ALU: add/sub/and/or/xor/shift.
            _ => ALU_COST,
        },

        // Comparisons retire on the integer ALU + cset.
        InstKind::ICmp(_, _, _) => ALU_COST,
        InstKind::FCmp(_, _, _) => FALU_COST,

        // Call frame overhead only — callee body cost lives elsewhere.
        InstKind::Call(_, _) => CALL_COST,

        // Stack adjust folded into prologue.
        InstKind::Alloca(_) | InstKind::AllocaBytes(_) => ALLOCA_COST,

        // Memory ops.
        InstKind::Load(_, _, _)
        | InstKind::LoadDyn(_, _, _)
        | InstKind::LoadDynScaled8(_, _, _)
        | InstKind::LoadU8Dyn(_, _) => LOAD_COST,
        InstKind::Store(_, _, _)
        | InstKind::StoreDyn(_, _, _)
        | InstKind::StoreDynScaled8(_, _, _) => STORE_COST,

        // Cross-pipeline numeric casts.
        InstKind::SiToFp(_) | InstKind::FpToSi(_) => CAST_COST,

        // Bit-width zero-extends collapse to a `mov` on aarch64.
        InstKind::ZExtBoolToI64(_) | InstKind::ZExtI32ToI64(_) => ALU_COST,

        // `Identity(op)` is dropped by the elaborator before codegen —
        // no machine code is ever emitted for it. Elaboration scores
        // it as a zero-cost alias so the cost model picks it whenever
        // it appears in an e-class.
        InstKind::Identity(_) => Cost::ZERO,

        // `Neg(op)` lowers to a single ALU op (aarch64
        // `sub Xd, XZR, Xn`; LLVM `sub i64 0, %x`).
        InstKind::Neg(_) => ALU_COST,

        // `Ctpop(op)` lowers to a 4-instruction GPR→SIMD→GPR round
        // trip (`fmov; cnt; addv; fmov`).
        InstKind::Ctpop(_) => Cost::new(4),

        // `Select` lowers to `cmp #0; csel` — csel itself is a
        // single-cycle ALU op (the cmp is often fused away when the
        // cond is a block-local single-use ICmp).
        InstKind::Select(_, _, _, _) => ALU_COST,

        // `CtpopRangeSum` emits an entire canned SIMD reduction loop
        // (~60 instructions, trip-dependent runtime). Elaboration
        // never scores it (formation runs after the egraph pass);
        // the explicit arm documents the order-of-magnitude so a
        // future cost consumer doesn't treat it as one ALU op.
        InstKind::CtpopRangeSum(_, _, _) => Cost::new(64),

        // Anything not enumerated above defaults to ALU cost. Adding a
        // new `InstKind` variant without updating this match is fine
        // (defensive default), but the variant author should add an
        // explicit arm if the cost differs from a single-cycle ALU op.
        _ => ALU_COST,
    }
}

/// Static cost of executing a block `Terminator`. Branches retire on the
/// branch unit; `Ret` is a single-instruction return; `Unreachable` lowers
/// to no machine code (or a trap, treated as zero for elaboration since
/// the path is dead anyway).
pub fn cost_of_terminator(term: &Terminator) -> Cost {
    match term {
        Terminator::Br(_) | Terminator::CondBr { .. } => BRANCH_COST,
        Terminator::Ret(_) => ALU_COST,
        Terminator::Unreachable => Cost::ZERO,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_arithmetic_saturates() {
        assert_eq!(Cost::new(0).as_u32(), 0);
        assert_eq!(Cost::new(100).add(Cost::new(50)).as_u32(), 150);
        // Saturation: adding HUGE to anything stays HUGE.
        assert_eq!(Cost::HUGE.add(Cost::new(1)), Cost::HUGE);
        // mul saturates.
        assert_eq!(Cost::new(u32::MAX / 8).mul(2), Cost::HUGE);
    }

    #[test]
    fn loop_depth_scales_exponentially() {
        let body = Cost::new(5);
        assert_eq!(body.scale_for_depth(0), body);
        assert_eq!(body.scale_for_depth(1), Cost::new(50));
        assert_eq!(body.scale_for_depth(2), Cost::new(500));
        // Deep nest saturates at HUGE rather than wrapping.
        assert_eq!(body.scale_for_depth(20), Cost::HUGE);
    }

    #[test]
    fn opcode_costs_match_pipeline_units() {
        use torajs_core::ssa::{IPred, Operand, Type};
        let dummy = Operand::ConstI64(0);

        // Integer ALU and shifts are 1 cycle.
        assert_eq!(
            cost_of_kind(&InstKind::BinOp(BinOp::Add, dummy, dummy)),
            ALU_COST
        );
        assert_eq!(
            cost_of_kind(&InstKind::BinOp(BinOp::Shl, dummy, dummy)),
            ALU_COST
        );

        // Mul is 3, sdiv is 10, fadd is 2, fmul is 4, fdiv is 15.
        assert_eq!(
            cost_of_kind(&InstKind::BinOp(BinOp::Mul, dummy, dummy)),
            MUL_COST
        );
        assert_eq!(
            cost_of_kind(&InstKind::BinOp(BinOp::SDiv, dummy, dummy)),
            DIV_COST
        );
        assert_eq!(
            cost_of_kind(&InstKind::BinOp(BinOp::FAdd, dummy, dummy)),
            FALU_COST
        );
        assert_eq!(
            cost_of_kind(&InstKind::BinOp(BinOp::FMul, dummy, dummy)),
            FMUL_COST
        );
        assert_eq!(
            cost_of_kind(&InstKind::BinOp(BinOp::FDiv, dummy, dummy)),
            FDIV_COST
        );

        // Memory + compare + call families.
        assert_eq!(
            cost_of_kind(&InstKind::Load(Type::I64, dummy, 0)),
            LOAD_COST
        );
        assert_eq!(cost_of_kind(&InstKind::Store(dummy, dummy, 0)), STORE_COST);
        assert_eq!(
            cost_of_kind(&InstKind::ICmp(IPred::Eq, dummy, dummy)),
            ALU_COST
        );
        assert_eq!(
            cost_of_kind(&InstKind::Call(torajs_core::ssa::FuncId(0), vec![])),
            CALL_COST
        );

        // Alloca folds into prologue.
        assert_eq!(cost_of_kind(&InstKind::Alloca(Type::I64)), Cost::ZERO);
        assert_eq!(cost_of_kind(&InstKind::AllocaBytes(32)), Cost::ZERO);

        // Casts cross the integer↔fp pipeline.
        assert_eq!(cost_of_kind(&InstKind::SiToFp(dummy)), CAST_COST);
        assert_eq!(cost_of_kind(&InstKind::FpToSi(dummy)), CAST_COST);
    }

    #[test]
    fn terminator_costs_classify_by_unit() {
        use torajs_core::ssa::{BlockId, Operand};
        assert_eq!(cost_of_terminator(&Terminator::Br(BlockId(0))), BRANCH_COST);
        assert_eq!(
            cost_of_terminator(&Terminator::CondBr {
                cond: Operand::ConstBool(true),
                then_blk: BlockId(0),
                else_blk: BlockId(1),
            }),
            BRANCH_COST
        );
        assert_eq!(cost_of_terminator(&Terminator::Ret(None)), ALU_COST);
        assert_eq!(cost_of_terminator(&Terminator::Unreachable), Cost::ZERO);
    }

    #[test]
    fn cost_ordering_supports_extraction() {
        // Extraction picks the lowest-cost representation of an eclass;
        // verify the ordering relation matches the numeric expectation.
        assert!(Cost::ZERO < ALU_COST);
        assert!(ALU_COST < MUL_COST);
        assert!(MUL_COST < DIV_COST);
        assert!(LOAD_COST > STORE_COST);
        assert!(FDIV_COST > FMUL_COST);
        assert!(FMUL_COST > FALU_COST);
    }
}
