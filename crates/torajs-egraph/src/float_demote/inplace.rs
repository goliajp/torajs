//! In-place slow path for a pure accumulator — the `total += c` loop
//! whose region cannot be cloned (a loop-body value read past the
//! loop, such as a string the throw exits drop). Instead of a second
//! copy of the loop, the demoted cell carries both representations:
//!
//! * integer mode: `acc` holds the exact sum, below 2^53
//! * float mode: `acc` holds `SENT` and the f64 cell `accf` holds the
//!   sum the f64 loop would have
//!
//! The hot path adds and tests bit 53 of the sum: an integer-mode sum
//! sets it only once it reaches 2^53, and `SENT + c` always sets it
//! (`c < 2^53` lands in SENT's clear low bits). The cold block takes
//! `accf = acc + c` rounded once on the first crossing and
//! `accf + c` in f64 after that, and parks `acc` at SENT. A read
//! past the loop selects `accf` when `acc` has SENT's bit 62, the exact
//! `sitofp acc` otherwise. Both addends are non-negative, so an
//! integer-mode sum never wraps and never reaches SENT.
//!
//! The encoding leaves the interval facts of `acc` and the sum
//! describing the f64 values, not the i64 ones; the caller drops them.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    BinOp, Block, BlockId, Function, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId,
};

use crate::interval::transfer::{NumFact, op_const_int};
use crate::rc_peephole::visit_value_operands;
use crate::slot_forward::rewrite_operands;

use super::apply::split_block_at;
use super::cfg::{PlanCfg, remap_term_operand, term_operand_value};
use super::guard::{GuardSite, MAX_EXACT};
use super::push_value;

/// Float mode's parked value: bit 53 set (the hot test), and bit 62,
/// which no integer-mode sum (< 2^54) has, marks float mode.
const SENT: i64 = (1 << 62) | (1 << 53);
const PARKED: i64 = 1 << 62;

/// One accumulator planned for the in-place slow path.
pub(crate) struct InPlace {
    acc: ValueId,
    sum: ValueId,
    addend: Operand,
    /// The add's block.
    site: BlockId,
    /// The loop-body `acc = copy sum`.
    latch: BlockId,
    /// The one def of `acc` outside the loop.
    init: BlockId,
    /// Every block the encoding touches: the loop, and the blocks that
    /// define or read `acc`.
    blocks: Vec<BlockId>,
}

impl InPlace {
    /// The values whose facts no longer describe their i64 contents.
    pub(crate) fn encoded(&self) -> [ValueId; 2] {
        [self.acc, self.sum]
    }
}

/// The in-place plans of the marked guarded sums, taken out of
/// `growth`; `Err` names the marked sums that no longer fit.
pub(crate) fn take_marked(
    func: &Function,
    cfg: &PlanCfg,
    set: &HashSet<ValueId>,
    growth: &mut Vec<GuardSite>,
    marked: &HashSet<ValueId>,
    facts: &HashMap<ValueId, NumFact>,
) -> Result<Vec<InPlace>, Vec<ValueId>> {
    if marked.is_empty() {
        return Ok(Vec::new());
    }
    let (mut ips, mut lost) = (Vec::new(), Vec::new());
    growth.retain(|g| {
        if !marked.contains(&g.result) {
            return true;
        }
        match plan(func, cfg, set, g, facts) {
            Some(ip) => ips.push(ip),
            None => lost.push(g.result),
        }
        false
    });
    if lost.is_empty() { Ok(ips) } else { Err(lost) }
}

/// Whether the evicted value `v` is a growth site the in-place path
/// can host.
pub(crate) fn fits(
    func: &Function,
    cfg: &PlanCfg,
    set: &HashSet<ValueId>,
    growth: &[GuardSite],
    v: ValueId,
    facts: &HashMap<ValueId, NumFact>,
) -> bool {
    growth
        .iter()
        .find(|g| g.result == v)
        .is_some_and(|g| plan(func, cfg, set, g, facts).is_some())
}

/// The in-place sums whose blocks a cloned region would copy: the
/// clone's side exits would write the parked SENT back as a number.
pub(crate) fn clashes(ips: &[InPlace], plans: &[super::plan::RegionPlan]) -> Vec<ValueId> {
    ips.iter()
        .filter(|ip| {
            plans
                .iter()
                .any(|p| ip.blocks.iter().any(|b| p.region.contains(b)))
        })
        .map(|ip| ip.sum)
        .collect()
}

/// The in-place plan for the guarded def `g`, an accumulator
/// `sum = fadd acc, c`: `c` in-set or an integer constant, both sides
/// non-negative, `c < 2^53`; `sum` read only by the loop's
/// `acc = copy sum`; `acc` read in the loop only by the add, defined
/// once more outside it at a block dominating the header, and not
/// feeding an in-set def outside the loop.
fn plan(
    func: &Function,
    cfg: &PlanCfg,
    set: &HashSet<ValueId>,
    g: &GuardSite,
    facts: &HashMap<ValueId, NumFact>,
) -> Option<InPlace> {
    let InstKind::BinOp(BinOp::FAdd, a, b) =
        &func.blocks[g.block.0 as usize].insts[g.inst_idx].kind
    else {
        return None;
    };
    let in_set_cell =
        |o: &Operand| matches!(o, Operand::Value(v) if set.contains(v) && cfg.cells.contains(v));
    let (acc, addend) = match (a, b) {
        (Operand::Value(v), x) if in_set_cell(a) => (*v, *x),
        (x, Operand::Value(v)) if in_set_cell(b) => (*v, *x),
        _ => return None,
    };
    if addend == Operand::Value(acc) || !exact_nonneg(&addend, set, facts) {
        return None;
    }
    let lp = cfg
        .loops
        .loops()
        .iter()
        .filter(|l| l.contains(&g.block))
        .min_by_key(|l| l.body.len())?;
    let in_loop = |b: BlockId| lp.contains(&b);

    let (mut latch, mut init) = (None, None);
    let (mut acc_reads, mut sum_reads) = (0usize, 0usize);
    for block in &func.blocks {
        let inside = in_loop(block.id);
        for inst in &block.insts {
            if inst.result == Some(acc) {
                let slot = if inside { &mut latch } else { &mut init };
                if slot.replace(block.id).is_some() {
                    return None;
                }
                // the loop stores the sum; the start is an integer-mode value
                let fits = match &inst.kind {
                    InstKind::Copy(_, Operand::Value(s)) if inside => *s == g.result,
                    InstKind::Copy(_, src) => !inside && exact_nonneg(src, set, facts),
                    _ => false,
                };
                if !fits {
                    return None;
                }
            }
            let (mut ra, mut rs) = (false, false);
            visit_value_operands(&inst.kind, |v| {
                ra |= v == acc;
                rs |= v == g.result;
            });
            if ra && (inside || inst.result.is_some_and(|r| set.contains(&r))) {
                acc_reads += 1;
            }
            sum_reads += rs as usize;
        }
        let t = term_operand_value(&block.term);
        if t == Some(g.result) || (inside && t == Some(acc)) {
            return None;
        }
    }
    let (latch, init) = (latch?, init?);
    if acc_reads != 1 || sum_reads != 1 || !cfg.dom.dominates(init, lp.header) {
        return None;
    }
    let (defs, uses) = cfg.value_blocks.get(&acc)?;
    let mut blocks: Vec<BlockId> = lp.body.iter().chain(defs).chain(uses).copied().collect();
    blocks.sort_by_key(|b| b.0);
    blocks.dedup();
    Some(InPlace {
        acc,
        sum: g.result,
        addend,
        site: g.block,
        latch,
        init,
        blocks,
    })
}

/// Whether `op` is an in-set value or an integer constant inside
/// `[0, 2^53)`.
fn exact_nonneg(op: &Operand, set: &HashSet<ValueId>, facts: &HashMap<ValueId, NumFact>) -> bool {
    let f = match op {
        Operand::Value(x) if set.contains(x) => facts.get(x).copied(),
        Operand::ConstF64(f) if f.is_sign_negative() => None,
        other => op_const_int(other).map(NumFact::point),
    };
    f.is_some_and(|f| f.lo >= 0 && f.hi <= MAX_EXACT as i128)
}

/// The f64 reads of `acc` past the loop, re-pointed before mutation:
/// each read names a fresh f64 value, defined by `install` once the
/// function is mutated. Returns those values with their blocks.
pub(crate) fn redirect_reads(func: &mut Function, ip: &InPlace) -> Vec<(BlockId, ValueId)> {
    let mut out = Vec::new();
    for bi in 0..func.blocks.len() {
        let id = BlockId(bi as u32);
        if id == ip.site {
            continue;
        }
        let n = func.blocks[bi].insts.len();
        for i in 0..n {
            let mut reads = false;
            visit_value_operands(&func.blocks[bi].insts[i].kind, |v| reads |= v == ip.acc);
            if reads {
                let w = push_value(&mut func.values, Type::F64);
                let map = HashMap::from_iter([(ip.acc, Operand::Value(w))]);
                rewrite_operands(&mut func.blocks[bi].insts[i].kind, &map);
                out.push((id, w));
            }
        }
        if term_operand_value(&func.blocks[bi].term) == Some(ip.acc) {
            let w = push_value(&mut func.values, Type::F64);
            let map = HashMap::from_iter([(ip.acc, Operand::Value(w))]);
            let term = std::mem::replace(&mut func.blocks[bi].term, Terminator::Unreachable);
            func.blocks[bi].term = remap_term_operand(term, &map);
            out.push((id, w));
        }
    }
    out
}

/// Install the encoding on the mutated function. Every position is
/// found by value: other installs may have moved the instructions.
pub(crate) fn install(func: &mut Function, ip: &InPlace, reads: &[(BlockId, ValueId)]) {
    let accf = push_value(&mut func.values, Type::F64);
    let i64v = |v| Operand::Value(v);
    let inst = |r: ValueId, kind| Inst {
        result: Some(r),
        kind,
        origin: None,
    };

    // the f64 cell is read only in float mode, after the first
    // crossing wrote it; the init only gives it a def on every path
    let init = &mut func.blocks[ip.init.0 as usize].insts;
    let at = init.iter().position(|i| i.result == Some(ip.acc)).unwrap() + 1;
    init.insert(
        at,
        inst(accf, InstKind::Copy(Type::F64, Operand::ConstF64(0.0))),
    );

    // each read past the loop: accf in float mode, the exact sum otherwise
    for &(b, w) in reads {
        let (test, ge) = parked(func, ip.acc);
        let fi = push_value(&mut func.values, Type::F64);
        let bridge = test.into_iter().chain([
            inst(fi, InstKind::SiToFp(i64v(ip.acc))),
            inst(
                w,
                InstKind::Select(Type::F64, i64v(ge), i64v(accf), i64v(fi)),
            ),
        ]);
        let insts = &mut func.blocks[b.0 as usize].insts;
        let mut at = insts.len();
        for (i, x) in insts.iter().enumerate() {
            let mut reads_w = false;
            visit_value_operands(&x.kind, |v| reads_w |= v == w);
            if reads_w {
                at = i;
                break;
            }
        }
        insts.splice(at..at, bridge);
    }

    // the latch reads the parked-or-exact sum
    let next = push_value(&mut func.values, Type::I64);
    for x in &mut func.blocks[ip.latch.0 as usize].insts {
        if x.result == Some(ip.acc) {
            x.kind = InstKind::Copy(Type::I64, i64v(next));
        }
    }

    // hot test on the sum's bit 53, then the cold block. Everything
    // after the add reads its copy `next`, so the sum dies at the copy
    // and the two share a register
    let (b, sum) = (ip.site, ip.sum);
    let pos = func.blocks[b.0 as usize]
        .insts
        .iter()
        .position(|i| i.result == Some(sum))
        .unwrap();
    let bit = push_value(&mut func.values, Type::I64);
    let hit = push_value(&mut func.values, Type::Bool);
    func.blocks[b.0 as usize].insts.splice(
        pos + 1..pos + 1,
        [
            inst(next, InstKind::Copy(Type::I64, i64v(sum))),
            inst(
                bit,
                InstKind::BinOp(BinOp::And, i64v(next), Operand::ConstI64(1 << 53)),
            ),
            inst(
                hit,
                InstKind::ICmp(IPred::Ne, i64v(bit), Operand::ConstI64(0)),
            ),
        ],
    );
    let cont = split_block_at(func, b, &[pos + 4])[0];

    let addend = match ip.addend {
        Operand::Value(_) => None,
        other => Some(Operand::ConstF64(op_const_int(&other).unwrap() as f64)),
    };
    let (test, m) = parked(func, next);
    let (cf, s, tf, r) = (
        push_value(&mut func.values, Type::F64),
        push_value(&mut func.values, Type::F64),
        push_value(&mut func.values, Type::F64),
        push_value(&mut func.values, Type::F64),
    );
    let mut cold = test.to_vec();
    let cf_op = match addend {
        Some(c) => c,
        None => {
            cold.push(inst(cf, InstKind::SiToFp(ip.addend)));
            i64v(cf)
        }
    };
    cold.extend([
        inst(s, InstKind::BinOp(BinOp::FAdd, i64v(accf), cf_op)),
        inst(tf, InstKind::SiToFp(i64v(next))),
        inst(r, InstKind::Select(Type::F64, i64v(m), i64v(s), i64v(tf))),
        inst(accf, InstKind::Copy(Type::F64, i64v(r))),
        inst(next, InstKind::Copy(Type::I64, Operand::ConstI64(SENT))),
    ]);
    let cold_id = BlockId(func.blocks.len() as u32);
    func.blocks.push(Block {
        id: cold_id,
        insts: cold,
        term: Terminator::Br(cont),
    });
    func.blocks[b.0 as usize].term = Terminator::CondBr {
        cond: i64v(hit),
        then_blk: cold_id,
        else_blk: cont,
    };
}

/// `v & PARKED != 0`: true when `v` is in float mode.
fn parked(func: &mut Function, v: ValueId) -> ([Inst; 2], ValueId) {
    let bit = push_value(&mut func.values, Type::I64);
    let on = push_value(&mut func.values, Type::Bool);
    let inst = |r, kind| Inst {
        result: Some(r),
        kind,
        origin: None,
    };
    let and = InstKind::BinOp(BinOp::And, Operand::Value(v), Operand::ConstI64(PARKED));
    let ne = InstKind::ICmp(IPred::Ne, Operand::Value(bit), Operand::ConstI64(0));
    ([inst(bit, and), inst(on, ne)], on)
}

#[cfg(test)]
mod tests {
    use super::super::{FloatDemoteStats, demote_in_function};
    use crate::interval;
    use torajs_core::ssa::{
        BinOp, Block, BlockId, FPred, Function, IPred, Inst, InstKind, Module, Operand, Terminator,
        Type, ValueId, ValueInfo,
    };

    fn inst(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    /// `for (i…) { c = i & 15; total += c; if (c == 99) { use(c); return total } }`
    /// — the early exit reads a loop-body value, so the region cannot
    /// be cloned and the accumulator takes the in-place slow path.
    fn early_exit_sum() -> Function {
        let types = [
            Type::I64,  // %0 n
            Type::F64,  // %1 total cell
            Type::I64,  // %2 i cell
            Type::Bool, // %3 i < n
            Type::F64,  // %4 sitofp c
            Type::F64,  // %5 fadd
            Type::I64,  // %6 i + 1
            Type::I64,  // %7 c
            Type::Bool, // %8 c == 99
            Type::I64,  // %9 c + 1 past the loop
        ];
        let block = |id, insts, term| Block {
            id: BlockId(id),
            insts,
            term,
        };
        let br2 = |c, t, e| Terminator::CondBr {
            cond: v(c),
            then_blk: BlockId(t),
            else_blk: BlockId(e),
        };
        Function {
            name: "f".into(),
            params: vec![ValueId(0)],
            ret: Type::F64,
            blocks: vec![
                block(
                    0,
                    vec![
                        inst(1, InstKind::Copy(Type::F64, Operand::ConstF64(0.0))),
                        inst(2, InstKind::Copy(Type::I64, Operand::ConstI64(0))),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![inst(3, InstKind::ICmp(IPred::Slt, v(2), v(0)))],
                    br2(3, 2, 3),
                ),
                block(
                    2,
                    vec![
                        inst(7, InstKind::BinOp(BinOp::And, v(2), Operand::ConstI64(15))),
                        inst(4, InstKind::SiToFp(v(7))),
                        inst(5, InstKind::BinOp(BinOp::FAdd, v(1), v(4))),
                        inst(8, InstKind::ICmp(IPred::Eq, v(7), Operand::ConstI64(99))),
                    ],
                    br2(8, 4, 5),
                ),
                block(3, vec![], Terminator::Ret(Some(v(1)))),
                block(
                    4,
                    vec![inst(
                        9,
                        InstKind::BinOp(BinOp::Add, v(7), Operand::ConstI64(1)),
                    )],
                    Terminator::Ret(Some(v(1))),
                ),
                block(
                    5,
                    vec![
                        inst(1, InstKind::Copy(Type::F64, v(5))),
                        inst(6, InstKind::BinOp(BinOp::Add, v(2), Operand::ConstI64(1))),
                        inst(2, InstKind::Copy(Type::I64, v(6))),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
            ],
            values: types
                .into_iter()
                .map(|ty| ValueInfo { ty, name: None })
                .collect(),
            current_origin: None,
        }
    }

    fn run(f: Function) -> (Function, FloatDemoteStats) {
        let mut m = Module {
            funcs: vec![f],
            ..Default::default()
        };
        let mut facts = interval::analyze_module(&m);
        let mut stats = FloatDemoteStats::default();
        demote_in_function(&mut m.funcs[0], &mut facts[0], &mut stats);
        if stats.accumulators_inplace > 0 {
            assert!(!facts[0].contains_key(&ValueId(1)) && !facts[0].contains_key(&ValueId(5)));
        }
        (m.funcs.remove(0), stats)
    }

    fn count(f: &Function, p: impl Fn(&InstKind) -> bool) -> usize {
        f.blocks
            .iter()
            .flat_map(|b| &b.insts)
            .filter(|i| p(&i.kind))
            .count()
    }

    #[test]
    fn an_unclonable_accumulator_demotes_in_place() {
        let (f, stats) = run(early_exit_sum());
        assert_eq!(stats.accumulators_inplace, 1);
        assert_eq!(stats.loops_versioned, 0);
        assert_eq!(f.values[1].ty, Type::I64);
        // one f64 add, in the cold block; no second copy of the loop
        assert_eq!(
            count(&f, |k| matches!(k, InstKind::BinOp(BinOp::FAdd, ..))),
            1
        );
        assert_eq!(
            count(&f, |k| matches!(k, InstKind::BinOp(BinOp::Add, ..))),
            3
        );
        // both returns read a selected f64, and the cold block selects once
        assert_eq!(
            count(&f, |k| matches!(k, InstKind::Select(Type::F64, ..))),
            3
        );
        for b in [3, 4] {
            let Terminator::Ret(Some(Operand::Value(w))) = f.blocks[b].term else {
                panic!("ret");
            };
            assert_ne!(w, ValueId(1));
            assert_eq!(f.values[w.0 as usize].ty, Type::F64);
        }
        // the add's block ends in the bit-53 test
        let Terminator::CondBr { cond, .. } = f.blocks[2].term else {
            panic!("the add's block branches on its test");
        };
        let tail = &f.blocks[2].insts[f.blocks[2].insts.len() - 2..];
        assert!(matches!(
            tail[0].kind,
            InstKind::BinOp(BinOp::And, Operand::Value(_), Operand::ConstI64(k)) if k == 1 << 53
        ));
        assert_eq!(Operand::Value(tail[1].result.unwrap()), cond);
    }

    #[test]
    fn a_second_read_in_the_loop_keeps_the_accumulator_f64() {
        // `total` also feeds a compare inside the loop: the parked SENT
        // would be read as a number, so the in-place path declines
        let mut f = early_exit_sum();
        f.blocks[2].insts[3].kind = InstKind::FCmp(FPred::Oeq, v(1), v(4));
        let (_, stats) = run(f);
        assert_eq!(stats.accumulators_inplace, 0);
    }

    #[test]
    fn a_start_past_2_53_keeps_the_accumulator_f64() {
        // 2^60 + c would keep bit 53 clear and add exactly in i64
        let mut f = early_exit_sum();
        f.blocks[0].insts[0].kind = InstKind::Copy(Type::F64, Operand::ConstF64(2f64.powi(60)));
        let (_, stats) = run(f);
        assert_eq!(stats.accumulators_inplace, 0);
    }
}
