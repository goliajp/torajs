//! The block-level edits a diamond-building pass makes: a fresh block,
//! an instruction appended to one, and the renumbering after blocks
//! move. `split_block_at` and `push_value` stay in `float_demote`,
//! which owns their tests.

use crate::id_hash::HashMap;

use torajs_core::ssa::{Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type};

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
