//! Float demotion — representation selection for integer-valued f64
//! flows (RFC 20260611-int-range-float-demotion phase 1b-i; the AOT
//! analogue of TurboFan simplified lowering's int representation
//! choice). Consumes the interval analysis: an f64 value with a fact
//! is integer-valued, and a fact bounded inside ±2^53 means every op
//! that produced it was exact — the i64 computation produces the
//! bit-identical trajectory.
//!
//! Demotion set: the largest set of f64 values where every def is a
//! rewritable shape over in-set / integer-const operands:
//!
//! * `sitofp x` with `x`'s fact inside ±2^53 (conversion exact) —
//!   becomes `copy x`
//! * `frem a, C` (C ≠ 0 integer const, a in set) — `srem` (truncated
//!   remainder, dividend-sign: identical to fmod on integers)
//! * `fdiv a, 2` whose result carries a fact — the interval transfer
//!   only proves fdiv under the frem-parity branch (even dividend ⇒
//!   exact) — becomes `sdiv`
//! * `fadd`/`fsub`/`fmul` whose result fact is finite (clamp53 left
//!   bounds < 2^53 ⇒ no rounding) — integer add/sub/mul
//! * `copy` of an in-set value (loop cells: every def must qualify)
//!
//! Values failing any def (growth ops whose bound widened to a
//! sentinel — the collatz `3n+1` shape — or non-integer entries) drop
//! out and the removal propagates; phase 1b-ii adds runtime guards +
//! loop versioning to win those back.
//!
//! Uses are patched in place: an `fcmp` whose operands are all in-set
//! or integer consts becomes the matching `icmp` (the set is NaN-free
//! so ordered/unordered collapse); any other f64 consumer gets a
//! `sitofp` bridge inserted before it. A bridged value must be
//! provably never `-0.0` (i64 has no negative zero, so the bridge
//! would launder `-0` into `+0` — observable through `1/x` or
//! `Object.is` downstream): `sitofp` never produces `-0`, `frem`
//! keeps the dividend's sign (needs a non-negative dividend), IEEE
//! add/sub only yield `-0` from two `-0`-shaped inputs, and `fmul`
//! demands sign-clean non-negative operands. A value needing a
//! bridge without the `-0`-free proof leaves the set (and the
//! removal re-propagates) before anything is mutated.
//!
//! Runs between the interval analysis and sext_elide — demoted values
//! keep their ValueId and fact, so in-i32-range demoted cells seed
//! sext_elide for free. `TORAJS_FLOAT_DEMOTE_OFF=1` skips,
//! `TORAJS_FLOAT_DEMOTE_STATS=1` dumps counters.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{
    BinOp, FPred, Function, IPred, Inst, InstKind, Module, Operand, Terminator, Type, ValueId,
    ValueInfo,
};

use crate::interval::transfer::{NEG_INF, NumFact, POS_INF, op_const_int};
use crate::rc_peephole::visit_value_operands;
use crate::slot_forward::rewrite_operands;

const F64_EXACT: i128 = 1i128 << 53;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FloatDemoteStats {
    /// f64 values rewritten to i64.
    pub values_demoted: u32,
    /// fcmp instructions converted to icmp.
    pub fcmp_converted: u32,
    /// sitofp bridges inserted at out-of-set f64 uses.
    pub bridges_inserted: u32,
}

pub fn demote_floats(module: &mut Module, facts: &[HashMap<ValueId, NumFact>]) -> FloatDemoteStats {
    let mut stats = FloatDemoteStats::default();
    let empty = HashMap::new();
    for (fi, func) in module.funcs.iter_mut().enumerate() {
        if func.is_declaration() {
            continue;
        }
        demote_in_function(func, facts.get(fi).unwrap_or(&empty), &mut stats);
    }
    stats
}

/// A fact whose every producing op was exact in f64 (clamp53 would
/// have widened a rounding bound to a sentinel).
fn exact(f: &NumFact) -> bool {
    f.lo > -F64_EXACT && f.hi < F64_EXACT && f.lo > NEG_INF && f.hi < POS_INF
}

fn fpred_to_ipred(p: FPred) -> IPred {
    match p {
        FPred::Oeq => IPred::Eq,
        FPred::One | FPred::Une => IPred::Ne,
        FPred::Olt => IPred::Slt,
        FPred::Ogt => IPred::Sgt,
        FPred::Ole => IPred::Sle,
        FPred::Oge => IPred::Sge,
    }
}

/// Operand qualifies as an in-set value or an integer constant.
fn op_ok(op: &Operand, set: &HashSet<ValueId>) -> bool {
    match op {
        Operand::Value(v) => set.contains(v),
        _ => op_const_int(op).is_some(),
    }
}

/// An fcmp convertible to icmp: at least one demoted operand, every
/// operand demoted or an integer constant.
fn fcmp_convertible(a: &Operand, b: &Operand, set: &HashSet<ValueId>) -> bool {
    let in_set = |op: &Operand| matches!(op, Operand::Value(v) if set.contains(v));
    (in_set(a) || in_set(b)) && op_ok(a, set) && op_ok(b, set)
}

fn demote_in_function(
    func: &mut Function,
    facts: &HashMap<ValueId, NumFact>,
    stats: &mut FloatDemoteStats,
) {
    // def sites per value (loop cells have several).
    let mut defs: HashMap<ValueId, Vec<InstKind>> = HashMap::new();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(r) = inst.result {
                defs.entry(r).or_default().push(inst.kind.clone());
            }
        }
    }

    // candidate set: integer-valued f64 values, shrunk until every
    // def is rewritable AND every bridged escape is -0-free.
    let mut set: HashSet<ValueId> = facts
        .keys()
        .filter(|v| func.values[v.0 as usize].ty == Type::F64)
        .copied()
        .collect();
    loop {
        // def-rewritability fixpoint (greatest: shrink from the full
        // candidate set so loop cells can certify through themselves)
        loop {
            let snapshot = set.clone();
            set.retain(|v| {
                defs.get(v).is_some_and(|dl| {
                    !dl.is_empty() && dl.iter().all(|d| def_rewritable(d, v, facts, &snapshot))
                })
            });
            if set.len() == snapshot.len() {
                break;
            }
        }
        if set.is_empty() {
            return;
        }
        // -0-free closure — coinductive (greatest fixpoint): assume
        // every in-set value clean, evict anything whose def could
        // mint a -0 under the current assumption, repeat. A least
        // fixpoint would never admit loop cells (cell ↔ op cycles);
        // the survivors here provably have no -0 source on any path.
        let mut nz: HashSet<ValueId> = set.clone();
        loop {
            let snapshot = nz.clone();
            nz.retain(|v| defs[v].iter().all(|d| def_no_neg_zero(d, facts, &snapshot)));
            if nz.len() == snapshot.len() {
                break;
            }
        }
        // classify every use: an in-set def operand or a convertible
        // fcmp operand is a useful (i64-native) use; anything else
        // needs a sitofp bridge. A bridged escape must be -0-free,
        // and a value with no useful use at all leaves the set — a
        // demote whose every consumer bridges back is a net loss
        // (copy + sitofp where a single sitofp stood).
        let mut bad: HashSet<ValueId> = HashSet::new();
        let mut useful: HashSet<ValueId> = HashSet::new();
        for block in &func.blocks {
            for inst in &block.insts {
                if inst.result.is_some_and(|r| set.contains(&r)) {
                    visit_value_operands(&inst.kind, |v| {
                        if set.contains(&v) {
                            useful.insert(v);
                        }
                    });
                    continue;
                }
                if let InstKind::FCmp(_, a, b) = &inst.kind {
                    if fcmp_convertible(a, b, &set) {
                        for op in [a, b] {
                            if let Operand::Value(v) = op {
                                useful.insert(*v);
                            }
                        }
                        continue;
                    }
                }
                visit_value_operands(&inst.kind, |v| {
                    if set.contains(&v) && !nz.contains(&v) {
                        bad.insert(v);
                    }
                });
            }
            if let Terminator::Ret(Some(Operand::Value(v))) = &block.term {
                if set.contains(v) && !nz.contains(v) {
                    bad.insert(*v);
                }
            }
        }
        for v in &set {
            if !useful.contains(v) {
                bad.insert(*v);
            }
        }
        if bad.is_empty() {
            break;
        }
        for v in bad {
            set.remove(&v);
        }
    }

    // mutate: split the borrow so bridge insertion can push values.
    // Single walk — every inst takes exactly one of three branches
    // (demoted def rewrite / fcmp→icmp conversion / bridge insertion),
    // so a freshly converted icmp's i64 operands are never mistaken
    // for f64 uses needing a bridge.
    let Function {
        blocks,
        values,
        ret,
        ..
    } = func;
    for block in blocks.iter_mut() {
        let old = std::mem::take(&mut block.insts);
        let mut out: Vec<Inst> = Vec::with_capacity(old.len());
        for mut inst in old {
            if inst.result.is_some_and(|r| set.contains(&r)) {
                inst.kind = rewrite_def(&inst.kind);
                out.push(inst);
                continue;
            }
            if let InstKind::FCmp(p, a, b) = &inst.kind {
                if fcmp_convertible(a, b, &set) {
                    inst.kind = InstKind::ICmp(fpred_to_ipred(*p), int_op(a), int_op(b));
                    stats.fcmp_converted += 1;
                    out.push(inst);
                    continue;
                }
            }
            let mut bridge_ops: Vec<ValueId> = Vec::new();
            visit_value_operands(&inst.kind, |v| {
                if set.contains(&v) && !bridge_ops.contains(&v) {
                    bridge_ops.push(v);
                }
            });
            for v in bridge_ops {
                let b = push_value(values, Type::F64);
                out.push(Inst {
                    result: Some(b),
                    kind: InstKind::SiToFp(Operand::Value(v)),
                    origin: inst.origin,
                });
                let mut map = HashMap::new();
                map.insert(v, Operand::Value(b));
                rewrite_operands(&mut inst.kind, &map);
                stats.bridges_inserted += 1;
            }
            out.push(inst);
        }
        if let Terminator::Ret(Some(Operand::Value(v))) = &block.term {
            if set.contains(v) && *ret == Type::F64 {
                let b = push_value(values, Type::F64);
                out.push(Inst {
                    result: Some(b),
                    kind: InstKind::SiToFp(Operand::Value(*v)),
                    origin: None,
                });
                block.term = Terminator::Ret(Some(Operand::Value(b)));
                stats.bridges_inserted += 1;
            }
        }
        block.insts = out;
    }

    for v in &set {
        values[v.0 as usize].ty = Type::I64;
    }
    stats.values_demoted += set.len() as u32;
}

/// One def site is rewritable to an exact i64 equivalent.
fn def_rewritable(
    d: &InstKind,
    v: &ValueId,
    facts: &HashMap<ValueId, NumFact>,
    set: &HashSet<ValueId>,
) -> bool {
    match d {
        InstKind::SiToFp(x) => match x {
            Operand::Value(xv) => facts.get(xv).is_some_and(exact),
            _ => op_const_int(x).is_some(),
        },
        InstKind::BinOp(BinOp::FRem, a, b) => {
            op_ok(a, set) && op_const_int(b).is_some_and(|c| c != 0)
        }
        // the interval transfer only assigns an fdiv result a fact
        // under the frem-parity guard — its presence IS the evenness
        // certificate; exactness needs no bound check (halving).
        InstKind::BinOp(BinOp::FDiv, a, b) => {
            op_ok(a, set) && op_const_int(b) == Some(2) && facts.contains_key(v)
        }
        InstKind::BinOp(BinOp::FAdd | BinOp::FSub | BinOp::FMul, a, b) => {
            op_ok(a, set) && op_ok(b, set) && facts.get(v).is_some_and(exact)
        }
        InstKind::Copy(Type::F64, a) | InstKind::Identity(a) => op_ok(a, set),
        _ => false,
    }
}

/// One def site provably never produces -0.0 given `nz` operands.
fn def_no_neg_zero(d: &InstKind, facts: &HashMap<ValueId, NumFact>, nz: &HashSet<ValueId>) -> bool {
    let op_nz = |op: &Operand| match op {
        Operand::Value(v) => nz.contains(v),
        Operand::ConstF64(f) => !(*f == 0.0 && f.is_sign_negative()),
        _ => op_const_int(op).is_some(),
    };
    let nonneg = |op: &Operand| match op {
        Operand::Value(v) => facts.get(v).is_some_and(|f| f.lo >= 0),
        _ => op_const_int(op).is_some_and(|c| c >= 0),
    };
    match d {
        // integer-to-float conversion never yields -0
        InstKind::SiToFp(_) => true,
        // fmod keeps the dividend's sign: non-negative -0-free
        // dividend ⇒ result in [+0, C)
        InstKind::BinOp(BinOp::FRem, a, _) => op_nz(a) && nonneg(a),
        // halving a -0-free value: only ±0 halves to ±0, and the
        // input zero is +0
        InstKind::BinOp(BinOp::FDiv, a, _) => op_nz(a),
        // x + y / x − y yields -0 only when both inputs are -0-shaped
        InstKind::BinOp(BinOp::FAdd | BinOp::FSub, a, b) => op_nz(a) && op_nz(b),
        // a*b yields -0 from a ±0 with a negative cofactor: demand
        // sign-clean non-negative -0-free operands
        InstKind::BinOp(BinOp::FMul, a, b) => op_nz(a) && op_nz(b) && nonneg(a) && nonneg(b),
        InstKind::Copy(_, a) | InstKind::Identity(a) => op_nz(a),
        _ => false,
    }
}

/// The i64 form of a demoted def site.
fn rewrite_def(d: &InstKind) -> InstKind {
    match d {
        InstKind::SiToFp(x) => InstKind::Copy(Type::I64, int_op(x)),
        InstKind::BinOp(BinOp::FRem, a, b) => InstKind::BinOp(BinOp::SRem, int_op(a), int_op(b)),
        InstKind::BinOp(BinOp::FDiv, a, b) => InstKind::BinOp(BinOp::SDiv, int_op(a), int_op(b)),
        InstKind::BinOp(BinOp::FAdd, a, b) => InstKind::BinOp(BinOp::Add, int_op(a), int_op(b)),
        InstKind::BinOp(BinOp::FSub, a, b) => InstKind::BinOp(BinOp::Sub, int_op(a), int_op(b)),
        InstKind::BinOp(BinOp::FMul, a, b) => InstKind::BinOp(BinOp::Mul, int_op(a), int_op(b)),
        InstKind::Copy(Type::F64, a) => InstKind::Copy(Type::I64, *a),
        other => other.clone(),
    }
}

/// Integer-const form of an operand (values pass through unchanged).
fn int_op(op: &Operand) -> Operand {
    match op {
        Operand::Value(_) => *op,
        other => match op_const_int(other) {
            Some(c) => Operand::ConstI64(c as i64),
            None => *other,
        },
    }
}

fn push_value(values: &mut Vec<ValueInfo>, ty: Type) -> ValueId {
    let id = ValueId(values.len() as u32);
    values.push(ValueInfo { ty, name: None });
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interval;
    use torajs_core::ssa::{Block, BlockId, Module};

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

    fn cf(x: f64) -> Operand {
        Operand::ConstF64(x)
    }

    fn func(ret: Type, values: Vec<Type>, blocks: Vec<Block>) -> Function {
        Function {
            name: "f".into(),
            params: vec![],
            ret,
            blocks,
            values: values
                .into_iter()
                .map(|ty| ValueInfo { ty, name: None })
                .collect(),
            current_origin: None,
        }
    }

    fn block(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }

    fn run(f: Function) -> (Module, FloatDemoteStats) {
        let mut m = Module {
            funcs: vec![f],
            ..Default::default()
        };
        let facts = interval::analyze_module(&m);
        let mut stats = FloatDemoteStats::default();
        demote_in_function(&mut m.funcs[0], &facts[0], &mut stats);
        (m, stats)
    }

    /// halve loop: sitofp entry, frem/fcmp parity guard, fdiv-by-2 in
    /// the even arm, f64 ret (bridge). The full closure demotes.
    fn halve_loop() -> Function {
        func(
            Type::F64,
            vec![
                Type::I64,  // %0 entry const
                Type::F64,  // %1 sitofp
                Type::F64,  // %2 n cell
                Type::F64,  // %3 frem
                Type::Bool, // %4 fcmp
                Type::F64,  // %5 fdiv
            ],
            vec![
                block(
                    0,
                    vec![
                        inst(0, InstKind::Copy(Type::I64, Operand::ConstI64(100000))),
                        inst(1, InstKind::SiToFp(v(0))),
                        inst(2, InstKind::Copy(Type::F64, v(1))),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![
                        inst(3, InstKind::BinOp(BinOp::FRem, v(2), cf(2.0))),
                        inst(4, InstKind::FCmp(FPred::Oeq, v(3), cf(0.0))),
                    ],
                    Terminator::CondBr {
                        cond: v(4),
                        then_blk: BlockId(2),
                        else_blk: BlockId(3),
                    },
                ),
                block(
                    2,
                    vec![
                        inst(5, InstKind::BinOp(BinOp::FDiv, v(2), cf(2.0))),
                        inst(2, InstKind::Copy(Type::F64, v(5))),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(3, vec![], Terminator::Ret(Some(v(2)))),
            ],
        )
    }

    #[test]
    fn halve_loop_demotes_fully() {
        let (m, stats) = run(halve_loop());
        // %1, %2, %3, %5 demote; the fcmp converts; ret bridges.
        assert_eq!(stats.values_demoted, 4);
        assert_eq!(stats.fcmp_converted, 1);
        assert_eq!(stats.bridges_inserted, 1);
        let f = &m.funcs[0];
        assert!(matches!(
            f.blocks[1].insts[0].kind,
            InstKind::BinOp(BinOp::SRem, _, Operand::ConstI64(2))
        ));
        assert!(matches!(
            f.blocks[1].insts[1].kind,
            InstKind::ICmp(IPred::Eq, _, Operand::ConstI64(0))
        ));
        assert!(matches!(
            f.blocks[2].insts[0].kind,
            InstKind::BinOp(BinOp::SDiv, _, Operand::ConstI64(2))
        ));
        // sitofp entry became a copy of the i64 source
        assert!(matches!(
            f.blocks[0].insts[1].kind,
            InstKind::Copy(Type::I64, Operand::Value(ValueId(0)))
        ));
        // ret goes through a fresh sitofp bridge
        let Terminator::Ret(Some(Operand::Value(rb))) = f.blocks[3].term else {
            panic!("expected ret of bridge");
        };
        assert!(matches!(
            f.blocks[3].insts.last().unwrap().kind,
            InstKind::SiToFp(Operand::Value(ValueId(2)))
        ));
        assert_eq!(f.blocks[3].insts.last().unwrap().result, Some(rb));
        // demoted cells are i64-typed now
        assert_eq!(f.values[2].ty, Type::I64);
    }

    #[test]
    fn growth_loop_stays_f64() {
        // collatz odd arm: n = 3n + 1 — the fmul/fadd bounds widen to
        // a sentinel, so nothing may demote (phase 1b-ii territory).
        let f = func(
            Type::F64,
            vec![
                Type::I64,  // %0
                Type::F64,  // %1 sitofp
                Type::F64,  // %2 n cell
                Type::F64,  // %3 fmul
                Type::F64,  // %4 fadd
                Type::Bool, // %5 fcmp
            ],
            vec![
                block(
                    0,
                    vec![
                        inst(0, InstKind::Copy(Type::I64, Operand::ConstI64(7))),
                        inst(1, InstKind::SiToFp(v(0))),
                        inst(2, InstKind::Copy(Type::F64, v(1))),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![
                        inst(3, InstKind::BinOp(BinOp::FMul, cf(3.0), v(2))),
                        inst(4, InstKind::BinOp(BinOp::FAdd, v(3), cf(1.0))),
                        inst(2, InstKind::Copy(Type::F64, v(4))),
                        inst(5, InstKind::FCmp(FPred::Une, v(2), cf(1.0))),
                    ],
                    Terminator::CondBr {
                        cond: v(5),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                ),
                block(2, vec![], Terminator::Ret(Some(v(2)))),
            ],
        );
        let (m, stats) = run(f);
        assert_eq!(stats.values_demoted, 0);
        assert!(matches!(
            m.funcs[0].blocks[1].insts[0].kind,
            InstKind::BinOp(BinOp::FMul, _, _)
        ));
    }

    #[test]
    fn non_integer_entry_stays_f64() {
        let f = func(
            Type::F64,
            vec![Type::F64, Type::F64],
            vec![block(
                0,
                vec![
                    inst(0, InstKind::Copy(Type::F64, cf(0.5))),
                    inst(1, InstKind::BinOp(BinOp::FAdd, v(0), cf(1.0))),
                ],
                Terminator::Ret(Some(v(1))),
            )],
        );
        let (_, stats) = run(f);
        assert_eq!(stats.values_demoted, 0);
    }

    #[test]
    fn negative_dividend_frem_not_demoted() {
        // frem over a possibly-negative dividend can yield -0, so its
        // result may not bridge; the sitofp entry then has no useful
        // i64 use left (a bridge-only demote is a net loss) and the
        // whole chain stays f64.
        let f = func(
            Type::F64,
            vec![Type::I64, Type::F64, Type::F64],
            vec![block(
                0,
                vec![
                    inst(0, InstKind::Copy(Type::I64, Operand::ConstI64(-8))),
                    inst(1, InstKind::SiToFp(v(0))),
                    inst(2, InstKind::BinOp(BinOp::FRem, v(1), cf(2.0))),
                ],
                Terminator::Ret(Some(v(2))),
            )],
        );
        let (m, stats) = run(f);
        assert_eq!(stats, FloatDemoteStats::default());
        let f0 = &m.funcs[0];
        assert!(
            f0.blocks[0]
                .insts
                .iter()
                .any(|i| matches!(i.kind, InstKind::BinOp(BinOp::FRem, _, _)))
        );
        assert!(matches!(f0.blocks[0].insts[1].kind, InstKind::SiToFp(_)));
        assert_eq!(f0.values[1].ty, Type::F64);
        assert_eq!(f0.values[2].ty, Type::F64);
    }

    #[test]
    fn fdiv_without_parity_proof_not_demoted() {
        let f = func(
            Type::F64,
            vec![Type::I64, Type::F64, Type::F64],
            vec![block(
                0,
                vec![
                    inst(0, InstKind::Copy(Type::I64, Operand::ConstI64(5))),
                    inst(1, InstKind::SiToFp(v(0))),
                    inst(2, InstKind::BinOp(BinOp::FDiv, v(1), cf(2.0))),
                ],
                Terminator::Ret(Some(v(2))),
            )],
        );
        let (m, stats) = run(f);
        // 5 / 2 = 2.5: no parity proof, the fdiv stays f64; the
        // entry's only use would bridge, so nothing fires.
        assert_eq!(stats, FloatDemoteStats::default());
        let f0 = &m.funcs[0];
        assert!(
            f0.blocks[0]
                .insts
                .iter()
                .any(|i| matches!(i.kind, InstKind::BinOp(BinOp::FDiv, _, _)))
        );
        assert_eq!(f0.values[1].ty, Type::F64);
        assert_eq!(f0.values[2].ty, Type::F64);
    }
}
