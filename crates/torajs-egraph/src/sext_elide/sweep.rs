//! The DCE that follows the sext-pair rewrites.

use crate::id_hash::HashSet;

use torajs_core::ssa::{BinOp, Function, InstKind, Operand, Terminator, ValueId};

use super::SextElideStats;
use crate::rc_peephole::visit_value_operands;

/// DCE — orphaned pure shifts and sitofps left behind by the
/// rewrites. A dead ashr frees its shl on the next round; loop
/// until stable.
pub(super) fn sweep_dead(func: &mut Function, stats: &mut SextElideStats) {
    loop {
        let mut used: HashSet<ValueId> = HashSet::default();
        for block in &func.blocks {
            for inst in &block.insts {
                visit_value_operands(&inst.kind, |v| {
                    used.insert(v);
                });
            }
            match &block.term {
                Terminator::Ret(Some(Operand::Value(v))) => {
                    used.insert(*v);
                }
                Terminator::CondBr {
                    cond: Operand::Value(v),
                    ..
                } => {
                    used.insert(*v);
                }
                _ => {}
            }
        }
        let mut removed_shifts = 0u32;
        let mut removed_casts = 0u32;
        for block in func.blocks.iter_mut() {
            block.insts.retain(|inst| {
                let Some(r) = inst.result else {
                    return true;
                };
                if used.contains(&r) {
                    return true;
                }
                match inst.kind {
                    InstKind::BinOp(BinOp::Shl | BinOp::AShr, _, _) => {
                        removed_shifts += 1;
                        false
                    }
                    InstKind::SiToFp(_) => {
                        removed_casts += 1;
                        false
                    }
                    _ => true,
                }
            });
        }
        if removed_shifts + removed_casts == 0 {
            break;
        }
        stats.dead_shifts_removed += removed_shifts;
        stats.dead_casts_removed += removed_casts;
    }
}
