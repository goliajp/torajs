//! A trial-division loop searched `LANES` divisors a step, the
//! original loop kept for the steps past the bound.
//!
//! ```text
//!                                     P:  e = i + MC; br H
//!                                     L:  i = i + C; cond_br i == e, S, H
//!                                     S:  xv = splat (sitofp n); yv = lanes i .. i + (K-1)C
//!                                         cond_br <test at i + (K-1)C>, F, H
//! H:  c = <test>; cond_br c, B, X     F:  m = fcmeq (frintz (xv / yv)), (xv / yv)
//! B:  ... cond_br (t * i == n), XF, L     cond_br (reduce.add m != 0), FH, FL
//!                                     FH: i = i + <first set lane> * C; br XF
//! L:  i = i + C; br H                 FL: i = i + KC; yv = yv + splat KC
//!                                         cond_br <test at i + (K-1)C>, F, H
//!                                     H, B as they were
//! ```
//!
//! A step of the original loop that neither leaves on its header test
//! nor on its divisibility test only adds `C` to `i`: the body is pure
//! and `i` is the one cell it writes. A group of `K` such steps is
//! then `i += KC`, taken when the header test holds at the group's
//! last divisor and no lane divides. The header test holds on a prefix
//! of the counter's values (`i * i <= b` or `i <= b`, `i >= 0`, the
//! product exact), so holding at the last divisor it holds at every
//! one before. A lane divides exactly when its quotient is integral:
//! `n ∈ [0, 2^53)` and `d ≥ 1` make the float quotient of a
//! non-multiple fall strictly between two integers (the srem_fquot
//! argument). A group with a dividing lane is where the original loop
//! leaves through its divisibility exit, at the first such lane, so
//! the search leaves there with that counter. A group whose last
//! divisor fails the header test is left to the original loop from
//! the group's first divisor: it reaches the same exit, with the same
//! `i`, within `K` steps. Resolving a found divisor in the search
//! itself matters: replaying it would put a second data-dependent exit,
//! waiting on a scalar divide, behind the vector test.
//!
//! The original loop takes the first `M` steps, where most entries find
//! their divisor, and its latch enters the search once, when the
//! counter has reached `i + MC`; the counter only grows, so a group
//! handed back to the original loop does not enter it again.
//!
//! The divisor lanes are a float counter: the counter's interval fact
//! keeps every lane, one group past the last value the loop gives it,
//! below 2^53 (and below `sqrt(2^63)` when the test squares it).

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::loop_analysis::NaturalLoop;
use torajs_core::ssa::{
    BinOp, BlockId, Function, IPred, InstKind, Operand, Terminator, Type, ValueId,
};

use super::plan::{self, Uses, blk};
use super::{F64_EXACT, Plan};
use crate::interval::NumFact;

/// Divisors a group tests; a multiple of two (one vector per two).
pub(super) const LANES: i64 = 4;

/// Steps the original loop takes before the search: most entries find
/// a small divisor in them, where the search's setup and the vector
/// test's latency cost more than the scalar steps.
pub(super) const PROLOGUE: i64 = 4;

/// `floor(sqrt(i64::MAX))`: a counter up to it squares exactly.
const ROOT_MAX: i128 = 3_037_000_499;

mod build;

pub(super) use build::apply;

pub(super) struct SearchPlan {
    pub header: BlockId,
    preheader: BlockId,
    latch: BlockId,
    iv: ValueId,
    step: i64,
    /// `sitofp n`, defined in the preheader.
    dividend: Operand,
    test: HeaderTest,
    /// Where the divisibility test leaves the loop.
    found: BlockId,
}

/// The header test `f(i) <pred> bound` (or `bound <pred> f(i)`), with
/// `f(i)` either `i` or `i * i`.
struct HeaderTest {
    square: bool,
    pred: IPred,
    iv_lhs: bool,
    bound: Operand,
    /// The loop goes on when the compare is true.
    stay_then: bool,
}

pub(super) fn find_plan(
    func: &Function,
    known: Option<&HashMap<ValueId, NumFact>>,
    done: &HashSet<BlockId>,
) -> Option<SearchPlan> {
    let known = known?;
    plan::find_plan_where(func, done, |p, l, uses| take(func, known, &p, l, uses))
}

fn take(
    func: &Function,
    known: &HashMap<ValueId, NumFact>,
    p: &Plan,
    l: &NaturalLoop,
    uses: &Uses,
) -> Option<SearchPlan> {
    let f = known.get(&p.iv)?;
    let reach = f.hi + i128::from(LANES * p.step);
    if !p.inv_is_lhs || f.lo < 1 || reach > F64_EXACT {
        return None;
    }
    let test = header_test(func, p, l)?;
    if test.square && reach > ROOT_MAX {
        return None;
    }
    let n = dividend_source(func, uses, p, l)?;
    let found = divisibility_exit(func, uses, p, l, n)?;
    if !known.get(&n).is_some_and(|f| f.lo >= 0 && f.hi < F64_EXACT) {
        return None;
    }
    // a skipped step may write no cell but the counter
    let writes_other = p.order().iter().any(|b| {
        blk(func, *b)
            .insts
            .iter()
            .any(|i| i.result.is_some_and(|r| r != p.iv && p.cells.contains(&r)))
    });
    (!writes_other).then_some(SearchPlan {
        header: p.header,
        preheader: p.preheader,
        latch: *p.chain.last()?,
        iv: p.iv,
        step: p.step,
        dividend: p.inv,
        test,
        found,
    })
}

/// The header's whole body: `c = icmp f(i), bound` after `f(i) = i * i`
/// or with `f(i) = i`, going on for the counter's smaller values.
fn header_test(func: &Function, p: &Plan, l: &NaturalLoop) -> Option<HeaderTest> {
    let hb = blk(func, p.header);
    let Terminator::CondBr {
        cond: Operand::Value(c),
        then_blk,
        ..
    } = hb.term
    else {
        return None;
    };
    let (square, cmp) = match hb.insts.as_slice() {
        [cmp] => (None, cmp),
        [sq, cmp] => (Some(sq), cmp),
        _ => return None,
    };
    let f = match square {
        None => p.iv,
        Some(sq) => match sq.kind {
            InstKind::BinOp(BinOp::Mul, Operand::Value(a), Operand::Value(b))
                if a == p.iv && b == p.iv =>
            {
                sq.result?
            }
            _ => return None,
        },
    };
    let InstKind::ICmp(pred, x, y) = cmp.kind else {
        return None;
    };
    if cmp.result != Some(c) {
        return None;
    }
    let (iv_lhs, bound) = match (x, y) {
        (Operand::Value(a), b) if a == f => (true, b),
        (a, Operand::Value(b)) if b == f => (false, a),
        _ => return None,
    };
    let invariant = match bound {
        Operand::Value(b) => !(0..func.blocks.len()).any(|bi| {
            l.contains(&BlockId(bi as u32))
                && func.blocks[bi].insts.iter().any(|i| i.result == Some(b))
        }),
        Operand::ConstI64(_) => true,
        _ => false,
    };
    let stay_then = l.contains(&then_blk);
    // the loop goes on while f(i) is small: `f < b` read on the stay
    // edge, or `f > b` read on the leave edge
    let below = matches!(pred, IPred::Slt | IPred::Sle);
    let above = matches!(pred, IPred::Sgt | IPred::Sge);
    let prefix = if iv_lhs == stay_then { below } else { above };
    (invariant && prefix).then_some(HeaderTest {
        square: square.is_some(),
        pred,
        iv_lhs,
        bound,
        stay_then,
    })
}

/// `n` for the dividend `sitofp n`: converted in the preheader, and
/// written neither after that there nor in the loop.
fn dividend_source(func: &Function, uses: &Uses, p: &Plan, l: &NaturalLoop) -> Option<ValueId> {
    let Operand::Value(a) = p.inv else {
        return None;
    };
    let (ab, at) = uses.single_def(a)?;
    let InstKind::SiToFp(Operand::Value(n)) = blk(func, ab).insts[at].kind else {
        return None;
    };
    let rewritten = uses.defs.get(&n).is_some_and(|ds| {
        ds.iter()
            .any(|(b, i)| l.contains(b) || (*b == ab && *i > at))
    });
    (ab == p.preheader && func.value_type(n) == Type::I64 && !rewritten).then_some(n)
}

/// The body leaves the loop at one branch only, taken when `t * i == n`
/// for the quotient's integer `t`; answers where it goes.
fn divisibility_exit(
    func: &Function,
    uses: &Uses,
    p: &Plan,
    l: &NaturalLoop,
    n: ValueId,
) -> Option<BlockId> {
    let exits: Vec<BlockId> = p
        .chain
        .iter()
        .copied()
        .filter(|b| {
            plan::successors(&blk(func, *b).term)
                .iter()
                .any(|s| !l.contains(s))
        })
        .collect();
    let [b] = exits[..] else {
        return None;
    };
    let Terminator::CondBr {
        cond: Operand::Value(c),
        then_blk,
        else_blk,
    } = blk(func, b).term
    else {
        return None;
    };
    let def = |v: ValueId| {
        uses.single_def(v)
            .map(|(db, i)| &blk(func, db).insts[i].kind)
    };
    let Some(InstKind::ICmp(pred, x, y)) = def(c) else {
        return None;
    };
    let product = match (x, y) {
        (Operand::Value(m), Operand::Value(o)) | (Operand::Value(o), Operand::Value(m))
            if *o == n && *m != n =>
        {
            *m
        }
        _ => return None,
    };
    let is_product = matches!(def(product),
        Some(InstKind::BinOp(BinOp::Mul, Operand::Value(a), Operand::Value(b)))
            if (*a, *b) == (p.t, p.iv) || (*a, *b) == (p.iv, p.t));
    match (is_product, pred, l.contains(&then_blk)) {
        (true, IPred::Eq, false) => Some(then_blk),
        (true, IPred::Ne, true) => Some(else_blk),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
