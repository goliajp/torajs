//! The rewrite: the first copy takes both quotients, the second copy
//! is a clone reading the second lane.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    BinOp, Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId, ValueInfo,
    VecInst, VecOp, VecShape, rewrite_value_operands,
};

use super::Plan;
use super::plan::Uses;

const F64X2: Type = Type::Vec(VecShape::F64x2);
const I64X2: Type = Type::Vec(VecShape::I64x2);

pub(super) fn apply(func: &mut Function, p: &Plan) {
    let snapshot: Vec<Block> = p
        .order()
        .iter()
        .map(|b| func.blocks[b.0 as usize].clone())
        .collect();
    // the dividend lanes, once per entry: the loop holds no call, so
    // the vectors live across it stay in their registers
    let xv = new_value(func, F64X2);
    let splat = InstKind::Vec(VecInst::Splat(VecShape::F64x2, p.inv));
    push(func, p.preheader, xv, splat);
    let counter = p.float_counter.then(|| float_counter(func, p));
    let tv = pair_site(func, p, xv, counter.map(|c| c.0));
    let second = clone_loop(func, p, &snapshot, tv);
    let latch = *p.chain.last().expect("the chain ends in the latch");
    func.blocks[latch.0 as usize].term = Terminator::Br(second[0]);
    if let Some((yv, sv)) = counter {
        let last = *second.last().expect("the clone ends in its latch");
        let next = new_value(func, F64X2);
        let v = Operand::Value;
        let add = InstKind::Vec(VecInst::BinOp(VecOp::FAdd, v(yv), v(sv)));
        push(func, last, next, add);
        push(func, last, yv, InstKind::Copy(F64X2, v(next)));
    }
    let mut swept = second;
    swept.push(p.site);
    sweep_dead(func, &swept, &p.cells);
}

fn new_value(func: &mut Function, ty: Type) -> ValueId {
    let id = ValueId(func.values.len() as u32);
    func.values.push(ValueInfo { ty, name: None });
    id
}

fn inst(result: ValueId, kind: InstKind) -> Inst {
    Inst {
        result: Some(result),
        kind,
        origin: None,
    }
}

fn push(func: &mut Function, b: BlockId, result: ValueId, kind: InstKind) {
    func.blocks[b.0 as usize].insts.push(inst(result, kind));
}

/// The preheader's float counter: a vector cell holding the counter
/// and the counter one step on, and the splat that steps both lanes
/// by two steps. Answers the cell and the splat.
fn float_counter(func: &mut Function, p: &Plan) -> (ValueId, ValueId) {
    let v = Operand::Value;
    let y0 = new_value(func, Type::F64);
    let y1 = new_value(func, Type::F64);
    let lanes = new_value(func, F64X2);
    let yv = new_value(func, F64X2);
    let sv = new_value(func, F64X2);
    let step = Operand::ConstF64(p.step as f64);
    let two = Operand::ConstF64((2 * p.step) as f64);
    push(func, p.preheader, y0, InstKind::SiToFp(v(p.iv)));
    push(
        func,
        p.preheader,
        y1,
        InstKind::BinOp(BinOp::FAdd, v(y0), step),
    );
    push(
        func,
        p.preheader,
        lanes,
        InstKind::Vec(VecInst::Pair(VecShape::F64x2, v(y0), v(y1))),
    );
    push(func, p.preheader, yv, InstKind::Copy(F64X2, v(lanes)));
    let splat = InstKind::Vec(VecInst::Splat(VecShape::F64x2, two));
    push(func, p.preheader, sv, splat);
    (yv, sv)
}

/// The site block with the chain `sitofp → fdiv → fptosi` widened to
/// two lanes, the divisor lanes from the float counter `counter` or
/// converted here; answers the lane vector.
fn pair_site(func: &mut Function, p: &Plan, xv: ValueId, counter: Option<ValueId>) -> ValueId {
    let tv = new_value(func, I64X2);
    func.values[p.q.0 as usize].ty = F64X2;
    let v = Operand::Value;
    let i1 = counter.is_none().then(|| new_value(func, Type::I64));
    let old = std::mem::take(&mut func.blocks[p.site.0 as usize].insts);
    let mut out = Vec::with_capacity(old.len() + 4);
    for (at, i) in old.into_iter().enumerate() {
        if at == p.fdiv_at {
            let yv = match (counter, i1) {
                (Some(yv), _) => yv,
                (None, Some(i1)) => {
                    let yf1 = new_value(func, Type::F64);
                    let yv = new_value(func, F64X2);
                    let step = Operand::ConstI64(p.step);
                    out.push(inst(i1, InstKind::BinOp(BinOp::Add, v(p.iv), step)));
                    out.push(inst(yf1, InstKind::SiToFp(v(i1))));
                    out.push(inst(
                        yv,
                        InstKind::Vec(VecInst::Pair(VecShape::F64x2, v(p.yf), v(yf1))),
                    ));
                    yv
                }
                (None, None) => unreachable!("a converted pair has its early step"),
            };
            let (a, b) = if p.inv_is_lhs {
                (v(xv), v(yv))
            } else {
                (v(yv), v(xv))
            };
            out.push(Inst {
                result: Some(p.q),
                kind: InstKind::Vec(VecInst::BinOp(VecOp::FDiv, a, b)),
                origin: i.origin,
            });
        } else if at == p.fptosi_at {
            out.push(inst(tv, InstKind::Vec(VecInst::FpToSi(v(p.q)))));
            out.push(Inst {
                result: Some(p.t),
                kind: InstKind::Vec(VecInst::Lane(0, v(tv))),
                origin: i.origin,
            });
        } else {
            out.push(i);
        }
    }
    func.blocks[p.site.0 as usize].insts = out;
    if let Some(i1) = i1 {
        rename_step(func, p, i1);
    }
    tv
}

/// The latch's own `iv + C` is the value the site computed early.
fn rename_step(func: &mut Function, p: &Plan, i1: ValueId) {
    let (sb, _) = p.step_at;
    func.blocks[sb.0 as usize]
        .insts
        .retain(|i| i.result != Some(p.step_add));
    for b in p.order() {
        for i in &mut func.blocks[b.0 as usize].insts {
            rewrite_value_operands(&mut i.kind, |x| if x == p.step_add { i1 } else { x });
        }
    }
}

/// The loop cloned from `snapshot` under fresh ids (cells keep theirs),
/// its quotient read from lane 1 of `tv`; its latch branches to the
/// original header. Answers the new blocks in loop order.
fn clone_loop(func: &mut Function, p: &Plan, snapshot: &[Block], tv: ValueId) -> Vec<BlockId> {
    let order = p.order();
    let bmap: HashMap<BlockId, BlockId> = order
        .iter()
        .map(|b| {
            let id = BlockId(func.blocks.len() as u32);
            func.blocks.push(Block {
                id,
                insts: Vec::new(),
                term: Terminator::Unreachable,
            });
            (*b, id)
        })
        .collect();
    let mut map: HashMap<ValueId, ValueId> = HashMap::default();
    for block in snapshot {
        for i in &block.insts {
            let r = i.result.expect("planned loops define values");
            if !p.cells.contains(&r) {
                let ty = func.values[r.0 as usize].ty;
                let fresh = new_value(func, ty);
                map.insert(r, fresh);
            }
        }
    }
    // the quotient itself is not cloned: lane 1 is its second copy
    map.insert(p.t, new_value(func, Type::I64));
    let fix = |map: &HashMap<ValueId, ValueId>, x: ValueId| map.get(&x).copied().unwrap_or(x);
    for block in snapshot {
        let nb = bmap[&block.id];
        for i in &block.insts {
            let r = i.result.expect("planned loops define values");
            if block.id == p.site && r == p.q {
                continue;
            }
            let kind = if block.id == p.site && r == p.t {
                InstKind::Vec(VecInst::Lane(1, Operand::Value(tv)))
            } else {
                let mut kind = i.kind.clone();
                rewrite_value_operands(&mut kind, |x| fix(&map, x));
                kind
            };
            func.blocks[nb.0 as usize].insts.push(Inst {
                result: Some(fix(&map, r)),
                kind,
                origin: i.origin,
            });
        }
        let mut term = block.term.clone();
        if let Terminator::CondBr {
            cond: Operand::Value(c),
            ..
        } = &mut term
        {
            *c = fix(&map, *c);
        }
        // in-loop edges go to the clones, the back edge to the original header
        let retarget = |t: &mut BlockId| {
            if *t != p.header
                && let Some(n) = bmap.get(t)
            {
                *t = *n;
            }
        };
        match &mut term {
            Terminator::Br(t) => retarget(t),
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => {
                retarget(then_blk);
                retarget(else_blk);
            }
            Terminator::Ret(_) | Terminator::Unreachable => {}
        }
        func.blocks[nb.0 as usize].term = term;
    }
    order.iter().map(|b| bmap[b]).collect()
}

/// Pure definitions in `blocks` nothing reads (the clone's own
/// conversion of the counter, left behind by the lane read).
fn sweep_dead(func: &mut Function, blocks: &[BlockId], cells: &HashSet<ValueId>) {
    loop {
        let uses = Uses::of(func);
        let mut changed = false;
        for &b in blocks {
            let before = func.blocks[b.0 as usize].insts.len();
            func.blocks[b.0 as usize].insts.retain(|i| {
                let r = i.result.expect("planned loops define values");
                cells.contains(&r)
                    || uses.reads.contains_key(&r)
                    || !matches!(
                        i.kind,
                        InstKind::SiToFp(_) | InstKind::BinOp(..) | InstKind::Vec(_)
                    )
            });
            changed |= func.blocks[b.0 as usize].insts.len() != before;
        }
        if !changed {
            return;
        }
    }
}
