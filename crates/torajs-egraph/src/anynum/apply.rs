//! Installing a plan: the fast part is copied, its web cells become
//! three lanes each, and every way out of the copy boxes the lanes
//! back.
//!
//! Lanes of a cell: `i` (I64) holds the value while `b` (Bool) says
//! the box would be an int32 — `i` means nothing otherwise — and `f`
//! (F64) always holds the value. A `box_int32` def writes `i`, `f`
//! and `b`; a `box_f64` def writes `f` and `b` and zeroes `i` (every
//! def writes every lane, so a lane is dead between the copy that
//! reads it and the next def and the allocator can give the copy's
//! two sides one register). `any_to_f64` of the cell reads `f`,
//! `any_to_int32` reads `i`, `any_is_int32` reads `b`,
//! `any_is_number` is true.
//!
//! The box a lane set stands for is `b ? box_int32 i : box_f64 f` —
//! the same encoding the slow loop would hold, bit for bit, since
//! every def recorded the int32 decision the box itself made.
//!
//! On the way in a seed block gives every lane its value from what
//! the entering blocks stored; a cell they stored an unknown `any`
//! into is tested there and the loop as it was takes anything that
//! is not a number. A side exit (an edge into the slow part) boxes
//! the cells the slow block reads and writes back every copied value
//! live there; an exit of the loop stores into the out cells the
//! readers after the loop were pointed at.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    BinOp, BlockId, Function, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId,
};

use super::live;
use super::web::{Defs, LaneDef, Plan, classify, single_defs};
use crate::anynum::expand::is_number_node;
use crate::float_demote::cfg::{remap_term_operand, retarget_edge, successors, term_operand_value};
use crate::float_demote::push_value;
use crate::int_entry::region::{FastCopy, cells_of, clone_blocks, entry_value};
use crate::rc_peephole::visit_value_operands;
use crate::slot_forward::rewrite_operands;

#[derive(Clone, Copy)]
pub(super) struct Lanes {
    i: ValueId,
    f: ValueId,
    b: ValueId,
}

fn mk(result: ValueId, kind: InstKind) -> Inst {
    Inst {
        result: Some(result),
        kind,
        origin: None,
    }
}

fn val(v: ValueId) -> Operand {
    Operand::Value(v)
}

struct Install<'p> {
    plan: &'p Plan,
    web: HashSet<ValueId>,
    region: HashSet<BlockId>,
    copy: FastCopy,
    lanes: HashMap<ValueId, Lanes>,
    /// the original single defs and the copy's, by id
    defs: HashMap<ValueId, InstKind>,
    /// the copy's id of a web cell → the cell
    fresh_web: HashMap<ValueId, ValueId>,
}

pub(super) fn apply(func: &mut Function, plan: &Plan, cells: &HashSet<ValueId>) {
    let live = live::live_in(func);
    let outside = func.blocks.len();
    let mut defs: HashMap<ValueId, InstKind> = single_defs(func, cells)
        .into_iter()
        .map(|(v, k)| (v, k.clone()))
        .collect();
    let copy = clone_blocks(func, &plan.fast);
    let fresh_cells: HashSet<ValueId> = cells
        .iter()
        .filter_map(|c| copy.values.get(c).copied())
        .collect();
    for b in &plan.fast {
        for inst in &func.blocks[copy.blocks[b].0 as usize].insts {
            if let Some(r) = inst.result
                && !fresh_cells.contains(&r)
            {
                defs.insert(r, inst.kind.clone());
            }
        }
    }
    let lanes: HashMap<ValueId, Lanes> = plan
        .web
        .iter()
        .map(|c| {
            let l = Lanes {
                i: push_value(&mut func.values, Type::I64),
                f: push_value(&mut func.values, Type::F64),
                b: push_value(&mut func.values, Type::Bool),
            };
            (*c, l)
        })
        .collect();
    let fresh_web = plan.web.iter().map(|c| (copy.values[c], *c)).collect();
    let ins = Install {
        plan,
        web: plan.web.iter().copied().collect(),
        region: plan.region.iter().copied().collect(),
        copy,
        lanes,
        defs,
        fresh_web,
    };
    let seed = ins.build_seed(func, cells, outside);
    for bi in 0..outside {
        if !ins.region.contains(&BlockId(bi as u32)) {
            retarget_edge(&mut func.blocks[bi].term, plan.header, seed);
        }
    }
    ins.rewrite_fast(func, cells);
    ins.write_out(func);
    for (from, to) in &plan.cuts {
        let exit = ins.side_exit(func, *to, &live.of(*to));
        let fb = ins.copy.blocks[from];
        retarget_edge(&mut func.blocks[fb.0 as usize].term, *to, exit);
    }
}

impl Install<'_> {
    /// The lane writes of one def.
    fn lane_defs(&self, func: &mut Function, l: Lanes, def: LaneDef, out: &mut Vec<Inst>) {
        let sitofp = |func: &mut Function, op: Operand, out: &mut Vec<Inst>| {
            let t = push_value(&mut func.values, Type::F64);
            out.push(mk(t, InstKind::SiToFp(op)));
            out.push(mk(l.f, InstKind::Copy(Type::F64, val(t))));
        };
        match def {
            LaneDef::Lanes(d) => {
                let s = self.lanes[&d];
                out.push(mk(l.i, InstKind::Copy(Type::I64, val(s.i))));
                out.push(mk(l.f, InstKind::Copy(Type::F64, val(s.f))));
                out.push(mk(l.b, InstKind::Copy(Type::Bool, val(s.b))));
            }
            LaneDef::Int32(op) => {
                out.push(mk(l.i, InstKind::Copy(Type::I64, op)));
                sitofp(func, op, out);
                out.push(mk(
                    l.b,
                    InstKind::Copy(Type::Bool, Operand::ConstBool(true)),
                ));
            }
            LaneDef::Double(op) => {
                out.push(mk(l.i, InstKind::Copy(Type::I64, Operand::ConstI64(0))));
                out.push(mk(l.f, InstKind::Copy(Type::F64, op)));
                out.push(mk(
                    l.b,
                    InstKind::Copy(Type::Bool, Operand::ConstBool(false)),
                ));
            }
            LaneDef::DoubleBits(op) => {
                let t = push_value(&mut func.values, Type::F64);
                out.push(mk(t, InstKind::BitCastI64ToF64(op)));
                out.push(mk(l.i, InstKind::Copy(Type::I64, Operand::ConstI64(0))));
                out.push(mk(l.f, InstKind::Copy(Type::F64, val(t))));
                out.push(mk(
                    l.b,
                    InstKind::Copy(Type::Bool, Operand::ConstBool(false)),
                ));
            }
            LaneDef::IntMaybe(op) => {
                out.push(mk(l.i, InstKind::Copy(Type::I64, op)));
                sitofp(func, op, out);
                let hi = push_value(&mut func.values, Type::I64);
                out.push(mk(
                    hi,
                    InstKind::BinOp(BinOp::Shl, op, Operand::ConstI64(32)),
                ));
                let lo = push_value(&mut func.values, Type::I64);
                out.push(mk(
                    lo,
                    InstKind::BinOp(BinOp::AShr, val(hi), Operand::ConstI64(32)),
                ));
                let fits = push_value(&mut func.values, Type::Bool);
                out.push(mk(fits, InstKind::ICmp(IPred::Eq, val(lo), op)));
                out.push(mk(l.b, InstKind::Copy(Type::Bool, val(fits))));
            }
            LaneDef::Const { i, f, is_int } => {
                out.push(mk(l.i, InstKind::Copy(Type::I64, Operand::ConstI64(i))));
                out.push(mk(l.f, InstKind::Copy(Type::F64, Operand::ConstF64(f))));
                out.push(mk(
                    l.b,
                    InstKind::Copy(Type::Bool, Operand::ConstBool(is_int)),
                ));
            }
            LaneDef::Opaque => unreachable!("an opaque def never reaches the fast part"),
        }
    }

    /// The box the lanes stand for.
    fn rebox(&self, func: &mut Function, l: Lanes, out: &mut Vec<Inst>) -> ValueId {
        let bi = push_value(&mut func.values, Type::Any);
        out.push(mk(bi, InstKind::BoxInt32(val(l.i))));
        let bf = push_value(&mut func.values, Type::Any);
        out.push(mk(bf, InstKind::BoxF64(val(l.f))));
        let s = push_value(&mut func.values, Type::Any);
        out.push(mk(
            s,
            InstKind::Select(Type::Any, val(l.b), val(bi), val(bf)),
        ));
        s
    }

    /// Whether a block before the loop stores the cell.
    fn stored_outside(&self, func: &Function, outside: usize, cell: ValueId) -> bool {
        (0..outside)
            .filter(|bi| !self.region.contains(&BlockId(*bi as u32)))
            .any(|bi| func.blocks[bi].insts.iter().any(|i| i.result == Some(cell)))
    }

    /// The seed block every entering edge is pointed at: the copy's
    /// cells and the lanes take their entry values, a cell entered
    /// with an unknown `any` is tested, and the chain falls through
    /// into the copy's header.
    fn build_seed(&self, func: &mut Function, cells: &HashSet<ValueId>, outside: usize) -> BlockId {
        let seed = func.add_block();
        let mut insts: Vec<Inst> = Vec::new();
        let mut dynamic: Vec<(ValueId, Operand)> = Vec::new();
        let entry = |func: &Function, c: ValueId| {
            entry_value(func, &self.region, self.plan.header, c, cells).unwrap_or(Operand::Value(c))
        };
        for c in cells_of(func, &self.plan.fast, cells) {
            if self.web.contains(&c) || !self.stored_outside(func, outside, c) {
                continue;
            }
            let ty = func.values[c.0 as usize].ty;
            let from = entry(func, c);
            insts.push(mk(self.copy.values[&c], InstKind::Copy(ty, from)));
        }
        let defs: Defs<'_> = self.defs.iter().map(|(v, k)| (*v, k)).collect();
        for &c in &self.plan.web {
            let l = self.lanes[&c];
            // a cell only the loop stores holds nothing on the way in
            if !self.stored_outside(func, outside, c) {
                let zero = LaneDef::Const {
                    i: 0,
                    f: 0.0,
                    is_int: false,
                };
                self.lane_defs(func, l, zero, &mut insts);
                continue;
            }
            let from = entry(func, c);
            match classify(func, &defs, &self.web, from) {
                LaneDef::Opaque | LaneDef::Lanes(_) => dynamic.push((c, from)),
                def => self.lane_defs(func, l, def, &mut insts),
            }
        }
        func.blocks[seed.0 as usize].insts = insts;
        let mut cur = seed;
        for (c, op) in dynamic {
            let l = self.lanes[&c];
            let is_num = push_value(&mut func.values, Type::Bool);
            func.blocks[cur.0 as usize]
                .insts
                .push(mk(is_num, InstKind::AnyIsNumber(op)));
            let pass = func.add_block();
            func.blocks[cur.0 as usize].term = Terminator::CondBr {
                cond: val(is_num),
                then_blk: pass,
                else_blk: self.plan.header,
            };
            let b = push_value(&mut func.values, Type::Bool);
            let i = push_value(&mut func.values, Type::I64);
            let f = push_value(&mut func.values, Type::F64);
            func.blocks[pass.0 as usize].insts = vec![
                mk(b, InstKind::AnyIsInt32(op)),
                mk(l.b, InstKind::Copy(Type::Bool, val(b))),
                mk(i, InstKind::AnyToInt32(op)),
                mk(l.i, InstKind::Copy(Type::I64, val(i))),
                mk(f, InstKind::AnyToF64(op, val(b))),
                mk(l.f, InstKind::Copy(Type::F64, val(f))),
            ];
            cur = pass;
        }
        func.blocks[cur.0 as usize].term = Terminator::Br(self.copy.blocks[&self.plan.header]);
        seed
    }

    /// The copy's web cells become lanes: every def writes them,
    /// every node reading one is answered from them.
    fn rewrite_fast(&self, func: &mut Function, cells: &HashSet<ValueId>) {
        let defs: Defs<'_> = self.defs.iter().map(|(v, k)| (*v, k)).collect();
        let no_web: HashSet<ValueId> = HashSet::default();
        let mut op_map: HashMap<ValueId, Operand> = HashMap::default();
        for b in &self.plan.fast {
            let fb = self.copy.blocks[b];
            let insts = std::mem::take(&mut func.blocks[fb.0 as usize].insts);
            let mut out: Vec<Inst> = Vec::with_capacity(insts.len());
            for inst in insts {
                let cell = |op: &Operand| match op {
                    Operand::Value(v) => self.fresh_web.get(v).copied(),
                    _ => None,
                };
                // the web cell the node reads, with its lanes
                let read = match &inst.kind {
                    InstKind::AnyIsInt32(v)
                    | InstKind::AnyIsNumber(v)
                    | InstKind::AnyToInt32(v)
                    | InstKind::AnyToF64(v, _)
                    | InstKind::RcIncAny(v)
                    | InstKind::RcDecAny(v) => cell(v).map(|c| self.lanes[&c]),
                    _ => None,
                };
                match (inst.result, &inst.kind, read) {
                    (Some(r), InstKind::Copy(Type::Any, src), _)
                        if self.fresh_web.contains_key(&r) =>
                    {
                        let def = match cell(src) {
                            Some(d) => LaneDef::Lanes(d),
                            None => classify(func, &defs, &no_web, *src),
                        };
                        let l = self.lanes[&self.fresh_web[&r]];
                        self.lane_defs(func, l, def, &mut out);
                    }
                    (Some(r), InstKind::AnyIsInt32(_), Some(l)) => {
                        op_map.insert(r, val(l.b));
                    }
                    (Some(r), InstKind::AnyIsNumber(_), Some(_)) => {
                        op_map.insert(r, Operand::ConstBool(true));
                    }
                    (Some(r), InstKind::AnyToInt32(_), Some(l)) => {
                        op_map.insert(r, val(l.i));
                    }
                    (Some(r), InstKind::AnyToF64(_, _), Some(l)) => {
                        op_map.insert(r, val(l.f));
                    }
                    (None, InstKind::RcIncAny(_) | InstKind::RcDecAny(_), Some(_)) => {}
                    _ => out.push(inst),
                }
            }
            func.blocks[fb.0 as usize].insts = out;
        }
        for b in &self.plan.fast {
            let block = &mut func.blocks[self.copy.blocks[b].0 as usize];
            for inst in &mut block.insts {
                rewrite_operands(&mut inst.kind, &op_map);
            }
            let term = std::mem::replace(&mut block.term, Terminator::Unreachable);
            block.term = match remap_term_operand(term, &op_map) {
                Terminator::CondBr {
                    cond: Operand::ConstBool(c),
                    then_blk,
                    else_blk,
                } => Terminator::Br(if c { then_blk } else { else_blk }),
                term => term,
            };
        }
        self.drop_dead_boxes(func, cells);
    }

    /// The boxes the lanes replaced have no reader left; a bit cast
    /// feeding one goes with it.
    fn drop_dead_boxes(&self, func: &mut Function, cells: &HashSet<ValueId>) {
        loop {
            let mut used: HashSet<ValueId> = HashSet::default();
            for block in &func.blocks {
                for inst in &block.insts {
                    visit_value_operands(&inst.kind, |v| {
                        used.insert(v);
                    });
                }
                if let Some(v) = term_operand_value(&block.term) {
                    used.insert(v);
                }
            }
            let mut dropped = false;
            for b in &self.plan.fast {
                let block = &mut func.blocks[self.copy.blocks[b].0 as usize];
                block.insts.retain(|inst| {
                    let dead = match (inst.result, &inst.kind) {
                        (Some(r), kind) if !used.contains(&r) && !cells.contains(&r) => {
                            is_number_node(kind)
                                || matches!(
                                    kind,
                                    InstKind::BoxAny(..)
                                        | InstKind::BitCastF64ToI64(_)
                                        | InstKind::SiToFp(_)
                                )
                        }
                        _ => false,
                    };
                    dropped |= dead;
                    !dead
                });
            }
            if !dropped {
                return;
            }
        }
    }

    /// A block on an edge into the slow part: the web cells the
    /// target reads are boxed back, every copied value it reads is
    /// written to its own id.
    /// `live`: the values live into `target`, in id order.
    fn side_exit(&self, func: &mut Function, target: BlockId, live: &[ValueId]) -> BlockId {
        let mut insts: Vec<Inst> = Vec::new();
        for &v in live {
            if let Some(l) = self.lanes.get(&v) {
                let boxed = self.rebox(func, *l, &mut insts);
                insts.push(mk(v, InstKind::Copy(Type::Any, val(boxed))));
            } else if let Some(fresh) = self.copy.values.get(&v) {
                let ty = func.values[v.0 as usize].ty;
                insts.push(mk(v, InstKind::Copy(ty, val(*fresh))));
            }
        }
        let id = func.add_block();
        func.blocks[id.0 as usize].insts = insts;
        func.blocks[id.0 as usize].term = Terminator::Br(target);
        id
    }

    /// Out cells for the loop cells read after the loop, stored on
    /// every edge leaving either copy; the readers read them.
    fn write_out(&self, func: &mut Function) {
        let plan = self.plan;
        let out: Vec<(ValueId, ValueId)> = plan
            .out
            .iter()
            .map(|c| {
                let ty = func.values[c.0 as usize].ty;
                (*c, push_value(&mut func.values, ty))
            })
            .collect();
        let to_out: HashMap<ValueId, Operand> = out.iter().map(|(c, o)| (*c, val(*o))).collect();
        for b in &plan.readers {
            let block = &mut func.blocks[b.0 as usize];
            for inst in block.insts.iter_mut() {
                rewrite_operands(&mut inst.kind, &to_out);
            }
            let term = std::mem::replace(&mut block.term, Terminator::Unreachable);
            block.term = remap_term_operand(term, &to_out);
        }
        let mut inside: HashSet<BlockId> = self.region.clone();
        inside.extend(self.copy.blocks.values().copied());
        let mut leaving: Vec<(BlockId, bool)> = plan.region.iter().map(|b| (*b, false)).collect();
        leaving.extend(plan.fast.iter().map(|b| (self.copy.blocks[b], true)));
        for (b, fast) in leaving {
            let targets: Vec<BlockId> = successors(&func.blocks[b.0 as usize].term)
                .filter(|t| !inside.contains(t))
                .collect();
            for target in targets {
                let mut insts: Vec<Inst> = Vec::new();
                for (c, o) in &out {
                    let ty = func.values[c.0 as usize].ty;
                    let from = match (fast, self.lanes.get(c), self.copy.values.get(c)) {
                        (true, Some(l), _) => val(self.rebox(func, *l, &mut insts)),
                        (true, None, Some(fresh)) => val(*fresh),
                        _ => val(*c),
                    };
                    insts.push(mk(*o, InstKind::Copy(ty, from)));
                }
                let exit = func.add_block();
                func.blocks[exit.0 as usize].insts = insts;
                func.blocks[exit.0 as usize].term = Terminator::Br(target);
                retarget_edge(&mut func.blocks[b.0 as usize].term, target, exit);
            }
        }
    }
}
