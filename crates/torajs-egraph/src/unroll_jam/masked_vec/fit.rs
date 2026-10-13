//! Whether a masked group's step packs, and how many pairs fit: the
//! step's instructions all have two-lane forms, and every pair's
//! vectors sit beside the splats and one step's temporaries in the
//! vector registers.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    BinOp, FPred, Function, IPred, Inst, InstKind, Operand, Type, ValueId, lowers_to_call,
    visit_value_operands,
};

use super::super::clone::reads_outside;
use super::super::retire::snapped;
use super::super::{MAX_LANES, Plan};

/// Vector registers the allocator hands out; every vector keeps one.
const VEC_REGISTERS: usize = 21;

/// What one pair's step holds in vector registers.
pub(crate) struct Packing {
    /// Vectors per pair: slot values, unfrozen snapshots and the flag.
    pub per_pair: usize,
    /// Distinct loop-invariant operands the step reads, splat once.
    pub splats: usize,
    /// Peak live temporaries in one pair's step. The `go` chain is not
    /// one: the flag is read once, at the step's first test, and its
    /// register is free until the step writes it back — unless the
    /// loop calls, when a vector saved around the call keeps its
    /// register to itself and the chain needs one more.
    pub temps: usize,
    /// Kept cells that freeze themselves (see the module doc).
    pub frozen: Vec<ValueId>,
}

/// Pairs for a packed group: as many as fit beside the splats and one
/// step's temporaries, at most `MAX_LANES` lanes.
pub(crate) fn lanes(pk: &Packing) -> usize {
    let room = VEC_REGISTERS.saturating_sub(pk.splats + pk.temps);
    2 * (room / pk.per_pair).clamp(1, MAX_LANES / 2)
}

/// How the group packs, or `None` where some instruction of the step
/// has no two-lane form: the step reads only i64 and f64 values, its
/// compares are read by their branches alone, and nothing reads the
/// counter.
pub(crate) fn packing(func: &Function, p: &Plan) -> Option<Packing> {
    let inner = p.inner_order();
    let insts: Vec<Inst> = inner
        .iter()
        .flat_map(|b| func.blocks[b.0 as usize].insts.iter().cloned())
        .collect();
    let defined: HashSet<ValueId> = insts.iter().filter_map(|i| i.result).collect();
    let mut compares: HashSet<ValueId> = HashSet::default();
    for inst in &insts {
        let r = inst.result?;
        let packs = match &inst.kind {
            InstKind::BinOp(
                BinOp::FAdd | BinOp::FSub | BinOp::FMul | BinOp::Add | BinOp::Sub,
                _,
                _,
            )
            | InstKind::Copy(Type::F64 | Type::I64, _) => true,
            InstKind::FCmp(FPred::Oeq | FPred::Olt | FPred::Ogt | FPred::Ole | FPred::Oge, ..)
            | InstKind::ICmp(IPred::Eq | IPred::Slt | IPred::Sgt | IPred::Sle | IPred::Sge, ..) => {
                compares.insert(r);
                true
            }
            _ => false,
        };
        let mut lanes_ok = true;
        visit_value_operands(&inst.kind, |v| {
            lanes_ok &= v != p.iv && matches!(func.value_type(v), Type::F64 | Type::I64);
        });
        if !packs || !lanes_ok {
            return None;
        }
    }
    let read_by_inst = func.blocks.iter().flat_map(|b| &b.insts).any(|i| {
        let mut hit = false;
        visit_value_operands(&i.kind, |v| hit |= compares.contains(&v));
        hit
    });
    if read_by_inst {
        return None;
    }
    let read_after_pre = reads_outside(func, &p.pre_order());
    let pre_defined: Vec<ValueId> = p
        .pre_order()
        .iter()
        .flat_map(|b| func.blocks[b.0 as usize].insts.iter())
        .filter_map(|i| i.result)
        .filter(|r| Some(*r) != p.skip && !p.shared.contains(r))
        .collect();
    let own = pre_defined
        .iter()
        .filter(|r| read_after_pre.contains(r))
        .count();
    let kept = snapped(func, p);
    let frozen: Vec<ValueId> = kept
        .iter()
        .copied()
        .filter(|c| p.exit_cells.is_some() && bump_of(&insts, *c).is_some())
        .collect();
    let mut invariant: HashSet<Operand> = HashSet::default();
    for inst in &insts {
        // a cell freezing itself by one steps by its own mask
        if let InstKind::BinOp(BinOp::Add, Operand::Value(c), Operand::ConstI64(1))
        | InstKind::BinOp(BinOp::Add, Operand::ConstI64(1), Operand::Value(c)) = inst.kind
            && frozen.contains(&c)
        {
            continue;
        }
        for op in args(&inst.kind) {
            if !matches!(op, Operand::Value(v) if defined.contains(&v) || pre_defined.contains(&v))
            {
                invariant.insert(op);
            }
        }
    }
    Some(Packing {
        per_pair: own + kept.len() - frozen.len() + 1,
        splats: invariant.len(),
        temps: peak_temps(&insts, &defined) + usize::from(calls(func, p)),
        frozen,
    })
}

/// Whether the outer loop's body calls anywhere.
fn calls(func: &Function, p: &Plan) -> bool {
    p.body
        .iter()
        .flat_map(|b| func.blocks[b.0 as usize].insts.iter())
        .any(|i| lowers_to_call(&i.kind))
}

/// The operands of a packable instruction, in order.
pub(crate) fn args(kind: &InstKind) -> Vec<Operand> {
    match kind {
        InstKind::BinOp(_, a, b) | InstKind::ICmp(_, a, b) | InstKind::FCmp(_, a, b) => {
            vec![*a, *b]
        }
        InstKind::Copy(_, a) => vec![*a],
        other => unreachable!("packing an unpacked instruction {other:?}"),
    }
}

/// The constant `c` is bumped by, when the step's one write of `c`
/// copies `c + k` (`k + c`) computed from `c` itself.
fn bump_of(insts: &[Inst], c: ValueId) -> Option<i64> {
    let writes: Vec<&Inst> = insts.iter().filter(|i| i.result == Some(c)).collect();
    let [write] = writes[..] else { return None };
    let InstKind::Copy(_, Operand::Value(x)) = write.kind else {
        return None;
    };
    let def = insts.iter().find(|i| i.result == Some(x))?;
    match def.kind {
        InstKind::BinOp(BinOp::Add, Operand::Value(a), Operand::ConstI64(k))
        | InstKind::BinOp(BinOp::Add, Operand::ConstI64(k), Operand::Value(a))
            if a == c =>
        {
            Some(k)
        }
        _ => None,
    }
}

/// Peak count of the step's own values (not cells) live at once, each
/// from its definition to its last read; a compare is read by its
/// branch at the end of its block.
fn peak_temps(insts: &[Inst], defined: &HashSet<ValueId>) -> usize {
    let cells: HashSet<ValueId> = insts
        .iter()
        .filter(|i| matches!(i.kind, InstKind::Copy(..)))
        .filter_map(|i| i.result)
        .collect();
    let mut last: HashMap<ValueId, usize> = HashMap::default();
    for (at, inst) in insts.iter().enumerate() {
        visit_value_operands(&inst.kind, |v| {
            last.insert(v, at);
        });
        if matches!(inst.kind, InstKind::FCmp(..) | InstKind::ICmp(..))
            && let Some(r) = inst.result
        {
            // its branch reads it: no earlier read would end it sooner
            last.insert(r, at);
        }
    }
    let (mut live, mut peak) = (0usize, 0usize);
    for (at, inst) in insts.iter().enumerate() {
        let mut dying: HashSet<ValueId> = HashSet::default();
        visit_value_operands(&inst.kind, |v| {
            if defined.contains(&v) && !cells.contains(&v) && last.get(&v) == Some(&at) {
                dying.insert(v);
            }
        });
        // an operand read for the last time frees its register to the
        // result
        live -= dying.len();
        if let Some(r) = inst.result
            && !cells.contains(&r)
            && let Some(&end) = last.get(&r)
        {
            live += 1;
            peak = peak.max(live);
            // read by its own branch alone: gone at the block's end
            if end == at {
                live -= 1;
            }
        }
    }
    peak
}
