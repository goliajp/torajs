//! Exact float remainder over a non-negative integer dividend —
//! `frem(sitofp x, C)` with `x ∈ [0, 2^53)` and `C` a positive integral
//! constant becomes `sitofp(srem x, C)`, and a number-to-string reader
//! of the result (`__torajs_f64_to_str`, or an already fused
//! `__torajs_str_concat_f64`) takes the integer through its i64 twin.
//!
//! C1 lowers every `%` whose dividend is not provably non-negative
//! through the float path (for the `-0` a negative dividend yields),
//! and frem_narrow recovers the int form only under a compare or an
//! fptosi sink. `"key" + (i % 4096)` has neither: the remainder feeds a
//! string concat, so every iteration paid a libm `fmod` call and then
//! formatted an integer through the f64 digit kernel (map-str-100k
//! decomposition 2026-09-17, S1: 7 ns of fmod per iteration).
//!
//! The interval facts are what make it exact: a dividend in
//! `[0, 2^53)` converts without rounding and never produces `-0`, and
//! for such a dividend `fmod` and the i64 remainder agree. The concat
//! rewrite needs the remainder below 2^53 too, which `0 ≤ r < C ≤ 2^53`
//! gives.
//!
//! Runs right after cmp_fact_fold (facts still id-valid).
//! `TORAJS_FREM_INT_OFF=1` skips, `TORAJS_FREM_INT_STATS=1` counts.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    BinOp, FuncId, Function, Inst, InstKind, Module, Operand, Terminator, Type, ValueId, ValueInfo,
};

use crate::interval::NumFact;
use crate::rc_peephole::visit_value_operands;

/// f64 number-to-string kernels and their i64 twins. For an integer
/// below 2^53 both spell the same digits.
const TWINS: [(&str, &str); 2] = [
    ("__torajs_f64_to_str", "__torajs_i64_to_str"),
    ("__torajs_str_concat_f64", "__torajs_str_concat_i64"),
];
const F64_EXACT: i128 = 1 << 53;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FremIntStats {
    /// frem defs rewritten to sitofp-of-srem.
    pub frems_exact: u32,
    /// f64 to-string / concat calls moved onto their i64 twin.
    pub to_str_int: u32,
}

pub fn exact_frems(module: &mut Module, facts: &[HashMap<ValueId, NumFact>]) -> FremIntStats {
    let mut stats = FremIntStats::default();
    let find = |name: &str| {
        module
            .funcs
            .iter()
            .position(|f| f.name == name)
            .map(|i| FuncId(i as u32))
    };
    let twins: Vec<(FuncId, FuncId)> = TWINS
        .iter()
        .filter_map(|(f, i)| find(f).zip(find(i)))
        .collect();
    for (func, fm) in module.funcs.iter_mut().zip(facts) {
        if func.is_declaration() || !has_frem(func) {
            continue;
        }
        rewrite_function(func, fm, &twins, &mut stats);
    }
    stats
}

/// Any `frem` at all: a function without one is not read for its defs.
pub(crate) fn has_frem(func: &Function) -> bool {
    func.blocks
        .iter()
        .flat_map(|b| &b.insts)
        .any(|i| matches!(i.kind, InstKind::BinOp(BinOp::FRem, ..)))
}

/// Kind of every value with exactly one def site.
pub(crate) fn single_def_kinds(func: &Function) -> HashMap<ValueId, InstKind> {
    let mut defs: HashMap<ValueId, Option<InstKind>> = HashMap::default();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(r) = inst.result {
                defs.entry(r)
                    .and_modify(|k| *k = None)
                    .or_insert_with(|| Some(inst.kind.clone()));
            }
        }
    }
    defs.into_iter()
        .filter_map(|(v, k)| Some((v, k?)))
        .collect()
}

/// A positive integral f64 constant `≤ 2^53`, spelled directly or
/// through a single-def `copy f64 <const>`.
pub(crate) fn positive_int_const(op: &Operand, defs: &HashMap<ValueId, InstKind>) -> Option<i64> {
    let c = match op {
        Operand::ConstF64(c) => *c,
        Operand::Value(v) => match defs.get(v)? {
            InstKind::Copy(Type::F64, Operand::ConstF64(c)) => *c,
            _ => return None,
        },
        _ => return None,
    };
    (c >= 1.0 && c <= F64_EXACT as f64 && c.fract() == 0.0).then_some(c as i64)
}

fn rewrite_function(
    func: &mut Function,
    facts: &HashMap<ValueId, NumFact>,
    twins: &[(FuncId, FuncId)],
    stats: &mut FremIntStats,
) {
    let defs = single_def_kinds(func);
    // frem result → (int remainder value)
    let mut remainders: HashMap<ValueId, ValueId> = HashMap::default();
    for bi in 0..func.blocks.len() {
        let mut i = 0;
        while i < func.blocks[bi].insts.len() {
            let inst = &func.blocks[bi].insts[i];
            let (Some(r), InstKind::BinOp(BinOp::FRem, a, b)) = (inst.result, &inst.kind) else {
                i += 1;
                continue;
            };
            // a multi-def result is a loop cell; its readers may see
            // another def, so they cannot take this remainder
            let Some(c) = positive_int_const(b, &defs).filter(|_| defs.contains_key(&r)) else {
                i += 1;
                continue;
            };
            let Operand::Value(av) = a else {
                i += 1;
                continue;
            };
            let Some(InstKind::SiToFp(Operand::Value(x))) = defs.get(av) else {
                i += 1;
                continue;
            };
            let x = *x;
            let in_range = facts.get(&x).is_some_and(|f| f.lo >= 0 && f.hi < F64_EXACT);
            if !in_range
                || func.values[x.0 as usize].ty != Type::I64
                || !stable_here(func, bi, i, *av, x)
            {
                i += 1;
                continue;
            }
            let s = ValueId(func.values.len() as u32);
            func.values.push(ValueInfo {
                ty: Type::I64,
                name: None,
            });
            let origin = func.blocks[bi].insts[i].origin;
            func.blocks[bi].insts[i].kind = InstKind::SiToFp(Operand::Value(s));
            func.blocks[bi].insts.insert(
                i,
                Inst {
                    result: Some(s),
                    kind: InstKind::BinOp(BinOp::SRem, Operand::Value(x), Operand::ConstI64(c)),
                    origin,
                },
            );
            remainders.insert(r, s);
            stats.frems_exact += 1;
            i += 2;
        }
    }
    for block in func.blocks.iter_mut() {
        for inst in block.insts.iter_mut() {
            let InstKind::Call(fid, args) = &mut inst.kind else {
                continue;
            };
            let Some(&(_, i64_fid)) = twins.iter().find(|(f, _)| f == fid) else {
                continue;
            };
            // the number is the last argument of both kernels
            if let Some(Operand::Value(n)) = args.last()
                && let Some(s) = remainders.get(n)
            {
                *args.last_mut().expect("matched above") = Operand::Value(*s);
                *fid = i64_fid;
                stats.to_str_int += 1;
            }
        }
    }
    sweep_dead_converts(func);
}

/// Drop `sitofp` defs nothing reads any more — the rewritten frem's
/// own result once every reader moved to the integer, then the
/// dividend conversion it read.
fn sweep_dead_converts(func: &mut Function) {
    loop {
        let mut used: HashSet<ValueId> = HashSet::default();
        for block in &func.blocks {
            for inst in &block.insts {
                visit_value_operands(&inst.kind, |v| {
                    used.insert(v);
                });
            }
            match &block.term {
                Terminator::CondBr {
                    cond: Operand::Value(v),
                    ..
                }
                | Terminator::Ret(Some(Operand::Value(v))) => {
                    used.insert(*v);
                }
                _ => {}
            }
        }
        let before: usize = func.blocks.iter().map(|b| b.insts.len()).sum();
        for block in func.blocks.iter_mut() {
            block.insts.retain(|i| {
                !matches!((&i.result, &i.kind), (Some(r), InstKind::SiToFp(_)) if !used.contains(r))
            });
        }
        if func.blocks.iter().map(|b| b.insts.len()).sum::<usize>() == before {
            break;
        }
    }
}

/// `x` still holds the value `sitofp` read when the frem at
/// `(bi, at)` runs: the sitofp `av` sits earlier in the same block and
/// nothing between redefines `x` (a multi-def loop cell).
pub(crate) fn stable_here(func: &Function, bi: usize, at: usize, av: ValueId, x: ValueId) -> bool {
    let insts = &func.blocks[bi].insts;
    let Some(conv) = insts[..at].iter().rposition(|i| i.result == Some(av)) else {
        return false;
    };
    !insts[conv + 1..at].iter().any(|i| i.result == Some(x))
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, BlockId};

    fn inst(r: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(r)),
            kind,
            origin: None,
        }
    }
    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    fn frem_fn() -> Function {
        let ty = |t| ValueInfo { ty: t, name: None };
        Function {
            name: "f".into(),
            params: vec![ValueId(0)],
            ret: Type::F64,
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![
                    inst(1, InstKind::SiToFp(v(0))),
                    inst(
                        2,
                        InstKind::BinOp(BinOp::FRem, v(1), Operand::ConstF64(4096.0)),
                    ),
                ],
                term: Terminator::Ret(Some(v(2))),
            }],
            values: vec![ty(Type::I64), ty(Type::F64), ty(Type::F64)],
            current_origin: None,
        }
    }

    #[test]
    fn nonnegative_dividend_takes_the_integer_remainder() {
        let mut f = frem_fn();
        let facts = HashMap::from_iter([(ValueId(0), NumFact::new(0, 99_999))]);
        let mut stats = FremIntStats::default();
        rewrite_function(&mut f, &facts, &[], &mut stats);
        assert_eq!(stats.frems_exact, 1);
        // the dividend's own conversion has no reader left and is swept
        let insts = &f.blocks[0].insts;
        assert_eq!(insts.len(), 2);
        assert_eq!(
            insts[0].kind,
            InstKind::BinOp(BinOp::SRem, v(0), Operand::ConstI64(4096))
        );
        assert_eq!(insts[1].kind, InstKind::SiToFp(v(3)));
        assert_eq!(insts[1].result, Some(ValueId(2)));
    }

    #[test]
    fn possibly_negative_dividend_keeps_the_frem() {
        let mut f = frem_fn();
        let facts = HashMap::from_iter([(ValueId(0), NumFact::new(-1, 99_999))]);
        let mut stats = FremIntStats::default();
        rewrite_function(&mut f, &facts, &[], &mut stats);
        assert_eq!(stats.frems_exact, 0);
    }
}
