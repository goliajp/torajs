//! Strength reduction of a loop counter's remainder: `srem i, p` where
//! `i` is a counter the loop only steps by one and `p` a divisor the
//! loop never redefines becomes a second counter `r` that moves with
//! `i` and wraps to 0 past `p - 1`:
//!
//! ```text
//!   entry:      m = sub p, 1
//!               r = copy (i mod p) - 1   ; -1 when i is 0 there
//!   each trip:  c  = icmp eq r, m
//!               t  = add r, 1
//!               rn = select c, 0, t      ; one CSINC
//!               r  = copy rn
//!   the site:   x = copy r               ; was srem i, p
//! ```
//!
//! `m` is computed where the counter starts, so it lives across this
//! loop only; a constant divisor's is an immediate. The move is two
//! instructions, as many as the `sdiv` / `msub` it replaces, so no
//! loop runs more of them; comparing after the increment against `p`
//! itself keeps one register fewer but costs one more instruction per
//! trip (array-any-str-to-num-1m ran 1.8% more instructions that way).
//!
//! The move runs once per trip around the loop when every back edge
//! leaves a block holding one step `i = copy (i + 1)`: no step
//! elsewhere, none in an inner loop, no site after a step in its
//! block. Each trip then runs one move ahead of its one
//! step. It sits at the top of the header, or of the body's first
//! block when the header is a bare test loop_rotate may take
//! (`Cfg::trip_block`). The latch, which tail_dup copies into the
//! arms only while it is small, does not grow: a counter moved after
//! the step kept the latch from being copied, the arms that lost
//! their copy paid a taken branch, and the case ran slower than with
//! the `sdiv`. Otherwise the move follows each step and the counter
//! starts at `i mod p` itself.
//!
//! `arr[i % pool]` paid an `sdiv` / `msub` pair every iteration
//! (array-any-double-sum-1m, r760 decomposition); a move is a
//! compare and a conditional increment.
//!
//! `r = i mod p` holds wherever `i` is read when `0 ≤ i` and `p ≥ 1`
//! there: the counter starts from the remainder of what `i` holds on
//! entry and every def of `i` in the loop is a `+ 1`, matched by one
//! move of `r`. The start goes at the end of the header's immediate
//! dominator, which no way into the loop passes a def of `i` after.
//! `0 ≤ i < 2^53` comes from the interval facts. `p ≥ 1` comes from the
//! facts, or from a test on the way to the site (`window`): `p < 1`
//! branching away, as int_entry's version check does, or the window
//! check frem_guard puts in front of its integer arm,
//! `(... | (p - 1) >>> 53) != 0`. With only the test, `p` may be
//! anything on entry; the counter then runs without being read, and
//! its start divides by 1 when `p < 1`. A start one move short is
//! `(i mod p) - 1`, never a sum with `i`, so nothing can wrap. A
//! dividend that may be negative, a step other than `+ 1`, a divisor
//! the loop writes, or one not proven `≥ 1` keeps the `srem`.
//! Powers of two keep it too: their remainder is one `and`.
//!
//! Runs in `fact_folds` after frem_guard (whose integer arms it takes)
//! and before srem_fquot. `TORAJS_SREM_COUNTER_OFF=1` skips,
//! `TORAJS_SREM_COUNTER_STATS=1` counts.

mod cfg;
mod window;

use crate::id_hash::HashMap;

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::{
    BinOp, BlockId, Function, IPred, Inst, InstKind, Module, Operand, Type, ValueId,
};

use crate::fact_folds::LoopShapes;
use crate::float_demote::push_value;
use crate::interval::NumFact;
use cfg::Cfg;

const F64_EXACT: i128 = 1 << 53;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SremCounterStats {
    /// counters introduced
    pub counters: u32,
    /// remainders now read from a counter
    pub srems: u32,
}

pub(crate) fn count_srems(
    module: &mut Module,
    facts: &[HashMap<ValueId, NumFact>],
    shapes: &mut LoopShapes,
) -> SremCounterStats {
    let mut stats = SremCounterStats::default();
    for (fi, (func, fm)) in module.funcs.iter_mut().zip(facts).enumerate() {
        if !func.is_declaration() {
            rewrite_function(func, fm, &mut stats, shapes, fi);
        }
    }
    stats
}

fn rewrite_function(
    func: &mut Function,
    facts: &HashMap<ValueId, NumFact>,
    stats: &mut SremCounterStats,
    shapes: &mut LoopShapes,
    fi: usize,
) {
    let groups = find_groups(func, facts, shapes, fi);
    stats.counters += groups.len() as u32;
    stats.srems += groups.iter().map(|g| g.sites.len() as u32).sum::<u32>();
    let mut edits = Vec::new();
    for g in groups {
        emit(func, g, &mut edits);
    }
    // last position first, so the earlier ones stay valid
    edits.sort_by_key(|e| std::cmp::Reverse(e.0));
    for ((b, k), insts) in edits {
        func.blocks[b].insts.splice(k..k, insts);
    }
}

type At = (usize, usize);

/// How `p ≥ 1` is known at a site.
#[derive(Clone, Copy)]
enum Proof {
    /// everywhere: the facts, or a constant
    Everywhere,
    /// past a test (`window`)
    Tested,
}

/// One counter: the remainders of `i` by `p` in one loop.
struct Group {
    i: ValueId,
    p: Operand,
    /// the block whose end starts the counter
    entry: BlockId,
    /// `i` is 0 at the end of `entry`
    zero: bool,
    /// `p ≥ 1` holds at `entry` too (else only at the sites)
    proven: bool,
    sites: Vec<At>,
    /// the in-loop `i = copy (i + 1)` defs
    steps: Vec<At>,
    /// the loop, by header
    header: BlockId,
    /// where the counter moves once per trip (`Cfg::trip_block`), when
    /// it does not follow each step instead
    trip: Option<BlockId>,
}

impl Group {
    /// A site after a step in the step's block reads `i` already moved
    /// on, which a counter moved once per trip has not: the counter
    /// then moves with the steps. (A site in the header keeps the
    /// header from being a bare test, so the trip move is at its top,
    /// ahead of the site.)
    fn add_site(&mut self, at: At) {
        if self.steps.iter().any(|s| s.0 == at.0 && s.1 < at.1) {
            self.trip = None;
        }
        self.sites.push(at);
    }
}

fn find_groups(
    func: &Function,
    facts: &HashMap<ValueId, NumFact>,
    shapes: &mut LoopShapes,
    fi: usize,
) -> Vec<Group> {
    if !worth_analyzing(func, facts) {
        return Vec::new();
    }
    let (dom, la) = shapes.of(fi, func);
    let cfg = Cfg::of(func, la);
    let mut groups: Vec<Group> = Vec::new();
    for (b, block) in func.blocks.iter().enumerate() {
        for (k, inst) in block.insts.iter().enumerate() {
            let InstKind::BinOp(BinOp::SRem, Operand::Value(x), p) = &inst.kind else {
                continue;
            };
            if !facts.get(x).is_some_and(|f| f.lo >= 0 && f.hi < F64_EXACT) {
                continue;
            }
            let i = &cfg.copy_source(b, k, *x);
            // the innermost loop around the site that steps `i`
            let Some(lp) = la
                .loops()
                .iter()
                .filter(|l| l.contains(&BlockId(b as u32)) && cfg.defined_in(*i, l))
                .min_by_key(|l| l.body.len())
            else {
                continue;
            };
            if let Some(g) = groups
                .iter_mut()
                .find(|g| g.header == lp.header && g.i == *i && g.p == *p)
            {
                // the group's start and steps are this site's too; its
                // own `p ≥ 1` is all it adds
                if divisor_at_least_one(&cfg, dom, facts, b, p).is_some() {
                    g.add_site((b, k));
                }
                continue;
            }
            let Some(steps) = cfg.steps_of(*i, lp) else {
                continue;
            };
            let Some(entry) = cfg.entry_block(dom, lp, *i) else {
                continue;
            };
            let proven = match (p, divisor_at_least_one(&cfg, dom, facts, b, p)) {
                (Operand::ConstI64(_), Some(_)) => true,
                (Operand::Value(pv), Some(proof)) if !cfg.defined_in(*pv, lp) => {
                    if !cfg.settled_at(*pv, entry, dom) {
                        continue;
                    }
                    matches!(proof, Proof::Everywhere)
                }
                _ => continue,
            };
            let mut g = Group {
                i: *i,
                p: *p,
                entry,
                zero: cfg.zero_at(entry, *i),
                proven,
                sites: Vec::new(),
                trip: cfg
                    .steps_close_iterations(la, lp, &steps)
                    .then(|| cfg.trip_block(lp)),
                steps,
                header: lp.header,
            };
            g.add_site((b, k));
            groups.push(g);
        }
    }
    groups
}

/// A cheap look before the dominator and loop analyses, which most
/// functions would pay for nothing: some remainder of a dividend in
/// `[0, 2^53)` (or of a value it copies) that something adds 1 to, by
/// a divisor that is a value or a constant other than a power of two.
/// The ids must be dense: blocks are looked up by id.
fn worth_analyzing(func: &Function, facts: &HashMap<ValueId, NumFact>) -> bool {
    let dense = func
        .blocks
        .iter()
        .enumerate()
        .all(|(k, b)| b.id.0 as usize == k);
    let insts = || func.blocks.iter().flat_map(|b| &b.insts);
    let mut dividends: Vec<ValueId> = insts()
        .filter_map(|i| match &i.kind {
            InstKind::BinOp(BinOp::SRem, Operand::Value(x), p)
                if facts.get(x).is_some_and(|f| f.lo >= 0 && f.hi < F64_EXACT)
                    && match p {
                        Operand::Value(_) => true,
                        Operand::ConstI64(c) => *c >= 3 && !(*c as u64).is_power_of_two(),
                        _ => false,
                    } =>
            {
                Some(*x)
            }
            _ => None,
        })
        .collect();
    if !dense || dividends.is_empty() {
        return false;
    }
    let sources: Vec<ValueId> = insts()
        .filter_map(|i| match (&i.kind, i.result) {
            (InstKind::Copy(_, Operand::Value(s)), Some(r)) if dividends.contains(&r) => Some(*s),
            _ => None,
        })
        .collect();
    dividends.extend(sources);
    insts().any(|i| {
        matches!(&i.kind,
            InstKind::BinOp(BinOp::Add, Operand::Value(v), Operand::ConstI64(1))
            | InstKind::BinOp(BinOp::Add, Operand::ConstI64(1), Operand::Value(v))
                if dividends.contains(v))
    })
}

/// How `p ≥ 1` is known at the site in block `b`, if it is. A
/// constant counts when it is not a power of two.
fn divisor_at_least_one(
    cfg: &Cfg,
    dom: &DominatorTree,
    facts: &HashMap<ValueId, NumFact>,
    b: usize,
    p: &Operand,
) -> Option<Proof> {
    match p {
        Operand::ConstI64(c) if *c >= 3 && !(*c as u64).is_power_of_two() => {
            Some(Proof::Everywhere)
        }
        Operand::Value(pv) if facts.get(pv).is_some_and(|f| f.lo >= 1) => Some(Proof::Everywhere),
        Operand::Value(pv) => window::tested_window(cfg, dom, b, *pv).then_some(Proof::Tested),
        _ => None,
    }
}

/// Rewrites `g`'s sites in place and queues the counter's defs.
fn emit(func: &mut Function, g: Group, edits: &mut Vec<(At, Vec<Inst>)>) {
    let r = push_value(&mut func.values, Type::I64);
    let mut head = Vec::new();
    let pm1 = match g.p {
        Operand::ConstI64(k) => Operand::ConstI64(k - 1),
        p => {
            let m = push_value(&mut func.values, Type::I64);
            head.push(inst(
                m,
                InstKind::BinOp(BinOp::Sub, p, Operand::ConstI64(1)),
            ));
            Operand::Value(m)
        }
    };
    let start = start_value(func, &g, &mut head);
    head.push(inst(r, InstKind::Copy(Type::I64, start)));
    let e = g.entry.0 as usize;
    edits.push(((e, func.blocks[e].insts.len()), head));
    if let Some(t) = g.trip {
        let moved = advance(func, r, pm1);
        edits.push(((t.0 as usize, 0), moved));
    } else {
        for &(b, k) in &g.steps {
            let moved = advance(func, r, pm1);
            edits.push(((b, k + 1), moved));
        }
    }
    for &(b, k) in &g.sites {
        func.blocks[b].insts[k].kind = InstKind::Copy(Type::I64, Operand::Value(r));
    }
}

/// What the counter holds on entry: `i mod p`, or, when each trip
/// moves it before anything reads it, one move short of that.
fn start_value(func: &mut Function, g: &Group, head: &mut Vec<Inst>) -> Operand {
    let short = g.trip.is_some();
    if g.zero {
        return Operand::ConstI64(if short { -1 } else { 0 });
    }
    let mut def = |func: &mut Function, ty: Type, kind: InstKind| {
        let v = push_value(&mut func.values, ty);
        head.push(inst(v, kind));
        Operand::Value(v)
    };
    let q = if g.proven {
        g.p
    } else {
        // `p < 1` divides by 1: the counter is never read then
        let low = def(
            func,
            Type::Bool,
            InstKind::ICmp(IPred::Slt, g.p, Operand::ConstI64(1)),
        );
        let pick = InstKind::Select(Type::I64, low, Operand::ConstI64(1), g.p);
        def(func, Type::I64, pick)
    };
    let s = def(
        func,
        Type::I64,
        InstKind::BinOp(BinOp::SRem, Operand::Value(g.i), q),
    );
    if !short {
        return s;
    }
    def(
        func,
        Type::I64,
        InstKind::BinOp(BinOp::Sub, s, Operand::ConstI64(1)),
    )
}

/// One move of the counter `r`: `r = r == p - 1 ? 0 : r + 1`, spelled
/// as the compare / add / select run the backend fuses into one CSINC.
/// A counter at -1 moves to 0, `p - 1` being at least 0 wherever the
/// counter is read.
fn advance(func: &mut Function, r: ValueId, pm1: Operand) -> Vec<Inst> {
    let c = push_value(&mut func.values, Type::Bool);
    let t = push_value(&mut func.values, Type::I64);
    let rn = push_value(&mut func.values, Type::I64);
    let rv = Operand::Value(r);
    let wrap = InstKind::Select(
        Type::I64,
        Operand::Value(c),
        Operand::ConstI64(0),
        Operand::Value(t),
    );
    vec![
        inst(c, InstKind::ICmp(IPred::Eq, rv, pm1)),
        inst(t, InstKind::BinOp(BinOp::Add, rv, Operand::ConstI64(1))),
        inst(rn, wrap),
        inst(r, InstKind::Copy(Type::I64, Operand::Value(rn))),
    ]
}

fn inst(r: ValueId, kind: InstKind) -> Inst {
    Inst {
        result: Some(r),
        kind,
        origin: None,
    }
}

#[cfg(test)]
mod tests;
