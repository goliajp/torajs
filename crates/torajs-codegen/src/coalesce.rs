//! Copy coalescing for the Linear-Scan allocator.
//!
//! φ destruction leaves `%w = copy %v` at the end of every predecessor
//! of a join. When `v` and `w` never hold different live values at
//! the same point, giving them one register turns the copy into
//! nothing (`emit_copy` skips a move whose source and destination
//! coincide). This module groups such values into classes; the sweep
//! allocates one register per class over the class's hull.
//!
//! Interference is tested per position, not per interval: a
//! loop-carried φ value's interval is the whole loop, so a hull test
//! would reject every candidate. Each inst slot `p` has two halves —
//! reads at `2p`, writes at `2p + 1` — so `y = op(x)` with `x` dying
//! at `p` does not interfere with `y` (Chaitin's copy exception falls
//! out of the numbering), while a value still read after another is
//! written does. The live segments come from the block live sets
//! `liveness::compute_liveness` already computes.
//!
//! Classes are formed greedily in copy order with union-find. A merge
//! whose hull would newly cross a call when neither side did is
//! refused: it would push a short caller-saved value into a
//! callee-saved register (or a spill) for nothing.
//!
//! Sharing a register with an operand means every emitter must read
//! all of its operands before it first writes `dst`; the emitters all
//! do (`regalloc::Assignment::def_gpr`).
//!
//! `TORAJS_COALESCE_OFF=1` disables coalescing (bisect gate).

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{Function, InstKind, Operand, VecInst};

use crate::linear_scan_calls::crosses_call;
use crate::liveness::{Interval, visit_inst_operands, visit_terminator_operands};
use crate::liveness_blocks::BlockLiveness;

/// Inclusive half-slot ranges, sorted by start and pairwise disjoint.
type Segments = Vec<(u32, u32)>;

struct Class {
    members: Vec<u32>,
    segs: Segments,
    hull: Interval,
}

/// Copy classes of two or more values, keyed by leader.
#[derive(Default)]
pub(crate) struct Classes {
    leader_of: HashMap<u32, u32>,
    classes: HashMap<u32, Class>,
}

impl Classes {
    /// Replace every member by its class leader: the leader's interval
    /// becomes the class hull and its weight the members' sum.
    pub(crate) fn fold_into(
        &self,
        intervals: &mut HashMap<u32, Interval>,
        weights: &mut HashMap<u32, u64>,
    ) {
        for (&leader, class) in &self.classes {
            let mut weight: u64 = 0;
            for &m in &class.members {
                weight = weight.saturating_add(weights.remove(&m).unwrap_or(0));
                intervals.remove(&m);
            }
            weights.insert(leader, weight);
            intervals.insert(leader, class.hull);
        }
    }

    /// The value whose home `v` shares: its class leader, or itself.
    pub(crate) fn leader(&self, v: u32) -> u32 {
        self.leader_of.get(&v).copied().unwrap_or(v)
    }

    /// Give every member its leader's final home.
    pub(crate) fn fan_out(&self, by_value: &mut HashMap<u32, crate::reg::Reg>) {
        for (&m, &leader) in &self.leader_of {
            if m != leader {
                let reg = by_value[&leader];
                by_value.insert(m, reg);
            }
        }
    }
}

/// Build copy classes. `excluded` holds values pinned to a fixed
/// lane (params, ret values); allocas never coalesce.
pub(crate) fn build_copy_classes(
    func: &Function,
    intervals: &HashMap<u32, Interval>,
    blocks: &BlockLiveness,
    excluded: &HashSet<u32>,
    call_sites: &[u32],
) -> Classes {
    if std::env::var_os("TORAJS_COALESCE_OFF").is_some_and(|v| v == "1") {
        return Classes::default();
    }
    let copies = collect_copies(func, intervals, excluded);
    if copies.is_empty() {
        return Classes::default();
    }
    let cands: HashSet<u32> = copies.iter().flat_map(|&(w, v)| [w, v]).collect();
    let mut segs = live_segments(func, blocks, &cands);

    let mut leader_of: HashMap<u32, u32> = HashMap::new();
    let mut classes: HashMap<u32, Class> = HashMap::new();
    let root = |leader_of: &HashMap<u32, u32>, v: u32| *leader_of.get(&v).unwrap_or(&v);
    for (w, v) in copies {
        let (rw, rv) = (root(&leader_of, w), root(&leader_of, v));
        if rw == rv {
            continue;
        }
        let a = classes
            .remove(&rw)
            .unwrap_or_else(|| singleton(rw, &mut segs, intervals));
        let b = classes
            .remove(&rv)
            .unwrap_or_else(|| singleton(rv, &mut segs, intervals));
        match try_merge(a, b, call_sites) {
            Ok(merged) => {
                for &m in &merged.members {
                    leader_of.insert(m, rw);
                }
                classes.insert(rw, merged);
            }
            Err((a, b)) => {
                classes.insert(rw, a);
                classes.insert(rv, b);
            }
        }
    }
    classes.retain(|_, c| c.members.len() > 1);
    leader_of.retain(|_, l| classes.contains_key(l));
    Classes { leader_of, classes }
}

/// `(w, v)` for every `w = copy v` both of whose values may share a
/// register: same register class, not pinned, not an alloca.
fn collect_copies(
    func: &Function,
    intervals: &HashMap<u32, Interval>,
    excluded: &HashSet<u32>,
) -> Vec<(u32, u32)> {
    let mut allocas: HashSet<u32> = HashSet::new();
    let mut copies: Vec<(u32, u32)> = Vec::new();
    for inst in func.blocks.iter().flat_map(|b| &b.insts) {
        match (&inst.kind, inst.result) {
            (InstKind::Alloca(_) | InstKind::AllocaBytes(_), Some(r)) => {
                allocas.insert(r.0);
            }
            (InstKind::Copy(_, Operand::Value(v)), Some(w)) => copies.push((w.0, v.0)),
            // a lane pair starts as its low operand's register
            (InstKind::Vec(VecInst::Pair(Operand::Value(v), _)), Some(w)) => {
                copies.push((w.0, v.0))
            }
            _ => {}
        }
    }
    let params: HashSet<u32> = func.params.iter().map(|p| p.0).collect();
    let is_fp = |v: u32| func.values[v as usize].ty.is_fpr_class();
    copies.retain(|&(w, v)| {
        w != v
            && [w, v].iter().all(|x| {
                intervals.contains_key(x)
                    && !excluded.contains(x)
                    && !params.contains(x)
                    && !allocas.contains(x)
            })
            && is_fp(w) == is_fp(v)
    });
    copies
}

fn singleton(
    v: u32,
    segs: &mut HashMap<u32, Segments>,
    intervals: &HashMap<u32, Interval>,
) -> Class {
    let mut s = segs.remove(&v).unwrap_or_default();
    s.sort_unstable();
    Class {
        members: vec![v],
        segs: s,
        hull: intervals[&v],
    }
}

fn try_merge(a: Class, b: Class, call_sites: &[u32]) -> Result<Class, (Class, Class)> {
    let hull = Interval {
        start: a.hull.start.min(b.hull.start),
        end: a.hull.end.max(b.hull.end),
    };
    let newly_crossing = crosses_call(hull, call_sites)
        && !crosses_call(a.hull, call_sites)
        && !crosses_call(b.hull, call_sites);
    if newly_crossing || overlaps(&a.segs, &b.segs) {
        return Err((a, b));
    }
    let mut segs = Vec::with_capacity(a.segs.len() + b.segs.len());
    let (mut i, mut j) = (0, 0);
    while i < a.segs.len() || j < b.segs.len() {
        if j == b.segs.len() || (i < a.segs.len() && a.segs[i] < b.segs[j]) {
            segs.push(a.segs[i]);
            i += 1;
        } else {
            segs.push(b.segs[j]);
            j += 1;
        }
    }
    let mut members = a.members;
    members.extend(b.members);
    Ok(Class {
        members,
        segs,
        hull,
    })
}

fn overlaps(a: &Segments, b: &Segments) -> bool {
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].1 < b[j].0 {
            i += 1;
        } else if b[j].1 < a[i].0 {
            j += 1;
        } else {
            return true;
        }
    }
    false
}

/// Half-slot ranges where each candidate holds a live value, from one
/// backward walk per block. A def with no later read still occupies
/// its write half: the register is clobbered there.
fn live_segments(
    func: &Function,
    blocks: &BlockLiveness,
    cands: &HashSet<u32>,
) -> HashMap<u32, Segments> {
    let mut segs: HashMap<u32, Segments> = HashMap::new();
    for (b, block) in func.blocks.iter().enumerate() {
        let (bs, be) = (blocks.start[b], blocks.end[b]);
        // live candidate → end of its currently open segment
        let mut open: HashMap<u32, u32> = blocks.live_out[b]
            .iter()
            .filter(|v| cands.contains(v))
            .map(|&v| (v, 2 * be + 1))
            .collect();
        visit_terminator_operands(&block.term, |v| {
            if cands.contains(&v.0) {
                open.entry(v.0).or_insert(2 * be);
            }
        });
        for (k, inst) in block.insts.iter().enumerate().rev() {
            let p = bs + k as u32;
            if let Some(r) = inst.result
                && cands.contains(&r.0)
            {
                let end = open.remove(&r.0).unwrap_or(2 * p + 1);
                segs.entry(r.0).or_default().push((2 * p + 1, end));
            }
            visit_inst_operands(&inst.kind, |v| {
                if cands.contains(&v.0) {
                    open.entry(v.0).or_insert(2 * p);
                }
            });
        }
        for (v, end) in open {
            segs.entry(v).or_default().push((2 * bs, end));
        }
    }
    segs
}

#[cfg(test)]
mod tests {
    use crate::linear_scan::allocate_linear_scan;
    use crate::reg::Reg;
    use torajs_core::ssa::{
        BinOp, Block, BlockId, Function, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId,
        ValueInfo,
    };

    fn v(n: u32) -> Operand {
        Operand::Value(ValueId(n))
    }

    fn def(r: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(r)),
            kind,
            origin: None,
        }
    }

    fn add(r: u32, a: Operand, b: Operand) -> Inst {
        def(r, InstKind::BinOp(BinOp::Add, a, b))
    }

    fn copy(r: u32, src: Operand) -> Inst {
        def(r, InstKind::Copy(Type::I64, src))
    }

    fn block(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }

    fn br(t: u32) -> Terminator {
        Terminator::Br(BlockId(t))
    }

    fn cond(c: u32, t: u32, e: u32) -> Terminator {
        Terminator::CondBr {
            cond: v(c),
            then_blk: BlockId(t),
            else_blk: BlockId(e),
        }
    }

    /// `bools` lists the ValueIds typed Bool; every other value is I64.
    fn func(nvals: u32, params: &[u32], bools: &[u32], blocks: Vec<Block>) -> Function {
        Function {
            name: "f".into(),
            params: params.iter().map(|&p| ValueId(p)).collect(),
            ret: Type::I64,
            blocks,
            values: (0..nvals)
                .map(|i| ValueInfo {
                    ty: if bools.contains(&i) {
                        Type::Bool
                    } else {
                        Type::I64
                    },
                    name: None,
                })
                .collect(),
            current_origin: None,
        }
    }

    fn home(f: &Function, n: u32) -> Reg {
        allocate_linear_scan(f).of(ValueId(n))
    }

    /// `i = 0; do { j = i + 1 } while (j < 10)` after φ destruction:
    /// the latch copy `i = j` becomes a no-op.
    #[test]
    fn loop_increment_shares_register() {
        let f = func(
            5,
            &[],
            &[3],
            vec![
                block(0, vec![copy(1, Operand::ConstI64(0))], br(1)),
                block(
                    1,
                    vec![
                        add(2, v(1), Operand::ConstI64(1)),
                        def(3, InstKind::ICmp(IPred::Slt, v(2), Operand::ConstI64(10))),
                    ],
                    cond(3, 3, 2),
                ),
                block(3, vec![copy(1, v(2))], br(1)),
                block(
                    2,
                    vec![add(4, v(2), Operand::ConstI64(0))],
                    Terminator::Ret(Some(v(4))),
                ),
            ],
        );
        assert_eq!(home(&f, 1), home(&f, 2));
    }

    /// `b = a; a = c; use(a, b)` — `b` is still read after `a` is
    /// rewritten, so they must not share a register.
    #[test]
    fn copy_source_rewritten_while_dest_live_refuses() {
        let f = func(
            5,
            &[],
            &[],
            vec![block(
                0,
                vec![
                    copy(1, Operand::ConstI64(1)),
                    copy(3, Operand::ConstI64(2)),
                    copy(2, v(1)),
                    copy(1, v(3)),
                    add(4, v(1), v(2)),
                ],
                Terminator::Ret(Some(v(4))),
            )],
        );
        let a = allocate_linear_scan(&f);
        assert_ne!(a.of(ValueId(1)), a.of(ValueId(2)));
    }

    /// A join value written by a copy in each predecessor joins both
    /// sources in one class.
    #[test]
    fn phi_web_joins_every_predecessor_source() {
        let f = func(
            6,
            &[0],
            &[5],
            vec![
                block(
                    0,
                    vec![def(
                        5,
                        InstKind::ICmp(IPred::Slt, v(0), Operand::ConstI64(0)),
                    )],
                    cond(5, 1, 2),
                ),
                block(
                    1,
                    vec![add(2, v(0), Operand::ConstI64(1)), copy(1, v(2))],
                    br(3),
                ),
                block(
                    2,
                    vec![add(3, v(0), Operand::ConstI64(2)), copy(1, v(3))],
                    br(3),
                ),
                block(
                    3,
                    vec![add(4, v(1), Operand::ConstI64(3))],
                    Terminator::Ret(Some(v(4))),
                ),
            ],
        );
        let a = allocate_linear_scan(&f);
        assert_eq!(a.of(ValueId(1)), a.of(ValueId(2)));
        assert_eq!(a.of(ValueId(1)), a.of(ValueId(3)));
    }

    /// A source defined in an earlier block merges with a copy whose
    /// source dies there.
    #[test]
    fn cross_block_source_merges() {
        let f = func(
            5,
            &[0],
            &[],
            vec![
                block(0, vec![add(1, v(0), Operand::ConstI64(1))], br(1)),
                block(
                    1,
                    vec![add(2, v(0), Operand::ConstI64(7)), copy(3, v(1))],
                    br(2),
                ),
                block(2, vec![add(4, v(3), v(2))], Terminator::Ret(Some(v(4)))),
            ],
        );
        assert_eq!(home(&f, 1), home(&f, 3));
    }

    /// Under pressure past the register pools, a class still has one
    /// home — a spilled class shares one slot.
    #[test]
    fn class_members_share_home_under_pressure() {
        let n = 24;
        let mut insts: Vec<Inst> = (1..=n)
            .map(|i| add(i, v(0), Operand::ConstI64(i as i64)))
            .collect();
        let (a, b) = (n + 1, n + 2);
        insts.push(add(a, v(1), v(2)));
        insts.push(copy(b, v(a)));
        let mut acc = b;
        for i in 3..=n {
            let next = n + i;
            insts.push(add(next, v(acc), v(i)));
            acc = next;
        }
        let f = func(
            acc + 1,
            &[0],
            &[],
            vec![block(0, insts, Terminator::Ret(Some(v(acc))))],
        );
        let alloc = allocate_linear_scan(&f);
        assert_eq!(alloc.of(ValueId(a)), alloc.of(ValueId(b)));
        assert!(
            (1..=n).any(|i| matches!(alloc.of(ValueId(i)), Reg::SpillGpr(_))),
            "fixture must exceed the GPR pools"
        );
    }
}
