//! Refinement through boolean structure. A short-circuit `a || b` /
//! `a && b` lowers to a bool cell written on two arms — the arm that
//! decided the result copies the first compare, the other copies the
//! second — and the loop body sits under a branch on that cell. On
//! the edge where the merged value is false every arm that can have
//! produced false was taken with its compare false, while the arm
//! taken only when `a` held is off the path; what the feasible arms
//! agree on holds on the edge. A `select` and a single-def copy chain
//! are read the same way, so the shapes select formation and the
//! elaborator leave behind are covered too.
//!
//! Exactness: a path is dropped only when it is contradictory — its
//! value is decided the other way at the arm's entry, or the ranges it
//! implies for one value do not meet — and what two feasible paths
//! disagree on is widened to the hull, never picked from one side.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{BinOp, BlockId, Function, InstKind, Operand, Terminator, Type, ValueId};

use super::SingleDefs;
use super::edge::edge_constraints;
use super::refine::Constraint;
use super::square::square_edge;
use super::transfer::{NEG_INF, NumFact, POS_INF, op_const_int};

/// Feasibility of `op == v` on a path: `None` when the path cannot
/// exist, else the constraints it implies.
type Path = Option<Vec<Constraint>>;

/// Copy chains and cells nest (`a || b || c`); past this the value is
/// left undecided.
const DEPTH: u32 = 8;

/// The entry constraints collected so far and how deep the walk is.
#[derive(Clone, Copy)]
struct Walk<'e> {
    entry: &'e [Vec<Constraint>],
    depth: u32,
}

impl Walk<'_> {
    fn deeper(self) -> Self {
        Self {
            entry: self.entry,
            depth: self.depth + 1,
        }
    }
}

pub(super) struct Logic<'a> {
    func: &'a Function,
    defs: &'a SingleDefs<'a>,
    preds: Vec<Vec<BlockId>>,
    /// per block: the multi-def values it writes
    cell_defs: Vec<HashSet<ValueId>>,
    /// per block: each cell's last def, `Some(op)` when it is a copy
    last_def: Vec<HashMap<ValueId, Option<Operand>>>,
    /// defining block of every single-def value
    home: HashMap<ValueId, BlockId>,
    /// per block: its predecessor when it has exactly one
    sole_pred: Vec<Option<usize>>,
}

impl<'a> Logic<'a> {
    pub(super) fn new(func: &'a Function, defs: &'a SingleDefs<'a>) -> Self {
        let n = func.blocks.len();
        let mut preds: Vec<Vec<BlockId>> = vec![Vec::new(); n];
        let mut cell_defs: Vec<HashSet<ValueId>> = vec![HashSet::default(); n];
        let mut last_def: Vec<HashMap<ValueId, Option<Operand>>> = vec![HashMap::default(); n];
        let mut home: HashMap<ValueId, BlockId> = HashMap::default();
        for block in &func.blocks {
            let bi = block.id.0 as usize;
            match &block.term {
                Terminator::Br(t) => preds[t.0 as usize].push(block.id),
                Terminator::CondBr {
                    then_blk, else_blk, ..
                } => {
                    preds[then_blk.0 as usize].push(block.id);
                    preds[else_blk.0 as usize].push(block.id);
                }
                _ => {}
            }
            for inst in &block.insts {
                let Some(r) = inst.result else { continue };
                if defs.contains_key(&r) {
                    home.insert(r, block.id);
                    continue;
                }
                cell_defs[bi].insert(r);
                let copied = match &inst.kind {
                    InstKind::Copy(_, op) | InstKind::Identity(op) => Some(*op),
                    _ => None,
                };
                last_def[bi].insert(r, copied);
            }
        }
        let sole_pred = preds
            .iter()
            .map(|ps| match ps.as_slice() {
                [p] => Some(p.0 as usize),
                _ => None,
            })
            .collect();
        Self {
            func,
            defs,
            preds,
            cell_defs,
            last_def,
            home,
            sole_pred,
        }
    }

    /// Add what every branch on a merged or selected condition says
    /// of its sole-predecessor targets. Iterated: an outer `||` reads
    /// the entry constraints an inner one just produced.
    pub(super) fn extend(&self, at_entry: &mut [Vec<Constraint>]) {
        loop {
            let mut changed = false;
            for block in &self.func.blocks {
                let Terminator::CondBr {
                    cond: Operand::Value(c),
                    then_blk,
                    else_blk,
                } = &block.term
                else {
                    continue;
                };
                if matches!(
                    self.defs.get(c),
                    Some(InstKind::ICmp(..) | InstKind::FCmp(..))
                ) {
                    continue;
                }
                let bi = block.id.0 as usize;
                for (target, taken) in [(*then_blk, true), (*else_blk, false)] {
                    if self.preds[target.0 as usize].len() != 1 {
                        continue;
                    }
                    let walk = Walk {
                        entry: at_entry,
                        depth: 0,
                    };
                    let Some(cs) = self.truth(&Operand::Value(*c), taken, block.id, walk) else {
                        continue;
                    };
                    let entry = &mut at_entry[target.0 as usize];
                    for c in cs {
                        // a cell the branching block rewrites is read
                        // fresh at the target
                        if self.cell_defs[bi].contains(&c.value()) || entry.contains(&c) {
                            continue;
                        }
                        entry.push(c);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// `op == v` where `op` is read in block `at`.
    fn truth(&self, op: &Operand, v: bool, at: BlockId, walk: Walk<'_>) -> Path {
        if walk.depth > DEPTH {
            return Some(Vec::new());
        }
        let x = match op {
            Operand::ConstBool(b) => return (*b == v).then(Vec::new),
            Operand::Value(x) => *x,
            other => return (op_const_int(other) == Some(v as i128)).then(Vec::new),
        };
        let Some(kind) = self.defs.get(&x) else {
            return self.cell(x, v, at, walk);
        };
        if self.decided_against(x, v, at) {
            return None;
        }
        match kind {
            InstKind::ICmp(..) | InstKind::FCmp(..) => Some(self.compare(kind, v, at)),
            InstKind::BinOp(op @ (BinOp::And | BinOp::Or), l, r)
                if self.func.values[x.0 as usize].ty == Type::Bool =>
            {
                self.logic(*op, (l, r), v, self.home[&x], walk)
            }
            InstKind::Copy(_, o) | InstKind::Identity(o) => {
                self.truth(o, v, self.home[&x], walk.deeper())
            }
            InstKind::Select(_, c, t, e) => self.select((c, t, e), v, self.home[&x], walk),
            _ => Some(Vec::new()),
        }
    }

    /// What the compare `kind` being `v` in block `at` says: the edge
    /// ranges, and the bound a square test puts on its root.
    fn compare(&self, kind: &InstKind, v: bool, at: BlockId) -> Vec<Constraint> {
        let mut cs = edge_constraints(kind, v, self.defs).unwrap_or_default();
        let at = at.0 as usize;
        cs.extend(square_edge(
            self.func,
            at,
            kind,
            v,
            self.defs,
            &self.sole_pred,
        ));
        cs
    }

    /// A compare guarded by another (`and`) or relieved by one (`or`):
    /// both halves hold on the edge that needs both, and what the two
    /// agree on on the other.
    fn logic(
        &self,
        op: BinOp,
        (l, r): (&Operand, &Operand),
        v: bool,
        home: BlockId,
        walk: Walk<'_>,
    ) -> Path {
        let lp = self.truth(l, v, home, walk.deeper());
        let rp = self.truth(r, v, home, walk.deeper());
        if (op == BinOp::And) == v {
            let mut cs = lp?;
            cs.extend(rp?);
            (!contradictory(&cs)).then_some(cs)
        } else {
            match (lp, rp) {
                (None, p) | (p, None) => p,
                (Some(a), Some(b)) => Some(common(&a, &b)),
            }
        }
    }

    /// `select c, t, e` being `v`: what the feasible arms agree on.
    fn select(
        &self,
        (c, t, e): (&Operand, &Operand, &Operand),
        v: bool,
        home: BlockId,
        walk: Walk<'_>,
    ) -> Path {
        let then = self.arm((c, true), (t, v), home, walk.deeper());
        let els = self.arm((c, false), (e, v), home, walk.deeper());
        match (then, els) {
            (None, p) | (p, None) => p,
            (Some(a), Some(b)) => Some(common(&a, &b)),
        }
    }

    /// One arm of a select: the condition went one way and the arm's
    /// operand is the selected value.
    fn arm(
        &self,
        cond: (&Operand, bool),
        arm: (&Operand, bool),
        at: BlockId,
        walk: Walk<'_>,
    ) -> Path {
        let mut cs = self.truth(cond.0, cond.1, at, walk)?;
        cs.extend(self.truth(arm.0, arm.1, at, walk)?);
        (!contradictory(&cs)).then_some(cs)
    }

    /// A multi-def cell read in `at` before `at` rewrites it holds the
    /// last def of whichever predecessor was taken: each predecessor's
    /// path carries its own entry constraints and what its copied
    /// operand being `v` implies.
    fn cell(&self, x: ValueId, v: bool, at: BlockId, walk: Walk<'_>) -> Path {
        let ai = at.0 as usize;
        if self.cell_defs[ai].contains(&x) || self.preds[ai].is_empty() {
            return Some(Vec::new());
        }
        if self.decided_against(x, v, at) {
            return None;
        }
        let mut paths: Vec<Vec<Constraint>> = Vec::new();
        for &p in &self.preds[ai] {
            let pi = p.0 as usize;
            // a predecessor without a def hands down an older one
            let Some(def) = self.last_def[pi].get(&x) else {
                return Some(Vec::new());
            };
            let learned = match def {
                Some(op) => self.truth(op, v, p, walk.deeper()),
                None => Some(Vec::new()),
            };
            let Some(learned) = learned else { continue };
            let survives = |c: &Constraint| !self.cell_defs[pi].contains(&c.value());
            let mut cs: Vec<Constraint> = walk.entry[pi].iter().copied().filter(survives).collect();
            cs.extend(learned.into_iter().filter(survives));
            if !contradictory(&cs) {
                paths.push(cs);
            }
        }
        let mut paths = paths.into_iter();
        let first = paths.next()?;
        Some(paths.fold(first, |acc, p| common(&acc, &p)))
    }

    /// `x` is fixed to `!v` at `at`'s entry: its sole predecessor
    /// branched on `x` and this is the other edge.
    fn decided_against(&self, x: ValueId, v: bool, at: BlockId) -> bool {
        let &[p] = self.preds[at.0 as usize].as_slice() else {
            return false;
        };
        let Terminator::CondBr {
            cond: Operand::Value(c),
            then_blk,
            else_blk,
        } = &self.func.blocks[p.0 as usize].term
        else {
            return false;
        };
        then_blk != else_blk && self.root(*c) == self.root(x) && (at == *then_blk) != v
    }

    /// The value at the end of a single-def copy chain.
    fn root(&self, mut x: ValueId) -> ValueId {
        while let Some(
            InstKind::Copy(_, Operand::Value(o)) | InstKind::Identity(Operand::Value(o)),
        ) = self.defs.get(&x)
        {
            x = *o;
        }
        x
    }
}

/// The meet of every range a path states for each value; `None` when
/// some value has none.
fn ranges(cs: &[Constraint]) -> Option<HashMap<ValueId, NumFact>> {
    let mut out: HashMap<ValueId, NumFact> = HashMap::default();
    for c in cs {
        let Constraint::Range(v, r) = c else { continue };
        let met = match out.get(v) {
            Some(have) => have.meet(*r)?,
            None => *r,
        };
        out.insert(*v, met);
    }
    Some(out)
}

/// The path states something no value can satisfy.
fn contradictory(cs: &[Constraint]) -> bool {
    let Some(rs) = ranges(cs) else { return true };
    cs.iter().any(|c| match c {
        Constraint::NonZero(v) => rs.get(v) == Some(&NumFact::point(0)),
        _ => false,
    })
}

/// What two feasible paths agree on: constraints both state, and the
/// hull of the ranges each states for a value both bound.
fn common(a: &[Constraint], b: &[Constraint]) -> Vec<Constraint> {
    let mut out: Vec<Constraint> = a
        .iter()
        .filter(|c| !matches!(c, Constraint::Range(..)) && b.contains(c))
        .copied()
        .collect();
    let (Some(ra), Some(rb)) = (ranges(a), ranges(b)) else {
        return out;
    };
    let mut hulls: Vec<(ValueId, NumFact)> = ra
        .iter()
        .filter_map(|(v, fa)| rb.get(v).map(|fb| (*v, fa.join(*fb))))
        .filter(|(_, h)| h.lo > NEG_INF || h.hi < POS_INF)
        .collect();
    hulls.sort_by_key(|(v, _)| v.0);
    out.extend(hulls.into_iter().map(|(v, h)| Constraint::Range(v, h)));
    out
}

#[cfg(test)]
mod tests {
    use super::super::refine::Constraints;
    use super::*;
    use torajs_core::ssa::{Block, IPred, Inst, Type, ValueInfo};

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

    fn br(t: u32) -> Terminator {
        Terminator::Br(BlockId(t))
    }

    fn cond_br(cond: u32, then_blk: u32, else_blk: u32) -> Terminator {
        Terminator::CondBr {
            cond: v(cond),
            then_blk: BlockId(then_blk),
            else_blk: BlockId(else_blk),
        }
    }

    fn func(blocks: Vec<Block>) -> Function {
        Function {
            name: "f".into(),
            params: vec![ValueId(0), ValueId(9)],
            ret: Type::Void,
            blocks,
            values: (0..16)
                .map(|i| ValueInfo {
                    ty: if i == 9 { Type::Ptr } else { Type::I64 },
                    name: None,
                })
                .collect(),
            current_origin: None,
        }
    }

    fn constraints(f: &Function) -> Constraints {
        Constraints::compute(f, &SingleDefs::of(f))
    }

    /// The meet of what block `b`'s entry states for `x`.
    fn range_at(cs: &Constraints, b: u32, x: u32) -> Option<NumFact> {
        let rs = ranges(cs.at(BlockId(b))).expect("entry constraints are consistent");
        rs.get(&ValueId(x)).copied()
    }

    /// `first || second` merged into cell %3 and branched on in bb3:
    /// bb0 branches on the first compare, bb1 (first true) copies it,
    /// bb2 evaluates the second and copies that; bb4 is the true
    /// target, bb5 the false one.
    fn or_shape(first: InstKind, second: InstKind) -> Function {
        func(vec![
            block(0, vec![inst(1, first)], cond_br(1, 1, 2)),
            block(1, vec![inst(3, InstKind::Copy(Type::Bool, v(1)))], br(3)),
            block(
                2,
                vec![inst(2, second), inst(3, InstKind::Copy(Type::Bool, v(2)))],
                br(3),
            ),
            block(3, vec![], cond_br(3, 4, 5)),
            block(4, vec![], Terminator::Ret(None)),
            block(5, vec![], Terminator::Ret(None)),
        ])
    }

    /// `first && second`: bb0 branches on the first compare, bb2
    /// (first false) copies it, bb1 evaluates the second.
    fn and_shape(first: InstKind, second: InstKind) -> Function {
        func(vec![
            block(0, vec![inst(1, first)], cond_br(1, 1, 2)),
            block(
                1,
                vec![inst(2, second), inst(3, InstKind::Copy(Type::Bool, v(2)))],
                br(3),
            ),
            block(2, vec![inst(3, InstKind::Copy(Type::Bool, v(1)))], br(3)),
            block(3, vec![], cond_br(3, 4, 5)),
            block(4, vec![], Terminator::Ret(None)),
            block(5, vec![], Terminator::Ret(None)),
        ])
    }

    fn below_48() -> InstKind {
        InstKind::ICmp(IPred::Slt, v(0), c(48))
    }

    fn above_57() -> InstKind {
        InstKind::ICmp(IPred::Sgt, v(0), c(57))
    }

    #[test]
    fn where_an_or_is_false_both_compares_are_false() {
        let f = or_shape(below_48(), above_57());
        let cs = constraints(&f);
        assert_eq!(range_at(&cs, 5, 0), Some(NumFact::new(48, 57)));
    }

    #[test]
    fn where_an_or_is_true_neither_compare_is_decided() {
        let f = or_shape(below_48(), above_57());
        let cs = constraints(&f);
        assert_eq!(range_at(&cs, 4, 0), None);
    }

    #[test]
    fn where_an_and_is_true_both_compares_are_true() {
        let f = and_shape(
            InstKind::ICmp(IPred::Sge, v(0), c(48)),
            InstKind::ICmp(IPred::Sle, v(0), c(57)),
        );
        let cs = constraints(&f);
        assert_eq!(range_at(&cs, 4, 0), Some(NumFact::new(48, 57)));
        assert_eq!(range_at(&cs, 5, 0), None);
    }

    #[test]
    fn a_select_is_read_through_its_arms() {
        // %3 = select %1, %1, %2 — the `||` after select formation
        let f = func(vec![
            block(
                0,
                vec![
                    inst(1, below_48()),
                    inst(2, above_57()),
                    inst(3, InstKind::Select(Type::Bool, v(1), v(1), v(2))),
                ],
                cond_br(3, 1, 2),
            ),
            block(1, vec![], Terminator::Ret(None)),
            block(2, vec![], Terminator::Ret(None)),
        ]);
        let cs = constraints(&f);
        assert_eq!(range_at(&cs, 2, 0), Some(NumFact::new(48, 57)));
        assert_eq!(range_at(&cs, 1, 0), None);
    }

    #[test]
    fn an_arm_the_entry_decided_the_other_way_is_off_the_path() {
        // `f() || d > 57`: the first arm copies an opaque bool that
        // the arm's own entry fixed true, so only the second arm can
        // have produced false
        let f = or_shape(InstKind::Load(Type::Bool, v(9), 0), above_57());
        let cs = constraints(&f);
        assert_eq!(range_at(&cs, 5, 0), Some(NumFact::new(NEG_INF, 57)));
        // `d < 48 || f()`: the false edge keeps the first compare's
        // verdict, the opaque second arm adds nothing
        let f = or_shape(below_48(), InstKind::Load(Type::Bool, v(9), 0));
        let cs = constraints(&f);
        assert_eq!(range_at(&cs, 5, 0), Some(NumFact::new(48, POS_INF)));
        assert_eq!(range_at(&cs, 4, 0), None);
    }

    #[test]
    fn a_cell_the_branching_block_rewrites_is_undecided() {
        let mut f = or_shape(below_48(), above_57());
        f.blocks[3]
            .insts
            .push(inst(3, InstKind::Copy(Type::Bool, v(9))));
        let cs = constraints(&f);
        assert_eq!(range_at(&cs, 5, 0), None);
        assert_eq!(range_at(&cs, 4, 0), None);
    }

    #[test]
    fn a_nested_or_reads_the_inner_merge() {
        // `(d < 48 || d > 57) || e < 0`: bb3 branches on the inner
        // cell into bb4 (copy it) / bb5 (evaluate the third), bb6
        // branches on the outer cell
        let f = func(vec![
            block(0, vec![inst(1, below_48())], cond_br(1, 1, 2)),
            block(1, vec![inst(3, InstKind::Copy(Type::Bool, v(1)))], br(3)),
            block(
                2,
                vec![
                    inst(2, above_57()),
                    inst(3, InstKind::Copy(Type::Bool, v(2))),
                ],
                br(3),
            ),
            block(3, vec![], cond_br(3, 4, 5)),
            block(4, vec![inst(6, InstKind::Copy(Type::Bool, v(3)))], br(6)),
            block(
                5,
                vec![
                    inst(5, InstKind::ICmp(IPred::Slt, v(10), c(0))),
                    inst(6, InstKind::Copy(Type::Bool, v(5))),
                ],
                br(6),
            ),
            block(6, vec![], cond_br(6, 7, 8)),
            block(7, vec![], Terminator::Ret(None)),
            block(8, vec![], Terminator::Ret(None)),
        ]);
        let cs = constraints(&f);
        assert_eq!(range_at(&cs, 8, 0), Some(NumFact::new(48, 57)));
        assert_eq!(range_at(&cs, 8, 10), Some(NumFact::new(0, POS_INF)));
        assert_eq!(range_at(&cs, 7, 0), None);
        assert_eq!(range_at(&cs, 7, 10), None);
    }
}
