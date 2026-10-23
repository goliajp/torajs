//! The run of tests below a join, and what each arm's definition of
//! the view makes them answer.

use torajs_core::ssa::{Block, BlockId, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId};

use super::{Facts, Holds, MAX_STEPS, UNDEF_CELL};

/// The view a test `icmp eq|ne view, NULL|cell` reads.
pub(super) fn tested_view(facts: &Facts<'_>, kind: &InstKind) -> Option<ValueId> {
    let InstKind::ICmp(IPred::Eq | IPred::Ne, a, b) = kind else {
        return None;
    };
    let other = |o: &Operand| match o {
        Operand::ConstPtrNull => true,
        Operand::Value(g) => is_cell_ref(facts, *g),
        _ => false,
    };
    match (a, b) {
        (Operand::Value(v), o) | (o, Operand::Value(v))
            if other(o) && !other(&Operand::Value(*v)) =>
        {
            Some(*v)
        }
        _ => None,
    }
}

fn is_cell_ref(facts: &Facts<'_>, g: ValueId) -> bool {
    facts.single.get(&g).is_some_and(|&(b, i)| {
        matches!(&facts.func.blocks[b].insts[i].kind, InstKind::GlobalRef(s) if s == UNDEF_CELL)
    })
}

/// What the last definition of `view` in block `p` puts in it, when
/// `p` defines it at all.
pub(super) fn holds_at_end(facts: &Facts<'_>, p: usize, view: ValueId) -> Option<Holds> {
    let def = facts.func.blocks[p]
        .insts
        .iter()
        .rev()
        .find(|i| i.result == Some(view))?;
    let InstKind::Copy(Type::Inline(_), Operand::Value(src)) = def.kind else {
        return None;
    };
    if is_cell_ref(facts, src) {
        return Some(Holds::Cell);
    }
    let &(b, i) = facts.single.get(&src)?;
    let inst = &facts.func.blocks[b].insts[i];
    let typed_elem = matches!(facts.func.values[src.0 as usize].ty, Type::Inline(_));
    (matches!(inst.kind, InstKind::IntToPtr(_)) && typed_elem).then_some(Holds::Elem)
}

/// Where J's branch leads while the view holds `holds`, through the
/// blocks that hold nothing but a test of it.
pub(super) fn walk(facts: &Facts<'_>, j: usize, view: ValueId, holds: Holds) -> Option<BlockId> {
    let func = facts.func;
    let mut cur = j;
    for step in 0..MAX_STEPS {
        let block = &func.blocks[cur];
        let Terminator::CondBr {
            cond: Operand::Value(c),
            then_blk,
            else_blk,
        } = block.term
        else {
            return (step > 0).then_some(BlockId(cur as u32));
        };
        let test = if step == 0 {
            block.insts.iter().rfind(|i| i.result == Some(c))
        } else {
            only_test(facts, block, c)
        };
        let Some(taken) = test.and_then(|t| decide(facts, &t.kind, view, holds)) else {
            return (step > 0).then_some(BlockId(cur as u32));
        };
        if facts.uses.get(&c) != Some(&1) {
            return (step > 0).then_some(BlockId(cur as u32));
        }
        let next = if taken { then_blk } else { else_blk };
        if next.0 as usize == j || next.0 as usize == cur {
            return (step > 0).then_some(BlockId(cur as u32));
        }
        cur = next.0 as usize;
    }
    Some(BlockId(cur as u32))
}

/// The compare of a block holding only address constants of the cell
/// read by that compare, and the compare ending it.
fn only_test<'a>(facts: &Facts<'_>, block: &'a Block, c: ValueId) -> Option<&'a Inst> {
    let (last, rest) = block.insts.split_last()?;
    let refs_only = rest.iter().all(|i| {
        i.result
            .is_some_and(|r| is_cell_ref(facts, r) && facts.uses.get(&r) == Some(&1))
    });
    (last.result == Some(c) && refs_only).then_some(last)
}

fn decide(facts: &Facts<'_>, kind: &InstKind, view: ValueId, holds: Holds) -> Option<bool> {
    let InstKind::ICmp(pred, a, b) = kind else {
        return None;
    };
    let other = match (a, b) {
        (Operand::Value(v), o) | (o, Operand::Value(v)) if *v == view => o,
        _ => return None,
    };
    let equal = match other {
        Operand::ConstPtrNull => false,
        Operand::Value(g) if is_cell_ref(facts, *g) => holds == Holds::Cell,
        _ => return None,
    };
    match pred {
        IPred::Eq => Some(equal),
        IPred::Ne => Some(!equal),
        _ => None,
    }
}
