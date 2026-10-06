//! Address facts of frame cells, and the control flow they decide —
//! the part of LLVM's InstCombine / SimplifyCFG that runs between
//! SROA rounds. heap_to_stack turns a fresh `obj_alloc` into an
//! `alloca_bytes`, but the inlined code still reaches the cell through
//! `id` copies and still asks of it what it asked of a heap pointer:
//! the release's `p == null || p == UNDEF_CELL`, and behind it the
//! refcount and flag diamonds. The copies make the slot passes treat
//! the cell as escaped, and the compares keep the release reachable.
//!
//! * an `id` whose operand is (a copy of) an `alloca` / `alloca_bytes`
//!   is that value: its uses read the alloca directly
//! * `icmp eq|ne` of a frame address against null or a `global_ref`
//!   folds: a frame slot is never null and never a global. Only the
//!   alloca itself (through copies) qualifies; a pointer loaded from a
//!   slot may be null, and two pointers are never compared here
//! * `and` / `or` / `xor` of two constant booleans folds
//! * a conditional branch on a constant, or with one arm an empty
//!   block that jumps to the other arm, becomes a plain branch; blocks
//!   the entry no longer reaches are emptied
//! * pure values nothing reads any more go: compares, boolean logic,
//!   copies, `global_ref`, loads from a frame cell or a global
//!
//! Runs first in each of the rounds that follow heap_to_stack over a
//! function it promoted a cell in, before the slot passes it feeds.
//! `TORAJS_FRAME_CELL_CLEAN_OFF=1` skips (bisect gate).

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{BinOp, Function, IPred, InstKind, Operand, Terminator, ValueId};

use crate::fresh_cell::fold::{detach_unreachable, used_values};
use crate::slot_forward::{resolve, rewrite_operands};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FrameCellCleanStats {
    /// `id` copies of a frame address replaced by the address.
    pub copies_resolved: u32,
    /// Compares and boolean ops folded to constants.
    pub values_folded: u32,
    /// Conditional branches turned into plain branches.
    pub branches_folded: u32,
    /// Dead pure instructions removed.
    pub insts_removed: u32,
}

pub(crate) fn clean_in_function(func: &mut Function, stats: &mut FrameCellCleanStats) {
    let roots: HashSet<ValueId> = func
        .blocks
        .iter()
        .flat_map(|b| &b.insts)
        .filter(|i| matches!(i.kind, InstKind::Alloca(_) | InstKind::AllocaBytes(_)))
        .filter_map(|i| i.result)
        .collect();
    if roots.is_empty() {
        return;
    }
    let globals: HashSet<ValueId> = func
        .blocks
        .iter()
        .flat_map(|b| &b.insts)
        .filter(|i| matches!(i.kind, InstKind::GlobalRef(_)))
        .filter_map(|i| i.result)
        .collect();
    let replace = plan(func, &roots, &globals, stats);
    if !replace.is_empty() {
        for block in func.blocks.iter_mut() {
            block
                .insts
                .retain(|i| !i.result.is_some_and(|r| replace.contains_key(&r)));
            for inst in block.insts.iter_mut() {
                rewrite_operands(&mut inst.kind, &replace);
            }
            match &mut block.term {
                Terminator::Ret(Some(op)) => *op = resolve(op, &replace),
                Terminator::CondBr { cond, .. } => *cond = resolve(cond, &replace),
                _ => {}
            }
        }
    }
    if fold_branches(func, stats) {
        detach_unreachable(func);
    }
    sweep_dead(func, &roots, &globals, stats);
}

/// Copies to resolve and values to fold, iterated until stable: a
/// fold's operands may be defined later in the layout than the fold.
fn plan(
    func: &Function,
    roots: &HashSet<ValueId>,
    globals: &HashSet<ValueId>,
    stats: &mut FrameCellCleanStats,
) -> HashMap<ValueId, Operand> {
    let mut replace: HashMap<ValueId, Operand> = HashMap::default();
    let is_root = |op: &Operand, replace: &HashMap<ValueId, Operand>| {
        let op = resolve(op, replace);
        matches!(op, Operand::Value(v) if roots.contains(&v))
    };
    let not_frame = |op: &Operand, replace: &HashMap<ValueId, Operand>| match resolve(op, replace) {
        Operand::ConstPtrNull | Operand::ConstI64(0) => true,
        Operand::Value(v) => globals.contains(&v),
        _ => false,
    };
    loop {
        let before = replace.len();
        for inst in func.blocks.iter().flat_map(|b| &b.insts) {
            let Some(r) = inst.result else { continue };
            if replace.contains_key(&r) {
                continue;
            }
            let to = match &inst.kind {
                InstKind::Identity(x) if is_root(x, &replace) => {
                    stats.copies_resolved += 1;
                    resolve(x, &replace)
                }
                InstKind::ICmp(p @ (IPred::Eq | IPred::Ne), a, b)
                    if (is_root(a, &replace) && not_frame(b, &replace))
                        || (is_root(b, &replace) && not_frame(a, &replace)) =>
                {
                    stats.values_folded += 1;
                    Operand::ConstBool(*p == IPred::Ne)
                }
                InstKind::BinOp(op @ (BinOp::And | BinOp::Or | BinOp::Xor), a, b) => {
                    let (Operand::ConstBool(x), Operand::ConstBool(y)) =
                        (resolve(a, &replace), resolve(b, &replace))
                    else {
                        continue;
                    };
                    stats.values_folded += 1;
                    Operand::ConstBool(match op {
                        BinOp::And => x & y,
                        BinOp::Or => x | y,
                        _ => x ^ y,
                    })
                }
                _ => continue,
            };
            replace.insert(r, to);
        }
        if replace.len() == before {
            return replace;
        }
    }
}

/// Turn decided conditional branches into plain ones; `true` when any
/// did.
fn fold_branches(func: &mut Function, stats: &mut FrameCellCleanStats) -> bool {
    let empty_to: Vec<Option<usize>> = func
        .blocks
        .iter()
        .map(|b| match b.term {
            Terminator::Br(t) if b.insts.is_empty() => Some(t.0 as usize),
            _ => None,
        })
        .collect();
    let mut folded = false;
    for block in func.blocks.iter_mut() {
        let Terminator::CondBr {
            cond,
            then_blk,
            else_blk,
        } = block.term
        else {
            continue;
        };
        let (t, e) = (then_blk.0 as usize, else_blk.0 as usize);
        let to = match cond {
            Operand::ConstBool(k) => Some(if k { then_blk } else { else_blk }),
            _ if t == e || empty_to[t] == Some(e) => Some(else_blk),
            _ if empty_to[e] == Some(t) => Some(then_blk),
            _ => None,
        };
        if let Some(to) = to {
            block.term = Terminator::Br(to);
            stats.branches_folded += 1;
            folded = true;
        }
    }
    folded
}

/// Remove pure instructions whose results nothing reads, until none
/// is left.
fn sweep_dead(
    func: &mut Function,
    roots: &HashSet<ValueId>,
    globals: &HashSet<ValueId>,
    stats: &mut FrameCellCleanStats,
) {
    loop {
        let used = used_values(func);
        let mut removed = 0;
        for block in func.blocks.iter_mut() {
            block.insts.retain(|inst| {
                let pure = match &inst.kind {
                    InstKind::ICmp(..)
                    | InstKind::BinOp(BinOp::And | BinOp::Or | BinOp::Xor, ..)
                    | InstKind::Identity(_)
                    | InstKind::GlobalRef(_) => true,
                    InstKind::Load(_, Operand::Value(p), _) => {
                        roots.contains(p) || globals.contains(p)
                    }
                    _ => false,
                };
                let dead = pure && inst.result.is_some_and(|r| !used.contains(&r));
                removed += dead as u32;
                !dead
            });
        }
        if removed == 0 {
            return;
        }
        stats.insts_removed += removed;
    }
}

#[cfg(test)]
mod tests;
