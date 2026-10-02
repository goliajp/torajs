//! Guarded integer remainder over an opaque f64 dividend — a `frem`
//! whose divisor is a positive integral constant `C ≤ 2^53` and whose
//! dividend nothing is known about (a global's load, a call result,
//! a product) splits into a diamond: a check that the dividend is an
//! integer in `[0, 2^53)`, an `srem` arm for when it is, and the
//! `frem` as it was for everything else.
//!
//! float_demote and frem_int turn a `frem` into `srem` where the facts
//! prove the dividend exact and non-negative; what they leave is the
//! `%` of a number that came in through `parseInt(argv)` or a global
//! slot (the LCG `seed % 6` of the string-generating bench cells),
//! and each one costs a libm `fmod` call per evaluation — 19% of the
//! tokenizer cell's wall clock in a 2026-10-02 sample.
//!
//! The check is the roundtrip test: `fptosi` the dividend, `sitofp`
//! it back, compare the two bit patterns. NaN, -0, a fraction, an
//! infinity and anything past 2^63 all come back different. What
//! survives is an integer; the window `t >> 53 == 0` then keeps only
//! `0 ≤ t < 2^53`, where the conversion was exact, `fmod` and the
//! i64 remainder agree bit for bit, and the result converts back
//! without rounding. A negative integer takes the slow arm so that
//! `-6 % 3` stays `-0`.
//!
//! The result keeps its ValueId and becomes a two-def copy cell, one
//! def per arm, the shape φ destruction leaves; readers are untouched.
//! The new blocks go in right after the head — window, fast arm, the
//! rest of the block — with the slow arm at the end of the function,
//! so the passing path falls through and only the slow arm pays a
//! taken branch.
//!
//! Runs in `fact_folds` right after frem_int. `TORAJS_FREM_GUARD_OFF=1`
//! skips, `TORAJS_FREM_GUARD_STATS=1` counts.

use std::collections::HashMap;

use torajs_core::ssa::{
    BinOp, Block, BlockId, Function, IPred, Inst, InstKind, Module, Operand, Terminator, Type,
    ValueId,
};

use crate::float_demote::{push_value, split_block_at};
use crate::frem_int::{positive_int_const, single_def_kinds};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FremGuardStats {
    /// frem sites split into a check / srem / frem diamond
    pub sites: u32,
}

pub fn guard_frems(module: &mut Module) -> FremGuardStats {
    let mut stats = FremGuardStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        stats.sites += guard_function(func);
    }
    stats
}

/// One matched `frem`: where it sits, what it reads, what it defines.
struct Site {
    at: usize,
    dividend: ValueId,
    divisor: i64,
    result: ValueId,
}

pub(crate) fn guard_function(func: &mut Function) -> u32 {
    let defs = single_def_kinds(func);
    let mut sites = 0;
    let mut bi = 0;
    // a split puts the rest of the block three positions on and the
    // slow arm at the end, so the scan reaches both; the slow arm's
    // frem defines a fresh value, which `defs` does not know, and is
    // passed over
    while bi < func.blocks.len() {
        if let Some(site) = find_site(func, bi, &defs) {
            guard_site(func, BlockId(bi as u32), &site);
            sites += 1;
        }
        bi += 1;
    }
    sites
}

/// The first `frem` of the block the pass applies to: a single-def
/// result, an f64 dividend not defined by a `sitofp` (frem_int's
/// case), a positive integral constant divisor.
fn find_site(func: &Function, bi: usize, defs: &HashMap<ValueId, InstKind>) -> Option<Site> {
    func.blocks[bi]
        .insts
        .iter()
        .enumerate()
        .find_map(|(at, inst)| {
            let (Some(result), InstKind::BinOp(BinOp::FRem, Operand::Value(dividend), b)) =
                (inst.result, &inst.kind)
            else {
                return None;
            };
            if !defs.contains_key(&result) || func.values[dividend.0 as usize].ty != Type::F64 {
                return None;
            }
            if matches!(defs.get(dividend), Some(InstKind::SiToFp(_))) {
                return None;
            }
            let divisor = positive_int_const(b, defs)?;
            Some(Site {
                at,
                dividend: *dividend,
                divisor,
                result,
            })
        })
}

fn new_block(func: &mut Function) -> BlockId {
    let id = BlockId(func.blocks.len() as u32);
    func.blocks.push(Block {
        id,
        insts: Vec::new(),
        term: Terminator::Unreachable,
    });
    id
}

fn emit(func: &mut Function, block: BlockId, ty: Type, kind: InstKind) -> Operand {
    let result = push_value(&mut func.values, ty);
    func.blocks[block.0 as usize].insts.push(Inst {
        result: Some(result),
        kind,
        origin: None,
    });
    Operand::Value(result)
}

/// head: roundtrip check → slow | window
/// window: `t >> 53 != 0` → slow | fast
/// fast: `r = sitofp (srem t, C)` → join
/// slow: `r = frem a, b` as it was → join
fn guard_site(func: &mut Function, head: BlockId, site: &Site) {
    let window = new_block(func);
    let fast = new_block(func);
    let join = split_block_at(func, head, &[site.at])[0];
    let slow = new_block(func);
    let a = Operand::Value(site.dividend);

    let mut frem = func.blocks[join.0 as usize].insts.remove(0);
    let origin = frem.origin;
    let fr = push_value(&mut func.values, Type::F64);
    frem.result = Some(fr);
    let slow_block = &mut func.blocks[slow.0 as usize];
    slow_block.insts.push(frem);
    slow_block.insts.push(Inst {
        result: Some(site.result),
        kind: InstKind::Copy(Type::F64, Operand::Value(fr)),
        origin,
    });
    slow_block.term = Terminator::Br(join);

    let t = emit(func, head, Type::I64, InstKind::FpToSi(a));
    let back = emit(func, head, Type::F64, InstKind::SiToFp(t));
    let bits = emit(func, head, Type::I64, InstKind::BitCastF64ToI64(a));
    let back_bits = emit(func, head, Type::I64, InstKind::BitCastF64ToI64(back));
    let differs = emit(
        func,
        head,
        Type::Bool,
        InstKind::ICmp(IPred::Ne, bits, back_bits),
    );
    func.blocks[head.0 as usize].term = Terminator::CondBr {
        cond: differs,
        then_blk: slow,
        else_blk: window,
    };

    let high = emit(
        func,
        window,
        Type::I64,
        InstKind::BinOp(BinOp::LShr, t, Operand::ConstI64(53)),
    );
    let outside = emit(
        func,
        window,
        Type::Bool,
        InstKind::ICmp(IPred::Ne, high, Operand::ConstI64(0)),
    );
    func.blocks[window.0 as usize].term = Terminator::CondBr {
        cond: outside,
        then_blk: slow,
        else_blk: fast,
    };

    let s = emit(
        func,
        fast,
        Type::I64,
        InstKind::BinOp(BinOp::SRem, t, Operand::ConstI64(site.divisor)),
    );
    let f = emit(func, fast, Type::F64, InstKind::SiToFp(s));
    func.blocks[fast.0 as usize].insts.push(Inst {
        result: Some(site.result),
        kind: InstKind::Copy(Type::F64, f),
        origin,
    });
    func.blocks[fast.0 as usize].term = Terminator::Br(join);

    // window · fast · join follow the head; the slow arm stays last
    let tail = &mut func.blocks[head.0 as usize + 1..];
    tail.rotate_right(4);
    tail[3..].rotate_left(1);
    renumber(func);
}

/// Re-establish `BlockId.0 == position` after moving blocks.
fn renumber(func: &mut Function) {
    let remap: HashMap<BlockId, BlockId> = func
        .blocks
        .iter()
        .enumerate()
        .map(|(pos, b)| (b.id, BlockId(pos as u32)))
        .collect();
    for block in func.blocks.iter_mut() {
        block.id = remap[&block.id];
        match &mut block.term {
            Terminator::Br(t) => *t = remap[t],
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => {
                *then_blk = remap[then_blk];
                *else_blk = remap[else_blk];
            }
            Terminator::Ret(_) | Terminator::Unreachable => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::ValueInfo;

    fn inst(r: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(r)),
            kind,
            origin: None,
        }
    }
    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    /// `%1 = fmul %0, 48271; %2 = frem %1, C; %3 = fadd %2, 1` then a
    /// second block the first falls into
    fn frem_fn(divisor: Operand) -> Function {
        let ty = |t| ValueInfo { ty: t, name: None };
        Function {
            name: "f".into(),
            params: vec![ValueId(0)],
            ret: Type::F64,
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: vec![
                        inst(
                            1,
                            InstKind::BinOp(BinOp::FMul, v(0), Operand::ConstF64(48271.0)),
                        ),
                        inst(2, InstKind::BinOp(BinOp::FRem, v(1), divisor)),
                        inst(
                            3,
                            InstKind::BinOp(BinOp::FAdd, v(2), Operand::ConstF64(1.0)),
                        ),
                    ],
                    term: Terminator::Br(BlockId(1)),
                },
                Block {
                    id: BlockId(1),
                    insts: vec![],
                    term: Terminator::Ret(Some(v(3))),
                },
            ],
            values: vec![ty(Type::F64), ty(Type::F64), ty(Type::F64), ty(Type::F64)],
            current_origin: None,
        }
    }

    fn defs_of(func: &Function, r: ValueId) -> Vec<(BlockId, InstKind)> {
        func.blocks
            .iter()
            .flat_map(|b| b.insts.iter().map(move |i| (b.id, i)))
            .filter(|(_, i)| i.result == Some(r))
            .map(|(b, i)| (b, i.kind.clone()))
            .collect()
    }

    fn cond_targets(term: &Terminator) -> (BlockId, BlockId) {
        let Terminator::CondBr {
            then_blk, else_blk, ..
        } = term
        else {
            panic!("expected a conditional, got {term:?}");
        };
        (*then_blk, *else_blk)
    }

    #[test]
    fn an_opaque_dividend_splits_into_a_diamond() {
        let mut f = frem_fn(Operand::ConstF64(2147483647.0));
        assert_eq!(guard_function(&mut f), 1);
        // head · window · fast · join · ret · slow, ids by position
        assert_eq!(f.blocks.len(), 6);
        for (i, b) in f.blocks.iter().enumerate() {
            assert_eq!(b.id.0 as usize, i);
        }
        let (head, window, fast, join, ret, slow) = (
            BlockId(0),
            BlockId(1),
            BlockId(2),
            BlockId(3),
            BlockId(4),
            BlockId(5),
        );
        assert_eq!(cond_targets(&f.blocks[0].term), (slow, window));
        assert_eq!(cond_targets(&f.blocks[1].term), (slow, fast));
        assert!(matches!(f.blocks[2].term, Terminator::Br(b) if b == join));
        assert!(matches!(f.blocks[3].term, Terminator::Br(b) if b == ret));
        assert!(matches!(f.blocks[4].term, Terminator::Ret(_)));
        assert!(matches!(f.blocks[5].term, Terminator::Br(b) if b == join));
        // the head keeps the product and grows the five check insts
        assert_eq!(f.blocks[head.0 as usize].insts.len(), 6);
        assert!(matches!(
            f.blocks[head.0 as usize].insts[1].kind,
            InstKind::FpToSi(Operand::Value(ValueId(1)))
        ));
        // the window shifts the truncated value by 53
        assert!(matches!(
            f.blocks[window.0 as usize].insts[0].kind,
            InstKind::BinOp(BinOp::LShr, _, Operand::ConstI64(53))
        ));
        // the fast arm takes the integer remainder by the same constant
        assert!(matches!(
            f.blocks[fast.0 as usize].insts[0].kind,
            InstKind::BinOp(BinOp::SRem, _, Operand::ConstI64(2147483647))
        ));
        // the result is defined once per arm, both as f64 copies
        let defs = defs_of(&f, ValueId(2));
        assert_eq!(defs.len(), 2);
        assert!(
            defs.iter()
                .all(|(_, k)| matches!(k, InstKind::Copy(Type::F64, _)))
        );
        assert_eq!(defs[0].0, fast);
        assert_eq!(defs[1].0, slow);
        // the slow arm holds the frem as it was, on a fresh value
        let slow_insts = &f.blocks[slow.0 as usize].insts;
        assert!(matches!(
            slow_insts[0].kind,
            InstKind::BinOp(
                BinOp::FRem,
                Operand::Value(ValueId(1)),
                Operand::ConstF64(_)
            )
        ));
        assert_ne!(slow_insts[0].result, Some(ValueId(2)));
        // the reader moved to the join untouched
        assert!(matches!(
            f.blocks[join.0 as usize].insts[0].kind,
            InstKind::BinOp(BinOp::FAdd, Operand::Value(ValueId(2)), _)
        ));
    }

    #[test]
    fn a_sitofp_dividend_is_frem_ints_case() {
        let mut f = frem_fn(Operand::ConstF64(6.0));
        f.blocks[0].insts[0].kind = InstKind::SiToFp(v(0));
        f.values[0].ty = Type::I64;
        assert_eq!(guard_function(&mut f), 0);
        assert_eq!(f.blocks.len(), 2);
    }

    #[test]
    fn a_variable_divisor_is_left_alone() {
        let mut f = frem_fn(v(0));
        assert_eq!(guard_function(&mut f), 0);
        assert_eq!(f.blocks.len(), 2);
    }

    #[test]
    fn a_negative_zero_or_fractional_divisor_is_left_alone() {
        for c in [-3.0, 0.0, 2.5, 9007199254740994.0] {
            let mut f = frem_fn(Operand::ConstF64(c));
            assert_eq!(guard_function(&mut f), 0, "divisor {c}");
        }
    }

    #[test]
    fn a_multi_def_result_is_left_alone() {
        let mut f = frem_fn(Operand::ConstF64(6.0));
        f.blocks[1]
            .insts
            .push(inst(2, InstKind::Copy(Type::F64, Operand::ConstF64(0.0))));
        assert_eq!(guard_function(&mut f), 0);
        assert_eq!(f.blocks.len(), 2);
    }

    #[test]
    fn two_sites_in_one_block_both_split() {
        let mut f = frem_fn(Operand::ConstF64(6.0));
        f.values.push(ValueInfo {
            ty: Type::F64,
            name: None,
        });
        f.blocks[0].insts.push(inst(
            4,
            InstKind::BinOp(BinOp::FRem, v(3), Operand::ConstF64(1000.0)),
        ));
        f.blocks[1].term = Terminator::Ret(Some(v(4)));
        assert_eq!(guard_function(&mut f), 2);
        assert_eq!(f.blocks.len(), 10);
        for (i, b) in f.blocks.iter().enumerate() {
            assert_eq!(b.id.0 as usize, i);
        }
        assert_eq!(defs_of(&f, ValueId(2)).len(), 2);
        assert_eq!(defs_of(&f, ValueId(4)).len(), 2);
        // both slow arms sit at the end, both joins before the ret
        assert!(matches!(
            f.blocks[8].insts[0].kind,
            InstKind::BinOp(BinOp::FRem, ..)
        ));
        assert!(matches!(
            f.blocks[9].insts[0].kind,
            InstKind::BinOp(BinOp::FRem, ..)
        ));
        assert!(matches!(f.blocks[7].term, Terminator::Ret(_)));
    }
}
