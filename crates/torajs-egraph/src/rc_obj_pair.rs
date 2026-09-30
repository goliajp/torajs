//! Retain/release pairing for a class instance, whose release is not
//! one instruction but the inline sequence lowering expands:
//!
//! ```text
//! B:    rc_inc %x
//!       ...rc-transparent...
//!       %n = (%x == null) | (%x == @undefined_cell)
//!       cond_br %n, after, dec
//! dec:  %rc = load i32, %x
//!       %rc1 = sub %rc, 1
//!       store %rc1, %x
//!       %z = icmp eq %rc1, 0
//!       cond_br %z, walk, buf
//! buf:  call __torajs_cycle_buffer(%x)      ; or the leaf form
//!       br after
//! walk: ...release the fields, free... → after
//! ```
//!
//! With the retain in front, a live `%x` enters `dec` holding at least
//! two references: the count cannot reach zero, `walk` cannot run, and
//! the pair's whole effect is a count put back where it was plus a
//! candidate-root note in `buf`. That note records a reference that
//! went away while others remain; here none went away — the owner
//! changed — so the instance is no more a cycle root than before the
//! pair. A nullish `%x` makes the retain a no-op and takes `after`
//! directly. Either way the block can drop the retain and branch to
//! `after`.
//!
//! Same guards as the single-instruction pairing in `rc_peephole`: the
//! retain and the test share a block, the operands are the same value,
//! and everything between is rc-transparent. `dec` must have no other
//! way in, and every path out of `buf` must reach `after` through the
//! buffer calls alone.

use std::collections::HashSet;

use torajs_core::ssa::{
    BinOp, BlockId, Function, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId,
};

use crate::branch_fold::sweep_unreachable_blocks;
use crate::throw_check_fold::{remove_unused, successors};

const UNDEF_CELL: &str = "___TORAJS_UNDEF_CELL";
const OBJ_PROPS_OFF: u64 = 24;
/// `buf` is one call, or the leaf form's test and its call block.
const MAX_BUFFER_BLOCKS: usize = 3;

/// Elide every retain paired with an inline class-instance release in
/// `func`. `transparent` is the window predicate of the caller;
/// `buffer_fids` are the candidate-root calls `buf` may make.
pub(crate) fn elide_obj_pairs(
    func: &mut Function,
    transparent: impl Fn(&InstKind) -> bool,
    buffer_fids: &HashSet<u32>,
) -> u32 {
    let mut pred_count = vec![0u32; func.blocks.len()];
    for block in &func.blocks {
        for s in successors(&block.term) {
            pred_count[s.0 as usize] += 1;
        }
    }
    let mut elided = 0;
    let mut dead: Vec<ValueId> = Vec::new();
    for b in 0..func.blocks.len() {
        let Some(release) = release_at_end(func, b, &pred_count, buffer_fids) else {
            continue;
        };
        let insts = &func.blocks[b].insts;
        let retain = insts.iter().enumerate().rev().find_map(|(i, inst)| {
            matches!(&inst.kind, InstKind::RcInc(Operand::Value(v)) if *v == release.value)
                .then_some(i)
        });
        let Some(retain) = retain else {
            continue;
        };
        let between = &insts[retain + 1..];
        let keeps_window = |i: &Inst| {
            i.result != Some(release.value)
                && (transparent(&i.kind) || matches!(i.kind, InstKind::GlobalRef(_)))
        };
        if !between.iter().all(keeps_window) {
            continue;
        }
        func.blocks[b].insts.remove(retain);
        func.blocks[b].term = Terminator::Br(release.after);
        dead.extend(release.test);
        elided += 1;
    }
    if elided > 0 {
        remove_unused(func, &dead);
        sweep_unreachable_blocks(func);
    }
    elided
}

struct Release {
    value: ValueId,
    after: BlockId,
    /// The nullish test's values, dead once the branch is gone.
    test: [ValueId; 4],
}

/// The inline release block `b` ends in, if it ends in one.
fn release_at_end(
    func: &Function,
    b: usize,
    pred_count: &[u32],
    buffer_fids: &HashSet<u32>,
) -> Option<Release> {
    let block = &func.blocks[b];
    let Terminator::CondBr {
        cond: Operand::Value(n),
        then_blk: after,
        else_blk: dec,
    } = block.term
    else {
        return None;
    };
    let def = |v: ValueId| block.insts.iter().find(|i| i.result == Some(v));
    let InstKind::BinOp(BinOp::Or, Operand::Value(is_null), Operand::Value(is_undef)) =
        def(n)?.kind
    else {
        return None;
    };
    let InstKind::ICmp(IPred::Eq, Operand::Value(x), Operand::ConstPtrNull) = def(is_null)?.kind
    else {
        return None;
    };
    let InstKind::ICmp(IPred::Eq, Operand::Value(x2), Operand::Value(cell)) = def(is_undef)?.kind
    else {
        return None;
    };
    if x2 != x || !matches!(&def(cell)?.kind, InstKind::GlobalRef(s) if s == UNDEF_CELL) {
        return None;
    }
    if after == dec || pred_count[dec.0 as usize] != 1 {
        return None;
    }
    let buf = decrement_of(func, dec, x)?;
    reaches_through_buffer_calls(func, buf, after, x, buffer_fids).then_some(Release {
        value: x,
        after,
        test: [n, is_null, is_undef, cell],
    })
}

/// `dec` as exactly the inline decrement of `x`; answers the block
/// taken while references remain.
fn decrement_of(func: &Function, dec: BlockId, x: ValueId) -> Option<BlockId> {
    let block = &func.blocks[dec.0 as usize];
    let [load, sub, store, zero] = block.insts.as_slice() else {
        return None;
    };
    let px = Operand::Value(x);
    let (Some(rc), Some(rc1), Some(z)) = (load.result, sub.result, zero.result) else {
        return None;
    };
    let shape = load.kind == InstKind::Load(Type::I32, px, 0)
        && sub.kind == InstKind::BinOp(BinOp::Sub, Operand::Value(rc), Operand::ConstI32(1))
        && store.kind == InstKind::Store(Operand::Value(rc1), px, 0)
        && zero.kind == InstKind::ICmp(IPred::Eq, Operand::Value(rc1), Operand::ConstI32(0));
    match block.term {
        Terminator::CondBr {
            cond: Operand::Value(c),
            else_blk,
            ..
        } if shape && c == z => Some(else_blk),
        _ => None,
    }
}

/// Does every path from `buf` reach `after` running nothing but the
/// candidate-root calls on `x` and the leaf form's sidecar test?
fn reaches_through_buffer_calls(
    func: &Function,
    buf: BlockId,
    after: BlockId,
    x: ValueId,
    buffer_fids: &HashSet<u32>,
) -> bool {
    let px = Operand::Value(x);
    let allowed = |inst: &Inst| match &inst.kind {
        InstKind::Call(f, args) => {
            inst.result.is_none() && buffer_fids.contains(&f.0) && args.as_slice() == [px]
        }
        InstKind::Load(Type::Ptr, p, OBJ_PROPS_OFF) => *p == px,
        InstKind::ICmp(IPred::Ne, _, Operand::ConstPtrNull) => true,
        _ => false,
    };
    let mut seen: Vec<BlockId> = Vec::new();
    let mut stack = vec![buf];
    while let Some(blk) = stack.pop() {
        if blk == after || seen.contains(&blk) {
            continue;
        }
        let block = &func.blocks[blk.0 as usize];
        if seen.len() == MAX_BUFFER_BLOCKS || !block.insts.iter().all(allowed) {
            return false;
        }
        if matches!(block.term, Terminator::Ret(_) | Terminator::Unreachable) {
            return false;
        }
        seen.push(blk);
        stack.extend(successors(&block.term));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::FuncId;

    const BUFFER: FuncId = FuncId(7);
    const OTHER: FuncId = FuncId(8);

    /// entry: [retain x] [extra] nullish(x) → (after, dec)
    /// dec → (walk, buf) · buf: buffer(x) → after · walk: free → after
    fn pair(retain: bool, extra: Option<InstKind>, buf_call: FuncId) -> (Function, BlockId) {
        let mut f = Function::new("t", Type::I64);
        let x = f.add_param(Type::Ptr, "x");
        let px = Operand::Value(x);
        let entry = f.add_block();
        let dec = f.add_block();
        let buf = f.add_block();
        let walk = f.add_block();
        let after = f.add_block();
        if retain {
            f.append_void(entry, InstKind::RcInc(px));
        }
        if let Some(kind) = extra {
            f.append_void(entry, kind);
        }
        let is_null = f.append_inst(
            entry,
            InstKind::ICmp(IPred::Eq, px, Operand::ConstPtrNull),
            Type::Bool,
            None,
        );
        let cell = f.append_inst(
            entry,
            InstKind::GlobalRef(UNDEF_CELL.into()),
            Type::Ptr,
            None,
        );
        let is_undef = f.append_inst(
            entry,
            InstKind::ICmp(IPred::Eq, px, Operand::Value(cell)),
            Type::Bool,
            None,
        );
        let n = f.append_inst(
            entry,
            InstKind::BinOp(BinOp::Or, Operand::Value(is_null), Operand::Value(is_undef)),
            Type::Bool,
            None,
        );
        f.set_term(
            entry,
            Terminator::CondBr {
                cond: Operand::Value(n),
                then_blk: after,
                else_blk: dec,
            },
        );
        let rc = f.append_inst(dec, InstKind::Load(Type::I32, px, 0), Type::I32, None);
        let rc1 = f.append_inst(
            dec,
            InstKind::BinOp(BinOp::Sub, Operand::Value(rc), Operand::ConstI32(1)),
            Type::I32,
            None,
        );
        f.append_void(dec, InstKind::Store(Operand::Value(rc1), px, 0));
        let z = f.append_inst(
            dec,
            InstKind::ICmp(IPred::Eq, Operand::Value(rc1), Operand::ConstI32(0)),
            Type::Bool,
            None,
        );
        f.set_term(
            dec,
            Terminator::CondBr {
                cond: Operand::Value(z),
                then_blk: walk,
                else_blk: buf,
            },
        );
        f.append_void(buf, InstKind::Call(buf_call, vec![px]));
        f.set_term(buf, Terminator::Br(after));
        f.append_void(walk, InstKind::Call(OTHER, vec![px]));
        f.set_term(walk, Terminator::Br(after));
        f.set_term(after, Terminator::Ret(Some(Operand::ConstI64(0))));
        (f, dec)
    }

    fn run(f: &mut Function) -> u32 {
        let buffers: HashSet<u32> = [BUFFER.0].into();
        elide_obj_pairs(f, crate::optimize::classify_pure, &buffers)
    }

    #[test]
    fn a_retain_in_front_of_the_inline_release_goes_with_it() {
        let (mut f, _) = pair(true, None, BUFFER);
        assert_eq!(run(&mut f), 1);
        assert_eq!(f.blocks.len(), 2);
        assert!(f.blocks[0].insts.is_empty());
        assert!(matches!(f.blocks[0].term, Terminator::Br(BlockId(1))));
    }

    #[test]
    fn a_release_with_no_retain_stays() {
        let (mut f, _) = pair(false, None, BUFFER);
        assert_eq!(run(&mut f), 0);
        assert_eq!(f.blocks.len(), 5);
    }

    #[test]
    fn a_call_between_the_pair_keeps_both() {
        let (mut f, _) = pair(true, Some(InstKind::Call(OTHER, Vec::new())), BUFFER);
        assert_eq!(run(&mut f), 0);
        assert!(matches!(f.blocks[0].insts[0].kind, InstKind::RcInc(_)));
    }

    #[test]
    fn a_second_way_into_the_decrement_keeps_both() {
        let (mut f, dec) = pair(true, None, BUFFER);
        let side = f.add_block();
        f.set_term(side, Terminator::Br(dec));
        assert_eq!(run(&mut f), 0);
    }

    #[test]
    fn an_unknown_call_on_the_still_referenced_path_keeps_both() {
        let (mut f, _) = pair(true, None, OTHER);
        assert_eq!(run(&mut f), 0);
    }
}
