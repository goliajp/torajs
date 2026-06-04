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
use torajs_core::ssa::{Function, Terminator, Type, ValueId};

use crate::reg::{Fpr, Gpr, Reg, aapcs64};

/// Per-function register assignment.
#[derive(Debug, Clone)]
pub struct Assignment {
    /// Map from SSA `ValueId` (debug-stable u32) to the register class
    /// + slot it lives in for its entire lifetime under trivial alloc.
    by_value: HashMap<u32, Reg>,
}

impl Assignment {
    /// Lookup the register for a `ValueId`. Panics if unallocated.
    pub fn of(&self, vid: ValueId) -> Reg {
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

/// Trivial allocator: F64 values go to FPR slots, everything else to
/// GPR slots. The function's return value (if any) is placed in the
/// AAPCS64 return slot (x0 for int/ptr, v0/d0 for f64); the rest get
/// caller-saved scratch in definition order.
///
/// Will panic if more than `CALLER_SAVED_SCRATCH.len()` GPR ValueIds
/// or `FP_CALLER_SAVED_SCRATCH.len()` FPR ValueIds need scratch — S3
/// lands Linear Scan with spill support before that limit becomes
/// load-bearing.
pub fn allocate_trivial(func: &Function) -> Assignment {
    let ret_vid = detect_ret_value(func);
    let mut by_value: HashMap<u32, Reg> = HashMap::new();
    let mut gpr_idx = 0usize;
    let mut fpr_idx = 0usize;

    for block in &func.blocks {
        for inst in &block.insts {
            let Some(result) = inst.result else {
                continue; // void inst (e.g. Store)
            };

            let ty = func
                .values
                .get(result.0 as usize)
                .map(|vi| &vi.ty)
                .expect("ValueId out of bounds");
            let is_fp = matches!(ty, Type::F64);
            let is_ret = ret_vid == Some(result);

            let reg = if is_ret {
                if is_fp {
                    Reg::Fpr(aapcs64::FP_ARG_RET[0])
                } else {
                    Reg::Gpr(aapcs64::ARG_RET[0])
                }
            } else if is_fp {
                let f = aapcs64::FP_CALLER_SAVED_SCRATCH[fpr_idx];
                fpr_idx += 1;
                Reg::Fpr(f)
            } else {
                let g = aapcs64::CALLER_SAVED_SCRATCH[gpr_idx];
                gpr_idx += 1;
                Reg::Gpr(g)
            };
            by_value.insert(result.0, reg);
        }
    }

    Assignment { by_value }
}

/// Find the ValueId returned by the function, if any. Assumes a
/// single-block function with a `Ret(Some(Value(_)))` terminator
/// (S1 surface — branches land in S5).
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

/// Backwards-compat helper for callers that know the value is in a
/// GPR slot. Cleaner than `.as_gpr()` callsites all over emit_inst.
#[allow(dead_code)]
pub(crate) fn require_gpr(reg: Reg) -> Gpr {
    reg.as_gpr()
}

/// Same for FPR.
#[allow(dead_code)]
pub(crate) fn require_fpr(reg: Reg) -> Fpr {
    reg.as_fpr()
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
        assert_eq!(alloc.of(v0), Reg::Gpr(Gpr::X0));
        assert!(alloc.contains(v0));
    }

    #[test]
    fn f64_ret_value_goes_to_v0() {
        let v0 = ValueId(0);
        let func = Function {
            name: "one_point_five_plus_two_point_five".into(),
            params: Vec::new(),
            ret: Type::F64,
            values: vec![ValueInfo {
                ty: Type::F64,
                name: Some("v0".into()),
            }],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![Inst {
                    result: Some(v0),
                    kind: InstKind::BinOp(
                        BinOp::FAdd,
                        Operand::ConstF64(1.5),
                        Operand::ConstF64(2.5),
                    ),
                    origin: None,
                }],
                term: Terminator::Ret(Some(Operand::Value(v0))),
            }],
            current_origin: None,
        };
        let alloc = allocate_trivial(&func);
        assert_eq!(alloc.of(v0), Reg::Fpr(Fpr::V0));
    }
}
