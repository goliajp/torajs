//! The run-time check `TORAJS_PIN_SCALAR_STRICT` adds after each
//! `pinned` read of a scalar.

use torajs_core::ssa::{
    BlockId, Function, IPred, InstKind, Operand, PinWord, Terminator, Type, ValueId,
};

use super::flow::def_of;

/// Right after `pinned` value `p`, compare it with the field in memory
/// and trap when they differ: the rest of its block moves to a new one.
pub(super) fn check(f: &mut Function, p: ValueId, fields: &[(u64, Type)]) {
    let (b, i) = def_of(f, p);
    let InstKind::Pinned(PinWord::Scalar(k)) = f.blocks[b].insts[i].kind else {
        unreachable!("a scalar's pinned value");
    };
    let (off, ty) = fields[k as usize];
    let rest = f.blocks[b].insts.split_off(i + 1);
    let term = std::mem::replace(&mut f.blocks[b].term, Terminator::Unreachable);
    let trap = f.add_block();
    let cont = f.add_block();
    f.blocks[cont.0 as usize].insts = rest;
    f.blocks[cont.0 as usize].term = term;
    let this = Operand::Value(f.params[0]);
    let at = BlockId(b as u32);
    let m = f.insert_inst(at, i + 1, InstKind::Load(ty, this, off), ty);
    let ne = InstKind::ICmp(IPred::Ne, Operand::Value(m), Operand::Value(p));
    let ne = f.insert_inst(at, i + 2, ne, Type::Bool);
    f.set_term(
        at,
        Terminator::CondBr {
            cond: Operand::Value(ne),
            then_blk: trap,
            else_blk: cont,
        },
    );
}
