//! Range test for a short-circuit of two compares. The diamond of
//! `x < lo || x > hi` merges the first compare on one arm and the
//! second on the other; instead of a select of the two, the join
//! value becomes one unsigned test on `x - lo`: subtraction wraps, so
//! `x - lo <u hi - lo + 1` holds exactly when `x` lies in `[lo, hi]`,
//! for every i64 `x`. `||` tests the complement, `&&` the range. Two
//! compares and a select (`cmp; cset; cmp; cset; cmp; csel`) become a
//! subtract and a compare.

use std::collections::HashSet;

use torajs_core::ssa::{
    BinOp, Function, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId, ValueInfo,
    visit_value_operands,
};

use super::ArmPlan;

/// What replaces the select: the header appends `insts` (the last
/// defines the join value) and the compares in `dead` are deleted —
/// the first from the header, the second from the arm's hoist list.
pub(super) struct Fold {
    pub insts: Vec<Inst>,
    pub dead: [ValueId; 2],
}

pub(super) fn fold(
    func: &mut Function,
    h: usize,
    cond: &Operand,
    tp: &ArmPlan,
    ep: &ArmPlan,
) -> Option<Fold> {
    let (vid, x, lo, width, or, c, o) = plan(func, h, cond, tp, ep)?;
    let mut insts = Vec::new();
    let subject = if lo == 0 {
        Operand::Value(x)
    } else {
        let shifted = ValueId(func.values.len() as u32);
        func.values.push(ValueInfo {
            ty: Type::I64,
            name: None,
        });
        insts.push(Inst {
            result: Some(shifted),
            kind: InstKind::BinOp(BinOp::Sub, Operand::Value(x), Operand::ConstI64(lo)),
            origin: None,
        });
        Operand::Value(shifted)
    };
    let pred = if or { IPred::Uge } else { IPred::Ult };
    insts.push(Inst {
        result: Some(vid),
        kind: InstKind::ICmp(pred, subject, Operand::ConstI64(width)),
        origin: None,
    });
    Some(Fold {
        insts,
        dead: [c, o],
    })
}

/// The fold's shape: join value, subject, range start, range width,
/// `||` (complement) or `&&`, and the two compares it retires.
fn plan(
    func: &Function,
    h: usize,
    cond: &Operand,
    tp: &ArmPlan,
    ep: &ArmPlan,
) -> Option<(ValueId, ValueId, i64, i64, bool, ValueId, ValueId)> {
    let (&[(vid, Type::Bool, t_src)], &[(_, _, e_src)]) = (&tp.copies[..], &ep.copies[..]) else {
        return None;
    };
    let Operand::Value(c) = *cond else {
        return None;
    };
    // `select c, c, o` is `c || o`; `select c, o, c` is `c && o`
    let (or, other) = if t_src == *cond {
        (true, e_src)
    } else if e_src == *cond {
        (false, t_src)
    } else {
        return None;
    };
    let Operand::Value(o) = other else {
        return None;
    };
    let header = &func.blocks[h].insts;
    let c_kind = &header.iter().find(|i| i.result == Some(c))?.kind;
    let o_kind = &tp
        .hoisted
        .iter()
        .chain(&ep.hoisted)
        .find(|i| i.result == Some(o))?
        .kind;
    let (x, c_set) = compare_set(c_kind)?;
    let (ox, o_set) = compare_set(o_kind)?;
    if x != ox || func.values[x.0 as usize].ty != Type::I64 {
        return None;
    }
    // both compares read the same `x`: it is not rewritten in the
    // header or on the arms (an arm rewriting it would have failed
    // the arm plan), and the compares have no other reader
    if header.iter().any(|i| i.result == Some(x)) {
        return None;
    }
    let uses = use_counts(func, &[c, o]);
    if uses != [2, 1] {
        return None;
    }
    let (lo, hi) = if or {
        // the complement of `(-inf, a] ∪ [b, inf)` is `[a + 1, b - 1]`
        let (a, b) = match (c_set, o_set) {
            (Set::Below(a), Set::Above(b)) | (Set::Above(b), Set::Below(a)) => (a, b),
            _ => return None,
        };
        (a + 1, b - 1)
    } else {
        match (c_set, o_set) {
            (Set::Below(a), Set::Above(b)) | (Set::Above(b), Set::Below(a)) => (b, a),
            _ => return None,
        }
    };
    if lo > hi {
        return None;
    }
    let width = hi - lo + 1;
    if width > i64::MAX as i128 {
        return None;
    }
    Some((vid, x, lo as i64, width as i64, or, c, o))
}

/// The values a signed compare against a constant accepts: a ray
/// closed at the constant.
#[derive(Clone, Copy)]
enum Set {
    /// `(-inf, k]`
    Below(i128),
    /// `[k, inf)`
    Above(i128),
}

fn compare_set(kind: &InstKind) -> Option<(ValueId, Set)> {
    let InstKind::ICmp(pred, a, b) = kind else {
        return None;
    };
    let (x, k, pred) = match (a, b) {
        (Operand::Value(x), Operand::ConstI64(k)) => (*x, *k as i128, *pred),
        (Operand::ConstI64(k), Operand::Value(x)) => (*x, *k as i128, mirror(*pred)?),
        _ => return None,
    };
    let set = match pred {
        IPred::Slt => Set::Below(k - 1),
        IPred::Sle => Set::Below(k),
        IPred::Sgt => Set::Above(k + 1),
        IPred::Sge => Set::Above(k),
        _ => return None,
    };
    Some((x, set))
}

/// `k <pred> x` as `x <pred'> k`.
fn mirror(pred: IPred) -> Option<IPred> {
    match pred {
        IPred::Slt => Some(IPred::Sgt),
        IPred::Sle => Some(IPred::Sge),
        IPred::Sgt => Some(IPred::Slt),
        IPred::Sge => Some(IPred::Sle),
        _ => None,
    }
}

/// How many operand slots read each of `of`, terminators included.
fn use_counts<const N: usize>(func: &Function, of: &[ValueId; N]) -> [u32; N] {
    let watched: HashSet<ValueId> = of.iter().copied().collect();
    let mut counts = [0u32; N];
    let mut bump = |v: ValueId| {
        if watched.contains(&v) {
            let i = of.iter().position(|w| *w == v).expect("watched");
            counts[i] += 1;
        }
    };
    for block in &func.blocks {
        for inst in &block.insts {
            visit_value_operands(&inst.kind, &mut bump);
        }
        match &block.term {
            Terminator::CondBr {
                cond: Operand::Value(v),
                ..
            }
            | Terminator::Ret(Some(Operand::Value(v))) => bump(*v),
            _ => {}
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::super::{SelectFormStats, form_once};
    use super::*;
    use torajs_core::ssa::{Block, BlockId};

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    fn c(n: i64) -> Operand {
        Operand::ConstI64(n)
    }

    fn inst(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }

    fn block(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }

    fn cond_br(cond: u32, then_blk: u32, else_blk: u32) -> Terminator {
        Terminator::CondBr {
            cond: v(cond),
            then_blk: BlockId(then_blk),
            else_blk: BlockId(else_blk),
        }
    }

    /// `%0 <first> || / && %0 <second>` merged into cell %3, branched
    /// on in bb3. `or`: bb1 (first true) copies the first compare,
    /// bb2 evaluates the second.
    fn shape(first: InstKind, second: InstKind, or: bool) -> Function {
        let short = block(
            1,
            vec![inst(3, InstKind::Copy(Type::Bool, v(1)))],
            Terminator::Br(BlockId(3)),
        );
        let eval = block(
            2,
            vec![inst(2, second), inst(3, InstKind::Copy(Type::Bool, v(2)))],
            Terminator::Br(BlockId(3)),
        );
        let term = if or {
            cond_br(1, 1, 2)
        } else {
            cond_br(1, 2, 1)
        };
        Function {
            name: "f".into(),
            params: vec![ValueId(0)],
            ret: Type::Void,
            blocks: vec![
                block(0, vec![inst(1, first)], term),
                short,
                eval,
                block(3, vec![], cond_br(3, 4, 5)),
                block(4, vec![], Terminator::Ret(None)),
                block(5, vec![], Terminator::Ret(None)),
            ],
            values: [Type::I64, Type::Bool, Type::Bool, Type::Bool]
                .into_iter()
                .map(|ty| ValueInfo { ty, name: None })
                .collect(),
            current_origin: None,
        }
    }

    fn run(f: &mut Function) -> SelectFormStats {
        let mut stats = SelectFormStats::default();
        while form_once(f, &mut stats) {}
        stats
    }

    #[test]
    fn an_or_of_two_rays_tests_the_complement_of_their_gap() {
        let mut f = shape(
            InstKind::ICmp(IPred::Slt, v(0), c(48)),
            InstKind::ICmp(IPred::Sgt, v(0), c(57)),
            true,
        );
        let stats = run(&mut f);
        assert_eq!(stats.range_tests_formed, 1);
        assert_eq!(stats.selects_formed, 0);
        let [sub, test] = &f.blocks[0].insts[..] else {
            panic!(
                "header holds the subtract and the test: {:?}",
                f.blocks[0].insts
            );
        };
        assert!(matches!(
            sub.kind,
            InstKind::BinOp(
                BinOp::Sub,
                Operand::Value(ValueId(0)),
                Operand::ConstI64(48)
            )
        ));
        assert_eq!(test.result, Some(ValueId(3)));
        assert!(
            matches!(test.kind, InstKind::ICmp(IPred::Uge, Operand::Value(s), Operand::ConstI64(10)) if Some(s) == sub.result)
        );
        assert!(matches!(f.blocks[0].term, Terminator::Br(BlockId(3))));
        assert!(f.blocks[1].insts.is_empty() && f.blocks[2].insts.is_empty());
    }

    #[test]
    fn an_and_of_two_rays_tests_their_overlap() {
        let mut f = shape(
            InstKind::ICmp(IPred::Sge, v(0), c(0)),
            InstKind::ICmp(IPred::Sle, v(0), c(9)),
            false,
        );
        let stats = run(&mut f);
        assert_eq!(stats.range_tests_formed, 1);
        // a range starting at zero needs no subtract
        let [test] = &f.blocks[0].insts[..] else {
            panic!("header holds only the test: {:?}", f.blocks[0].insts);
        };
        assert!(matches!(
            test.kind,
            InstKind::ICmp(
                IPred::Ult,
                Operand::Value(ValueId(0)),
                Operand::ConstI64(10)
            )
        ));
    }

    #[test]
    fn rays_that_do_not_bound_a_range_keep_the_select() {
        // x < 48 || x > 20: the rays overlap, every x passes
        let mut f = shape(
            InstKind::ICmp(IPred::Slt, v(0), c(48)),
            InstKind::ICmp(IPred::Sgt, v(0), c(20)),
            true,
        );
        let stats = run(&mut f);
        assert_eq!(stats.range_tests_formed, 0);
        assert_eq!(stats.selects_formed, 1);
    }

    #[test]
    fn a_compare_with_another_reader_keeps_the_select() {
        let mut f = shape(
            InstKind::ICmp(IPred::Slt, v(0), c(48)),
            InstKind::ICmp(IPred::Sgt, v(0), c(57)),
            true,
        );
        f.blocks[4].term = Terminator::Ret(Some(v(1)));
        f.ret = Type::Bool;
        let stats = run(&mut f);
        assert_eq!(stats.range_tests_formed, 0);
        assert_eq!(stats.selects_formed, 1);
    }
}
