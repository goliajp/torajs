//! Whether a masked group's step packs, and how many pairs fit: the
//! step's instructions all have two-lane forms, and every vector the
//! loop holds at once has a register of its own, since a vector never
//! spills (a spill slot holds 8 bytes).

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    BinOp, BlockId, FPred, Function, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId,
    lowers_to_call, visit_value_operands,
};

use super::super::plan::Ctx;
use super::super::retire::{exits, snapped};
use super::super::{MAX_LANES, Plan};

/// Vector registers the allocator hands out.
const VEC_REGISTERS: usize = 21;
/// Vectors the loop holds beyond the pairs' and the splats outside a
/// step: in `N` the zero splat and one shifted vector, at the group
/// test the or-chain of the flags.
const MIN_TEMPS: usize = 2;

/// How a masked group packs its lanes.
pub(crate) struct Packing {
    /// Lanes in flight, an even count.
    pub lanes: usize,
    /// Vectors per pair: packed slot values, unfrozen snapshots and the
    /// flag.
    pub per_pair: usize,
    /// Distinct loop-invariant operands the step reads, splat once.
    pub splats: usize,
    /// Peak live temporaries in one pair's step, at least `MIN_TEMPS`.
    /// The `go` chain is not one: the flag is read once, at the step's
    /// first test, and its register is free until the step writes it
    /// back — unless the loop calls, when a vector saved around the
    /// call keeps its register to itself and the chain needs one more.
    pub temps: usize,
    /// The slot values the step reads or writes, one vector per pair;
    /// a slot only the code after the inner loop reads stays one
    /// scalar per lane.
    pub packed: HashSet<ValueId>,
    /// Kept cells that freeze themselves (see the module doc), each
    /// with the sum its bump writes.
    pub frozen: Vec<(ValueId, ValueId)>,
    /// The operand every lane's packed slot starts from, where they
    /// all start from one: the pair is a splat of it.
    pub starts: HashMap<ValueId, Operand>,
}

impl Packing {
    pub(crate) fn is_frozen(&self, c: ValueId) -> bool {
        self.frozen.iter().any(|(cell, _)| *cell == c)
    }

    /// The cell whose freezing bump computes `sum`.
    pub(crate) fn bump_of(&self, sum: ValueId) -> Option<ValueId> {
        self.frozen.iter().find(|(_, s)| *s == sum).map(|(c, _)| *c)
    }
}

/// How the group packs, or `None` where some instruction of the step
/// has no two-lane form (the step reads only i64 and f64 values, its
/// compares are read by its branches alone, and nothing reads the
/// counter) or where not even one pair's vectors fit.
pub(crate) fn packing(func: &Function, p: &Plan, ctx: &Ctx) -> Option<Packing> {
    let inner = p.inner_order();
    let insts: Vec<Inst> = inner
        .iter()
        .flat_map(|b| func.blocks[b.0 as usize].insts.iter().cloned())
        .collect();
    let compares = two_lane_forms(func, p, &insts)?;
    // each branch folds a packed compare into the flag
    for b in &inner {
        match func.blocks[b.0 as usize].term {
            Terminator::Br(_) => {}
            Terminator::CondBr {
                cond: Operand::Value(c),
                ..
            } if compares.contains(&c) => {}
            _ => return None,
        }
    }
    let read_after_pre = p
        .read_after_pre
        .as_ref()
        .expect("a masked plan has its reads past pre");
    let lane_owned: HashSet<ValueId> = p
        .pre_order()
        .iter()
        .flat_map(|b| func.blocks[b.0 as usize].insts.iter())
        .filter_map(|i| i.result)
        .filter(|r| Some(*r) != p.skip)
        .chain(p.lane_cells.iter().copied())
        .filter(|r| !p.shared.contains(r) && read_after_pre.contains(r))
        .collect();
    let mut touched: HashSet<ValueId> = HashSet::default();
    for inst in &insts {
        visit_value_operands(&inst.kind, |v| {
            touched.insert(v);
        });
        touched.extend(inst.result);
    }
    let packed: HashSet<ValueId> = lane_owned.intersection(&touched).copied().collect();
    let defined: HashSet<ValueId> = insts.iter().filter_map(|i| i.result).collect();
    let kept = snapped(func, p);
    if carried_outside(&insts, &defined, &packed) || kept.iter().any(|c| !packed.contains(c)) {
        return None;
    }
    let uses = |v: &ValueId| ctx.uses.get(v).copied().unwrap_or(0);
    // a compare is read by the step's branches alone
    for c in &compares {
        let branches = inner.iter().filter(|b| {
            super::super::plan::term_value(&func.blocks[b.0 as usize].term) == Some(*c)
        });
        if uses(c) != branches.count() as u32 {
            return None;
        }
    }
    let mut frozen = frozen_cells(func, p, &inner, &kept);
    // the bump's sum is read by the cell's write alone
    frozen.retain(|(_, s)| uses(s) == 1);
    let starts = common_starts(func, p, ctx, &packed);
    let pk = Packing {
        lanes: 0,
        per_pair: packed.len() + kept.len() - frozen.len() + 1,
        splats: splats(&insts, &defined, &lane_owned, &frozen),
        temps: peak_temps(func, &inner, &packed, &frozen).max(MIN_TEMPS)
            + usize::from(calls(func, p)),
        packed,
        frozen,
        starts,
    };
    let room = VEC_REGISTERS.saturating_sub(pk.splats + pk.temps);
    let pairs = (room / pk.per_pair).min(MAX_LANES / 2);
    (pairs > 0).then_some(Packing {
        lanes: 2 * pairs,
        ..pk
    })
}

/// The step's compares, when every instruction of the step has a
/// two-lane form and reads no bool and not the counter.
fn two_lane_forms(func: &Function, p: &Plan, insts: &[Inst]) -> Option<HashSet<ValueId>> {
    let mut compares: HashSet<ValueId> = HashSet::default();
    for inst in insts {
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
    Some(compares)
}

/// Whether the step reads a value of its own ahead of writing it, other
/// than a packed slot: such a value would carry from one step to the
/// next outside the pair's vectors.
fn carried_outside(insts: &[Inst], defined: &HashSet<ValueId>, packed: &HashSet<ValueId>) -> bool {
    let mut written: HashSet<ValueId> = HashSet::default();
    let mut carried = false;
    for inst in insts {
        visit_value_operands(&inst.kind, |v| {
            carried |= defined.contains(&v) && !packed.contains(&v) && !written.contains(&v);
        });
        written.extend(inst.result);
    }
    carried
}

/// Distinct operands the step reads from outside its lanes; a frozen
/// bump by one reads none (it steps by its own mask).
fn splats(
    insts: &[Inst],
    defined: &HashSet<ValueId>,
    lane_owned: &HashSet<ValueId>,
    frozen: &[(ValueId, ValueId)],
) -> usize {
    let mut invariant: HashSet<Operand> = HashSet::default();
    for inst in insts {
        let bump = frozen.iter().any(|(_, s)| inst.result == Some(*s));
        if bump && bump_by(&inst.kind).is_some_and(|(_, k)| k == 1) {
            continue;
        }
        for op in args(&inst.kind) {
            let own =
                matches!(op, Operand::Value(v) if defined.contains(&v) || lane_owned.contains(&v));
            if !own {
                invariant.insert(op);
            }
        }
    }
    invariant.len()
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

/// `c + k` (`k + c`) as the cell and the constant.
pub(crate) fn bump_by(kind: &InstKind) -> Option<(ValueId, i64)> {
    match *kind {
        InstKind::BinOp(BinOp::Add, Operand::Value(c), Operand::ConstI64(k))
        | InstKind::BinOp(BinOp::Add, Operand::ConstI64(k), Operand::Value(c)) => Some((c, k)),
        _ => None,
    }
}

/// The kept cells that may freeze themselves, with their sums: the
/// step's one write of `c` copies `c + k`, the sum and the write both
/// come after the step's last exit test, and nothing else reads the
/// sum (`packing` checks that last). A bump ahead of an exit would
/// leave a lane that exits there holding `c + k` where its snapshot
/// holds `c`.
fn frozen_cells(
    func: &Function,
    p: &Plan,
    inner: &[BlockId],
    kept: &[ValueId],
) -> Vec<(ValueId, ValueId)> {
    let Some(&(last, _)) = exits(func, inner).last() else {
        return Vec::new();
    };
    if p.exit_cells.is_none() {
        return Vec::new();
    }
    let at = |b: &BlockId| func.blocks[b.0 as usize].insts.iter();
    let step: Vec<&Inst> = inner.iter().flat_map(at).collect();
    let after: Vec<&Inst> = inner[last + 1..].iter().flat_map(at).collect();
    let freezes = |c: ValueId| -> Option<ValueId> {
        let [write] = step
            .iter()
            .filter(|i| i.result == Some(c))
            .collect::<Vec<_>>()[..]
        else {
            return None;
        };
        let InstKind::Copy(_, Operand::Value(sum)) = write.kind else {
            return None;
        };
        let def = after.iter().find(|i| i.result == Some(sum))?;
        let late = after.iter().any(|i| std::ptr::eq(*i, *write));
        (late && bump_by(&def.kind)?.0 == c).then_some(sum)
    };
    kept.iter()
        .filter_map(|&c| freezes(c).map(|s| (c, s)))
        .collect()
}

/// The operand every lane's packed slot `v` starts from where `pre`
/// defines `v` once, copying an operand no lane renames (a value
/// neither `pre` defines nor a lane cell nor the counter): a constant,
/// or such a value, followed to the constant its one definition copies.
/// Every lane reads it in `GB`, where nothing writes it.
fn common_starts(
    func: &Function,
    p: &Plan,
    ctx: &Ctx,
    packed: &HashSet<ValueId>,
) -> HashMap<ValueId, Operand> {
    let pre = p.pre_order();
    let defs = |v: ValueId| ctx.defs.get(&v).map_or(&[][..], Vec::as_slice);
    let constant = |op: Operand| matches!(op, Operand::ConstF64(_) | Operand::ConstI64(_));
    let copies = |(b, at): (BlockId, usize)| match func.blocks[b.0 as usize].insts[at].kind {
        InstKind::Copy(_, src) => Some(src),
        _ => None,
    };
    let mut out: HashMap<ValueId, Operand> = HashMap::default();
    for &v in packed {
        let mut in_pre = defs(v).iter().filter(|(b, _)| pre.contains(b));
        let (Some(&def), None) = (in_pre.next(), in_pre.next()) else {
            continue;
        };
        let start = match copies(def) {
            Some(c) if constant(c) => c,
            Some(Operand::Value(x))
                if x != p.iv
                    && !p.lane_cells.contains(&x)
                    && !defs(x).iter().any(|(b, _)| pre.contains(b)) =>
            {
                match defs(x) {
                    &[one] => copies(one).filter(|c| constant(*c)),
                    _ => None,
                }
                .unwrap_or(Operand::Value(x))
            }
            _ => continue,
        };
        out.insert(v, start);
    }
    out
}

/// Peak count of one pair's step's own vectors live at once: each
/// value the step defines other than a packed slot (a cell of the
/// step's own from its first write) lives to its last read, and a
/// compare to the end of its block, where the flag reads it; a frozen
/// bump by more than one holds its masked step for an instant.
fn peak_temps(
    func: &Function,
    inner: &[BlockId],
    packed: &HashSet<ValueId>,
    frozen: &[(ValueId, ValueId)],
) -> usize {
    // one position per instruction and one per block end
    let mut last: HashMap<ValueId, usize> = HashMap::default();
    let mut at = 0;
    for b in inner {
        let block = &func.blocks[b.0 as usize];
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |v| {
                last.insert(v, at);
            });
            at += 1;
        }
        if let Some(c) = super::super::plan::term_value(&block.term) {
            last.insert(c, at);
        }
        at += 1;
    }
    let mut live: HashSet<ValueId> = HashSet::default();
    let (mut peak, mut at) = (0usize, 0);
    let ends_after = |v: &ValueId, at: usize| last.get(v).is_some_and(|&end| end > at);
    for b in inner {
        for inst in &func.blocks[b.0 as usize].insts {
            // an operand read for the last time frees its register to
            // the result
            live.retain(|v| ends_after(v, at));
            if let Some(r) = inst.result.filter(|r| !packed.contains(r)) {
                live.insert(r);
            }
            let masked_step = frozen.iter().any(|(_, s)| inst.result == Some(*s))
                && bump_by(&inst.kind).is_some_and(|(_, k)| k != 1);
            peak = peak.max(live.len() + usize::from(masked_step));
            // a result nothing reads later is gone at once
            live.retain(|v| ends_after(v, at));
            at += 1;
        }
        live.retain(|v| ends_after(v, at));
        at += 1;
    }
    peak
}
