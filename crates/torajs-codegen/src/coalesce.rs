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
//! A copy whose destination has no other def holds the same value as
//! its source until the source is written again, so the two may share
//! a register while both are live (Boissinot et al. 2009, "Revisiting
//! Out-of-SSA Translation", value-based interference): such a pair
//! merges unless a member of the source's class is defined somewhere
//! the destination is live. `ptrtoint` / `inttoptr` move the same bits
//! and count as copies; the emitters of all three move nothing for a
//! coalesced pair. A lane pair coalesces with its low operand only
//! where the two are never live together: its high lane differs.
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
//! `TORAJS_COALESCE_OFF=1` disables coalescing, and
//! `TORAJS_COALESCE_LIVE_OFF=1` only the merging of a copy with its
//! source while both are live (bisect gates).

use torajs_core::ssa::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{Function, InstKind, Operand, VecInst, VecShape};

use crate::linear_scan_calls::crosses_call;
use crate::liveness::Interval;
use crate::liveness_blocks::BlockLiveness;

/// Inclusive half-slot ranges, sorted by start and pairwise disjoint.
type Segments = Vec<(u32, u32)>;

mod segments;
use segments::{Live, NONE, any_inside, live_segments, overlaps, union};

struct Class {
    members: Vec<u32>,
    segs: Segments,
    hull: Interval,
    /// Write half-slots of the members' defs, ascending.
    defs: Vec<u32>,
}

/// A copy candidate `w = copy v`; `exact` when `w` holds exactly the
/// bits of `v` (a copy, `ptrtoint`, `inttoptr`; not a lane pair).
struct Copy {
    w: u32,
    v: u32,
    exact: bool,
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

    /// Every class, as its leader and its members (the leader included).
    pub(crate) fn iter(&self) -> impl Iterator<Item = (u32, &[u32])> {
        (self.classes.iter()).map(|(&l, c)| (l, c.members.as_slice()))
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
/// lane (params, ret values); allocas never coalesce. `widened` holds
/// the widenings of a 32-bit load (`zext_load::widened_loads`).
pub(crate) fn build_copy_classes(
    func: &Function,
    intervals: &HashMap<u32, Interval>,
    blocks: &BlockLiveness,
    excluded: &HashSet<u32>,
    call_sites: &[u32],
    widened: &HashSet<u32>,
) -> Classes {
    let switches = crate::switches::get();
    if switches.coalesce_off {
        return Classes::default();
    }
    let copies = collect_copies(func, intervals, excluded, widened);
    if copies.is_empty() {
        return Classes::default();
    }
    let live_ok = !switches.coalesce_live_off;
    // candidate index per value: the walk asks once per operand, too
    // often for a hash lookup
    let mut slot = vec![NONE; func.values.len()];
    let mut cands = 0;
    for x in copies.iter().flat_map(|c| [c.w, c.v]) {
        if slot[x as usize] == NONE {
            slot[x as usize] = cands;
            cands += 1;
        }
    }
    let mut live = live_segments(func, blocks, &slot, cands as usize);

    let mut leader_of: HashMap<u32, u32> = HashMap::default();
    let mut classes: HashMap<u32, Class> = HashMap::default();
    let root = |leader_of: &HashMap<u32, u32>, v: u32| *leader_of.get(&v).unwrap_or(&v);
    for Copy { w, v, exact } in copies {
        let (rw, rv) = (root(&leader_of, w), root(&leader_of, v));
        if rw == rv {
            continue;
        }
        let a = classes
            .remove(&rw)
            .unwrap_or_else(|| singleton(rw, &slot, &mut live, intervals));
        let b = classes
            .remove(&rv)
            .unwrap_or_else(|| singleton(rv, &slot, &mut live, intervals));
        if mergeable(&a, &b, exact && live_ok, call_sites) {
            let from = a.members.len();
            let merged = merge(a, b);
            // the members `a` brought already answer `rw`
            leader_of.insert(rw, rw);
            for &m in &merged.members[from..] {
                leader_of.insert(m, rw);
            }
            classes.insert(rw, merged);
        } else {
            classes.insert(rw, a);
            classes.insert(rv, b);
        }
    }
    classes.retain(|_, c| c.members.len() > 1);
    leader_of.retain(|_, l| classes.contains_key(l));
    Classes { leader_of, classes }
}

/// Every `w = copy v` (or a `ptrtoint` / `inttoptr`, or a widening of
/// a 32-bit load in `widened` (`zext_load`), each of which moves the
/// same bits, or a lane pair, which starts as its low operand's
/// register) both of whose values may share a register: same register
/// class, not pinned, not an alloca.
fn collect_copies(
    func: &Function,
    intervals: &HashMap<u32, Interval>,
    excluded: &HashSet<u32>,
    widened: &HashSet<u32>,
) -> Vec<Copy> {
    let mut allocas: HashSet<u32> = HashSet::default();
    let mut copies: Vec<Copy> = Vec::new();
    for inst in func.blocks.iter().flat_map(|b| &b.insts) {
        let (exact, v) = match &inst.kind {
            InstKind::ZExtI32ToI64(Operand::Value(v))
                if inst.result.is_some_and(|r| widened.contains(&r.0)) =>
            {
                (true, v.0)
            }
            InstKind::Alloca(_) | InstKind::AllocaBytes(_) => {
                allocas.extend(inst.result.map(|r| r.0));
                continue;
            }
            InstKind::Copy(_, Operand::Value(v))
            | InstKind::PtrToInt(Operand::Value(v))
            | InstKind::IntToPtr(Operand::Value(v)) => (true, v.0),
            InstKind::Vec(VecInst::Pair(VecShape::F64x2, Operand::Value(v), _)) => (false, v.0),
            _ => continue,
        };
        if let Some(w) = inst.result {
            copies.push(Copy { w: w.0, v, exact });
        }
    }
    let params: HashSet<u32> = func.params.iter().map(|p| p.0).collect();
    let is_fp = |v: u32| func.values[v as usize].ty.is_fpr_class();
    copies.retain(|&Copy { w, v, .. }| {
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

fn singleton(v: u32, slot: &[u32], live: &mut Live, intervals: &HashMap<u32, Interval>) -> Class {
    let i = slot[v as usize] as usize;
    Class {
        members: vec![v],
        segs: std::mem::take(&mut live.segs[i]),
        hull: intervals[&v],
        defs: std::mem::take(&mut live.defs[i]),
    }
}

fn joint_hull(a: &Class, b: &Class) -> Interval {
    Interval {
        start: a.hull.start.min(b.hull.start),
        end: a.hull.end.max(b.hull.end),
    }
}

/// Whether the class `a` of a copy's destination may merge into the
/// class `b` of its source. Besides disjoint classes, when the copy
/// moves the source's bits unchanged (`exact`), a destination with that
/// copy as its only def may join while live together with the source:
/// it holds the source's value until a member of `b` is written, so no
/// such write may fall where `a` is live.
fn mergeable(a: &Class, b: &Class, exact: bool, call_sites: &[u32]) -> bool {
    let newly_crossing = crosses_call(joint_hull(a, b), call_sites)
        && !crosses_call(a.hull, call_sites)
        && !crosses_call(b.hull, call_sites);
    if newly_crossing {
        return false;
    }
    !overlaps(&a.segs, &b.segs)
        || (exact && a.members.len() == 1 && a.defs.len() == 1 && !any_inside(&b.defs, &a.segs))
}

/// Merge two classes [`mergeable`] accepted.
fn merge(a: Class, b: Class) -> Class {
    let hull = joint_hull(&a, &b);
    let segs = union(&a.segs, &b.segs);
    let mut defs = a.defs;
    defs.extend(b.defs);
    defs.sort_unstable();
    let mut members = a.members;
    members.extend(b.members);
    Class {
        members,
        segs,
        hull,
        defs,
    }
}

#[cfg(test)]
mod tests {
    use crate::linear_scan::allocate_linear_scan;
    use crate::reg::Reg;
    use torajs_core::ssa::{
        BinOp, Block, BlockId, Function, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId,
        ValueInfo, VecInst, VecOp, VecShape,
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

    /// `b = a; use(a, b)` with `a` never written again: `b` holds `a`'s
    /// value wherever both are live, so the two share a register and the
    /// copy moves nothing.
    #[test]
    fn copy_of_a_source_left_alone_shares_while_both_live() {
        let f = func(
            4,
            &[0],
            &[],
            vec![block(
                0,
                vec![
                    add(1, v(0), Operand::ConstI64(1)),
                    copy(2, v(1)),
                    add(3, v(1), v(2)),
                ],
                Terminator::Ret(Some(v(3))),
            )],
        );
        assert_eq!(home(&f, 1), home(&f, 2));
    }

    /// A lane pair differs from its low operand in the high lane, so it
    /// takes the operand's register only where the operand is dead.
    #[test]
    fn lane_pair_stays_apart_from_a_live_low_operand() {
        let mut f = Function::new("pair", Type::F64);
        let b = f.add_block();
        let x = f.append_inst(b, InstKind::SiToFp(Operand::ConstI64(3)), Type::F64, None);
        let pair = VecInst::Pair(VecShape::F64x2, v(x.0), v(x.0));
        let w = f.append_inst(b, InstKind::Vec(pair), Type::Vec(VecShape::F64x2), None);
        let sum = VecInst::Reduce(VecOp::FAdd, v(w.0));
        let r = f.append_inst(b, InstKind::Vec(sum), Type::F64, None);
        let kind = InstKind::BinOp(BinOp::FAdd, v(r.0), v(x.0));
        let t = f.append_inst(b, kind, Type::F64, None);
        f.set_term(b, Terminator::Ret(Some(v(t.0))));
        let alloc = allocate_linear_scan(&f);
        assert!(matches!(alloc.of(w), Reg::Fpr(_)));
        assert_ne!(alloc.of(w), alloc.of(x));
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
