//! Integer remainder for a float `%` whose operands are integers at
//! run time. `frem` is a libm `fmod` call; a `%` over two integers in
//! range is an `sdiv` / `msub` pair, or an `and` by a power of two.
//!
//! float_demote, frem_narrow and frem_int turn a `frem` into `srem`
//! where the facts prove both operands exact, the dividend
//! non-negative and the divisor non-zero. What they leave is every
//! `%` over a number the facts know too little about — a count from
//! `parseInt(argv)` (`arr[i % pool]`, `"key" + (i % keys)`), the LCG
//! `seed % 6` of the string-generating bench cells, a gcd's `a % b` —
//! whatever the sink: an index, a compare, a concat, an argument, a
//! store. Each paid a `fmod` call per evaluation (22% of
//! array-any-indexed-100k, 19% of the tokenizer cell, 2026-10-02).
//!
//! The rewrite is per site, not per sink: the `frem` splits into a
//! diamond — a head that checks both operands (see [`lane`]), an
//! integer arm, and the `frem` as it was for everything else, so `-0`,
//! NaN, ±Inf, a zero divisor, a fraction and anything past 2^53 keep
//! their exact `fmod` answer. The interval facts drop the checks they
//! prove: a loop counter `sitofp i` with `i ∈ [0, 2^53)` needs none,
//! and when nothing is left to check the `frem` is rewritten in place
//! with no diamond. An operand defined outside the loop that reads it
//! is converted and checked once, after its def ([`hoist`]).
//!
//! The result keeps its ValueId and gains one def per arm, the shape φ
//! destruction leaves; readers are untouched. The new blocks go in
//! right after the head — fast arm, the rest of the block — with the
//! slow arm at the end of the function, so the passing path falls
//! through and only the slow arm pays a taken branch.
//!
//! Runs in `fact_folds` right after frem_int. `TORAJS_FREM_GUARD_OFF=1`
//! skips, `TORAJS_FREM_GUARD_STATS=1` counts.

mod hoist;
mod lane;

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    BinOp, BlockId, Function, IPred, Inst, InstKind, Module, Operand, Terminator, Type, ValueId,
};

use crate::cfg_edit::{emit, new_block, renumber};
use crate::float_demote::{push_value, split_block_at};
use crate::frem_int::{has_frem, single_def_kinds, stable_here};
use crate::interval::NumFact;
use lane::{Lane, Role, classify};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FremGuardStats {
    /// frem sites split into a check / integer / frem diamond
    pub sites: u32,
    /// frem sites the facts proved outright, rewritten with no check
    pub proven: u32,
}

pub fn guard_frems(module: &mut Module, facts: &[HashMap<ValueId, NumFact>]) -> FremGuardStats {
    let mut stats = FremGuardStats::default();
    for (func, fm) in module.funcs.iter_mut().zip(facts) {
        if func.is_declaration() || !has_frem(func) {
            continue;
        }
        let (sites, proven) = guard_function(func, fm);
        stats.sites += sites;
        stats.proven += proven;
    }
    stats
}

/// One matched `frem`: where it sits, what it defines, how each
/// operand reaches the integer remainder.
struct Site {
    at: usize,
    result: ValueId,
    a: Lane,
    b: Lane,
}

pub(crate) fn guard_function(func: &mut Function, facts: &HashMap<ValueId, NumFact>) -> (u32, u32) {
    let defs = single_def_kinds(func);
    let mut fixed = hoist::fixed_values(func);
    // the frems present on entry; a slow arm's frem defines a fresh
    // value outside this set and is passed over
    let mut todo: HashSet<ValueId> = func
        .blocks
        .iter()
        .flat_map(|b| &b.insts)
        .filter(|i| matches!(i.kind, InstKind::BinOp(BinOp::FRem, _, _)))
        .filter_map(|i| i.result)
        .collect();
    let mut hoisted = HashMap::default();
    let (mut sites, mut proven) = (0, 0);
    let mut bi = 0;
    // a split puts the rest of the block one position on and the slow
    // arm at the end, so the scan reaches both
    while bi < func.blocks.len() {
        while let Some(site) = find_site(func, bi, &todo, &fixed, &defs, facts) {
            todo.remove(&site.result);
            let site = Site {
                a: hoist::hoist(func, bi, site.a, Role::Dividend, &fixed, &mut hoisted),
                b: hoist::hoist(func, bi, site.b, Role::Divisor, &fixed, &mut hoisted),
                ..site
            };
            if site.a.free(Role::Dividend) && site.b.free(Role::Divisor) {
                rewrite_in_place(func, bi, &site);
                proven += 1;
            } else {
                guard_site(func, BlockId(bi as u32), &site);
                // the result now has one def per arm
                fixed.remove(&site.result);
                sites += 1;
                break;
            }
        }
        bi += 1;
    }
    (sites, proven)
}

/// The first `frem` of the block still to do whose operands may both
/// be inside their windows.
fn find_site(
    func: &Function,
    bi: usize,
    todo: &HashSet<ValueId>,
    fixed: &HashSet<ValueId>,
    defs: &HashMap<ValueId, InstKind>,
    facts: &HashMap<ValueId, NumFact>,
) -> Option<Site> {
    let f64_op = |op: &Operand| match op {
        Operand::Value(v) => func.values[v.0 as usize].ty == Type::F64,
        _ => true,
    };
    func.blocks[bi]
        .insts
        .iter()
        .enumerate()
        .find_map(|(at, inst)| {
            let (Some(result), InstKind::BinOp(BinOp::FRem, a, b)) = (inst.result, &inst.kind)
            else {
                return None;
            };
            if !todo.contains(&result) || !f64_op(a) || !f64_op(b) {
                return None;
            }
            let stable = |v: ValueId, x: ValueId| {
                fixed.contains(&x)
                    || stable_here(func, bi, at, v, x)
                    || hoist::stable_chain(func, bi, at, v, x)
            };
            let a = classify(a, Role::Dividend, defs, facts, &stable)?;
            let b = classify(b, Role::Divisor, defs, facts, &stable)?;
            if matches!((a, b), (Lane::Const(_), Lane::Const(_))) {
                return None;
            }
            Some(Site { at, result, a, b })
        })
}

/// `t mod d` for `t ≥ 0`: an `and` when `d` is a power-of-two constant.
fn remainder(t: Operand, d: Operand) -> InstKind {
    match d {
        Operand::ConstI64(c) if (c as u64).is_power_of_two() => {
            InstKind::BinOp(BinOp::And, t, Operand::ConstI64(c - 1))
        }
        _ => InstKind::BinOp(BinOp::SRem, t, d),
    }
}

/// Every check proven away: the `frem` becomes `sitofp` of the integer
/// remainder where it stands.
fn rewrite_in_place(func: &mut Function, bi: usize, site: &Site) {
    let origin = func.blocks[bi].insts[site.at].origin;
    let mut new: Vec<Inst> = Vec::new();
    let values = &mut func.values;
    let mut emit = |ty: Type, kind: InstKind| {
        let result = push_value(values, ty);
        new.push(Inst {
            result: Some(result),
            kind,
            origin,
        });
        Operand::Value(result)
    };
    let (ta, _) = site.a.lower(Role::Dividend, &mut emit);
    let (tb, _) = site.b.lower(Role::Divisor, &mut emit);
    let s = emit(Type::I64, remainder(ta, tb));
    let block = &mut func.blocks[bi];
    block.insts[site.at].kind = InstKind::SiToFp(s);
    block.insts.splice(site.at..site.at, new);
}

/// head: operand checks ORed, `≠ 0` → slow | fast
/// fast: `r = sitofp (srem ta, tb)` → join
/// slow: `r = frem a, b` as it was → join
fn guard_site(func: &mut Function, head: BlockId, site: &Site) {
    let fast = new_block(func);
    let join = split_block_at(func, head, &[site.at])[0];
    let slow = new_block(func);

    let mut frem = func.blocks[join.0 as usize].insts.remove(0);
    let origin = frem.origin;
    let fr = push_value(&mut func.values, Type::F64);
    frem.result = Some(fr);
    let slow_block = &mut func.blocks[slow.0 as usize];
    slow_block.insts.push(frem);
    slow_block.insts.push(Inst {
        result: Some(site.result),
        kind: InstKind::Copy(Type::F64, Operand::Value(fr)),
        origin,
    });
    slow_block.term = Terminator::Br(join);

    let mut in_head = |ty: Type, kind: InstKind| emit(func, head, ty, kind);
    let (ta, check_a) = site.a.lower(Role::Dividend, &mut in_head);
    let (tb, check_b) = site.b.lower(Role::Divisor, &mut in_head);
    let bad = match (check_a, check_b) {
        (Some(x), Some(y)) => in_head(Type::I64, InstKind::BinOp(BinOp::Or, x, y)),
        (Some(x), None) | (None, Some(x)) => x,
        (None, None) => unreachable!("a site with nothing to check is rewritten in place"),
    };
    let outside = in_head(
        Type::Bool,
        InstKind::ICmp(IPred::Ne, bad, Operand::ConstI64(0)),
    );
    func.blocks[head.0 as usize].term = Terminator::CondBr {
        cond: outside,
        then_blk: slow,
        else_blk: fast,
    };

    let s = emit(func, fast, Type::I64, remainder(ta, tb));
    let f = emit(func, fast, Type::F64, InstKind::SiToFp(s));
    func.blocks[fast.0 as usize].insts.push(Inst {
        result: Some(site.result),
        kind: InstKind::Copy(Type::F64, f),
        origin,
    });
    func.blocks[fast.0 as usize].term = Terminator::Br(join);

    // fast · join follow the head; the slow arm stays last
    let tail = &mut func.blocks[head.0 as usize + 1..];
    tail.rotate_right(3);
    tail[2..].rotate_left(1);
    renumber(func);
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, ValueInfo};

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

    /// `%1 = fmul %0, 48271; %2 = frem %1, <divisor>; %3 = fadd %2, 1`
    /// then a second block the first falls into
    fn frem_fn(divisor: Operand) -> Function {
        let ty = |t| ValueInfo { ty: t, name: None };
        Function {
            name: "f".into(),
            params: vec![ValueId(0)],
            ret: Type::F64,
            blocks: vec![
                Block {
                    id: BlockId(0),
                    insts: vec![
                        inst(
                            1,
                            InstKind::BinOp(BinOp::FMul, v(0), Operand::ConstF64(48271.0)),
                        ),
                        inst(2, InstKind::BinOp(BinOp::FRem, v(1), divisor)),
                        inst(
                            3,
                            InstKind::BinOp(BinOp::FAdd, v(2), Operand::ConstF64(1.0)),
                        ),
                    ],
                    term: Terminator::Br(BlockId(1)),
                },
                Block {
                    id: BlockId(1),
                    insts: vec![],
                    term: Terminator::Ret(Some(v(3))),
                },
            ],
            values: vec![ty(Type::F64), ty(Type::F64), ty(Type::F64), ty(Type::F64)],
            current_origin: None,
        }
    }

    fn guard(f: &mut Function) -> (u32, u32) {
        guard_function(f, &HashMap::default())
    }

    fn defs_of(func: &Function, r: ValueId) -> Vec<(BlockId, InstKind)> {
        func.blocks
            .iter()
            .flat_map(|b| b.insts.iter().map(move |i| (b.id, i)))
            .filter(|(_, i)| i.result == Some(r))
            .map(|(b, i)| (b, i.kind.clone()))
            .collect()
    }

    fn cond_targets(term: &Terminator) -> (BlockId, BlockId) {
        let Terminator::CondBr {
            then_blk, else_blk, ..
        } = term
        else {
            panic!("expected a conditional, got {term:?}");
        };
        (*then_blk, *else_blk)
    }

    fn fast_kind(f: &Function) -> InstKind {
        f.blocks[1].insts[0].kind.clone()
    }

    #[test]
    fn an_opaque_dividend_splits_into_a_diamond() {
        let mut f = frem_fn(Operand::ConstF64(2147483647.0));
        assert_eq!(guard(&mut f), (1, 0));
        // head · fast · join · ret · slow, ids by position
        assert_eq!(f.blocks.len(), 5);
        for (i, b) in f.blocks.iter().enumerate() {
            assert_eq!(b.id.0 as usize, i);
        }
        let (fast, join, slow) = (BlockId(1), BlockId(2), BlockId(4));
        assert_eq!(cond_targets(&f.blocks[0].term), (slow, fast));
        assert!(matches!(f.blocks[1].term, Terminator::Br(b) if b == join));
        assert!(matches!(f.blocks[2].term, Terminator::Br(b) if b == BlockId(3)));
        assert!(matches!(f.blocks[3].term, Terminator::Ret(_)));
        assert!(matches!(f.blocks[4].term, Terminator::Br(b) if b == join));
        // the head keeps the product, truncates, checks the roundtrip
        // and the window, and branches once
        let head = &f.blocks[0].insts;
        assert!(matches!(
            head[1].kind,
            InstKind::FpToSi(Operand::Value(ValueId(1)))
        ));
        assert!(
            head.iter()
                .any(|i| matches!(i.kind, InstKind::BinOp(BinOp::Xor, _, _)))
        );
        assert!(head.iter().any(|i| matches!(
            i.kind,
            InstKind::BinOp(BinOp::LShr, _, Operand::ConstI64(53))
        )));
        assert!(matches!(
            head.last().unwrap().kind,
            InstKind::ICmp(IPred::Ne, _, Operand::ConstI64(0))
        ));
        // the fast arm takes the integer remainder by the same constant
        assert!(matches!(
            fast_kind(&f),
            InstKind::BinOp(BinOp::SRem, _, Operand::ConstI64(2147483647))
        ));
        // the result is defined once per arm, both as f64 copies
        let defs = defs_of(&f, ValueId(2));
        assert_eq!(defs.len(), 2);
        assert!(
            defs.iter()
                .all(|(_, k)| matches!(k, InstKind::Copy(Type::F64, _)))
        );
        assert_eq!(defs[0].0, fast);
        assert_eq!(defs[1].0, slow);
        // the slow arm holds the frem as it was, on a fresh value
        let slow_insts = &f.blocks[slow.0 as usize].insts;
        assert!(matches!(
            slow_insts[0].kind,
            InstKind::BinOp(
                BinOp::FRem,
                Operand::Value(ValueId(1)),
                Operand::ConstF64(_)
            )
        ));
        assert_ne!(slow_insts[0].result, Some(ValueId(2)));
        // the reader moved to the join untouched
        assert!(matches!(
            f.blocks[join.0 as usize].insts[0].kind,
            InstKind::BinOp(BinOp::FAdd, Operand::Value(ValueId(2)), _)
        ));
    }

    #[test]
    fn a_power_of_two_divisor_masks() {
        let mut f = frem_fn(Operand::ConstF64(4096.0));
        assert_eq!(guard(&mut f), (1, 0));
        assert!(matches!(
            fast_kind(&f),
            InstKind::BinOp(BinOp::And, _, Operand::ConstI64(4095))
        ));
    }

    #[test]
    fn a_variable_divisor_is_checked_too() {
        let mut f = frem_fn(v(0));
        assert_eq!(guard(&mut f), (1, 0));
        assert!(matches!(
            fast_kind(&f),
            InstKind::BinOp(BinOp::SRem, Operand::Value(_), Operand::Value(_))
        ));
        // the divisor's window is shifted down by one before the shift
        assert!(
            f.blocks[0]
                .insts
                .iter()
                .any(|i| matches!(i.kind, InstKind::BinOp(BinOp::Sub, _, Operand::ConstI64(1))))
        );
    }

    /// `%1 = sitofp %0` (i64); with `facts` on `%0` and the divisor
    fn sitofp_fn(divisor: Operand) -> Function {
        let mut f = frem_fn(divisor);
        f.blocks[0].insts[0].kind = InstKind::SiToFp(v(0));
        f.values[0].ty = Type::I64;
        f
    }

    #[test]
    fn a_sitofp_dividend_checks_only_its_window() {
        let mut f = sitofp_fn(Operand::ConstF64(6.0));
        assert_eq!(guard(&mut f), (1, 0));
        // no truncation: the i64 itself is shifted and divided
        let head = &f.blocks[0].insts;
        assert!(!head.iter().any(|i| matches!(i.kind, InstKind::FpToSi(_))));
        assert!(matches!(
            head[1].kind,
            InstKind::BinOp(
                BinOp::LShr,
                Operand::Value(ValueId(0)),
                Operand::ConstI64(53)
            )
        ));
        assert!(matches!(
            fast_kind(&f),
            InstKind::BinOp(
                BinOp::SRem,
                Operand::Value(ValueId(0)),
                Operand::ConstI64(6)
            )
        ));
    }

    #[test]
    fn facts_proving_both_operands_rewrite_in_place() {
        // %4 = sitofp %5 (i64 in [1, 100]) is the divisor
        let mut f = sitofp_fn(v(4));
        f.values.push(ValueInfo {
            ty: Type::F64,
            name: None,
        });
        f.values.push(ValueInfo {
            ty: Type::I64,
            name: None,
        });
        f.blocks[0].insts.insert(1, inst(4, InstKind::SiToFp(v(5))));
        let facts = HashMap::from_iter([
            (ValueId(0), NumFact::new(0, 1 << 40)),
            (ValueId(5), NumFact::new(1, 100)),
        ]);
        assert_eq!(guard_function(&mut f, &facts), (0, 1));
        assert_eq!(f.blocks.len(), 2);
        let insts = &f.blocks[0].insts;
        assert!(matches!(
            insts[2].kind,
            InstKind::BinOp(
                BinOp::SRem,
                Operand::Value(ValueId(0)),
                Operand::Value(ValueId(5))
            )
        ));
        assert_eq!(insts[3].result, Some(ValueId(2)));
        assert!(matches!(insts[3].kind, InstKind::SiToFp(_)));
    }

    #[test]
    fn a_divisor_fact_that_admits_zero_keeps_its_check() {
        let mut f = frem_fn(v(0));
        let facts = HashMap::from_iter([(ValueId(0), NumFact::new(0, 100))]);
        assert_eq!(guard_function(&mut f, &facts), (1, 0));
    }

    #[test]
    fn a_dividend_fact_still_checks_the_sign_of_zero() {
        // %1 = fmul … proven in [0, 1000]: only `-0` is left to rule out
        let mut f = frem_fn(Operand::ConstF64(7.0));
        let facts = HashMap::from_iter([(ValueId(1), NumFact::new(0, 1000))]);
        assert_eq!(guard_function(&mut f, &facts), (1, 0));
        let head = &f.blocks[0].insts;
        assert!(!head.iter().any(|i| matches!(i.kind, InstKind::SiToFp(_))));
        assert!(head.iter().any(|i| matches!(
            i.kind,
            InstKind::BinOp(BinOp::LShr, _, Operand::ConstI64(63))
        )));
    }

    #[test]
    fn a_redefined_sitofp_source_is_truncated_instead() {
        // `%1 = sitofp %0; %0 = …; %2 = frem %1, 6` — %0 moved on
        let mut f = sitofp_fn(Operand::ConstF64(6.0));
        f.blocks[0]
            .insts
            .insert(1, inst(0, InstKind::Copy(Type::I64, Operand::ConstI64(9))));
        assert_eq!(guard(&mut f), (1, 0));
        assert!(matches!(
            f.blocks[0].insts[2].kind,
            InstKind::FpToSi(Operand::Value(ValueId(1)))
        ));
    }

    #[test]
    fn a_constant_outside_its_window_is_left_alone() {
        for c in [
            -3.0,
            0.0,
            -0.0,
            2.5,
            9007199254740994.0,
            f64::INFINITY,
            f64::NAN,
        ] {
            let mut f = frem_fn(Operand::ConstF64(c));
            assert_eq!(guard(&mut f), (0, 0), "divisor {c}");
        }
        for c in [-1.0, -0.0, 0.5, 9007199254740992.0] {
            let mut f = frem_fn(v(0));
            f.blocks[0].insts[1].kind = InstKind::BinOp(BinOp::FRem, Operand::ConstF64(c), v(0));
            assert_eq!(guard(&mut f), (0, 0), "dividend {c}");
        }
    }

    #[test]
    fn a_multi_def_result_is_guarded_too() {
        let mut f = frem_fn(Operand::ConstF64(6.0));
        f.blocks[1]
            .insts
            .push(inst(2, InstKind::Copy(Type::F64, Operand::ConstF64(0.0))));
        assert_eq!(guard(&mut f), (1, 0));
        assert_eq!(defs_of(&f, ValueId(2)).len(), 3);
    }

    /// bb0: `%5 = 0` → bb1: `%1 = sitofp %5`, `cond_br` → bb2: `%2 =
    /// frem %1, %0; %5 = add %5, 1` → bb1; bb3: ret. `%0` is a param.
    fn counter_loop() -> Function {
        let ty = |t| ValueInfo { ty: t, name: None };
        let block = |id, insts, term| Block {
            id: BlockId(id),
            insts,
            term,
        };
        Function {
            name: "f".into(),
            params: vec![ValueId(0)],
            ret: Type::F64,
            blocks: vec![
                block(
                    0,
                    vec![inst(5, InstKind::Copy(Type::I64, Operand::ConstI64(0)))],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![inst(1, InstKind::SiToFp(v(5)))],
                    Terminator::CondBr {
                        cond: Operand::ConstBool(true),
                        then_blk: BlockId(2),
                        else_blk: BlockId(3),
                    },
                ),
                block(
                    2,
                    vec![
                        inst(2, InstKind::BinOp(BinOp::FRem, v(1), v(0))),
                        inst(5, InstKind::BinOp(BinOp::Add, v(5), Operand::ConstI64(1))),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(3, vec![], Terminator::Ret(Some(v(2)))),
            ],
            values: [
                Type::F64,
                Type::F64,
                Type::F64,
                Type::F64,
                Type::F64,
                Type::I64,
            ]
            .map(ty)
            .to_vec(),
            current_origin: None,
        }
    }

    #[test]
    fn a_loop_counter_reaches_the_remainder_untruncated() {
        let mut f = counter_loop();
        assert_eq!(guard(&mut f), (1, 0));
        // the counter cell is read as it is; only its window is checked
        let head = &f.blocks[2].insts;
        assert!(matches!(
            head[0].kind,
            InstKind::BinOp(
                BinOp::LShr,
                Operand::Value(ValueId(5)),
                Operand::ConstI64(53)
            )
        ));
        assert!(!head.iter().any(|i| matches!(i.kind, InstKind::FpToSi(_))));
    }

    #[test]
    fn a_divisor_defined_outside_the_loop_is_checked_once_on_entry() {
        let mut f = counter_loop();
        assert_eq!(guard(&mut f), (1, 0));
        // entry truncates and checks the divisor before the loop
        let entry = &f.blocks[0].insts;
        assert!(matches!(
            entry[0].kind,
            InstKind::FpToSi(Operand::Value(ValueId(0)))
        ));
        assert!(
            entry
                .iter()
                .any(|i| matches!(i.kind, InstKind::BinOp(BinOp::Sub, _, Operand::ConstI64(1))))
        );
        // the loop's head only ORs the two checks
        let head = &f.blocks[2].insts;
        assert!(!head.iter().any(|i| matches!(i.kind, InstKind::FpToSi(_))));
        assert_eq!(head.len(), 3);
    }

    #[test]
    fn a_counter_redefined_on_the_way_is_truncated() {
        let mut f = counter_loop();
        // bb1 bumps the counter after the sitofp read it
        f.blocks[1].insts.push(inst(
            5,
            InstKind::BinOp(BinOp::Add, v(5), Operand::ConstI64(1)),
        ));
        assert_eq!(guard(&mut f), (1, 0));
        assert!(
            f.blocks[2]
                .insts
                .iter()
                .any(|i| matches!(i.kind, InstKind::FpToSi(Operand::Value(ValueId(1)))))
        );
    }

    #[test]
    fn a_divisor_outside_any_loop_stays_at_its_site() {
        // with no back edge the frem runs at most once per call
        let mut f = counter_loop();
        f.blocks[2].term = Terminator::Br(BlockId(3));
        assert_eq!(guard(&mut f), (1, 0));
        assert!(
            !f.blocks[0]
                .insts
                .iter()
                .any(|i| matches!(i.kind, InstKind::FpToSi(_)))
        );
    }

    #[test]
    fn a_guarded_result_is_not_hoisted_as_a_divisor() {
        // entry: `%3 = frem %0, 7` guarded first; the loop then reads
        // `%2 = frem %1, %3`, whose divisor now has a def per arm
        let mut f = counter_loop();
        f.blocks[0].insts.push(inst(
            3,
            InstKind::BinOp(BinOp::FRem, v(0), Operand::ConstF64(7.0)),
        ));
        f.blocks[2].insts[0] = inst(2, InstKind::BinOp(BinOp::FRem, v(1), v(3)));
        assert_eq!(guard(&mut f), (2, 0));
        // the divisor is truncated where the loop reads it, not in an arm
        let truncs: Vec<BlockId> = f
            .blocks
            .iter()
            .filter(|b| {
                b.insts
                    .iter()
                    .any(|i| matches!(i.kind, InstKind::FpToSi(Operand::Value(ValueId(3)))))
            })
            .map(|b| b.id)
            .collect();
        assert_eq!(truncs.len(), 1);
        let loop_head = f.blocks.iter().find(|b| {
            matches!(b.term, Terminator::CondBr { .. })
                && b.insts
                    .iter()
                    .any(|i| matches!(i.kind, InstKind::FpToSi(Operand::Value(ValueId(3)))))
        });
        assert!(loop_head.is_some());
        assert_eq!(defs_of(&f, ValueId(3)).len(), 2);
    }

    #[test]
    fn two_sites_in_one_block_both_split() {
        let mut f = frem_fn(Operand::ConstF64(6.0));
        f.values.push(ValueInfo {
            ty: Type::F64,
            name: None,
        });
        f.blocks[0].insts.push(inst(
            4,
            InstKind::BinOp(BinOp::FRem, v(3), Operand::ConstF64(1000.0)),
        ));
        f.blocks[1].term = Terminator::Ret(Some(v(4)));
        assert_eq!(guard(&mut f), (2, 0));
        assert_eq!(f.blocks.len(), 8);
        for (i, b) in f.blocks.iter().enumerate() {
            assert_eq!(b.id.0 as usize, i);
        }
        assert_eq!(defs_of(&f, ValueId(2)).len(), 2);
        assert_eq!(defs_of(&f, ValueId(4)).len(), 2);
        // both slow arms sit at the end, the ret right before them
        assert!(matches!(
            f.blocks[6].insts[0].kind,
            InstKind::BinOp(BinOp::FRem, ..)
        ));
        assert!(matches!(
            f.blocks[7].insts[0].kind,
            InstKind::BinOp(BinOp::FRem, ..)
        ));
        assert!(matches!(f.blocks[5].term, Terminator::Ret(_)));
    }
}
