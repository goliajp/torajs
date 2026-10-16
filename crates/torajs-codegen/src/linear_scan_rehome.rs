//! After the sweep: a call-crossing value left in a caller-saved
//! register moves into a callee-saved one when that costs the frame
//! nothing more.
//!
//! The sweep decides callee- against caller-saved one value at a time,
//! in start order. A value whose calls are rare declines a callee-saved
//! register the frame does not save yet and is stored and reloaded
//! around each call instead; a later value often takes that register
//! anyway, so the frame pays its save and the declined value still
//! pays its saves around the calls. Once every home is known, such a
//! value takes a callee-saved register the frame saves already and
//! whose holders are dead wherever it is live, or, when the frame saves
//! an odd number of GPRs, the next one: the save then joins its
//! neighbour's STP / LDP and adds no instruction.

use crate::linear_scan_sweep::{Sweep, slot};
use crate::live_ranges::LiveRanges;
use crate::reg::{Reg, aapcs64};

impl Sweep<'_> {
    pub(crate) fn rehome_saved(&mut self) {
        if self.caller_crossing.is_empty() || !self.share {
            return;
        }
        // per pool register, the union of the ranges of the values in it
        let mut held: Vec<Vec<(u32, u32)>> = vec![Vec::new(); 64];
        for (&v, &reg) in &self.by_value {
            if let Some(k) = slot(reg).filter(|_| is_callee(reg)) {
                held[k] = LiveRanges::union_from(&held[k], self.ranges.of(v), 0);
            }
        }
        let mut kept = Vec::with_capacity(self.caller_crossing.len());
        for vid in std::mem::take(&mut self.caller_crossing) {
            let reg = self.by_value[&vid];
            let mine = self.ranges.of(vid);
            let home = match reg {
                _ if self.unspillable.contains(&vid) || mine.is_empty() => None,
                Reg::Gpr(_) => self.gpr_homes().find(|&r| fits(&held, r, mine)),
                Reg::Fpr(_) => self.fpr_homes().find(|&r| fits(&held, r, mine)),
                Reg::SpillGpr(_) | Reg::SpillFpr(_) => None,
            };
            let Some(home) = home else {
                kept.push(vid);
                continue;
            };
            let k = slot(home).expect("a pool register");
            held[k] = LiveRanges::union_from(&held[k], mine, 0);
            match home {
                Reg::Gpr(g) => self.used_callee_gpr_mask |= 1 << g.idx(),
                Reg::Fpr(f) => self.used_callee_fpr_mask |= 1 << f.idx(),
                Reg::SpillGpr(_) | Reg::SpillFpr(_) => {}
            }
            self.by_value.insert(vid, home);
            self.saved.remove(&vid);
        }
        self.caller_crossing = kept;
    }

    /// The callee-saved GPRs the frame saves, then, when their count is
    /// odd, the first one of the pool it does not.
    fn gpr_homes(&self) -> impl Iterator<Item = Reg> + '_ {
        let mask = self.used_callee_gpr_mask;
        let used = aapcs64::CALLEE_SAVED_SCRATCH
            .iter()
            .filter(move |g| mask & (1 << g.idx()) != 0);
        let pair = (mask.count_ones() % 2 == 1)
            .then(|| {
                self.free_callee_gpr
                    .iter()
                    .find(|g| mask & (1 << g.idx()) == 0)
            })
            .flatten();
        used.chain(pair).map(|&g| Reg::Gpr(g))
    }

    /// The callee-saved FPRs the frame saves (each is saved alone).
    fn fpr_homes(&self) -> impl Iterator<Item = Reg> + '_ {
        let mask = self.used_callee_fpr_mask;
        aapcs64::FP_CALLEE_SAVED_SCRATCH
            .iter()
            .filter(move |f| mask & (1 << f.idx()) != 0)
            .map(|&f| Reg::Fpr(f))
    }
}

fn is_callee(reg: Reg) -> bool {
    match reg {
        Reg::Gpr(g) => aapcs64::CALLEE_SAVED_SCRATCH.contains(&g),
        Reg::Fpr(f) => aapcs64::FP_CALLEE_SAVED_SCRATCH.contains(&f),
        Reg::SpillGpr(_) | Reg::SpillFpr(_) => false,
    }
}

fn fits(held: &[Vec<(u32, u32)>], reg: Reg, mine: &[(u32, u32)]) -> bool {
    slot(reg).is_some_and(|k| !LiveRanges::intersect(&held[k], mine))
}

#[cfg(test)]
mod tests {
    use crate::linear_scan::allocate_linear_scan;
    use crate::reg::{Reg, aapcs64};
    use torajs_core::ssa::{
        BinOp, Block, BlockId, FuncId, Function, IPred, Inst, InstKind, Operand, THROW_ACTIVE_SYM,
        Terminator, Type, ValueId, ValueInfo,
    };

    fn inst(v: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(v)),
            kind,
            origin: None,
        }
    }

    fn val(v: u32) -> Operand {
        Operand::Value(ValueId(v))
    }

    fn call(v: u32) -> Inst {
        inst(v, InstKind::Call(FuncId(0), Vec::new()))
    }

    /// v0 crosses only the call of an arm that raises (cold), so the
    /// sweep saves it around that call rather than open x19; v5 and v6
    /// cross hot calls later and take callee-saved registers anyway.
    /// v0 is dead by then and ends in one of them, saved nowhere.
    #[test]
    fn a_value_saved_around_rare_calls_moves_into_a_saved_register() {
        let func = Function {
            name: "rehome".into(),
            params: Vec::new(),
            ret: Type::I64,
            values: (0..10)
                .map(|_| ValueInfo {
                    ty: Type::I64,
                    name: None,
                })
                .collect(),
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: vec![
                        call(0),
                        inst(1, InstKind::ICmp(IPred::Eq, val(0), Operand::ConstI64(0))),
                    ],
                    term: Terminator::CondBr {
                        cond: val(1),
                        then_blk: BlockId(1),
                        else_blk: BlockId(3),
                    },
                },
                Block {
                    id: BlockId(1),
                    insts: vec![
                        call(2),
                        inst(3, InstKind::GlobalRef(THROW_ACTIVE_SYM.to_string())),
                        inst(4, InstKind::Load(Type::I64, val(3), 0)),
                        inst(9, InstKind::ICmp(IPred::Ne, val(4), Operand::ConstI64(0))),
                    ],
                    term: Terminator::CondBr {
                        cond: val(9),
                        then_blk: BlockId(3),
                        else_blk: BlockId(2),
                    },
                },
                Block {
                    id: BlockId(2),
                    insts: Vec::new(),
                    term: Terminator::Unreachable,
                },
                Block {
                    id: BlockId(3),
                    insts: vec![inst(
                        5,
                        InstKind::BinOp(BinOp::Add, val(0), Operand::ConstI64(1)),
                    )],
                    term: Terminator::Br(BlockId(4)),
                },
                Block {
                    id: BlockId(4),
                    insts: vec![
                        call(6),
                        call(7),
                        inst(8, InstKind::BinOp(BinOp::Add, val(5), val(6))),
                    ],
                    term: Terminator::Ret(Some(val(8))),
                },
            ],
            current_origin: None,
        };
        let a = allocate_linear_scan(&func);
        let Reg::Gpr(g) = a.of(ValueId(0)) else {
            panic!("{:?}", a.of(ValueId(0)));
        };
        assert!(aapcs64::CALLEE_SAVED_SCRATCH.contains(&g), "{g:?}");
        assert!(a.call_saves.values().all(|s| s.is_empty()));
        assert_eq!(a.used_callee_gpr_mask.count_ones(), 2);
    }
}
