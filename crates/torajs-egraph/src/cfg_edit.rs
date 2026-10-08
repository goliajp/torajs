//! The block-level edits a diamond-building pass makes: a fresh block,
//! an instruction appended to one, the renumbering after blocks
//! move, and folding straight-line chains back into one block.
//! `split_block_at` and `push_value` stay in `float_demote`, which
//! owns their tests.

use crate::id_hash::HashMap;

use torajs_core::ssa::{Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type};

use crate::branch_fold::sweep_unreachable_blocks;
use crate::float_demote::cfg::successors;
use crate::float_demote::push_value;

/// An empty block at the end of the function, ending in `unreachable`
/// until the caller wires it.
pub(crate) fn new_block(func: &mut Function) -> BlockId {
    let id = BlockId(func.blocks.len() as u32);
    func.blocks.push(Block {
        id,
        insts: Vec::new(),
        term: Terminator::Unreachable,
    });
    id
}

/// `kind` appended to `block`, defining a fresh value of type `ty`.
pub(crate) fn emit(func: &mut Function, block: BlockId, ty: Type, kind: InstKind) -> Operand {
    let result = push_value(&mut func.values, ty);
    func.blocks[block.0 as usize].insts.push(Inst {
        result: Some(result),
        kind,
        origin: None,
    });
    Operand::Value(result)
}

/// Re-establish `BlockId.0 == position` after moving blocks.
pub(crate) fn renumber(func: &mut Function) {
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

/// Fold every block reached only by an unconditional branch from one
/// other block into that block, then drop the emptied ones. Answers
/// how many were folded.
pub(crate) fn merge_straight_chains(func: &mut Function) -> u32 {
    let mut merged = 0;
    loop {
        let mut preds = vec![0u32; func.blocks.len()];
        for block in &func.blocks {
            for s in successors(&block.term) {
                preds[s.0 as usize] += 1;
            }
        }
        let mut changed = false;
        for b in 0..func.blocks.len() {
            let Terminator::Br(t) = func.blocks[b].term else {
                continue;
            };
            let t = t.0 as usize;
            if t == b || t == 0 || preds[t] != 1 {
                continue;
            }
            let insts = std::mem::take(&mut func.blocks[t].insts);
            let term = std::mem::replace(&mut func.blocks[t].term, Terminator::Unreachable);
            func.blocks[b].insts.extend(insts);
            func.blocks[b].term = term;
            preds[t] = 0;
            merged += 1;
            changed = true;
        }
        if !changed {
            break;
        }
    }
    sweep_unreachable_blocks(func);
    merged
}
