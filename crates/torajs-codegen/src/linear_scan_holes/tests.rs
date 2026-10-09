use super::MAX_HOLDERS;
use crate::linear_scan_calls::CallCosts;
use crate::linear_scan_sweep::Sweep;
use crate::live_ranges::LiveRanges;
use crate::liveness::Interval;
use crate::reg::{Reg, aapcs64};
use torajs_core::ssa::id_hash::HashMap;

const GPRS: usize = aapcs64::CALLER_SAVED_SCRATCH.len() + aapcs64::CALLEE_SAVED_SCRATCH.len();
const FPRS: usize = aapcs64::FP_CALLER_SAVED_SCRATCH.len() + aapcs64::FP_CALLEE_SAVED_SCRATCH.len();

/// A sweep over values `0..ranges.len()`, value `v` live on `ranges[v]`,
/// with the given spill weights.
fn sweep(ranges: &[Vec<(u32, u32)>], weights: &[(u32, u64)]) -> Sweep {
    let pairs: Vec<(u32, (u32, u32))> = (ranges.iter().enumerate())
        .flat_map(|(v, r)| r.iter().map(move |&p| (v as u32, p)))
        .collect();
    let ranges = LiveRanges::from_pairs(ranges.len(), &pairs);
    let weights: HashMap<u32, u64> = weights.iter().copied().collect();
    Sweep::new(0, weights, CallCosts::new(&[], &[], &[]), ranges)
}

fn hull(s: &Sweep, v: u32) -> Interval {
    let r = s.ranges.of(v);
    Interval {
        start: r[0].0,
        end: r[r.len() - 1].1,
    }
}

/// What the driver does for a value that crosses no call.
fn place(s: &mut Sweep, v: u32, is_fp: bool) -> Reg {
    let it = hull(s, v);
    s.expire(it.start);
    let reg = s.alloc_noncrossing(v, it, is_fp);
    s.by_value.insert(v, reg);
    s.activate(v, it, reg);
    reg
}

fn active(s: &Sweep) -> Vec<u32> {
    let mut v: Vec<u32> = s.active.iter().map(|e| e.0).collect();
    v.sort_unstable();
    v
}

fn free_gprs(s: &Sweep) -> usize {
    s.free_caller_gpr.len() + s.free_callee_gpr.len()
}

/// Waiting holders come back in the order their next range starts,
/// and a register shared by two of them returns to the pool only when
/// the later one is done.
#[test]
fn waiting_holders_wake_by_next_start() {
    let mut s = sweep(
        &[
            vec![(0, 1), (20, 21)],
            vec![(0, 1), (10, 11)],
            vec![(0, 1), (15, 16)],
            vec![(0, 1), (30, 31)],
            vec![(2, 3), (25, 26)],
        ],
        &[],
    );
    for v in 0..4 {
        place(&mut s, v, false);
    }
    // v4 starts where every holder is in a hole and misses all of them
    let shared = place(&mut s, 4, false);
    assert_eq!(shared, s.by_value[&0]);
    assert_eq!(free_gprs(&s), GPRS - 4);
    s.expire(5);
    assert!(active(&s).is_empty());
    s.expire(10);
    assert_eq!(active(&s), [1]);
    s.expire(15);
    assert_eq!(active(&s), [2]);
    assert_eq!(free_gprs(&s), GPRS - 3, "v1 is done");
    s.expire(20);
    assert_eq!(active(&s), [0]);
    s.expire(25);
    assert_eq!(active(&s), [4]);
    s.expire(30);
    assert_eq!(active(&s), [3]);
    assert_eq!(free_gprs(&s), GPRS - 1, "v3 still holds its register");
    s.expire(40);
    assert_eq!(free_gprs(&s), GPRS);
}

/// Fill every GPR with a holder `g` in a hole at 5 and a holder `a`
/// that shares it from 5 to 10, then let `n` arrive at 10 with the
/// pools empty. `g7` meets `n`.
fn crowded(weights: &[(u32, u64)]) -> (Sweep, u32) {
    let n = GPRS as u32;
    let mut ranges: Vec<Vec<(u32, u32)>> = Vec::new();
    for i in 0..n {
        let back = if i == 7 { 11 } else { 20 };
        ranges.push(vec![(0, 1), (back, 30)]);
    }
    for _ in 0..n {
        ranges.push(vec![(5, 10)]);
    }
    ranges.push(vec![(10, 12)]);
    let mut s = sweep(&ranges, weights);
    for v in 0..2 * n {
        place(&mut s, v, false);
    }
    for i in 0..n {
        assert_eq!(s.by_value[&(n + i)], s.by_value[&i], "a{i} shares g{i}");
    }
    (s, 2 * n)
}

/// The victim must be the only holder of its register that meets the
/// arrival: the lightest active holder whose co-holder meets the
/// arrival is passed over for the next lightest.
#[test]
fn victim_skips_a_register_whose_coholder_meets_the_arrival() {
    let n = GPRS as u32;
    let mut weights: Vec<(u32, u64)> = (0..2 * n).map(|v| (v, 100)).collect();
    weights.push((n + 7, 1));
    weights.push((n + 3, 2));
    weights.push((2 * n, 50));
    let (mut s, arrival) = crowded(&weights);
    let a3 = s.by_value[&(n + 3)];
    let reg = place(&mut s, arrival, false);
    assert_eq!(reg, a3);
    assert!(matches!(s.by_value[&(n + 3)], Reg::SpillGpr(_)));
    assert!(matches!(s.by_value[&(n + 7)], Reg::Gpr(_)));
    assert!(
        matches!(s.by_value[&3], Reg::Gpr(_)),
        "g3 misses the arrival"
    );
}

/// Without a co-holder in the way, the lightest holder is the victim.
#[test]
fn victim_is_the_lightest_when_its_coholders_miss() {
    let n = GPRS as u32;
    let mut weights: Vec<(u32, u64)> = (0..2 * n).map(|v| (v, 100)).collect();
    weights.push((n + 4, 1));
    weights.push((n + 3, 2));
    weights.push((2 * n, 50));
    let (mut s, arrival) = crowded(&weights);
    let a4 = s.by_value[&(n + 4)];
    assert_eq!(place(&mut s, arrival, false), a4);
    assert!(matches!(s.by_value[&(n + 4)], Reg::SpillGpr(_)));
}

/// A vector arrives with every FPR held by an active holder whose
/// co-holder in a hole meets it, so no single victim frees a register:
/// both holders of one register are spilled, and the spilled one in a
/// hole never comes back.
#[test]
fn vector_spills_every_holder_that_meets_it() {
    let n = FPRS as u32;
    let mut ranges: Vec<Vec<(u32, u32)>> = Vec::new();
    for _ in 0..n {
        ranges.push(vec![(0, 1), (20, 30)]);
    }
    for _ in 0..n {
        ranges.push(vec![(5, 10)]);
    }
    ranges.push(vec![(10, 25)]);
    let mut weights: Vec<(u32, u64)> = (0..2 * n).map(|v| (v, 100)).collect();
    weights.push((2, 1));
    weights.push((n + 2, 1));
    let mut s = sweep(&ranges, &weights);
    for v in 0..2 * n {
        place(&mut s, v, true);
    }
    let vector = 2 * n;
    let pair = s.by_value[&2];
    s.unspillable.insert(vector);
    assert_eq!(place(&mut s, vector, true), pair, "the lightest pair's");
    assert!(matches!(s.by_value[&2], Reg::SpillFpr(_)));
    assert!(matches!(s.by_value[&(n + 2)], Reg::SpillFpr(_)));
    s.expire(20);
    assert!(!active(&s).contains(&2));
    assert_eq!(active(&s).len(), n as usize, "the other g's and the vector");
    s.expire(40);
    let free = s.free_caller_fpr.len() + s.free_callee_fpr.len();
    assert_eq!(free, FPRS);
}

/// Values that never meet share one register up to `MAX_HOLDERS`, then
/// move on to the next.
#[test]
fn a_register_takes_at_most_max_holders() {
    let k = MAX_HOLDERS as u32 + 3;
    // value i is live at 2i and again at 100 + i: each one starts in a
    // hole of all the earlier ones
    let ranges: Vec<Vec<(u32, u32)>> = (0..k)
        .map(|i| vec![(2 * i, 2 * i), (100 + i, 100 + i)])
        .collect();
    let mut s = sweep(&ranges, &[]);
    let regs: Vec<Reg> = (0..k).map(|v| place(&mut s, v, false)).collect();
    let first = (regs.iter()).filter(|&&r| r == regs[0]).count();
    assert_eq!(first, MAX_HOLDERS as usize);
    assert_eq!(regs[MAX_HOLDERS as usize], regs[MAX_HOLDERS as usize + 1]);
    s.expire(200);
    assert_eq!(free_gprs(&s), GPRS);
}
