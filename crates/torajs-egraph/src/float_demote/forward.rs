//! A converted compare reads through the copies demotion leaves behind.
//!
//! `fcmp (sitofp a), (sitofp b)` over two demoted conversions becomes
//! `icmp %ca, %cb` with `%ca = copy a`, `%cb = copy b` in front of it.
//! Each copy costs a move, and loop rotation leaves a header holding
//! one alone, so the compare reads `a` and `b` themselves: `a` when it
//! is single-def (its def dominates the copy), or when the compare sits
//! after the copy in the same block with no def of `a` between. A copy
//! nothing reads any more goes.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{Function, InstKind, Operand, Type, ValueId};

use super::cfg::{multi_def_values, term_operand_value};
use crate::rc_peephole::visit_value_operands;

/// Forward the demoted single-def copies in `set` into the compares
/// reading them; returns how many operands moved.
pub(super) fn forward_into_icmps(func: &mut Function, set: &HashSet<ValueId>) -> u32 {
    let multi = multi_def_values(func);
    // demoted single-def copy -> (block, index, source)
    let mut copies: HashMap<ValueId, (usize, usize, ValueId)> = HashMap::new();
    for (bi, block) in func.blocks.iter().enumerate() {
        for (ii, inst) in block.insts.iter().enumerate() {
            if let (Some(r), InstKind::Copy(Type::I64, Operand::Value(s))) =
                (inst.result, &inst.kind)
                && set.contains(&r)
                && !multi.contains(&r)
            {
                copies.insert(r, (bi, ii, *s));
            }
        }
    }
    let mut moved = 0;
    for bi in 0..func.blocks.len() {
        for ii in 0..func.blocks[bi].insts.len() {
            let InstKind::ICmp(p, a, b) = func.blocks[bi].insts[ii].kind else {
                continue;
            };
            let mut fwd = |op: Operand| match op {
                Operand::Value(c) => match copies.get(&c) {
                    Some(&(cb, ci, s)) if source_holds(func, &multi, (cb, ci), (bi, ii), s) => {
                        moved += 1;
                        Operand::Value(s)
                    }
                    _ => op,
                },
                _ => op,
            };
            let (a, b) = (fwd(a), fwd(b));
            func.blocks[bi].insts[ii].kind = InstKind::ICmp(p, a, b);
        }
    }
    drop_unread(func, &copies);
    moved
}

/// Whether `s`, copied at `copy`, still holds its copied value at `at`.
fn source_holds(
    func: &Function,
    multi: &HashSet<ValueId>,
    copy: (usize, usize),
    at: (usize, usize),
    s: ValueId,
) -> bool {
    // a compare above its copy in one block reads the copy's value from
    // the previous iteration, which `s` no longer holds
    if copy.0 == at.0 && copy.1 > at.1 {
        return false;
    }
    if !multi.contains(&s) {
        return true;
    }
    copy.0 == at.0
        && copy.1 < at.1
        && !func.blocks[at.0].insts[copy.1 + 1..at.1]
            .iter()
            .any(|i| i.result == Some(s))
}

/// Remove the copies in `copies` that nothing reads.
fn drop_unread(func: &mut Function, copies: &HashMap<ValueId, (usize, usize, ValueId)>) {
    let mut read: HashSet<ValueId> = HashSet::new();
    for block in &func.blocks {
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |v| {
                read.insert(v);
            });
        }
        read.extend(term_operand_value(&block.term));
    }
    for block in &mut func.blocks {
        block.insts.retain(|i| {
            i.result
                .is_none_or(|r| !copies.contains_key(&r) || read.contains(&r))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{BinOp, Block, BlockId, IPred, Inst, Terminator, ValueInfo};

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

    /// `%0` a cell set twice, `%1 = mul %0, %0`, the demoted copies
    /// `%2 = copy %1`, `%3 = copy %0`, then `%4 = icmp sle %2, %3`, with
    /// `between` placed after the copies.
    fn header(between: Vec<Inst>) -> Function {
        let mut insts = vec![
            inst(0, InstKind::Copy(Type::I64, Operand::ConstI64(7))),
            inst(1, InstKind::BinOp(BinOp::Mul, v(0), v(0))),
            inst(2, InstKind::Copy(Type::I64, v(1))),
            inst(3, InstKind::Copy(Type::I64, v(0))),
        ];
        insts.extend(between);
        insts.push(inst(4, InstKind::ICmp(IPred::Sle, v(2), v(3))));
        insts.push(inst(0, InstKind::Copy(Type::I64, Operand::ConstI64(8))));
        Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Bool,
            blocks: vec![Block {
                id: BlockId(0),
                insts,
                term: Terminator::Ret(Some(v(4))),
            }],
            values: (0..5)
                .map(|i| ValueInfo {
                    ty: if i == 4 { Type::Bool } else { Type::I64 },
                    name: None,
                })
                .collect(),
            current_origin: None,
        }
    }

    fn set() -> HashSet<ValueId> {
        [ValueId(2), ValueId(3)].into_iter().collect()
    }

    #[test]
    fn the_compare_reads_the_copied_values_and_the_copies_go() {
        let mut f = header(vec![]);
        assert_eq!(forward_into_icmps(&mut f, &set()), 2);
        let kinds: Vec<_> = f.blocks[0].insts.iter().map(|i| i.kind.clone()).collect();
        assert_eq!(kinds.len(), 4);
        assert!(matches!(kinds[2], InstKind::ICmp(IPred::Sle, a, b) if a == v(1) && b == v(0)));
    }

    #[test]
    fn a_cell_redefined_after_its_copy_is_not_read_past_it() {
        let redef = inst(0, InstKind::Copy(Type::I64, Operand::ConstI64(9)));
        let mut f = header(vec![redef]);
        assert_eq!(forward_into_icmps(&mut f, &set()), 1);
        let cmp = f.blocks[0]
            .insts
            .iter()
            .find(|i| i.result == Some(ValueId(4)))
            .unwrap();
        assert!(matches!(cmp.kind, InstKind::ICmp(_, a, b) if a == v(1) && b == v(3)));
        // the copy the compare still reads stays
        assert!(
            f.blocks[0]
                .insts
                .iter()
                .any(|i| i.result == Some(ValueId(3)))
        );
        assert!(
            !f.blocks[0]
                .insts
                .iter()
                .any(|i| i.result == Some(ValueId(2)))
        );
    }

    #[test]
    fn a_compare_above_its_copy_keeps_reading_the_copy() {
        let mut f = header(vec![]);
        // `%4 = icmp %2, %3` ahead of `%2 = copy %1`: on a back edge %2 is
        // the previous iteration's square, which %1 no longer holds
        let insts = &mut f.blocks[0].insts;
        let cmp = insts.remove(4);
        insts.insert(2, cmp);
        assert_eq!(
            forward_into_icmps(&mut f, &[ValueId(2)].into_iter().collect()),
            0
        );
        let cmp = f.blocks[0]
            .insts
            .iter()
            .find(|i| i.result == Some(ValueId(4)))
            .unwrap();
        assert!(matches!(cmp.kind, InstKind::ICmp(_, a, _) if a == v(2)));
    }
}
