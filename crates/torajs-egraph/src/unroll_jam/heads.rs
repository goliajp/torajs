//! The fused loop's inner header: each lane's copy of the inner loop
//! test, and which of them the fused loop has to ask.

use crate::id_hash::HashMap;

use torajs_core::ssa::{
    BlockId, Function, IPred, InstKind, Operand, Terminator, ValueId, visit_value_operands,
};

use super::Plan;
use super::clone::clone_into;
use super::emit::reuse_shared;

/// `F0…`: lane j's inner header test in `heads[j]`; the last lane goes
/// on to `fb`, any lane done to `done`. A test on a shared value is
/// every lane's: `heads[0]` then computes every lane's header values
/// and tests once (the other heads are left unreached). So does a test
/// lane 0 decides for every lane (`lane0_decides`), whose other lanes'
/// copies are not made.
pub(super) fn inner_heads(
    func: &mut Function,
    p: &Plan,
    heads: &[BlockId],
    fb: BlockId,
    done: BlockId,
    lanes: &mut [HashMap<ValueId, ValueId>],
) {
    let ih = p.inner_header;
    let Terminator::CondBr {
        cond: Operand::Value(ic),
        ..
    } = func.blocks[ih.0 as usize].term
    else {
        unreachable!("planned inner header ends in cond_br");
    };
    let rising = lane0_decides(func, p, ic);
    let one_test = p.shared.contains(&ic) || rising;
    let insts = func.blocks[ih.0 as usize].insts.clone();
    let (first, rest) = lanes.split_at_mut(1);
    let lane0 = &mut first[0];
    for inst in &insts {
        clone_into(func, heads[0], inst, lane0);
    }
    for (j, map) in rest.iter_mut().enumerate() {
        let head = if one_test { heads[0] } else { heads[j + 1] };
        for inst in insts.iter().filter(|i| !rising || i.result != Some(ic)) {
            if !reuse_shared(p, inst, Some(lane0), map) {
                clone_into(func, head, inst, map);
            }
        }
    }
    let tested = if one_test { 1 } else { p.lanes };
    for (j, map) in lanes.iter().enumerate().take(tested) {
        let stay = if j + 1 < tested { heads[j + 1] } else { fb };
        let (then_blk, else_blk) = if p.inner_on_true {
            (stay, done)
        } else {
            (done, stay)
        };
        func.blocks[heads[j].0 as usize].term = Terminator::CondBr {
            cond: Operand::Value(map[&ic]),
            then_blk,
            else_blk,
        };
    }
    for &h in &heads[tested..] {
        func.blocks[h.0 as usize].term = Terminator::Br(fb);
    }
}

/// The inner test stays while `x <= iv` or `x < iv`, with `x` a value
/// every lane computes alike and `iv` the outer counter, and its branch
/// is its one reader. Lane `j` tests `x` against `iv + j·C` (`C >= 1`;
/// a group starts only when `iv + (L-1)·C` passes the outer test, so
/// nothing wraps): while lane 0 stays every lane stays, and the fused
/// loop, which leaves when any lane is done, can ask lane 0 alone.
fn lane0_decides(func: &Function, p: &Plan, ic: ValueId) -> bool {
    let Some(InstKind::ICmp(pred, a, b)) = func.blocks[p.inner_header.0 as usize]
        .insts
        .iter()
        .find(|i| i.result == Some(ic))
        .map(|i| &i.kind)
    else {
        return false;
    };
    let shared = |o: &Operand| match o {
        Operand::Value(v) => p.shared.contains(v),
        Operand::ConstI64(_) => true,
        _ => false,
    };
    let is_iv = |o: &Operand| matches!(o, Operand::Value(v) if *v == p.iv);
    // as `x <pred> iv`
    let pred = if shared(a) && is_iv(b) {
        *pred
    } else if is_iv(a) && shared(b) {
        match pred {
            IPred::Slt => IPred::Sgt,
            IPred::Sle => IPred::Sge,
            IPred::Sgt => IPred::Slt,
            IPred::Sge => IPred::Sle,
            _ => return false,
        }
    } else {
        return false;
    };
    let rising = matches!(
        (p.inner_on_true, pred),
        (true, IPred::Sle | IPred::Slt) | (false, IPred::Sgt | IPred::Sge)
    );
    let mut reads = 0;
    for block in &func.blocks {
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |v| reads += usize::from(v == ic));
        }
        if let Terminator::CondBr {
            cond: Operand::Value(c),
            ..
        } = block.term
        {
            reads += usize::from(c == ic);
        }
    }
    rising && reads == 1
}
