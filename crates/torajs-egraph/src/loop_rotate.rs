//! Loop rotation — turn a top-tested loop into a bottom-tested one
//! (LLVM LoopRotate, the "loop inversion" of every textbook backend).
//!
//! The lowerer emits `while` loops as
//!
//! ```text
//! H:  %c = <exit test>          ; header, also the continue target
//!     cond_br %c, B, X
//! B:  ...body...
//! L:  ...
//!     br H                      ; latch
//! ```
//!
//! so every iteration pays two branches: the latch's `br H` and the
//! header's test. Rotation copies the header's test into the latch
//! (`L: …; %c' = <exit test>; cond_br %c', B, X`) and leaves `H` as a
//! once-only entry guard. The loop now runs `B … L` with a single
//! conditional branch per iteration.
//!
//! Measured on the prime_count inner loop (mini, hand-assembled
//! variants, 7 runs each): with frem_narrow's zero guard absorbed but
//! the loop still top-tested the kernel runs 47.8 ms, *slower* than
//! the guarded original (44.3 ms); rotated it runs 42.2 ms. Removing
//! the guard alone made a 3-branch loop whose exit branch the
//! predictor handles worse; the rotation is what turns the guard's
//! removal into a win.
//!
//! The IR has no φ nodes (post-φ loop cells are multi-def `Copy`), so
//! a new edge into `X` or `B` carries nothing but control. What the
//! rewrite must preserve is where values are defined, hence the
//! conditions: a single latch ending in an unconditional `br H`; `H`
//! holds only pure, cheap ops (no calls, loads or copies) whose
//! results are single-def and read nowhere outside `H`; `B` has `H` as
//! its only predecessor. The copies get fresh value ids.
//!
//! Last pass before block_layout, so the header test is final.
//! `TORAJS_LOOP_ROTATE_OFF=1` skips, `TORAJS_LOOP_ROTATE_STATS=1`
//! dumps counters.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    BinOp, BlockId, Function, Inst, InstKind, Module, Operand, Terminator, ValueId,
};

use crate::rc_peephole::visit_value_operands;
use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::loop_analysis::LoopAnalysis;

/// Header instructions copied per rotation, at most.
const MAX_HEADER_INSTS: usize = 4;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LoopRotateStats {
    /// Loops turned bottom-tested.
    pub loops_rotated: u32,
}

pub fn rotate_module(module: &mut Module) -> LoopRotateStats {
    let mut stats = LoopRotateStats::default();
    for func in module.funcs.iter_mut() {
        if func.is_declaration() {
            continue;
        }
        // A rewrite changes only its own latch's edges (to B and X
        // instead of H), so rotations whose loop bodies and exit
        // blocks are pairwise disjoint cannot change what another is
        // judged on and share one analysis. Recomputing dominators and
        // loops once per rotation made this pass cubic in main's block
        // count — a main of ~3000 blocks with a few hundred drop loops
        // did not finish compiling. A rotated latch ends in a cond_br,
        // so the same loop never qualifies twice.
        loop {
            let plans = find_rotations(func);
            if plans.is_empty() {
                break;
            }
            for plan in plans {
                apply(func, plan);
                stats.loops_rotated += 1;
            }
        }
    }
    stats
}

struct Rotation {
    header: BlockId,
    latch: BlockId,
}

/// Every rotatable loop of one analysis whose blocks — body plus the
/// header's two targets — no earlier pick in this round has claimed,
/// in analysis order (the order the one-at-a-time loop took them).
fn find_rotations(func: &Function) -> Vec<Rotation> {
    let dom = DominatorTree::compute(func);
    let la = LoopAnalysis::compute(func, &dom);
    let preds = predecessors(func);
    let mut claimed: HashSet<BlockId> = HashSet::default();
    let mut plans = Vec::new();
    for l in la.loops() {
        let [latch] = l.back_edge_sources.as_slice() else {
            continue;
        };
        let body: HashSet<BlockId> = l.body.iter().copied().collect();
        let mut touched = body.clone();
        if let Terminator::CondBr {
            then_blk, else_blk, ..
        } = &func.blocks[l.header.0 as usize].term
        {
            touched.insert(*then_blk);
            touched.insert(*else_blk);
        }
        if !touched.is_disjoint(&claimed) {
            continue;
        }
        if rotatable(func, &preds, &body, l.header, *latch) {
            claimed.extend(touched);
            plans.push(Rotation {
                header: l.header,
                latch: *latch,
            });
        }
    }
    plans
}

fn predecessors(func: &Function) -> HashMap<BlockId, Vec<BlockId>> {
    let mut preds: HashMap<BlockId, Vec<BlockId>> = HashMap::default();
    for block in &func.blocks {
        match &block.term {
            Terminator::Br(t) => preds.entry(*t).or_default().push(block.id),
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => {
                preds.entry(*then_blk).or_default().push(block.id);
                preds.entry(*else_blk).or_default().push(block.id);
            }
            Terminator::Ret(_) | Terminator::Unreachable => {}
        }
    }
    preds
}

fn rotatable(
    func: &Function,
    preds: &HashMap<BlockId, Vec<BlockId>>,
    body: &HashSet<BlockId>,
    header: BlockId,
    latch: BlockId,
) -> bool {
    let h = &func.blocks[header.0 as usize];
    if latch == header
        || !matches!(func.blocks[latch.0 as usize].term, Terminator::Br(t) if t == header)
    {
        return false;
    }
    let Terminator::CondBr {
        then_blk, else_blk, ..
    } = h.term
    else {
        return false;
    };
    let entry = match (body.contains(&then_blk), body.contains(&else_blk)) {
        (true, false) => then_blk,
        (false, true) => else_blk,
        _ => return false,
    };
    if entry == header || preds.get(&entry).map(Vec::as_slice) != Some(&[header]) {
        return false;
    }
    h.insts.len() <= MAX_HEADER_INSTS
        && h.insts
            .iter()
            .all(|i| i.result.is_some() && duplicable(&i.kind))
        && header_values_stay_local(func, header)
}

/// Pure, trap-free and cheap enough to run twice in the binary.
pub(crate) fn duplicable(kind: &InstKind) -> bool {
    match kind {
        InstKind::BinOp(op, _, _) => !matches!(op, BinOp::SDiv | BinOp::SRem | BinOp::FRem),
        InstKind::ICmp(..) | InstKind::FCmp(..) => true,
        InstKind::SiToFp(_)
        | InstKind::FpToSi(_)
        | InstKind::FpToInt32(_)
        | InstKind::ZExtBoolToI64(_)
        | InstKind::ZExtI32ToI64(_) => true,
        _ => false,
    }
}

/// Every value the header defines is single-def and read only inside
/// the header (its insts or its terminator).
fn header_values_stay_local(func: &Function, header: BlockId) -> bool {
    let h = &func.blocks[header.0 as usize];
    let defined: HashSet<ValueId> = h.insts.iter().filter_map(|i| i.result).collect();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(r) = inst.result
                && defined.contains(&r)
                && block.id != header
            {
                return false;
            }
            if block.id == header {
                continue;
            }
            let mut escapes = false;
            visit_value_operands(&inst.kind, |v| escapes |= defined.contains(&v));
            if escapes {
                return false;
            }
        }
        if block.id != header
            && let Terminator::CondBr {
                cond: Operand::Value(v),
                ..
            }
            | Terminator::Ret(Some(Operand::Value(v))) = &block.term
            && defined.contains(v)
        {
            return false;
        }
    }
    h.insts.len() == defined.len()
}

fn apply(func: &mut Function, r: Rotation) {
    let h = func.blocks[r.header.0 as usize].clone();
    let mut rename: HashMap<ValueId, ValueId> = HashMap::default();
    let mut copies: Vec<Inst> = Vec::with_capacity(h.insts.len());
    for inst in &h.insts {
        let old = inst.result.expect("rotatable header insts define values");
        let fresh = ValueId(func.values.len() as u32);
        func.values.push(func.values[old.0 as usize].clone());
        let mut kind = inst.kind.clone();
        remap_operands(&mut kind, &rename);
        rename.insert(old, fresh);
        copies.push(Inst {
            result: Some(fresh),
            kind,
            origin: inst.origin,
        });
    }
    let Terminator::CondBr {
        cond,
        then_blk,
        else_blk,
    } = h.term
    else {
        unreachable!("rotatable header ends in cond_br");
    };
    let cond = match cond {
        Operand::Value(v) => Operand::Value(rename.get(&v).copied().unwrap_or(v)),
        c => c,
    };
    let latch = &mut func.blocks[r.latch.0 as usize];
    latch.insts.extend(copies);
    latch.term = Terminator::CondBr {
        cond,
        then_blk,
        else_blk,
    };
}

fn remap_operands(kind: &mut InstKind, rename: &HashMap<ValueId, ValueId>) {
    let fix = |op: &mut Operand| {
        if let Operand::Value(v) = op
            && let Some(n) = rename.get(v)
        {
            *v = *n;
        }
    };
    match kind {
        InstKind::BinOp(_, a, b) | InstKind::ICmp(_, a, b) | InstKind::FCmp(_, a, b) => {
            fix(a);
            fix(b);
        }
        InstKind::SiToFp(a)
        | InstKind::FpToSi(a)
        | InstKind::FpToInt32(a)
        | InstKind::ZExtBoolToI64(a)
        | InstKind::ZExtI32ToI64(a) => fix(a),
        _ => unreachable!("only duplicable kinds are copied"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, IPred, Type, ValueInfo};

    fn inst(r: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(r)),
            kind,
            origin: None,
        }
    }
    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }
    fn blk(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }

    /// bb0 → bb1(header: %2 = mul %1,%1; %3 = icmp sle %2,%0) →
    /// bb2(body) → bb3(latch: %1 = add %1,1; br bb1) ; exit bb4.
    fn counted_loop(extra_body_pred: bool) -> Function {
        let ty = |t| ValueInfo { ty: t, name: None };
        let entry_term = if extra_body_pred {
            Terminator::CondBr {
                cond: Operand::ConstBool(true),
                then_blk: BlockId(1),
                else_blk: BlockId(2),
            }
        } else {
            Terminator::Br(BlockId(1))
        };
        Function {
            name: "f".into(),
            params: vec![ValueId(0)],
            ret: Type::I64,
            blocks: vec![
                blk(
                    0,
                    vec![inst(1, InstKind::Copy(Type::I64, Operand::ConstI64(2)))],
                    entry_term,
                ),
                blk(
                    1,
                    vec![
                        inst(2, InstKind::BinOp(BinOp::Mul, v(1), v(1))),
                        inst(3, InstKind::ICmp(IPred::Sle, v(2), v(0))),
                    ],
                    Terminator::CondBr {
                        cond: v(3),
                        then_blk: BlockId(2),
                        else_blk: BlockId(4),
                    },
                ),
                blk(2, vec![], Terminator::Br(BlockId(3))),
                blk(
                    3,
                    vec![inst(
                        1,
                        InstKind::BinOp(BinOp::Add, v(1), Operand::ConstI64(1)),
                    )],
                    Terminator::Br(BlockId(1)),
                ),
                blk(4, vec![], Terminator::Ret(Some(v(1)))),
            ],
            values: vec![ty(Type::I64), ty(Type::I64), ty(Type::I64), ty(Type::Bool)],
            current_origin: None,
        }
    }

    #[test]
    fn top_tested_loop_becomes_bottom_tested() {
        let mut m = Module {
            funcs: vec![counted_loop(false)],
            ..Default::default()
        };
        let stats = rotate_module(&mut m);
        assert_eq!(stats.loops_rotated, 1);
        let f = &m.funcs[0];
        let latch = &f.blocks[3];
        assert_eq!(latch.insts.len(), 3);
        assert_eq!(latch.insts[1].kind, InstKind::BinOp(BinOp::Mul, v(1), v(1)));
        assert_eq!(latch.insts[2].kind, InstKind::ICmp(IPred::Sle, v(4), v(0)));
        assert!(matches!(
            latch.term,
            Terminator::CondBr {
                cond: Operand::Value(ValueId(5)),
                then_blk: BlockId(2),
                else_blk: BlockId(4),
            }
        ));
        // the header stays as the entry guard
        assert_eq!(f.blocks[1].insts.len(), 2);
    }

    #[test]
    fn body_entry_with_another_predecessor_is_left_alone() {
        let mut m = Module {
            funcs: vec![counted_loop(true)],
            ..Default::default()
        };
        assert_eq!(rotate_module(&mut m).loops_rotated, 0);
    }
}
