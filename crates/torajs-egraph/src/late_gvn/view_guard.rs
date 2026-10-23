//! The receiver guard of an inline element view, decided by the
//! predecessors of the join the view was merged at.
//!
//! `const t = xs[i]` on an array that holds its class in place binds
//! `t` to the element's address, or, for an index past the end, to the
//! generic undefined cell typed as the element. The two arms of the
//! bounds check each define the view by a copy and rejoin; a field
//! read of `t` below the join first tests it against NULL and the
//! cell. Each arm already knows the answer: an element address
//! (`inttoptr` typed `Type::Inline`) lies in the array's data block,
//! which holds no word spelling a missing element, so it is neither
//! NULL nor any static cell; the cell arm holds the cell.
//!
//! This is the edge half of jump threading with the join copied. For a
//! join J whose every predecessor defines the view itself, and whose
//! branch out is a run of those tests on it, the arms that hold an
//! element keep J with its branch set to where the tests send an
//! element; the arms that hold the cell go to one copy of J's
//! instructions that branches to where the tests send the cell. Every
//! path still runs J's instructions once and in order (a store or a
//! call between the read and the field read happens before the
//! TypeError), and the blocks holding only the tests are skipped.
//!
//! The copy gives every value J defines a fresh id, so J's values must
//! be defined once, read only inside J and after their definition: a
//! value with another definition (a local `MEM2REG_PHI` merged) would
//! leave the copy reading an id it has not defined yet, and the
//! original unchanged for the code below. Blocks no edge reaches any
//! more are swept.
//!
//! `TORAJS_VIEW_GUARD_OFF=1` skips, `TORAJS_VIEW_GUARD_STATS=1` counts.

mod scan;
mod test_run;

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    Block, BlockId, Function, IPred, InstKind, Module, Operand, Terminator, Type, ValueId,
    rewrite_value_operands, visit_value_operands,
};

use crate::branch_fold::sweep_unreachable_blocks;
use scan::{defs_and_uses, predecessors};
use test_run::{holds_at_end, tested_view, walk};

const UNDEF_CELL: &str = "___TORAJS_UNDEF_CELL";

/// Bound on test blocks walked from one join.
const MAX_STEPS: usize = 8;
/// Largest join copied, in instructions.
const MAX_JOIN_INSTS: usize = 24;

#[derive(Debug, Default)]
pub struct ViewGuardStats {
    /// Joins whose tests the predecessors decided.
    pub joins: u32,
    /// Joins copied for the predecessors holding the cell.
    pub copies: u32,
    pub blocks_swept: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Holds {
    Elem,
    Cell,
}

pub(crate) fn thread_view_guards(module: &mut Module) -> ViewGuardStats {
    let mut stats = ViewGuardStats::default();
    // a view is an element of an array laid out in place
    if !(module.arr_layouts.iter()).any(|t| matches!(t, Type::Inline(_))) {
        return stats;
    }
    for func in module.funcs.iter_mut() {
        if func.is_declaration() || !tests_a_view(func) {
            continue;
        }
        let before = stats.joins;
        while thread_one(func, &mut stats) {}
        if stats.joins > before {
            stats.blocks_swept += sweep_unreachable_blocks(func);
        }
    }
    stats
}

/// Some block ends on a compare of a view: most functions hold none,
/// and skip the maps.
fn tests_a_view(func: &Function) -> bool {
    let is_view = |o: &Operand| matches!(o, Operand::Value(v) if matches!(func.values[v.0 as usize].ty, Type::Inline(_)));
    func.blocks.iter().any(|b| {
        let Terminator::CondBr {
            cond: Operand::Value(c),
            ..
        } = b.term
        else {
            return false;
        };
        (b.insts.iter().rfind(|i| i.result == Some(c))).is_some_and(|i| {
            matches!(&i.kind, InstKind::ICmp(IPred::Eq | IPred::Ne, x, y) if is_view(x) || is_view(y))
        })
    })
}

/// Thread the first join that qualifies; answers whether one did.
fn thread_one(func: &mut Function, stats: &mut ViewGuardStats) -> bool {
    let (single, uses) = defs_and_uses(func);
    let preds = predecessors(func);
    let facts = Facts {
        func,
        single: &single,
        uses: &uses,
    };
    let found = (preds.iter().enumerate().skip(1))
        .find_map(|(j, from)| plan_join(&facts, from, j).map(|plan| (j, plan)));
    let Some((j, plan)) = found else {
        return false;
    };
    apply(func, j, plan, stats);
    true
}

struct Facts<'a> {
    func: &'a Function,
    /// The defining instruction of every value defined once.
    single: &'a HashMap<ValueId, (usize, usize)>,
    /// Operand occurrences per value, terminators included.
    uses: &'a HashMap<ValueId, u32>,
}

struct Plan {
    /// The compare ending J, dropped from J and from the copy.
    test: usize,
    cell_preds: Vec<usize>,
    elem_to: Option<BlockId>,
    cell_to: BlockId,
}

fn plan_join(facts: &Facts<'_>, preds: &[usize], j: usize) -> Option<Plan> {
    let (test, view) = join_test(facts, preds, j)?;
    let mut cell_preds = Vec::new();
    let mut elem = false;
    for &p in preds {
        match holds_at_end(facts, p, view)? {
            Holds::Cell => cell_preds.push(p),
            Holds::Elem => elem = true,
        }
    }
    let elem_to = if elem {
        Some(walk(facts, j, view, Holds::Elem)?)
    } else {
        None
    };
    let cell_to = walk(facts, j, view, Holds::Cell)?;
    let copied = !cell_preds.is_empty();
    if !copied && elem_to.is_none() || copied && !copyable(facts, &facts.func.blocks[j]) {
        return None;
    }
    Some(Plan {
        test,
        cell_preds,
        elem_to,
        cell_to,
    })
}

/// The compare ending J and the view it tests, when J may be threaded:
/// a small block, not its own predecessor, whose values (the view
/// aside, which its predecessors define) are read only inside it.
fn join_test(facts: &Facts<'_>, preds: &[usize], j: usize) -> Option<(usize, ValueId)> {
    let func = facts.func;
    let block = &func.blocks[j];
    let Terminator::CondBr {
        cond: Operand::Value(c),
        ..
    } = block.term
    else {
        return None;
    };
    let test = block.insts.iter().rposition(|i| i.result == Some(c))?;
    let view = tested_view(facts, &block.insts[test].kind)?;
    if preds.is_empty()
        || preds.contains(&j)
        || block.insts.len() > MAX_JOIN_INSTS
        || facts.uses.get(&c) != Some(&1)
        || !func
            .values
            .get(view.0 as usize)
            .is_some_and(|v| matches!(v.ty, Type::Inline(_)))
    {
        return None;
    }
    let local: HashSet<ValueId> = block.insts.iter().filter_map(|i| i.result).collect();
    (!local.contains(&view) && read_only_inside(func, j, &local)).then_some((test, view))
}

/// J can be copied with fresh ids: no frame slot or suspension point,
/// and every value it defines is defined once, there, before J reads
/// it.
fn copyable(facts: &Facts<'_>, block: &Block) -> bool {
    let local: HashSet<ValueId> = block.insts.iter().filter_map(|i| i.result).collect();
    let mut defined: HashSet<ValueId> = HashSet::default();
    block.insts.iter().all(|inst| {
        let mut early = false;
        visit_value_operands(&inst.kind, |v| {
            early |= local.contains(&v) && !defined.contains(&v)
        });
        let once = inst.result.is_none_or(|r| {
            defined.insert(r);
            facts.single.contains_key(&r)
        });
        let frame = matches!(
            inst.kind,
            InstKind::Alloca(_) | InstKind::AllocaBytes(_) | InstKind::Suspend(..)
        );
        !early && once && !frame
    })
}

/// Every value of `local` is read only by instructions of block `j`.
fn read_only_inside(func: &Function, j: usize, local: &HashSet<ValueId>) -> bool {
    func.blocks.iter().enumerate().all(|(b, block)| {
        b == j || {
            let mut outside = false;
            for inst in &block.insts {
                visit_value_operands(&inst.kind, |v| outside |= local.contains(&v));
            }
            match &block.term {
                Terminator::CondBr {
                    cond: Operand::Value(v),
                    ..
                }
                | Terminator::Ret(Some(Operand::Value(v))) => outside |= local.contains(v),
                _ => {}
            }
            !outside
        }
    })
}

fn apply(func: &mut Function, j: usize, plan: Plan, stats: &mut ViewGuardStats) {
    stats.joins += 1;
    let mut insts = std::mem::take(&mut func.blocks[j].insts);
    insts.remove(plan.test);
    if !plan.cell_preds.is_empty() {
        stats.copies += 1;
        let copy = BlockId(func.blocks.len() as u32);
        let mut fresh: HashMap<ValueId, ValueId> = HashMap::default();
        for r in insts.iter().filter_map(|i| i.result) {
            fresh.insert(r, ValueId(func.values.len() as u32));
            func.values.push(func.values[r.0 as usize].clone());
        }
        let copied = (insts.iter().cloned())
            .map(|mut inst| {
                inst.result = inst.result.map(|r| fresh[&r]);
                rewrite_value_operands(&mut inst.kind, |v| fresh.get(&v).copied().unwrap_or(v));
                inst
            })
            .collect();
        func.blocks.push(Block {
            id: copy,
            insts: copied,
            term: Terminator::Br(plan.cell_to),
        });
        for p in plan.cell_preds {
            retarget(&mut func.blocks[p].term, BlockId(j as u32), copy);
        }
    }
    func.blocks[j].insts = insts;
    // with no arm holding an element J has no way in left; the sweep
    // takes it
    func.blocks[j].term = Terminator::Br(plan.elem_to.unwrap_or(plan.cell_to));
}

fn retarget(term: &mut Terminator, from: BlockId, to: BlockId) {
    match term {
        Terminator::Br(t) if *t == from => *t = to,
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => {
            for t in [then_blk, else_blk] {
                if *t == from {
                    *t = to;
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
