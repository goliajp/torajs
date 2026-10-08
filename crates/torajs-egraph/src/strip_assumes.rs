//! Assumptions out of the clones `fn_int_clone` made, once every
//! analysis has read them: the chain of window checks the clone's
//! entry runs, each branching to one shared empty block ending in
//! `unreachable` or on to the next, always takes the next (the failing
//! edge is undefined behaviour, so dropping it is always sound), and
//! the comparisons deciding it die. The same move as LLVM's late
//! removal of branches to `unreachable` after `assume`. Only that
//! chain goes: an `unreachable` the lowering sealed inside the body is
//! where the original traps when an invariant breaks, and the clone
//! keeps it.
//!
//! Before `loop_rotate`; `merge_straight_chains` below sweeps the
//! empty block once nothing reaches it.

use crate::id_hash::{HashMap, HashSet};

use crate::cfg_edit::merge_straight_chains;

use torajs_core::ssa::{
    BlockId, FuncId, Function, InstKind, Module, Operand, Terminator, ValueId,
    rewrite_value_operands, visit_value_operands,
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct StripStats {
    pub assumes_stripped: u32,
    pub blocks_merged: u32,
    /// branches left in the clones that can still take the original
    pub dispatch_checks_left: u32,
    /// calls left in the clones to the function they were cloned from
    pub slow_calls_left: u32,
}

pub(crate) fn strip_assumes(module: &mut Module, clones: &[(FuncId, FuncId)]) -> StripStats {
    let mut stats = StripStats::default();
    for &(orig, clone) in clones {
        let func = &mut module.funcs[clone.0 as usize];
        stats.assumes_stripped += strip_function(func);
        forward_param_copies(func);
        // the splits that built the clone left its straight-line code
        // in blocks scattered to the end of the function
        stats.blocks_merged += merge_straight_chains(func);
        count_dispatch(func, orig, &mut stats);
    }
    stats
}

fn strip_function(func: &mut Function) -> u32 {
    let empty_unreachable = |f: &Function, b: BlockId| {
        let block = &f.blocks[b.0 as usize];
        block.insts.is_empty() && block.term == Terminator::Unreachable
    };
    let mut never: Option<BlockId> = None;
    let mut dead_conds: HashSet<ValueId> = HashSet::default();
    let mut stripped = 0;
    let mut at = BlockId(0);
    let mut seen: HashSet<BlockId> = HashSet::default();
    while seen.insert(at) {
        let Terminator::CondBr {
            cond,
            then_blk,
            else_blk,
        } = func.blocks[at.0 as usize].term
        else {
            break;
        };
        let is_never = |b: BlockId| never.map_or_else(|| empty_unreachable(func, b), |n| n == b);
        let (keep, drop) = match (is_never(then_blk), is_never(else_blk)) {
            (true, false) => (else_blk, then_blk),
            (false, true) => (then_blk, else_blk),
            _ => break,
        };
        never = Some(drop);
        func.blocks[at.0 as usize].term = Terminator::Br(keep);
        if let Operand::Value(c) = cond {
            dead_conds.insert(c);
        }
        stripped += 1;
        at = keep;
    }
    let mut read: HashSet<ValueId> = HashSet::default();
    for block in &func.blocks {
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |v| {
                read.insert(v);
            });
        }
        if let Terminator::CondBr {
            cond: Operand::Value(v),
            ..
        }
        | Terminator::Ret(Some(Operand::Value(v))) = block.term
        {
            read.insert(v);
        }
    }
    for block in func.blocks.iter_mut() {
        block.insts.retain(|inst| {
            !(matches!(inst.kind, InstKind::ICmp(..))
                && inst
                    .result
                    .is_some_and(|r| dead_conds.contains(&r) && !read.contains(&r)))
        });
    }
    stripped
}

/// The copies the parameters were read through carried the asserted
/// window to the analyses; with the assertions gone the body reads
/// the parameters themselves, and the entry block no longer needs a
/// register that outlives the calls (which would keep the base case
/// from running frameless).
fn forward_param_copies(func: &mut Function) {
    let mut defs: HashMap<ValueId, u32> = HashMap::default();
    for inst in func.blocks.iter().flat_map(|b| &b.insts) {
        if let Some(r) = inst.result {
            *defs.entry(r).or_default() += 1;
        }
    }
    let mut map: HashMap<ValueId, ValueId> = HashMap::default();
    for inst in func.blocks.iter().flat_map(|b| &b.insts) {
        if let (Some(r), InstKind::Copy(_, Operand::Value(p))) = (inst.result, &inst.kind)
            && func.params.contains(p)
            && defs[&r] == 1
        {
            map.insert(r, *p);
        }
    }
    for block in func.blocks.iter_mut() {
        block
            .insts
            .retain(|inst| !inst.result.is_some_and(|r| map.contains_key(&r)));
        for inst in block.insts.iter_mut() {
            rewrite_value_operands(&mut inst.kind, |v| map.get(&v).copied().unwrap_or(v));
        }
        match &mut block.term {
            Terminator::CondBr {
                cond: Operand::Value(v),
                ..
            }
            | Terminator::Ret(Some(Operand::Value(v))) => {
                if let Some(p) = map.get(v) {
                    *v = *p;
                }
            }
            _ => {}
        }
    }
}

fn count_dispatch(func: &Function, orig: FuncId, stats: &mut StripStats) {
    let calls_orig: Vec<u32> = func
        .blocks
        .iter()
        .map(|b| {
            b.insts
                .iter()
                .filter(|i| matches!(&i.kind, InstKind::Call(c, _) if *c == orig))
                .count() as u32
        })
        .collect();
    stats.slow_calls_left += calls_orig.iter().sum::<u32>();
    for block in &func.blocks {
        if let Terminator::CondBr {
            then_blk, else_blk, ..
        } = block.term
            && (calls_orig[then_blk.0 as usize] + calls_orig[else_blk.0 as usize] > 0)
        {
            stats.dispatch_checks_left += 1;
        }
    }
}
