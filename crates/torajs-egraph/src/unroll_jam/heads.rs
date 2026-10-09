//! The fused loop's inner header: each lane's copy of the inner loop
//! test, and which of them the fused loop has to ask.

use crate::id_hash::HashMap;

use torajs_core::ssa::{BlockId, Function, Operand, Terminator, ValueId};

use super::Plan;
use super::clone::clone_into;
use super::emit::reuse_shared;

/// `F0…`: lane j's inner header test in `heads[j]`; the last lane goes
/// on to `fb`, any lane done to `done`. A test on a shared value is
/// every lane's: `heads[0]` then computes every lane's header values
/// and tests once (the other heads are left unreached).
pub(super) fn inner_heads(
    func: &mut Function,
    p: &Plan,
    heads: &[BlockId],
    fb: BlockId,
    done: BlockId,
    lanes: &mut [HashMap<ValueId, ValueId>],
) {
    let ih = p.inner_header;
    let ih_term = func.blocks[ih.0 as usize].term.clone();
    let Terminator::CondBr {
        cond: Operand::Value(ic),
        ..
    } = ih_term
    else {
        unreachable!("planned inner header ends in cond_br");
    };
    let one_test = p.shared.contains(&ic);
    let insts = func.blocks[ih.0 as usize].insts.clone();
    let (first, rest) = lanes.split_at_mut(1);
    let lane0 = &mut first[0];
    for inst in &insts {
        clone_into(func, heads[0], inst, lane0);
    }
    for (j, map) in rest.iter_mut().enumerate() {
        let head = if one_test { heads[0] } else { heads[j + 1] };
        for inst in &insts {
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
