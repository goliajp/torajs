//! Float quotient for a divisibility exit test in a loop: `srem x, y
//! == 0` with `x ∈ [0, 2^53)` and `y ≥ 1` proven by the interval facts
//! becomes `trunc(fdiv(x, y)) * y == x`. The M-series f64 divider
//! issues twice as often as the 64-bit integer one.
//!
//! Exactness, `y ≤ 2^53`: both operands convert exactly; an integral
//! `x / y` divides exactly; otherwise the quotient `k + r/y` (`1 ≤ r <
//! y`) is at least `1/y` below `k + 1` and the fdiv errs by less than
//! `2^-53 · x/y < 1/y`, so it truncates to `k`. `y > 2^53` converts to
//! at least `2^53 > x`, the fdiv stays below `1 - 2^-53` and truncates
//! to 0. A zero divisor is outside the window (JS gives NaN).
//!
//! The float path has the longer latency, so the zero test must be a
//! loop exit, taken once per entry and well predicted. A data reader
//! (`arr[i % pool]`) or a branch inside the loop (`if (n % d === 0) s
//! += d`, mispredicted on the divisibility pattern) pays the latency
//! and measured slower. With the compare as the only reader, the gcd
//! recurrence `b = a % b` (a copy) stays out too. Once select_form has
//! turned that branch into a select, `select` takes the test after it.
//!
//! The rewritten remainder's id names the product `q * y`, so its
//! interval fact is dropped. Constant divisors are left alone, so the
//! order against srem_parity does not matter.
//! `TORAJS_SREM_FQUOT_OFF=1` skips, `TORAJS_SREM_FQUOT_STATS=1` counts.

mod hoist;
mod select;

pub use select::float_quotient_select_srems;

use crate::id_hash::HashMap;

use torajs_core::ssa::{
    BinOp, BlockId, Function, IPred, Inst, InstKind, Module, Operand, Terminator, Type, ValueId,
    visit_value_operands,
};

use crate::fact_folds::LoopShapes;
use crate::float_demote::push_value;
use crate::interval::NumFact;
use crate::interval::transfer::op_const_int;

const F64_EXACT: i128 = 1 << 53;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SremFquotStats {
    /// zero-compared remainders rewritten to `q * y == x`
    pub compared: u32,
}

/// Who may read the zero compare of a rewritten remainder.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Readers {
    /// only the branch of its block, leaving the loop
    ExitBranch,
    /// only selects deciding on it, off any recurrence into the
    /// remainder's operands (`select::selects_only`)
    Selects,
}

pub(crate) fn float_quotient_srems(
    module: &mut Module,
    facts: &mut [HashMap<ValueId, NumFact>],
    shapes: &mut LoopShapes,
) -> SremFquotStats {
    let mut stats = SremFquotStats::default();
    for (fi, (func, fm)) in module.funcs.iter_mut().zip(facts).enumerate() {
        if !func.is_declaration() {
            for r in rewrite_function(func, fm, &mut stats, Readers::ExitBranch, shapes, fi) {
                fm.remove(&r);
            }
        }
    }
    stats
}

/// `[lo, hi)` holds every value `op` may take.
fn within(op: &Operand, facts: &HashMap<ValueId, NumFact>, lo: i128, hi: i128) -> bool {
    let fits = |a: i128, b: i128| a >= lo && b < hi;
    match op {
        Operand::Value(v) => facts.get(v).is_some_and(|f| fits(f.lo, f.hi)),
        c => op_const_int(c).is_some_and(|c| fits(c, c)),
    }
}

/// Where each value is read (a terminator at index `insts.len()`) and
/// how many times it is defined, built once per function.
#[derive(Default)]
struct Uses {
    reads: HashMap<ValueId, Vec<(usize, usize)>>,
    defs: HashMap<ValueId, u32>,
}

impl Uses {
    fn of(func: &Function) -> Self {
        let mut uses = Uses::default();
        for (b, block) in func.blocks.iter().enumerate() {
            let mut read = |v, i| uses.reads.entry(v).or_default().push((b, i));
            for (i, inst) in block.insts.iter().enumerate() {
                visit_value_operands(&inst.kind, |v| read(v, i));
            }
            if let Terminator::Ret(Some(Operand::Value(v))) = &block.term {
                read(*v, block.insts.len());
            }
            if let Terminator::CondBr {
                cond: Operand::Value(v),
                ..
            } = &block.term
            {
                read(*v, block.insts.len());
            }
            for r in block.insts.iter().filter_map(|i| i.result) {
                *uses.defs.entry(r).or_default() += 1;
            }
        }
        uses
    }

    fn one_def(&self, v: ValueId) -> bool {
        self.defs.get(&v) == Some(&1)
    }
}

/// `r`'s zero compares are each read only by the block's branch, and
/// that branch leaves the loop.
fn exit_tests_only(func: &Function, uses: &Uses, s: &Site, body: &[BlockId]) -> bool {
    let insts = &func.blocks[s.bi].insts;
    let leaves = match &func.blocks[s.bi].term {
        Terminator::CondBr {
            cond: Operand::Value(c),
            then_blk,
            else_blk,
        } if !body.contains(then_blk) || !body.contains(else_blk) => Some(*c),
        _ => None,
    };
    // the compare's one reader is the branch (read at `insts.len()`)
    let exit_branch_only = |i: usize| {
        let c = insts[i].result;
        c.is_some() && c == leaves && uses.reads[&c.unwrap()] == [(s.bi, insts.len())]
    };
    zero_tests_only(func, uses, s, exit_branch_only)
}

/// Every use of `r` is an eq/ne compare against 0 later in block `bi`,
/// with no def of `x` between, that `reader_ok` accepts (given its
/// index); `r` has one def.
fn zero_tests_only(
    func: &Function,
    uses: &Uses,
    s: &Site,
    reader_ok: impl Fn(usize) -> bool,
) -> bool {
    let insts = &func.blocks[s.bi].insts;
    let xv = match s.x {
        Operand::Value(v) => Some(v),
        _ => None,
    };
    let x_kept = |i: usize| {
        !insts[s.at + 1..i]
            .iter()
            .any(|j| xv.is_some() && j.result == xv)
    };
    let zero_cmp = |i: usize| {
        x_kept(i)
            && matches!(&insts[i].kind, InstKind::ICmp(IPred::Eq | IPred::Ne, Operand::Value(a), c)
                if *a == s.r && op_const_int(c) == Some(0))
    };
    let reads = uses.reads.get(&s.r).map_or(&[][..], Vec::as_slice);
    uses.one_def(s.r)
        && reads
            .iter()
            .all(|&(b, i)| b == s.bi && i > s.at && i < insts.len() && zero_cmp(i) && reader_ok(i))
}

struct Site {
    bi: usize,
    at: usize,
    r: ValueId,
    x: Operand,
    y: Operand,
    /// the innermost loop holding the site
    lp: usize,
}

impl Readers {
    /// Whether the zero compare of the remainder at `site` has only the
    /// readers this kind admits.
    fn admit(self, func: &Function, uses: &Uses, site: &Site, body: &[BlockId]) -> bool {
        match self {
            Readers::ExitBranch => exit_tests_only(func, uses, site, body),
            Readers::Selects => select::selects_only(func, uses, site, body),
        }
    }
}

/// The zero tests of the rewritten remainder `s.r`, which now names the
/// product `q * y`, compare it with the dividend instead.
fn compare_with_dividend(insts: &mut [Inst], s: &Site) {
    for i in insts {
        if let InstKind::ICmp(_, Operand::Value(a), c) = &mut i.kind
            && *a == s.r
        {
            *c = s.x;
        }
    }
}

/// Rewrites the qualifying sites of `func`; answers the remainders
/// whose ids now name the product.
fn rewrite_function(
    func: &mut Function,
    facts: &HashMap<ValueId, NumFact>,
    stats: &mut SremFquotStats,
    readers: Readers,
    shapes: &mut LoopShapes,
    fi: usize,
) -> Vec<ValueId> {
    // the site test below without its loop and reader parts: most
    // functions have no such remainder and skip the analyses
    let candidate = func.blocks.iter().flat_map(|b| &b.insts).any(|i| {
        matches!(&i.kind, InstKind::BinOp(BinOp::SRem, x, y @ Operand::Value(_))
            if within(x, facts, 0, F64_EXACT) && within(y, facts, 1, i128::MAX))
    });
    if !candidate {
        return Vec::new();
    }
    let uses = Uses::of(func);
    let (_, la) = shapes.of(fi, func);
    let innermost = |b: BlockId| {
        (0..la.loops().len())
            .filter(|&l| la.loops()[l].body.contains(&b))
            .min_by_key(|&l| la.loops()[l].body.len())
    };
    let mut sites = Vec::new();
    for (bi, block) in func.blocks.iter().enumerate() {
        for (at, inst) in block.insts.iter().enumerate() {
            let (Some(r), InstKind::BinOp(BinOp::SRem, x, y)) = (inst.result, &inst.kind) else {
                continue;
            };
            // a constant divisor keeps the integer division
            if !matches!(y, Operand::Value(_))
                || !within(x, facts, 0, F64_EXACT)
                || !within(y, facts, 1, i128::MAX)
            {
                continue;
            }
            let Some(lp) = innermost(BlockId(bi as u32)) else {
                continue;
            };
            let site = Site {
                bi,
                at,
                r,
                x: *x,
                y: *y,
                lp,
            };
            if readers.admit(func, &uses, &site, &la.loops()[lp].body) {
                sites.push(site);
            }
        }
    }
    let rewritten = sites.iter().map(|s| s.r).collect();
    let mut converted: HashMap<(ValueId, BlockId), ValueId> = HashMap::default();
    // last site first, so earlier positions in a block stay valid
    for s in sites.into_iter().rev() {
        let mut pre_insts: Vec<Inst> = Vec::new();
        let mut to_f64 = |func: &mut Function, op: Operand, pre: &mut Vec<Inst>| -> Operand {
            if let Operand::Value(v) = op
                && let v = hoist::copy_source(func, &uses, s.bi, s.at, v)
                && let Some(p) = hoist::invariant_preheader(func, &la.loops()[s.lp], v)
            {
                if let Some(f) = converted.get(&(v, p)) {
                    return Operand::Value(*f);
                }
                let f = push_value(&mut func.values, Type::F64);
                func.blocks[p.0 as usize]
                    .insts
                    .push(inst(f, InstKind::SiToFp(Operand::Value(v))));
                converted.insert((v, p), f);
                return Operand::Value(f);
            }
            let f = push_value(&mut func.values, Type::F64);
            pre.push(inst(f, InstKind::SiToFp(op)));
            Operand::Value(f)
        };
        let xf = to_f64(func, s.x, &mut pre_insts);
        let yf = to_f64(func, s.y, &mut pre_insts);
        let qf = push_value(&mut func.values, Type::F64);
        pre_insts.push(inst(qf, InstKind::BinOp(BinOp::FDiv, xf, yf)));
        let q = push_value(&mut func.values, Type::I64);
        pre_insts.push(inst(q, InstKind::FpToSi(Operand::Value(qf))));
        let block = &mut func.blocks[s.bi];
        block.insts[s.at].kind = InstKind::BinOp(BinOp::Mul, Operand::Value(q), s.y);
        compare_with_dividend(&mut block.insts[s.at + 1..], &s);
        block.insts.splice(s.at..s.at, pre_insts);
        stats.compared += 1;
    }
    rewritten
}

fn inst(r: ValueId, kind: InstKind) -> Inst {
    Inst {
        result: Some(r),
        kind,
        origin: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{Block, ValueInfo};

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    fn i(r: u32, kind: InstKind) -> Inst {
        inst(ValueId(r), kind)
    }

    fn block(id: u32, insts: Vec<Inst>, term: Terminator) -> Block {
        Block {
            id: BlockId(id),
            insts,
            term,
        }
    }

    fn cond(c: u32, then_blk: u32, else_blk: u32) -> Terminator {
        Terminator::CondBr {
            cond: v(c),
            then_blk: BlockId(then_blk),
            else_blk: BlockId(else_blk),
        }
    }

    /// `for (i = 2; i <= n; ...) { <body> }` with `n = %0`, `i = %1`,
    /// the remainder `%3 = srem %0, y` at the top of `bb2`.
    fn trial_loop(y: Operand, body: Vec<Inst>, latch: Vec<Inst>) -> Function {
        let mut insts = vec![i(3, InstKind::BinOp(BinOp::SRem, v(0), y))];
        insts.extend(body);
        let start = i(1, InstKind::Copy(Type::I64, Operand::ConstI64(2)));
        let test = i(2, InstKind::ICmp(IPred::Sle, v(1), v(0)));
        Function {
            name: "f".into(),
            params: vec![ValueId(0)],
            ret: Type::I64,
            blocks: vec![
                block(0, vec![start], Terminator::Br(BlockId(1))),
                block(1, vec![test], cond(2, 2, 4)),
                block(2, insts, cond(4, 4, 3)),
                block(3, latch, Terminator::Br(BlockId(1))),
                block(4, vec![], Terminator::Ret(Some(v(1)))),
            ],
            values: (0..7)
                .map(|_| ValueInfo {
                    ty: Type::I64,
                    name: None,
                })
                .collect(),
            current_origin: None,
        }
    }

    fn step() -> Vec<Inst> {
        vec![
            i(5, InstKind::BinOp(BinOp::Add, v(1), Operand::ConstI64(1))),
            i(1, InstKind::Copy(Type::I64, v(5))),
        ]
    }

    fn zero_test() -> Vec<Inst> {
        vec![i(4, InstKind::ICmp(IPred::Eq, v(3), Operand::ConstI64(0)))]
    }

    fn run(f: &mut Function, n_hi: i128, i_lo: i128) -> SremFquotStats {
        run_for(f, n_hi, i_lo, Readers::ExitBranch)
    }

    fn run_for(f: &mut Function, n_hi: i128, i_lo: i128, readers: Readers) -> SremFquotStats {
        let facts = HashMap::from_iter([
            (ValueId(0), NumFact::new(0, n_hi)),
            (ValueId(1), NumFact::new(i_lo, 1 << 70)),
        ]);
        let mut stats = SremFquotStats::default();
        rewrite_function(f, &facts, &mut stats, readers, &mut LoopShapes::new(1), 0);
        stats
    }

    /// The trial loop with its zero test read by `%6 = select %4, %1,
    /// %0` instead of the branch, and `latch` closing the iteration.
    fn select_loop(latch: Vec<Inst>) -> Function {
        let mut body = zero_test();
        body.push(i(6, InstKind::Select(Type::I64, v(4), v(1), v(0))));
        let mut f = trial_loop(v(1), body, latch);
        f.blocks[2].term = Terminator::Br(BlockId(3));
        f
    }

    #[test]
    fn a_test_only_selects_read_takes_the_float_quotient() {
        let mut f = select_loop(step());
        assert_eq!(run_for(&mut f, 1000, 2, Readers::Selects).compared, 1);
        assert!(matches!(
            f.blocks[2].insts[1].kind,
            InstKind::BinOp(BinOp::FDiv, _, _)
        ));
        // the exit-test rule leaves it to the select rule
        let mut f = select_loop(step());
        assert_eq!(run(&mut f, 1000, 2), NONE);
    }

    #[test]
    fn a_select_feeding_the_divisor_keeps_srem() {
        // `i = select(...)`: the quotient's latency would sit on the
        // recurrence through the divisor
        let mut latch = step();
        latch[1] = i(1, InstKind::Copy(Type::I64, v(6)));
        let mut f = select_loop(latch);
        assert_eq!(run_for(&mut f, 1000, 2, Readers::Selects), NONE);
    }

    const NONE: SremFquotStats = SremFquotStats { compared: 0 };

    #[test]
    fn zero_compare_becomes_product_compare_with_hoisted_dividend() {
        let mut f = trial_loop(v(1), zero_test(), step());
        assert_eq!(run(&mut f, 1000, 2).compared, 1);
        // the dividend converts once, before the loop
        assert_eq!(f.blocks[0].insts[1].kind, InstKind::SiToFp(v(0)));
        let xf = Operand::Value(f.blocks[0].insts[1].result.unwrap());
        let body = &f.blocks[2].insts;
        let res = |k: usize| Operand::Value(body[k].result.unwrap());
        assert_eq!(body[0].kind, InstKind::SiToFp(v(1)));
        assert_eq!(body[1].kind, InstKind::BinOp(BinOp::FDiv, xf, res(0)));
        assert_eq!(body[2].kind, InstKind::FpToSi(res(1)));
        assert_eq!(body[3].kind, InstKind::BinOp(BinOp::Mul, res(2), v(1)));
        assert_eq!(body[4].kind, InstKind::ICmp(IPred::Eq, v(3), v(0)));
    }

    #[test]
    fn remainder_with_a_data_reader_keeps_srem() {
        let mut body = zero_test();
        body.push(i(6, InstKind::BinOp(BinOp::Add, v(3), v(0))));
        let mut f = trial_loop(v(1), body, step());
        assert_eq!(run(&mut f, 1000, 2), NONE);
    }

    #[test]
    fn test_branching_inside_the_loop_keeps_srem() {
        let mut f = trial_loop(v(1), zero_test(), step());
        f.blocks[2].term = cond(4, 3, 3);
        assert_eq!(run(&mut f, 1000, 2), NONE);
    }

    #[test]
    fn remainder_on_its_divisor_recurrence_keeps_srem() {
        // gcd shape: the remainder becomes the next divisor
        let latch = vec![i(1, InstKind::Copy(Type::I64, v(3)))];
        let mut f = trial_loop(v(1), zero_test(), latch);
        assert_eq!(run(&mut f, 1000, 2), NONE);
        let srem = &f.blocks[2].insts[0].kind;
        assert!(matches!(srem, InstKind::BinOp(BinOp::SRem, _, _)));
    }

    #[test]
    fn dividend_window_ends_below_2_pow_53() {
        let mut f = trial_loop(v(1), zero_test(), step());
        assert_eq!(run(&mut f, F64_EXACT, 2), NONE);
        let mut f = trial_loop(v(1), zero_test(), step());
        assert_eq!(run(&mut f, F64_EXACT - 1, 2).compared, 1);
    }

    #[test]
    fn constant_or_possibly_zero_divisor_keeps_srem() {
        let mut f = trial_loop(Operand::ConstI64(7), zero_test(), step());
        assert_eq!(run(&mut f, 1000, 2), NONE);
        let mut f = trial_loop(v(1), zero_test(), step());
        assert_eq!(run(&mut f, 1000, 0), NONE);
    }
}
