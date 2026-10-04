//! Recognition of the integer sum loop float_demote leaves behind:
//!
//! ```text
//! H:     c = icmp slt i, K ; cond_br c, <chain>, <exit>
//! chain: straight-line blocks; each cond_br is a growth guard whose
//!        `then` arm leaves the loop
//!        e = load_dyn i64, base, (shl i, 3)      element reads
//!        t = add e, inv | copy e | add e, e'     lane values
//!        S' = add S, x                           the one accumulation
//!        S = copy S' ; i = copy (add i, 1)       back to H
//! ```
//!
//! Everything else in the body (a call, a store, a float op, another
//! branch) leaves the loop alone. A guard on a lane value must pass
//! every value in `[0, 2^53)`: the widened loop checks lanes against
//! exactly that range, so a stricter guard could be skipped. A guard on
//! `S'` must bound it within `±2^53`, which keeps a block's i64 sum
//! from overflowing. A loop with no guard on `S'` is a wrapping i64
//! sum, the same in any order.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::loop_analysis::NaturalLoop;
use torajs_core::ssa::{
    BinOp, BlockId, Function, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId,
    visit_value_operands,
};

use super::defs::DefSites;
use super::flow::{live_in, successors};
use super::guard::{Test, bounds_sum, bounds_sum_above, passes_exact_range};
use crate::load_licm::ARR_DATA_PTR_OFF;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Step {
    /// `r = load_dyn i64, base, i*8`
    Elem(ValueId),
    /// `r = copy src`
    Alias(ValueId, ValueId),
    /// `r = add a, b`, at least one operand a lane value
    Add(ValueId, Operand, Operand),
}

#[derive(Debug)]
pub(super) struct Plan {
    pub header: BlockId,
    pub entries: Vec<BlockId>,
    pub counter: ValueId,
    pub bound: Operand,
    pub arr: Operand,
    pub base: Operand,
    pub acc: ValueId,
    pub steps: Vec<Step>,
    pub addend: ValueId,
    /// Lane values whose every lane must lie in `[0, 2^53)`.
    pub checked: Vec<ValueId>,
    pub sum_tests: Vec<Test>,
    /// Loop-invariant instructions of the body, in order: global
    /// reads (nothing in the body writes memory) and compares of them.
    pub invariant: Vec<Inst>,
    /// Exits on an invariant condition: `(cond, leaves when true)`.
    /// Taken at all, such an exit is taken in the first iteration.
    pub entry_exits: Vec<(ValueId, bool)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Idx8,
    Next,
    Lane,
    SumNext,
    Shr(ValueId, i64),
    Guard(ValueId, Test),
    /// a value no iteration changes, computed in the body
    Fixed,
}

enum Src {
    Class(Class),
    Invariant,
    Carried(ValueId),
    Other,
}

struct Walk<'a> {
    func: &'a Function,
    defs: &'a DefSites,
    in_loop: HashSet<ValueId>,
    class: HashMap<ValueId, Class>,
    counter: ValueId,
    acc: Option<ValueId>,
    sum_next: Option<ValueId>,
    addend: Option<ValueId>,
    base: Option<Operand>,
    counter_step: bool,
    acc_step: bool,
    steps: Vec<Step>,
    checked: Vec<ValueId>,
    sum_tests: Vec<Test>,
    invariant: Vec<Inst>,
    entry_exits: Vec<(ValueId, bool)>,
}

pub(super) fn match_loop(func: &Function, lp: &NaturalLoop, defs: &DefSites) -> Option<Plan> {
    let [latch] = lp.back_edge_sources[..] else {
        return None;
    };
    let header = &func.blocks[lp.header.0 as usize];
    let [ref cmp] = header.insts[..] else {
        return None;
    };
    let (InstKind::ICmp(IPred::Slt, Operand::Value(counter), bound), Some(c)) =
        (&cmp.kind, cmp.result)
    else {
        return None;
    };
    let Terminator::CondBr {
        cond: Operand::Value(cv),
        then_blk,
        else_blk,
    } = header.term
    else {
        return None;
    };
    if cv != c || !lp.contains(&then_blk) || lp.contains(&else_blk) {
        return None;
    }
    let in_loop: HashSet<ValueId> = lp
        .body
        .iter()
        .flat_map(|b| func.blocks[b.0 as usize].insts.iter())
        .filter_map(|i| i.result)
        .collect();
    let bound = *bound;
    if !matches!(bound, Operand::ConstI64(_))
        && !matches!(bound, Operand::Value(k) if !in_loop.contains(&k) && func.value_type(k) == Type::I64)
    {
        return None;
    }
    let mut w = Walk {
        func,
        defs,
        in_loop,
        class: HashMap::new(),
        counter: *counter,
        acc: None,
        sum_next: None,
        addend: None,
        base: None,
        counter_step: false,
        acc_step: false,
        steps: Vec::new(),
        checked: Vec::new(),
        sum_tests: Vec::new(),
        invariant: Vec::new(),
        entry_exits: Vec::new(),
    };
    let chain_len = w.walk_chain(lp, then_blk, latch)?;
    if chain_len + 1 != lp.body.len() || !w.counter_step || !w.acc_step {
        return None;
    }
    let (acc, addend, base) = (w.acc?, w.addend?, w.base?);
    // without a guard the sum is a wrapping i64 sum, exact in any
    // order; with one, the block check needs every addend non-negative
    // and the sum bounded above
    let mut checked = w.checked;
    if !w.sum_tests.is_empty() {
        if !w.sum_tests.iter().any(|t| bounds_sum_above(*t)) {
            return None;
        }
        if !checked.contains(&addend) {
            checked.push(addend);
        }
    }
    let arr = array_of(func, defs, base)?;
    // the scalar loop restarts from H after a block; nothing it left
    // in a body value may be read past H's exit
    let exit = else_blk;
    let mut carried: Vec<ValueId> = w.in_loop.iter().copied().collect();
    carried.sort_by_key(|v| v.0);
    if carried
        .iter()
        .any(|&v| v != acc && v != w.counter && live_in(func, exit, v))
    {
        return None;
    }
    let entries = (0..func.blocks.len() as u32)
        .map(BlockId)
        .filter(|b| !lp.contains(b))
        .filter(|b| successors(&func.blocks[b.0 as usize].term).any(|s| s == lp.header))
        .collect();
    Some(Plan {
        header: lp.header,
        entries,
        counter: w.counter,
        bound,
        arr,
        base,
        acc,
        steps: w.steps,
        addend,
        checked,
        sum_tests: w.sum_tests,
        invariant: w.invariant,
        entry_exits: w.entry_exits,
    })
}

impl Walk<'_> {
    /// Walk the body from `start` back to the header; the number of
    /// blocks walked, or `None` off the shape.
    fn walk_chain(&mut self, lp: &NaturalLoop, start: BlockId, latch: BlockId) -> Option<usize> {
        let mut seen: HashSet<BlockId> = HashSet::new();
        let mut cur = start;
        loop {
            if cur == lp.header || !lp.contains(&cur) || !seen.insert(cur) {
                return None;
            }
            let block = &self.func.blocks[cur.0 as usize];
            for inst in &block.insts {
                if self.fixed(inst) {
                    self.invariant.push(inst.clone());
                    self.class.insert(inst.result?, Class::Fixed);
                    continue;
                }
                self.inst(inst.result?, &inst.kind)?;
            }
            cur = match block.term {
                Terminator::Br(next) if next == lp.header => {
                    return (cur == latch).then_some(seen.len());
                }
                Terminator::Br(next) => next,
                Terminator::CondBr {
                    cond: Operand::Value(c),
                    then_blk,
                    else_blk,
                } if self.class.get(&c) == Some(&Class::Fixed) => {
                    let leaves_on_true = !lp.contains(&then_blk);
                    if leaves_on_true == !lp.contains(&else_blk) {
                        return None;
                    }
                    self.entry_exits.push((c, leaves_on_true));
                    if leaves_on_true { else_blk } else { then_blk }
                }
                Terminator::CondBr {
                    cond: Operand::Value(c),
                    then_blk,
                    else_blk,
                } if !lp.contains(&then_blk) => {
                    let Some(&Class::Guard(v, test)) = self.class.get(&c) else {
                        return None;
                    };
                    self.guard(v, test)?;
                    else_blk
                }
                _ => return None,
            };
        }
    }

    fn guard(&mut self, v: ValueId, test: Test) -> Option<()> {
        if Some(v) == self.sum_next {
            bounds_sum(test).then(|| self.sum_tests.push(test))
        } else {
            if !passes_exact_range(test) {
                return None;
            }
            if !self.checked.contains(&v) {
                self.checked.push(v);
            }
            Some(())
        }
    }

    /// A global address, a read through one, or a compare of values
    /// fixed for the whole loop.
    fn fixed(&self, inst: &Inst) -> bool {
        let fixed = |op: &Operand| match op {
            Operand::Value(v) => {
                self.class.get(v) == Some(&Class::Fixed) || !self.in_loop.contains(v)
            }
            Operand::ConstI64(_) | Operand::ConstPtrNull | Operand::ConstBool(_) => true,
            _ => false,
        };
        match &inst.kind {
            InstKind::GlobalRef(_) => true,
            InstKind::Load(Type::I64, Operand::Value(g), _) => {
                let global = |i: &Inst| matches!(i.kind, InstKind::GlobalRef(_));
                self.invariant
                    .iter()
                    .any(|i| i.result == Some(*g) && global(i))
                    || (!self.in_loop.contains(g)
                        && self.defs.single_def(self.func, *g).is_some_and(global))
            }
            InstKind::ICmp(_, a, b) => fixed(a) && fixed(b),
            _ => false,
        }
    }

    fn src(&self, op: &Operand) -> Src {
        match op {
            Operand::Value(v) if self.class.get(v) == Some(&Class::Fixed) => Src::Invariant,
            Operand::Value(v) => match self.class.get(v) {
                Some(c) => Src::Class(*c),
                None if self.in_loop.contains(v) => Src::Carried(*v),
                None => Src::Invariant,
            },
            Operand::ConstI64(_) => Src::Invariant,
            _ => Src::Other,
        }
    }

    fn is_counter(&self, op: &Operand) -> bool {
        *op == Operand::Value(self.counter)
    }

    fn lane_or_sum(&self, v: &Operand) -> Option<ValueId> {
        match self.src(v) {
            Src::Class(Class::Lane | Class::SumNext) => match v {
                Operand::Value(x) => Some(*x),
                _ => None,
            },
            _ => None,
        }
    }

    fn element(&mut self, r: ValueId, base: &Operand) -> Option<Class> {
        if !matches!(self.src(base), Src::Invariant) || !matches!(base, Operand::Value(_)) {
            return None;
        }
        if self.base.is_some_and(|b| b != *base) {
            return None;
        }
        self.base = Some(*base);
        self.steps.push(Step::Elem(r));
        Some(Class::Lane)
    }

    fn inst(&mut self, r: ValueId, kind: &InstKind) -> Option<()> {
        // a def of a cell is its copy in the latch; a read after that
        // copy would see the next iteration's value
        let mut late_read = false;
        visit_value_operands(kind, |v| {
            late_read |=
                (self.counter_step && v == self.counter) || (self.acc_step && Some(v) == self.acc)
        });
        let cell_def = r == self.counter || Some(r) == self.acc;
        if late_read || (cell_def && !matches!(kind, InstKind::Copy(..))) {
            return None;
        }
        let class = match kind {
            InstKind::BinOp(BinOp::Shl, i, Operand::ConstI64(3)) if self.is_counter(i) => {
                Class::Idx8
            }
            InstKind::BinOp(BinOp::Add, i, Operand::ConstI64(1)) if self.is_counter(i) => {
                Class::Next
            }
            InstKind::LoadDyn(Type::I64, base, Operand::Value(off))
                if self.class.get(off) == Some(&Class::Idx8) =>
            {
                self.element(r, base)?
            }
            InstKind::LoadDynScaled8(Type::I64, base, i) if self.is_counter(i) => {
                self.element(r, base)?
            }
            InstKind::Copy(Type::I64, Operand::Value(src)) => return self.copy(r, *src),
            InstKind::BinOp(BinOp::Add, a, b) => self.add(r, a, b)?,
            InstKind::BinOp(BinOp::LShr, v, Operand::ConstI64(k)) => {
                Class::Shr(self.lane_or_sum(v)?, *k)
            }
            InstKind::ICmp(IPred::Ne, Operand::Value(t), Operand::ConstI64(0)) => {
                let Some(&Class::Shr(v, k)) = self.class.get(t) else {
                    return None;
                };
                Class::Guard(v, Test::HighBits(k))
            }
            InstKind::ICmp(pred @ (IPred::Sgt | IPred::Slt), v, c) => {
                let v = self.lane_or_sum(v)?;
                let c = self.defs.const_i64(self.func, c)?;
                Class::Guard(
                    v,
                    if *pred == IPred::Sgt {
                        Test::Above(c)
                    } else {
                        Test::Below(c)
                    },
                )
            }
            _ => return None,
        };
        self.class.insert(r, class);
        Some(())
    }

    fn copy(&mut self, r: ValueId, src: ValueId) -> Option<()> {
        let class = self.class.get(&src).copied();
        if r == self.counter {
            self.counter_step = class == Some(Class::Next);
            return self.counter_step.then_some(());
        }
        if self.acc == Some(r) {
            self.acc_step = class == Some(Class::SumNext);
            return self.acc_step.then_some(());
        }
        if class != Some(Class::Lane) {
            return None;
        }
        self.steps.push(Step::Alias(r, src));
        self.class.insert(r, Class::Lane);
        Some(())
    }

    fn add(&mut self, r: ValueId, a: &Operand, b: &Operand) -> Option<Class> {
        match (self.src(a), self.src(b)) {
            (Src::Class(Class::Lane), Src::Class(Class::Lane) | Src::Invariant)
            | (Src::Invariant, Src::Class(Class::Lane)) => {
                self.steps.push(Step::Add(r, *a, *b));
                Some(Class::Lane)
            }
            (Src::Carried(s), Src::Class(Class::Lane)) => self.accumulate(r, s, b),
            (Src::Class(Class::Lane), Src::Carried(s)) => self.accumulate(r, s, a),
            _ => None,
        }
    }

    fn accumulate(&mut self, r: ValueId, s: ValueId, x: &Operand) -> Option<Class> {
        let Operand::Value(x) = x else { return None };
        if s == self.counter || self.sum_next.is_some() || self.func.value_type(s) != Type::I64 {
            return None;
        }
        self.acc = Some(s);
        self.sum_next = Some(r);
        self.addend = Some(*x);
        Some(Class::SumNext)
    }
}

/// The array whose data pointer `base` was read from.
fn array_of(func: &Function, defs: &DefSites, base: Operand) -> Option<Operand> {
    let Operand::Value(b) = base else { return None };
    // a cell redefined between that read and the loop would hand the
    // length check another array: the array is defined once, or is a
    // parameter the body never assigns
    match &defs.single_def(func, b)?.kind {
        InstKind::Load(Type::Ptr, arr @ Operand::Value(a), ARR_DATA_PTR_OFF)
            if if func.params.contains(a) {
                !defs.has_def(*a)
            } else {
                defs.single_def(func, *a).is_some()
            } =>
        {
            Some(*arr)
        }
        _ => None,
    }
}
