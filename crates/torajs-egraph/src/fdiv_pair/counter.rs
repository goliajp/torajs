//! A divisor that is a loop counter, carried in vector cells beside it.
//!
//! ```text
//! P:  ...                         P:  yv = splat (sitofp d); ys = splat C.0
//! B:  y = sitofp d                    dv = splat d; ds = splat C
//!     yv' = splat y               B:  ... vec.fdiv xv, yv ...
//!     ... vec.fdiv xv, yv' ...        yv = copy (vec.fadd yv, ys)
//!     dv' = splat d                   ... vec.and mv, dv ...
//!     ... vec.and mv, dv' ...         dv = copy (vec.add dv, ds)
//!     w = add d, C; d = copy w        w = add d, C; d = copy w
//! ```
//!
//! The splats move a general register into the vector file every trip
//! (`scvtf` + `dup`, `dup Vd, Xn`); the cells take one vector add a
//! trip instead (induction widening, as in LLVM's loop vectorizer, and
//! unroll_pair's float counter). The cells are set at the preheader's
//! end from `d` as it enters the loop and stepped with the loop's one
//! write of `d`, so wherever the loop reads them they hold `d` as it is
//! there. The i64 cell wraps as `d` does.
//!
//! The f64 cell is built in two cases, else the divisor is splatted
//! as before:
//! - the interval fact of `d` keeps it and one step past it inside
//!   ±2^53: every lane value and every step's sum is an exact integer,
//!   so the cell equals `sitofp d`;
//! - every divide of the divisor feeds a divisibility mask (`accum`,
//!   whose window gives `n ∈ [0, 2^53)` and `d ≥ 1`) and `d` steps up
//!   by `C ∈ [1, 2^52]`. Up to 2^53 the cell is `sitofp d` exactly, as
//!   above. Past it the cell never drops below 2^53 (each rounded sum
//!   is at least its addend), while `d > 2^53 > n` divides `n` only
//!   when `n = 0`; and the lane gives the same answer: `0 / y'` is 0,
//!   integral, and for `n > 0`, `n / y' ≤ (2^53 - 1) / 2^53`, which
//!   is representable and below 1, so not integral.

use crate::id_hash::HashMap;
use crate::interval::NumFact;

use torajs_core::ssa::loop_analysis::NaturalLoop;
use torajs_core::ssa::{
    BinOp, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId, VecInst, VecOp,
    VecShape, visit_value_operands,
};

use super::accum::Chain;
use super::{Quot, inst, new_value};

const F64_EXACT: i128 = 1 << 53;
/// A step every lane sum below 2^53 takes exactly.
const MASKED_STEP_MAX: i64 = 1 << 52;

/// A counter `d`: its one write in the loop is `d = copy w`, with
/// `w = add d, step` before it in the same block.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Counter {
    pub d: ValueId,
    pub step: i64,
}

/// `d` as a counter of `lp`, when the loop writes it nowhere but its
/// step's copy, and that copy is not in block `b` within `from..to`,
/// where the cells stand in for `d`'s reads.
pub(super) fn of(
    func: &Function,
    lp: &NaturalLoop,
    b: BlockId,
    d: ValueId,
    (from, to): (usize, usize),
) -> Option<Counter> {
    if func.values[d.0 as usize].ty != Type::I64 {
        return None;
    }
    let mut writes = in_loop_defs(func, lp, d);
    let (wb, at, w) = match (writes.next(), writes.next()) {
        (Some((wb, at, InstKind::Copy(Type::I64, Operand::Value(w)))), None) => (wb, at, *w),
        _ => return None,
    };
    if wb == b && from <= at && at < to {
        return None;
    }
    // the add runs on every trip that runs the copy, and reads `d` as
    // the copy overwrites it
    let mut adds = in_loop_defs(func, lp, w);
    let step = match (adds.next(), adds.next()) {
        (Some((ab, add_at, InstKind::BinOp(BinOp::Add, x, y))), None)
            if ab == wb && add_at < at =>
        {
            match (x, y) {
                (Operand::Value(x), Operand::ConstI64(c))
                | (Operand::ConstI64(c), Operand::Value(x))
                    if *x == d =>
                {
                    *c
                }
                _ => return None,
            }
        }
        _ => return None,
    };
    (step != 0).then_some(Counter { d, step })
}

/// The counter divisors of one block's pairs.
#[derive(Default)]
pub(super) struct Plan {
    /// per f64 divisor `y = sitofp d`, the counter `d`
    pub float: Vec<(ValueId, Counter)>,
    /// per i64 divisor operand of the masked adds, the counter it is
    pub int: Vec<(ValueId, Counter)>,
}

pub(super) fn plan(
    func: &Function,
    b: BlockId,
    lp: &NaturalLoop,
    known: &HashMap<ValueId, NumFact>,
    pairs: &[(Quot, Quot)],
    chains: &[Option<Chain>],
) -> Plan {
    let insts = &func.blocks[b.0 as usize].insts;
    let mut plan = Plan::default();
    // the splat of `y` sits at its first divide; the later divides of
    // `y` read it, so the cell stands in up to the last of them
    let mut last_read: Vec<(ValueId, usize, bool)> = Vec::new();
    for ((q1, _), chain) in pairs.iter().zip(chains) {
        match last_read.iter_mut().find(|(y, _, _)| *y == q1.y) {
            Some((_, at, masked)) => {
                *at = q1.fdiv;
                *masked &= chain.is_some();
            }
            None => last_read.push((q1.y, q1.fdiv, chain.is_some())),
        }
    }
    for (y, last, masked) in last_read {
        let Some(y_at) = insts.iter().position(|i| i.result == Some(y)) else {
            continue;
        };
        let InstKind::SiToFp(Operand::Value(d)) = insts[y_at].kind else {
            continue;
        };
        let fits = |c: &Counter| {
            exact_in_f64(c, known) || masked && (1..=MASKED_STEP_MAX).contains(&c.step)
        };
        if let Some(c) = of(func, lp, b, d, (y_at, last)).filter(fits) {
            plan.float.push((y, c));
        }
    }
    // accum keeps `d` unwritten in the block up to the selects
    let mut last_sel: Vec<(ValueId, ValueId, usize)> = Vec::new();
    for ch in chains.iter().flatten() {
        match last_sel.iter_mut().find(|(dd, _, _)| *dd == ch.dd) {
            Some((_, _, at)) => *at = (*at).max(ch.last_sel()),
            None => last_sel.push((ch.dd, ch.d, ch.last_sel())),
        }
    }
    for (dd, d, last) in last_sel {
        if let Some(c) = of(func, lp, b, d, (0, last)) {
            plan.int.push((dd, c));
        }
    }
    plan
}

/// The f64 cell is exact: `d` and its step stay inside ±2^53.
pub(super) fn exact_in_f64(c: &Counter, known: &HashMap<ValueId, NumFact>) -> bool {
    let reach = F64_EXACT - i128::from(c.step).abs();
    known
        .get(&c.d)
        .is_some_and(|f| f.lo >= -reach && f.hi <= reach)
}

fn in_loop_defs<'a>(
    func: &'a Function,
    lp: &'a NaturalLoop,
    v: ValueId,
) -> impl Iterator<Item = (BlockId, usize, &'a InstKind)> {
    lp.body.iter().flat_map(move |b| {
        func.blocks[b.0 as usize]
            .insts
            .iter()
            .enumerate()
            .filter(move |(_, i)| i.result == Some(v))
            .map(move |(at, i)| (*b, at, &i.kind))
    })
}

/// A vector cell of a counter and the splat it steps by.
#[derive(Clone, Copy)]
pub(super) struct Cell {
    pub cell: ValueId,
    step: ValueId,
    float: bool,
}

/// Sets a cell of `c` at the preheader's end: f64 lanes when `float`,
/// else i64.
pub(super) fn build(func: &mut Function, pre: BlockId, c: &Counter, float: bool) -> Cell {
    let v = Operand::Value;
    let (shape, by) = if float {
        (VecShape::F64x2, Operand::ConstF64(c.step as f64))
    } else {
        (VecShape::I64x2, Operand::ConstI64(c.step))
    };
    let ty = Type::Vec(shape);
    let mut insts = Vec::with_capacity(4);
    let start = if float {
        let y0 = new_value(func, Type::F64);
        insts.push(inst(y0, InstKind::SiToFp(v(c.d))));
        v(y0)
    } else {
        v(c.d)
    };
    let [v0, cell, step] = [(); 3].map(|_| new_value(func, ty));
    insts.push(inst(v0, InstKind::Vec(VecInst::Splat(shape, start))));
    insts.push(inst(cell, InstKind::Copy(ty, v(v0))));
    insts.push(inst(step, InstKind::Vec(VecInst::Splat(shape, by))));
    func.blocks[pre.0 as usize].insts.extend(insts);
    Cell { cell, step, float }
}

/// Steps each cell once a trip, with the loop's one write of `d`.
///
/// When the write sits in `b`, the block holding the cell's reads, past
/// the last of them, the step goes right after that read instead: both
/// run on every trip that runs either, and no read falls between them,
/// so every read still sees `d`. The next trip's lanes are then ready
/// a whole trip ahead: a step at the end of the trip puts the f64
/// add's latency in front of the next trip's divides.
pub(super) fn step_cells(
    func: &mut Function,
    lp: &NaturalLoop,
    b: BlockId,
    d: ValueId,
    cells: &[Cell],
) {
    let (wb, at) = {
        let mut writes = in_loop_defs(func, lp, d).map(|(wb, at, _)| (wb, at));
        let (Some(w), None) = (writes.next(), writes.next()) else {
            unreachable!("a counter has one write in its loop");
        };
        w
    };
    let v = Operand::Value;
    let mut sites: Vec<(usize, Cell)> = cells
        .iter()
        .map(|c| {
            let reads = |i: &Inst| {
                let mut hit = false;
                visit_value_operands(&i.kind, |x| hit |= x == c.cell);
                hit
            };
            let last = func.blocks[b.0 as usize].insts.iter().rposition(reads);
            match last {
                Some(r) if wb == b && r < at => (r, *c),
                _ => (at, *c),
            }
        })
        .collect();
    // from the back, so the earlier positions stay put
    sites.sort_by_key(|(after, _)| std::cmp::Reverse(*after));
    for (after, c) in sites {
        let (op, ty) = if c.float {
            (VecOp::FAdd, Type::Vec(VecShape::F64x2))
        } else {
            (VecOp::Add, Type::Vec(VecShape::I64x2))
        };
        let next = new_value(func, ty);
        let step = [
            inst(
                next,
                InstKind::Vec(VecInst::BinOp(op, v(c.cell), v(c.step))),
            ),
            inst(c.cell, InstKind::Copy(ty, v(next))),
        ];
        func.blocks[wb.0 as usize]
            .insts
            .splice(after + 1..after + 1, step);
    }
}

/// The cells of a block's counters, built in the preheader.
#[derive(Default)]
pub(super) struct Cells {
    /// per f64 divisor, the cell its divides read
    pub float: HashMap<ValueId, ValueId>,
    /// per i64 divisor operand, the cell its masked adds read
    pub int: HashMap<ValueId, ValueId>,
    /// per counter, the cells its write steps
    pub by_counter: Vec<(ValueId, Vec<Cell>)>,
}

pub(super) fn build_cells(func: &mut Function, pre: BlockId, plan: &Plan) -> Cells {
    let mut cells = Cells::default();
    let float = plan.float.iter().map(|(y, c)| (y, c, true));
    let int = plan.int.iter().map(|(dd, c)| (dd, c, false));
    for (key, c, is_float) in float.chain(int) {
        let cell = build(func, pre, c, is_float);
        let map = if is_float {
            &mut cells.float
        } else {
            &mut cells.int
        };
        map.insert(*key, cell.cell);
        match cells.by_counter.iter_mut().find(|(d, _)| *d == c.d) {
            Some((_, cs)) => cs.push(cell),
            None => cells.by_counter.push((c.d, vec![cell])),
        }
    }
    cells
}

/// Drops from block `b` the conversion or copy of `d` that only the
/// splats read, now that the cells stand in for them.
pub(super) fn drop_unread(func: &mut Function, b: BlockId, cells: &Cells) {
    for v in cells.float.keys().chain(cells.int.keys()) {
        let mut reads = 0u32;
        let mut defs = 0u32;
        for blk in &func.blocks {
            for i in &blk.insts {
                defs += u32::from(i.result == Some(*v));
                visit_value_operands(&i.kind, |x| reads += u32::from(x == *v));
            }
            if let Terminator::CondBr {
                cond: Operand::Value(x),
                ..
            }
            | Terminator::Ret(Some(Operand::Value(x))) = &blk.term
            {
                reads += u32::from(x == v);
            }
        }
        if reads == 0 && defs == 1 {
            func.blocks[b.0 as usize].insts.retain(|i| {
                i.result != Some(*v)
                    || !matches!(i.kind, InstKind::SiToFp(_) | InstKind::Copy(Type::I64, _))
            });
        }
    }
}
