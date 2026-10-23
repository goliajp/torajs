//! What a loop reads that int_entry can convert: the f64 sources nothing
//! is known of, and the comparisons of its counter's image with one.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{BlockId, FPred, Function, IPred, InstKind, Operand, Type, ValueId};

use super::BoundUse;
use super::guard::Round;
use crate::float_demote::cfg::term_operand_value;
use crate::rc_peephole::visit_value_operands;

/// An f64 nothing is known of, defined outside the region: a call's
/// answer, a load, a parameter.
pub(super) fn is_source(
    func: &Function,
    region: &HashSet<BlockId>,
    defs: &HashMap<ValueId, (BlockId, &InstKind)>,
    v: ValueId,
) -> bool {
    if func.values[v.0 as usize].ty != Type::F64 {
        return false;
    }
    match defs.get(&v) {
        Some((at, kind)) => {
            !region.contains(at)
                && matches!(
                    kind,
                    InstKind::Call(..)
                        | InstKind::CallIndirect(..)
                        | InstKind::Load(..)
                        | InstKind::LoadDyn(..)
                )
        }
        None => func.params.contains(&v),
    }
}

fn ordered(region: &HashSet<BlockId>) -> Vec<BlockId> {
    let mut ordered: Vec<BlockId> = region.iter().copied().collect();
    ordered.sort_by_key(|b| b.0);
    ordered
}

/// The sources the region reads anywhere but in the matched
/// comparisons, in first-read order.
pub(super) fn value_sources(
    func: &Function,
    region: &HashSet<BlockId>,
    defs: &HashMap<ValueId, (BlockId, &InstKind)>,
    matched: &HashSet<(BlockId, usize)>,
) -> Vec<ValueId> {
    let mut out: Vec<ValueId> = Vec::new();
    for b in ordered(region) {
        let block = &func.blocks[b.0 as usize];
        let mut read: Vec<ValueId> = Vec::new();
        for (i, inst) in block.insts.iter().enumerate() {
            if !matched.contains(&(b, i)) {
                visit_value_operands(&inst.kind, |v| read.push(v));
            }
        }
        read.extend(term_operand_value(&block.term));
        for v in read {
            if is_source(func, region, defs, v) && !out.contains(&v) {
                out.push(v);
            }
        }
    }
    out
}

pub(super) fn bound_uses(
    func: &Function,
    region: &HashSet<BlockId>,
    defs: &HashMap<ValueId, (BlockId, &InstKind)>,
    cells: &HashSet<ValueId>,
) -> Vec<BoundUse> {
    // a cell the loop does not write is some other loop's, and a plan
    // of that loop points its readers at an out cell this compare,
    // made from the counter as planned, would never see
    let written: HashSet<ValueId> = region
        .iter()
        .flat_map(|b| {
            func.blocks[b.0 as usize]
                .insts
                .iter()
                .filter_map(|i| i.result)
        })
        .collect();
    let counter_of = |v: ValueId| match defs.get(&v) {
        Some((_, InstKind::SiToFp(Operand::Value(x))))
            if cells.contains(x) && !written.contains(x) =>
        {
            None
        }
        Some((_, InstKind::SiToFp(x))) => Some(*x),
        _ => None,
    };
    let is_source = |v: ValueId| is_source(func, region, defs, v);
    let mut out = Vec::new();
    for b in ordered(region) {
        for (i, inst) in func.blocks[b.0 as usize].insts.iter().enumerate() {
            let InstKind::FCmp(pred, Operand::Value(l), Operand::Value(r)) = &inst.kind else {
                continue;
            };
            // counter on the left reads as written; on the right the
            // predicate mirrors
            let (counter, source, pred) = match (counter_of(*l), counter_of(*r)) {
                (Some(x), _) if is_source(*r) => (x, *r, *pred),
                (_, Some(x)) if is_source(*l) => (x, *l, mirrored(*pred)),
                _ => continue,
            };
            let (pred, round) = match pred {
                FPred::Olt => (IPred::Slt, Round::Ceil),
                FPred::Ole => (IPred::Sle, Round::Floor),
                FPred::Ogt => (IPred::Sgt, Round::Floor),
                FPred::Oge => (IPred::Sge, Round::Ceil),
                FPred::Oeq | FPred::One | FPred::Une => continue,
            };
            out.push(BoundUse {
                block: b,
                inst: i,
                source,
                counter,
                pred,
                round,
            });
        }
    }
    out
}

fn mirrored(pred: FPred) -> FPred {
    match pred {
        FPred::Olt => FPred::Ogt,
        FPred::Ogt => FPred::Olt,
        FPred::Ole => FPred::Oge,
        FPred::Oge => FPred::Ole,
        p => p,
    }
}
