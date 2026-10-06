//! Whether a planned loop is worth the rewrite and safe to run early:
//! size limits, purity of the parts lanes run ahead, and an inner loop
//! whose step waits on a long-latency operation.

use crate::id_hash::HashMap;

use torajs_core::ssa::{
    BinOp, BlockId, Function, InstKind, Operand, Terminator, ValueId, visit_value_operands,
};

use super::clone::reads_outside;
use super::plan::blk;
use super::{MAX_INNER_INSTS, MAX_LANES, MAX_POST_INSTS, MAX_PRE_INSTS, MIN_LANES, Plan};

/// Cycles one inner step must wait on its own previous step before the
/// interleaving pays: below this, a single lane already keeps the core
/// busy.
const MIN_CARRIED_LATENCY: u32 = 3;

/// Cycles between two divides the core can start: the integer divider
/// takes a new `sdiv` every 2 cycles (Apple M-series, eight independent
/// `sdiv` chains measured at 16 cycles per round). The float divider
/// keeps up with eight `fdiv` chains at their 13-cycle latency, so it is
/// counted as one.
const INT_DIV_ISSUE: u32 = 2;
const FLOAT_DIV_ISSUE: u32 = 1;

/// General registers the lanes' own cells may take, out of the ~21 the
/// allocator hands out, leaving the rest to shared values and the outer
/// loop.
const LANE_GPR_BUDGET: usize = 18;

/// Pure, trap-free, reads no memory.
fn pure(kind: &InstKind) -> bool {
    match kind {
        InstKind::BinOp(op, _, _) => *op != BinOp::FRem,
        InstKind::ICmp(..)
        | InstKind::FCmp(..)
        | InstKind::Copy(..)
        | InstKind::Select(..)
        | InstKind::SiToFp(_)
        | InstKind::FpToSi(_)
        | InstKind::FpToInt32(_)
        | InstKind::ZExtBoolToI64(_)
        | InstKind::ZExtI32ToI64(_)
        | InstKind::BitCastF64ToI64(_)
        | InstKind::BitCastI64ToF64(_)
        | InstKind::TruncI64ToBool(_)
        | InstKind::Neg(_)
        | InstKind::Ctpop(_)
        | InstKind::Identity(_) => true,
        _ => false,
    }
}

/// Size limits, purity of the parts lanes run early, and latency for
/// the interleaving to hide: a divide on the inner loop (its throughput
/// is the bound once lanes overlap), or a value the inner loop carries
/// from one step to the next through a chain of at least
/// `MIN_CARRIED_LATENCY` cycles (a multiply-add recurrence).
pub(super) fn regions_fit(func: &Function, p: &Plan) -> bool {
    let count = |bs: &[BlockId]| bs.iter().map(|b| blk(func, *b).insts.len()).sum::<usize>();
    let early = p.pre_order().into_iter().chain(p.inner_order());
    let all_pure = early
        .flat_map(|b| blk(func, b).insts.iter())
        .all(|i| i.result.is_some() && pure(&i.kind));
    let inner = p.inner_order();
    let divides = inner
        .iter()
        .flat_map(|b| blk(func, *b).insts.iter())
        .any(|i| {
            matches!(
                i.kind,
                InstKind::BinOp(BinOp::SDiv | BinOp::SRem | BinOp::FDiv, _, _)
            )
        });
    all_pure
        && count(&p.pre_order()) <= MAX_PRE_INSTS
        && count(&inner) <= MAX_INNER_INSTS
        && count(&p.post) <= MAX_POST_INSTS
        && (divides || carried_latency(func, &inner) >= MIN_CARRIED_LATENCY)
}

/// Lanes for a divide-bound inner loop whose lanes step in lockstep:
/// enough recurrences in flight to cover one step's chain while the
/// divider starts each lane's divides back to back (`ceil(chain /
/// issue)`), within the register budget for the lanes' cells.
/// `r = (r + k) % i` under a shared `i <= m` (13 cycles, one divide)
/// takes 7 lanes.
///
/// The rest keep the minimum. Lanes whose inner loops end apart (gcd's
/// `b != 0`) leave the fused loop at the first lane done and finish
/// alone at the chain's latency, so a wider group lengthens that tail:
/// gcd1m at 6 lanes ran 7% fewer cycles but 0.7% more instructions. A
/// multiply-add chain is short enough that more lanes only add
/// register pressure (collatz at 8 lanes: 5% more cycles).
pub(super) fn lane_count(func: &Function, p: &Plan) -> usize {
    let inner = p.inner_order();
    if !lockstep(func, p) {
        return MIN_LANES;
    }
    let issue: u32 = inner
        .iter()
        .flat_map(|b| blk(func, *b).insts.iter())
        .map(|i| match i.kind {
            InstKind::BinOp(BinOp::SDiv | BinOp::SRem, _, _) => INT_DIV_ISSUE,
            InstKind::BinOp(BinOp::FDiv, _, _) => FLOAT_DIV_ISSUE,
            _ => 0,
        })
        .sum();
    if issue == 0 {
        return MIN_LANES;
    }
    let wanted = carried_latency(func, &inner).div_ceil(issue) as usize;
    // each lane holds, across the fused loop, its counter and its own
    // cells read past `pre`
    let read_after_pre = reads_outside(func, &p.pre_order());
    let per_lane = p
        .lane_cells
        .iter()
        .filter(|c| !p.shared.contains(c) && read_after_pre.contains(c))
        .count()
        + 1;
    wanted
        .min(LANE_GPR_BUDGET / per_lane)
        .clamp(MIN_LANES, MAX_LANES)
}

/// Every lane's inner loop runs as many steps as every other's: the
/// inner test reads only values the lanes compute alike.
fn lockstep(func: &Function, p: &Plan) -> bool {
    match blk(func, p.inner_header).term {
        Terminator::CondBr {
            cond: Operand::Value(c),
            ..
        } => p.shared.contains(&c),
        _ => false,
    }
}

/// Approximate result latency in cycles of a pure instruction on a
/// current out-of-order core; a copy is a rename.
fn latency(kind: &InstKind) -> u32 {
    match kind {
        InstKind::Copy(..) | InstKind::Identity(_) => 0,
        // `srem` is an `sdiv` and an `msub` (4)
        InstKind::BinOp(BinOp::SDiv, _, _) => 8,
        InstKind::BinOp(BinOp::SRem, _, _) => 12,
        InstKind::BinOp(BinOp::FDiv, _, _) => 10,
        InstKind::BinOp(BinOp::Mul | BinOp::FAdd | BinOp::FSub | BinOp::FMul, _, _)
        | InstKind::SiToFp(_)
        | InstKind::FpToSi(_)
        | InstKind::FpToInt32(_)
        | InstKind::BitCastF64ToI64(_)
        | InstKind::BitCastI64ToF64(_) => 3,
        _ => 1,
    }
}

/// The longest chain, in cycles, from a value one inner step starts
/// with to the value it leaves for the next step under the same id:
/// the recurrence that bounds the inner loop's speed when it runs
/// alone. `inner` is the inner header then the body, in step order.
fn carried_latency(func: &Function, inner: &[BlockId]) -> u32 {
    let insts = || inner.iter().flat_map(|b| blk(func, *b).insts.iter());
    let mut carried: Vec<ValueId> = insts()
        .filter(|i| matches!(i.kind, InstKind::Copy(..)))
        .filter_map(|i| i.result)
        .collect();
    carried.sort_by_key(|v| v.0);
    carried.dedup();
    let mut longest = 0;
    for &start in &carried {
        // cycles from `start`'s value at the top of the step to each
        // value it reaches; a redef from values it does not reach drops
        // the entry
        let mut dist: HashMap<ValueId, u32> = HashMap::default();
        dist.insert(start, 0);
        for inst in insts() {
            let Some(r) = inst.result else { continue };
            let mut reach: Option<u32> = None;
            visit_value_operands(&inst.kind, |v| {
                if let Some(&d) = dist.get(&v) {
                    reach = Some(reach.map_or(d, |m| m.max(d)));
                }
            });
            match reach {
                Some(d) => dist.insert(r, d + latency(&inst.kind)),
                None => dist.remove(&r),
            };
        }
        longest = longest.max(dist.get(&start).copied().unwrap_or(0));
    }
    longest
}
