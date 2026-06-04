//! Register allocation.
//!
//! Per RFC D2 the target algorithm is Linear Scan (Poletto & Sarkar
//! 1999). Full LS lands in S2/S3 once arithmetic / mem / cast surface
//! broadens; S1 uses a trivial allocator that's just enough for the
//! `1 + 2` end-to-end fixture:
//!
//!   - Result ValueId of the *last* defining instruction with a
//!     ret-shaped Terminator goes to x0 (AAPCS64 ret register).
//!   - All other ValueIds get a scratch from the AAPCS64 caller-saved
//!     pool (x9, x10, ...), assigned in definition order.
//!
//! Trivial alloc is correct (every SSA value still lives in a distinct
//! register, no false aliasing) but not space-efficient — there's no
//! interval reuse, so we'd run out of regs around ~14 SSA values.
//! That's plenty for S1, fine for the S2 arithmetic surface, and gets
//! replaced by real LS in S3 once Load/Store/Alloca need spill support.

use std::collections::HashMap;
use torajs_core::ssa::{Function, InstKind, Terminator, ValueId};

use crate::reg::{Gpr, aapcs64};

/// Per-function register assignment.
#[derive(Debug, Clone)]
pub struct Assignment {
    /// Map from SSA `ValueId` (debug-stable u32) to the GPR it lives
    /// in for its entire lifetime under trivial alloc.
    by_value: HashMap<u32, Gpr>,
}

impl Assignment {
    /// Lookup register for a `ValueId`. Panics if unallocated — caller
    /// should `allocate()` first.
    pub fn of(&self, vid: ValueId) -> Gpr {
        *self
            .by_value
            .get(&vid.0)
            .unwrap_or_else(|| panic!("ValueId({}) not allocated", vid.0))
    }

    /// `true` if `vid` has a register assigned.
    pub fn contains(&self, vid: ValueId) -> bool {
        self.by_value.contains_key(&vid.0)
    }
}

/// Trivial allocator: assign x0 to the return value, scratch regs
/// (x9+) to everything else, in definition order. Sufficient for S1.
///
/// Returns the allocation. Will panic if more than
/// `aapcs64::CALLER_SAVED_SCRATCH.len()` ValueIds need scratch —
/// S2/S3 land Linear Scan with spill support before that limit
/// becomes load-bearing.
pub fn allocate_trivial(func: &Function) -> Assignment {
    let ret_vid = detect_ret_value(func);
    let mut by_value: HashMap<u32, Gpr> = HashMap::new();
    let mut scratch_idx = 0usize;

    for block in &func.blocks {
        for inst in &block.insts {
            let Some(result) = inst.result else {
                continue; // void inst (e.g. Store)
            };

            let needs_x0 = ret_vid == Some(result);
            // Skip allocating a separate slot for the ret value if we
            // can place it directly in x0 via the def site. (Trivial
            // alloc doesn't do coalescing — every value gets its own
            // reg — but the ret-x0 placement is a textbook simple win
            // and matches what the byte-equal acceptance test expects.)
            let gpr = if needs_x0 && !matches_zero_arg_kind(&inst.kind) {
                Gpr::X0
            } else {
                let scratch = aapcs64::CALLER_SAVED_SCRATCH[scratch_idx];
                scratch_idx += 1;
                scratch
            };
            by_value.insert(result.0, gpr);
        }
    }

    Assignment { by_value }
}

/// Find the ValueId returned by the function, if any. S1 assumes a
/// single-block function with a `Ret(Some(Value(_)))` terminator.
fn detect_ret_value(func: &Function) -> Option<ValueId> {
    for block in &func.blocks {
        if let Terminator::Ret(Some(op)) = &block.term
            && let torajs_core::ssa::Operand::Value(v) = op
        {
            return Some(*v);
        }
    }
    None
}

/// True if this InstKind takes no operands (currently nothing in our
/// supported surface; placeholder for InstKind variants that S2+ add
/// where def-to-x0 placement is invalid).
fn matches_zero_arg_kind(_kind: &InstKind) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{
        BinOp, Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId,
        ValueInfo,
    };

    fn build_one_plus_two() -> Function {
        let v0 = ValueId(0);
        Function {
            name: "one_plus_two".into(),
            params: Vec::new(),
            ret: Type::I64,
            values: vec![ValueInfo {
                ty: Type::I64,
                name: Some("v0".into()),
            }],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![Inst {
                    result: Some(v0),
                    kind: InstKind::BinOp(BinOp::Add, Operand::ConstI64(1), Operand::ConstI64(2)),
                    origin: None,
                }],
                term: Terminator::Ret(Some(Operand::Value(v0))),
            }],
            current_origin: None,
        }
    }

    #[test]
    fn one_plus_two_allocates_v0_to_x0() {
        let func = build_one_plus_two();
        let alloc = allocate_trivial(&func);
        let v0 = ValueId(0);
        assert_eq!(alloc.of(v0), Gpr::X0);
        assert!(alloc.contains(v0));
    }
}
