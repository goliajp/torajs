//! The resume function's control flow: how the body ends, how it
//! parks, where each landing picks it up, and the entry that chooses.

use torajs_core::ssa::suspend::FRAME_STATE_OFF;
use torajs_core::ssa::{
    BlockId, Function, Inst, InstKind, Operand, SuspendFn, SuspendLane, SuspendRuntime, Terminator,
    Type, ValueId,
};

use super::cut::{Point, fresh_block, load, position, store};
use super::spill::new_value;

fn call(fid: torajs_core::ssa::FuncId, args: Vec<Operand>) -> Inst {
    Inst {
        result: None,
        kind: InstKind::Call(fid, args),
        origin: None,
    }
}

/// Every return settles the result: a propagating throw (the block
/// that carries `ThrowSignal`) rejects it, any other return resolves
/// it with the returned value under the body's value form. The resume
/// function itself answers nothing.
pub(super) fn settle_returns(f: &mut Function, s: &SuspendFn, rt: &SuspendRuntime, frame: ValueId) {
    let ret_ty = f.ret;
    for bi in 0..f.blocks.len() {
        let Terminator::Ret(v) = f.blocks[bi].term.clone() else {
            continue;
        };
        let fr = Operand::Value(frame);
        let insts = &mut f.blocks[bi].insts;
        if let Some(at) = (insts.iter()).position(|i| matches!(i.kind, InstKind::ThrowSignal)) {
            insts.remove(at);
            insts.push(call(rt.throw, vec![fr]));
        } else {
            let bits = match v {
                Some(op) => value_bits(f, bi, op, ret_ty),
                None => Operand::ConstI64(0),
            };
            let repr = Operand::ConstI64(s.ret_repr);
            f.blocks[bi].insts.push(call(rt.ret, vec![fr, repr, bits]));
        }
        f.blocks[bi].term = Terminator::Ret(None);
    }
    f.ret = Type::Void;
}

/// The returned value as the i64 the result's value word holds, in the
/// form the body's `REPR_*` names: a number at the width the body
/// returns it, as its bits.
fn value_bits(f: &mut Function, bi: usize, op: Operand, ty: Type) -> Operand {
    let kind = match (op, ty) {
        (Operand::ConstBool(b), _) => return Operand::ConstI64(b as i64),
        (Operand::ConstF64(x), _) => return Operand::ConstI64(x.to_bits() as i64),
        (Operand::ConstI64(c), Type::F64) => return Operand::ConstI64((c as f64).to_bits() as i64),
        (Operand::ConstI64(_), _) | (_, Type::I64) => return op,
        (_, Type::F64) => InstKind::BitCastF64ToI64(op),
        (_, Type::Bool) => InstKind::ZExtBoolToI64(op),
        (_, t) if t.is_pointer_shaped() => InstKind::PtrToInt(op),
        (_, t) => panic!("internal error: an async body returns {}", t.as_str()),
    };
    let r = new_value(f, Type::I64);
    f.blocks[bi].insts.push(Inst {
        result: Some(r),
        kind,
        origin: None,
    });
    Operand::Value(r)
}

/// Replace each cut block's suspension and branch with the park:
/// copy the cells crossing it into the frame, record the point, hand
/// the frame to the awaited promise or the queue, return. Each landing
/// copies its cells back.
pub(super) fn park_and_land(
    f: &mut Function,
    points: &[Point],
    cells: &[Vec<(ValueId, u64, Type)>],
    rt: &SuspendRuntime,
    frame: ValueId,
) {
    let fr = Operand::Value(frame);
    for (k, p) in points.iter().enumerate() {
        let susp = f.blocks[p.cut].insts.pop().unwrap();
        let InstKind::Suspend(_, _, src, _) = susp.kind else {
            unreachable!("a cut block ends at its suspension")
        };
        for &(cell, off, ty) in &cells[k] {
            let v = new_value(f, ty);
            f.blocks[p.cut].insts.push(load(v, ty, cell, 0));
            f.blocks[p.cut]
                .insts
                .push(store(Operand::Value(v), frame, off));
        }
        let state = Operand::ConstI64(k as i64 + 1);
        let park = &mut f.blocks[p.cut];
        park.insts.push(store(state, frame, FRAME_STATE_OFF));
        park.insts.push(match p.lane {
            SuspendLane::Promise(_) => call(rt.await_promise, vec![fr, src]),
            SuspendLane::Direct => call(rt.await_direct, vec![fr]),
        });
        park.term = Terminator::Ret(None);
        let mut refill: Vec<Inst> = Vec::with_capacity(2 * cells[k].len());
        for &(cell, off, ty) in &cells[k] {
            let v = new_value(f, ty);
            refill.push(load(v, ty, frame, off));
            refill.push(store(Operand::Value(v), cell, 0));
        }
        // after the result's read and the clearing of its frame word
        let at = match p.lane {
            SuspendLane::Promise(_) => 2,
            SuspendLane::Direct => 0,
        };
        let l = position(f, p.landing);
        f.blocks[l].insts.splice(at..at, refill);
    }
}

/// The entry becomes the dispatch: every stack slot of the function
/// (each must dominate the landings that refill it), then the state's
/// test, 0 to the body's start (moved to a block of its own) and `k`
/// to landing `k` (`branch`: a chain of compares for a few points, a
/// binary search for more).
pub(super) fn dispatch(f: &mut Function, points: &[Point], frame: ValueId) {
    let start = fresh_block(f);
    let s = start.0 as usize;
    f.blocks[s].insts = std::mem::take(&mut f.blocks[0].insts);
    f.blocks[s].term = std::mem::replace(&mut f.blocks[0].term, Terminator::Unreachable);
    for b in &mut f.blocks {
        retarget(&mut b.term, BlockId(0), start);
    }
    let mut allocas: Vec<Inst> = Vec::new();
    for b in &mut f.blocks {
        let (slots, rest): (Vec<Inst>, Vec<Inst>) = std::mem::take(&mut b.insts)
            .into_iter()
            .partition(|i| matches!(i.kind, InstKind::Alloca(_) | InstKind::AllocaBytes(_)));
        allocas.extend(slots);
        b.insts = rest;
    }
    if points.is_empty() {
        // the optimizer proved every await unreachable
        f.blocks[0].insts = allocas;
        f.blocks[0].term = Terminator::Br(start);
        return;
    }
    let state = new_value(f, Type::I64);
    f.blocks[0].insts = allocas;
    f.blocks[0]
        .insts
        .push(load(state, Type::I64, frame, FRAME_STATE_OFF));
    let st = Operand::Value(state);
    // state 0 first: the call that starts the body is the one every
    // invocation makes
    let landings: Vec<(i64, BlockId)> = (points.iter().enumerate())
        .map(|(k, p)| (k as i64 + 1, p.landing))
        .collect();
    let rest = match landings[..] {
        [(_, only)] => only,
        _ => fresh_block(f),
    };
    super::branch::on_state(f, BlockId(0), st, &[(0, start)], Some(rest));
    if landings.len() > 1 {
        super::branch::on_state(f, rest, st, &landings, None);
    }
}

fn retarget(term: &mut Terminator, from: BlockId, to: BlockId) {
    let fix = |b: &mut BlockId| {
        if *b == from {
            *b = to
        }
    };
    match term {
        Terminator::Br(b) => fix(b),
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => {
            fix(then_blk);
            fix(else_blk);
        }
        Terminator::Ret(_) | Terminator::Unreachable => {}
    }
}
