//! The clone's body: the narrowed parameters and every value that is
//! one of them plus an integer constant become i64, in place.
//!
//! Each such value is an integer within `MAX_OFFSET` of a parameter
//! inside its window, so the f64 the original computed is exact and
//! is the `sitofp` of the i64 the clone computes; it is never -0,
//! since a sum or difference of integers that comes out zero is +0.
//! A comparison between such values and integer constants becomes the
//! integer comparison (no NaN to order); every other reader gets that
//! `sitofp` in front of it.

use crate::id_hash::HashMap;

use torajs_core::ssa::{
    BinOp, BlockId, FuncId, Function, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId,
    rewrite_value_operands, visit_value_operands,
};

use super::{Affine, Window, affine_values, dispatch, int_const};
use crate::cfg_edit::{emit, new_block};
use crate::float_demote::cfg::retarget_edge;
use crate::float_demote::fit::fpred_to_ipred;
use crate::float_demote::{push_value, split_block_at};

/// The clone of `orig`, with how many calls it makes to itself and how
/// many window comparisons stand in front of them.
pub(super) fn build_clone(
    orig: &Function,
    me: FuncId,
    clone_id: FuncId,
    windows: &[Option<Window>],
    name: String,
) -> (Function, u32, u32) {
    let mut f = orig.clone();
    f.name = name;
    let narrowed: Vec<bool> = windows.iter().map(Option::is_some).collect();
    let mut aff = affine_values(&f, &narrowed);
    let mut roots: HashMap<ValueId, ValueId> = HashMap::default();
    for (i, p) in orig.params.iter().enumerate() {
        if narrowed[i] {
            f.values[p.0 as usize].ty = Type::I64;
            let read = push_value(&mut f.values, Type::I64);
            roots.insert(*p, read);
            let a = aff.remove(p).expect("a narrowed parameter is affine");
            aff.insert(read, a);
        }
    }
    for block in f.blocks.iter_mut() {
        for inst in block.insts.iter_mut() {
            rewrite_value_operands(&mut inst.kind, |v| roots.get(&v).copied().unwrap_or(v));
        }
        rename_term(&mut block.term, &roots);
    }
    retype(&mut f, &aff);
    bridge_uses(&mut f, &aff, me, &narrowed);
    let (sites, checks) = dispatch::redirect_internal(&mut f, me, clone_id, windows, &aff);
    assume_entry(&mut f, windows, &roots);
    (f, sites, checks)
}

fn rename_term(term: &mut Terminator, map: &HashMap<ValueId, ValueId>) {
    match term {
        Terminator::CondBr {
            cond: Operand::Value(v),
            ..
        }
        | Terminator::Ret(Some(Operand::Value(v))) => {
            if let Some(n) = map.get(v) {
                *v = *n;
            }
        }
        _ => {}
    }
}

/// The definitions of the affine values, as integer operations.
fn retype(f: &mut Function, aff: &HashMap<ValueId, Affine>) {
    for block in f.blocks.iter_mut() {
        for inst in block.insts.iter_mut() {
            let Some(r) = inst.result.filter(|r| aff.contains_key(r)) else {
                continue;
            };
            let as_int = |op: &Operand| match int_const(op) {
                Some(k) => Operand::ConstI64(k),
                None => *op,
            };
            inst.kind = match &inst.kind {
                InstKind::BinOp(BinOp::FAdd, a, b) => {
                    InstKind::BinOp(BinOp::Add, as_int(a), as_int(b))
                }
                InstKind::BinOp(BinOp::FSub, a, b) => {
                    InstKind::BinOp(BinOp::Sub, as_int(a), as_int(b))
                }
                InstKind::Copy(_, a) => InstKind::Copy(Type::I64, *a),
                other => unreachable!("affine value defined by {other:?}"),
            };
            f.values[r.0 as usize].ty = Type::I64;
        }
    }
}

/// Integer comparisons where both sides are integers; a `sitofp` in
/// front of every other reader. The narrowed arguments of the clone's
/// own calls stay as they are for `dispatch` to route.
fn bridge_uses(f: &mut Function, aff: &HashMap<ValueId, Affine>, me: FuncId, narrowed: &[bool]) {
    for bi in 0..f.blocks.len() {
        let insts = std::mem::take(&mut f.blocks[bi].insts);
        let mut out: Vec<Inst> = Vec::with_capacity(insts.len());
        for mut inst in insts {
            if inst.result.is_some_and(|r| aff.contains_key(&r)) {
                out.push(inst);
                continue;
            }
            if let Some(cmp) = int_compare(&inst.kind, aff) {
                inst.kind = cmp;
                out.push(inst);
                continue;
            }
            let mut readers: Vec<ValueId> = Vec::new();
            match &inst.kind {
                InstKind::Call(callee, args) if *callee == me => {
                    for (i, a) in args.iter().enumerate() {
                        if let Operand::Value(v) = a
                            && !narrowed[i]
                            && aff.contains_key(v)
                        {
                            readers.push(*v);
                        }
                    }
                }
                kind => visit_value_operands(kind, |v| {
                    if aff.contains_key(&v) {
                        readers.push(v);
                    }
                }),
            }
            let mut map: HashMap<ValueId, ValueId> = HashMap::default();
            for v in readers {
                map.entry(v)
                    .or_insert_with(|| bridge(&mut f.values, &mut out, v));
            }
            if !map.is_empty() {
                if let InstKind::Call(callee, args) = &mut inst.kind
                    && *callee == me
                {
                    for (i, a) in args.iter_mut().enumerate() {
                        match a {
                            Operand::Value(v) if !narrowed[i] && map.contains_key(v) => *v = map[v],
                            _ => {}
                        }
                    }
                } else {
                    rewrite_value_operands(&mut inst.kind, |v| map.get(&v).copied().unwrap_or(v));
                }
            }
            out.push(inst);
        }
        if let Terminator::Ret(Some(Operand::Value(v))) = f.blocks[bi].term
            && aff.contains_key(&v)
        {
            let b = bridge(&mut f.values, &mut out, v);
            f.blocks[bi].term = Terminator::Ret(Some(Operand::Value(b)));
        }
        f.blocks[bi].insts = out;
    }
}

/// `sitofp v` appended to `out`.
pub(super) fn bridge(
    values: &mut Vec<torajs_core::ssa::ValueInfo>,
    out: &mut Vec<Inst>,
    v: ValueId,
) -> ValueId {
    let b = push_value(values, Type::F64);
    out.push(Inst {
        result: Some(b),
        kind: InstKind::SiToFp(Operand::Value(v)),
        origin: None,
    });
    b
}

/// An `fcmp` between affine values and integer constants, as the
/// `icmp` it is between integers.
fn int_compare(kind: &InstKind, aff: &HashMap<ValueId, Affine>) -> Option<InstKind> {
    let InstKind::FCmp(pred, a, b) = kind else {
        return None;
    };
    let side = |op: &Operand| match op {
        Operand::Value(v) if aff.contains_key(v) => Some((*op, true)),
        c => int_const(c).map(|k| (Operand::ConstI64(k), false)),
    };
    let ((a, av), (b, bv)) = (side(a)?, side(b)?);
    (av || bv).then(|| InstKind::ICmp(fpred_to_ipred(*pred), a, b))
}

/// Block 0 becomes a chain asserting every narrowed parameter's
/// window, ending in the copies the body reads them through.
fn assume_entry(f: &mut Function, windows: &[Option<Window>], roots: &HashMap<ValueId, ValueId>) {
    let body = split_block_at(f, BlockId(0), &[0])[0];
    for block in f.blocks.iter_mut().skip(1) {
        retarget_edge(&mut block.term, BlockId(0), body);
    }
    let never = new_block(f);
    let mut open = BlockId(0);
    for (i, w) in windows.iter().enumerate() {
        let Some(w) = w else { continue };
        let p = Operand::Value(f.params[i]);
        for (pred, k) in [(IPred::Sgt, w.hi), (IPred::Slt, w.lo)] {
            let out = emit(
                f,
                open,
                Type::Bool,
                InstKind::ICmp(pred, p, Operand::ConstI64(k)),
            );
            let next = new_block(f);
            f.blocks[open.0 as usize].term = Terminator::CondBr {
                cond: out,
                then_blk: never,
                else_blk: next,
            };
            open = next;
        }
    }
    for p in f.params.clone() {
        if let Some(read) = roots.get(&p) {
            f.blocks[open.0 as usize].insts.push(Inst {
                result: Some(*read),
                kind: InstKind::Copy(Type::I64, Operand::Value(p)),
                origin: None,
            });
        }
    }
    f.blocks[open.0 as usize].term = Terminator::Br(body);
}
