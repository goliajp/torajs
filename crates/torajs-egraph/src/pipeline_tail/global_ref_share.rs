//! One address per symbol in a straight run of a block. A module `let`
//! read, changed and written back (`n = n + 1`) takes the symbol's
//! address twice, and codegen folds each into its own access as an
//! `ADRP` plus the page offset. Sharing the first address lets the
//! later access read the page from its register, one `ADRP` fewer.
//!
//! A run ends at an instruction codegen lowers to a call: an address
//! kept across one would need a callee-saved register or a spill,
//! which costs more than the `ADRP` it saves.

use crate::id_hash::HashMap;

use torajs_core::ssa::{
    Function, InstKind, Module, Operand, Terminator, ValueId, lowers_to_call,
    rewrite_value_operands,
};

use crate::float_demote::cfg::multi_def_values;

#[derive(Debug, Default)]
pub struct GlobalRefShareStats {
    /// Addresses replaced by an earlier one of the same symbol.
    pub shared: u32,
}

pub(crate) fn share_global_refs(module: &mut Module) -> GlobalRefShareStats {
    let mut stats = GlobalRefShareStats::default();
    for func in &mut module.funcs {
        stats.shared += share_in(func);
    }
    stats
}

fn share_in(func: &mut Function) -> u32 {
    let multi = multi_def_values(func);
    let mut to: HashMap<ValueId, ValueId> = HashMap::default();
    for block in &mut func.blocks {
        let mut first: HashMap<String, ValueId> = HashMap::default();
        block.insts.retain(|inst| {
            if lowers_to_call(&inst.kind) {
                first.clear();
                return true;
            }
            let (InstKind::GlobalRef(sym), Some(r)) = (&inst.kind, inst.result) else {
                return true;
            };
            if multi.contains(&r) {
                return true;
            }
            match first.get(sym) {
                Some(&f) => {
                    to.insert(r, f);
                    false
                }
                None => {
                    first.insert(sym.clone(), r);
                    true
                }
            }
        });
    }
    if to.is_empty() {
        return 0;
    }
    for block in &mut func.blocks {
        for inst in &mut block.insts {
            rewrite_value_operands(&mut inst.kind, |v| to.get(&v).copied().unwrap_or(v));
        }
        if let Terminator::CondBr {
            cond: Operand::Value(v),
            ..
        }
        | Terminator::Ret(Some(Operand::Value(v))) = &mut block.term
            && let Some(&f) = to.get(v)
        {
            *v = f;
        }
    }
    to.len() as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{BinOp, Block, BlockId, FuncId, Inst, SigId, Type, ValueInfo};

    fn val(r: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(r)),
            kind,
            origin: None,
        }
    }

    fn void(kind: InstKind) -> Inst {
        Inst {
            result: None,
            kind,
            origin: None,
        }
    }

    fn v(n: u32) -> Operand {
        Operand::Value(ValueId(n))
    }

    fn sym(s: &str) -> InstKind {
        InstKind::GlobalRef(s.into())
    }

    fn load(p: u32) -> InstKind {
        InstKind::Load(Type::I64, v(p), 0)
    }

    fn func(blocks: Vec<(Vec<Inst>, Terminator)>) -> Function {
        Function {
            name: "f".into(),
            params: vec![],
            ret: Type::I64,
            blocks: (blocks.into_iter().enumerate())
                .map(|(i, (insts, term))| Block {
                    id: BlockId(i as u32),
                    insts,
                    term,
                })
                .collect(),
            values: (0..16)
                .map(|_| ValueInfo {
                    ty: Type::Ptr,
                    name: None,
                })
                .collect(),
            current_origin: None,
        }
    }

    fn one_block(insts: Vec<Inst>, ret: u32) -> Function {
        func(vec![(insts, Terminator::Ret(Some(v(ret))))])
    }

    /// The `GlobalRef` results left in the function, in order.
    fn refs(f: &Function) -> Vec<u32> {
        (f.blocks.iter().flat_map(|b| &b.insts))
            .filter(|i| matches!(i.kind, InstKind::GlobalRef(_)))
            .filter_map(|i| i.result.map(|r| r.0))
            .collect()
    }

    #[test]
    fn a_later_address_of_the_same_symbol_reads_the_first() {
        let mut f = one_block(
            vec![
                val(0, sym("n")),
                val(1, load(0)),
                val(2, sym("n")),
                void(InstKind::Store(v(1), v(2), 0)),
            ],
            1,
        );
        assert_eq!(share_in(&mut f), 1);
        assert_eq!(refs(&f), vec![0]);
        assert_eq!(f.blocks[0].insts[2].kind, InstKind::Store(v(1), v(0), 0));
    }

    #[test]
    fn every_call_site_ends_the_run() {
        let calls = [
            InstKind::Call(FuncId(0), vec![]),
            InstKind::CallIndirect(SigId(0), v(9), vec![]),
            InstKind::BinOp(BinOp::FRem, v(9), v(9)),
            InstKind::CtpopRangeSum(v(9), v(9), v(9)),
            InstKind::RcDecAny(v(9)),
        ];
        for call in calls {
            let mut f = one_block(
                vec![
                    val(0, sym("n")),
                    val(1, load(0)),
                    val(8, call.clone()),
                    val(2, sym("n")),
                    val(3, load(2)),
                ],
                3,
            );
            assert_eq!(share_in(&mut f), 0, "{call:?}");
            assert_eq!(refs(&f), vec![0, 2], "{call:?}");
        }
    }

    #[test]
    fn a_run_restarts_after_a_call() {
        let mut f = one_block(
            vec![
                val(0, sym("n")),
                void(InstKind::Call(FuncId(0), vec![])),
                val(2, sym("n")),
                val(3, sym("n")),
                val(4, load(3)),
            ],
            4,
        );
        assert_eq!(share_in(&mut f), 1);
        assert_eq!(refs(&f), vec![0, 2]);
        assert_eq!(f.blocks[0].insts[3].kind, load(2));
    }

    #[test]
    fn a_value_defined_twice_takes_no_part() {
        // `1` is defined twice: neither replaced nor a replacement
        let mut f = one_block(
            vec![
                val(1, sym("n")),
                val(1, sym("n")),
                val(2, sym("n")),
                val(3, load(2)),
            ],
            3,
        );
        assert_eq!(share_in(&mut f), 0);
        assert_eq!(refs(&f), vec![1, 1, 2]);
        let mut f = one_block(
            vec![
                val(0, sym("n")),
                val(1, sym("n")),
                val(1, InstKind::Identity(v(0))),
                val(3, load(1)),
            ],
            3,
        );
        assert_eq!(share_in(&mut f), 0);
        assert_eq!(refs(&f), vec![0, 1]);
    }

    #[test]
    fn terminators_and_other_blocks_read_the_first() {
        let mut f = func(vec![
            (
                vec![val(0, sym("n")), val(1, load(0)), val(2, sym("n"))],
                Terminator::CondBr {
                    cond: v(2),
                    then_blk: BlockId(1),
                    else_blk: BlockId(2),
                },
            ),
            (vec![val(3, load(2))], Terminator::Ret(Some(v(3)))),
            (vec![], Terminator::Ret(Some(v(2)))),
        ]);
        assert_eq!(share_in(&mut f), 1);
        assert_eq!(refs(&f), vec![0]);
        assert!(matches!(f.blocks[0].term, Terminator::CondBr { cond, .. } if cond == v(0)));
        assert_eq!(f.blocks[1].insts[0].kind, load(0));
        assert_eq!(f.blocks[2].term, Terminator::Ret(Some(v(0))));
    }

    #[test]
    fn different_symbols_or_blocks_share_nothing() {
        let mut f = one_block(
            vec![
                val(0, sym("n")),
                val(1, sym("m")),
                val(2, load(0)),
                val(3, load(1)),
            ],
            3,
        );
        assert_eq!(share_in(&mut f), 0);
        assert_eq!(refs(&f), vec![0, 1]);
        let mut f = func(vec![
            (vec![val(0, sym("n"))], Terminator::Br(BlockId(1))),
            (
                vec![val(1, sym("n")), val(2, load(1))],
                Terminator::Ret(Some(v(2))),
            ),
        ]);
        assert_eq!(share_in(&mut f), 0);
        assert_eq!(refs(&f), vec![0, 1]);
    }
}
