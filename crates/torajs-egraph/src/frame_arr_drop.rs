//! Releases of array literals the escape verifier kept in the frame.
//!
//! Such a literal is an `alloca_bytes` cell whose header lowering
//! writes itself: `FLAG_STATIC_LITERAL`, no props bag, the data
//! pointer aiming at the cell's own slots (`cell + 40`). Its release,
//! `__torajs_arr_drop_scalar`, frees only what a grow or a hole may
//! have attached since — a spilled data buffer and a props bag — and
//! puts the cell back in that born shape. Lowering emits it like any
//! array release: behind `p == null || p == UNDEF_CELL`, through a
//! call, although on almost every exit the cell still has its born
//! shape and the call does nothing.
//!
//! Here the call moves behind an inline test of that shape,
//! `(props | (data ^ (cell + 40))) != 0`, the two words being adjacent
//! in the header; then the nullish compares on the cell fold
//! (`frame_cell_clean`): a frame address is neither null nor a global.
//!
//! `TORAJS_FRAME_ARR_DROP_OFF=1` skips, `TORAJS_FRAME_ARR_DROP_STATS=1`
//! counts.

use crate::id_hash::HashSet;

use torajs_core::ssa::{
    BinOp, BlockId, FuncId, Function, IPred, InstKind, Module, Operand, Terminator, Type, ValueId,
};

use crate::cfg_edit::{emit, new_block};
use crate::frame_cell_clean::{FrameCellCleanStats, clean_in_function};
use crate::interval::elem_header::{ARR_CELL_SIZE, ARR_PROPS_OFF};
use crate::load_licm::ARR_DATA_PTR_OFF;

const DROP: &str = "__torajs_arr_drop_scalar";

#[derive(Debug, Default)]
pub struct FrameArrDropStats {
    /// Releases moved behind the born-shape test.
    pub drops_guarded: u32,
    /// Compares, branches and dead values `frame_cell_clean` then folded.
    pub folded: FrameCellCleanStats,
}

pub fn guard_frame_arr_drops(module: &mut Module) -> FrameArrDropStats {
    let mut stats = FrameArrDropStats::default();
    let Some(drop) = module
        .funcs
        .iter()
        .position(|f| f.name == DROP)
        .map(|i| FuncId(i as u32))
    else {
        return stats;
    };
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        let n = guard_in_function(func, drop);
        if n > 0 {
            stats.drops_guarded += n;
            clean_in_function(func, &mut stats.folded);
        }
    }
    stats
}

/// Frame-cell array literals: `alloca_bytes` values typed as an array,
/// large enough for the header.
fn frame_arrays(func: &Function) -> HashSet<ValueId> {
    func.blocks
        .iter()
        .flat_map(|b| &b.insts)
        .filter(|i| matches!(i.kind, InstKind::AllocaBytes(n) if n >= ARR_CELL_SIZE))
        .filter_map(|i| i.result)
        .filter(|r| matches!(func.value_type(*r), Type::Arr(_)))
        .collect()
}

fn guard_in_function(func: &mut Function, drop: FuncId) -> u32 {
    let cells = frame_arrays(func);
    if cells.is_empty() {
        return 0;
    }
    let releases = |kind: &InstKind| match kind {
        InstKind::Call(f, args) if *f == drop => match args.as_slice() {
            [Operand::Value(c)] if cells.contains(c) => Some(*c),
            _ => None,
        },
        _ => None,
    };
    let mut slow_arms: HashSet<BlockId> = HashSet::default();
    // a split moves the rest of the block to a new block at the end,
    // which this walk then reaches in turn; the slow arm it also makes
    // holds the release already guarded
    let mut b = 0;
    while b < func.blocks.len() {
        let block = BlockId(b as u32);
        b += 1;
        if slow_arms.contains(&block) {
            continue;
        }
        let at = func.blocks[block.0 as usize]
            .insts
            .iter()
            .enumerate()
            .find_map(|(i, inst)| releases(&inst.kind).map(|c| (i, c)));
        if let Some((at, cell)) = at {
            slow_arms.insert(guard(func, block, at, cell));
        }
    }
    slow_arms.len() as u32
}

/// `block`: … · test · `cond_br slow, rest` | slow: the release · `br
/// rest` | rest: what followed the release, and the block's old
/// terminator. Answers the slow arm.
fn guard(func: &mut Function, block: BlockId, at: usize, cell: ValueId) -> BlockId {
    let slow = new_block(func);
    let rest = new_block(func);
    let b = block.0 as usize;
    let tail = func.blocks[b].insts.split_off(at + 1);
    let release = func.blocks[b].insts.pop().expect("the release is at `at`");
    func.blocks[rest.0 as usize].insts = tail;
    func.blocks[rest.0 as usize].term =
        std::mem::replace(&mut func.blocks[b].term, Terminator::Unreachable);
    func.blocks[slow.0 as usize].insts.push(release);
    func.blocks[slow.0 as usize].term = Terminator::Br(rest);
    let c = Operand::Value(cell);
    let props = emit(
        func,
        block,
        Type::I64,
        InstKind::Load(Type::I64, c, ARR_PROPS_OFF),
    );
    let data = emit(
        func,
        block,
        Type::I64,
        InstKind::Load(Type::I64, c, ARR_DATA_PTR_OFF),
    );
    let inline = InstKind::BinOp(BinOp::Add, c, Operand::ConstI64(ARR_CELL_SIZE as i64));
    let inline = emit(func, block, Type::I64, inline);
    let moved = emit(
        func,
        block,
        Type::I64,
        InstKind::BinOp(BinOp::Xor, data, inline),
    );
    let legs = emit(
        func,
        block,
        Type::I64,
        InstKind::BinOp(BinOp::Or, props, moved),
    );
    let any = InstKind::ICmp(IPred::Ne, legs, Operand::ConstI64(0));
    let any = emit(func, block, Type::Bool, any);
    func.blocks[b].term = Terminator::CondBr {
        cond: any,
        then_blk: slow,
        else_blk: rest,
    };
    slow
}

#[cfg(test)]
mod tests;
