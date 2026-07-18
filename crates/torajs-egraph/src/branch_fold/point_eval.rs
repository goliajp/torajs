//! Demand-driven per-point value evaluation — the LVI-lite half of
//! branch_fold (LLVM LazyValueInfo analogue over the post-φ cell
//! form). The flow-insensitive interval analysis joins every def of a
//! multi-def Copy cell, so a straight-line cell like the post-ctpop
//! count (`%c = copy 0; %c = copy (add %c, ctpop %n)`) widens to a
//! sentinel through its own self-referential join. This evaluator
//! instead resolves a cell read at a concrete program point to its
//! *unique last def* when one provably exists, then evaluates that
//! def's operands recursively at the def's own point.
//!
//! Unique-last-def rule: among the defs that dominate the read point,
//! take the lowest one (they sit on the point's dominator chain, so
//! program-point dominance orders them totally); it is the unique
//! last def iff no *other* def of the cell can reach the read point
//! along a path avoiding it (block-granular DFS — traversing a block
//! executes all of its instructions, so a path "passes" a def exactly
//! when it traverses the def's block through or up to the def index).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use torajs_core::ssa::{Function, InstKind, Operand, Terminator, ValueId};

use crate::dominator::DominatorTree;
use crate::interval::transfer::{self, AbsVal, NumFact};

/// A program point: `(block index, inst index)`. Operand reads of the
/// instruction at `inst_idx` see defs strictly before it; a block's
/// terminator reads at `(block, insts.len())`.
pub(crate) type Point = (usize, usize);

/// Recursion fuel — bounds the def-chain walk (memoized, so this only
/// caps pathological chains).
const FUEL: u32 = 256;

pub(crate) struct PointEval<'a> {
    func: &'a Function,
    dom: &'a DominatorTree,
    /// Every def site of every value, in layout order.
    defs: HashMap<ValueId, Vec<Point>>,
    memo: RefCell<HashMap<(ValueId, usize, usize), Option<NumFact>>>,
    fuel: Cell<u32>,
}

impl<'a> PointEval<'a> {
    pub(crate) fn new(func: &'a Function, dom: &'a DominatorTree) -> Self {
        let mut defs: HashMap<ValueId, Vec<Point>> = HashMap::new();
        for (bi, block) in func.blocks.iter().enumerate() {
            for (ii, inst) in block.insts.iter().enumerate() {
                if let Some(r) = inst.result {
                    defs.entry(r).or_default().push((bi, ii));
                }
            }
        }
        Self {
            func,
            dom,
            defs,
            memo: RefCell::new(HashMap::new()),
            fuel: Cell::new(FUEL),
        }
    }

    /// Evaluate an operand as read at `point`.
    pub(crate) fn eval_operand(&self, op: &Operand, point: Point) -> Option<NumFact> {
        if let Some(k) = transfer::op_const_int(op) {
            return Some(NumFact::point(k));
        }
        match op {
            Operand::Value(v) => self.eval_value(*v, point),
            _ => None,
        }
    }

    /// Evaluate a value as read at `point`: resolve its unique last
    /// def and evaluate that def's instruction at the def site.
    pub(crate) fn eval_value(&self, v: ValueId, point: Point) -> Option<NumFact> {
        let key = (v, point.0, point.1);
        if let Some(cached) = self.memo.borrow().get(&key) {
            return *cached;
        }
        if self.fuel.get() == 0 {
            return None;
        }
        self.fuel.set(self.fuel.get() - 1);
        // Seed the memo with None so a def cycle (loop-carried cell
        // reached through its own operands) settles as unknown
        // instead of recursing forever.
        self.memo.borrow_mut().insert(key, None);
        let result = self.eval_value_uncached(v, point);
        self.memo.borrow_mut().insert(key, result);
        result
    }

    fn eval_value_uncached(&self, v: ValueId, point: Point) -> Option<NumFact> {
        let sites = self.defs.get(&v)?;
        // Lowest def that dominates the read point (program-point
        // dominance: strictly-dominating block, or same block at a
        // lower inst index).
        let dominating: Vec<Point> = sites
            .iter()
            .copied()
            .filter(|d| self.point_dominates(*d, point))
            .collect();
        let last = *dominating.iter().max_by(|a, b| {
            if a.0 == b.0 {
                a.1.cmp(&b.1)
            } else if self
                .dom
                .dominates(self.func.blocks[a.0].id, self.func.blocks[b.0].id)
            {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            }
        })?;
        // Every other def must be unable to reach the read point
        // without re-passing `last`.
        for d in sites {
            if *d != last && self.def_reaches_avoiding(*d, point, last) {
                return None;
            }
        }
        let inst = &self.func.blocks[last.0].insts[last.1];
        self.eval_def_inst(&inst.kind, inst.result?, last)
    }

    /// Evaluate a def instruction with its operands read at the def's
    /// own point. Arithmetic shapes delegate to the shared interval
    /// transfer function.
    fn eval_def_inst(&self, kind: &InstKind, r: ValueId, at: Point) -> Option<NumFact> {
        let ty = self.func.values[r.0 as usize].ty;
        let env = |op: &Operand| match self.eval_operand(op, at) {
            Some(f) => AbsVal::Fact(f),
            None => AbsVal::Top,
        };
        match transfer::eval_inst(kind, ty, &env, &|_| false) {
            AbsVal::Fact(f) => Some(f),
            _ => None,
        }
    }

    fn point_dominates(&self, d: Point, p: Point) -> bool {
        if d.0 == p.0 {
            return d.1 < p.1;
        }
        let (db, pb) = (self.func.blocks[d.0].id, self.func.blocks[p.0].id);
        db != pb && self.dom.dominates(db, pb)
    }

    /// Can execution starting *just after* def `from` reach the read
    /// point `to` without executing def `avoid`? Block-granular DFS:
    /// traversing a block through its terminator executes every inst
    /// in it; entering `to`'s block from the top executes indices
    /// `[0, to.1)`.
    fn def_reaches_avoiding(&self, from: Point, to: Point, avoid: Point) -> bool {
        // In-block straight segment from the def to the read point.
        if from.0 == to.0 && from.1 < to.1 {
            let avoided = avoid.0 == from.0 && avoid.1 > from.1 && avoid.1 < to.1;
            if !avoided {
                return true;
            }
        }
        // Exiting `from`'s block executes indices (from.1, len) — if
        // `avoid` sits there, every escape passes it.
        if avoid.0 == from.0 && avoid.1 > from.1 {
            return false;
        }
        let mut stack: Vec<usize> = successors(&self.func.blocks[from.0].term)
            .into_iter()
            .filter_map(|t| self.block_index(t.0))
            .collect();
        let mut visited = vec![false; self.func.blocks.len()];
        while let Some(b) = stack.pop() {
            if visited[b] {
                continue;
            }
            visited[b] = true;
            if b == to.0 && !(avoid.0 == b && avoid.1 < to.1) {
                return true;
            }
            if b == avoid.0 {
                // Traversing this block through executes the avoided
                // def — the path dies here.
                continue;
            }
            for t in successors(&self.func.blocks[b].term) {
                if let Some(i) = self.block_index(t.0) {
                    stack.push(i);
                }
            }
        }
        false
    }

    fn block_index(&self, id: u32) -> Option<usize> {
        // BlockId.0 == position invariant holds throughout the
        // pipeline; fall back to a scan defensively in debug only.
        debug_assert!(self.func.blocks[id as usize].id.0 == id);
        Some(id as usize)
    }
}

pub(crate) fn successors(term: &Terminator) -> Vec<torajs_core::ssa::BlockId> {
    match term {
        Terminator::Br(t) => vec![*t],
        Terminator::CondBr {
            then_blk, else_blk, ..
        } => vec![*then_blk, *else_blk],
        Terminator::Ret(_) | Terminator::Unreachable => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{BinOp, Block, BlockId, Inst, Terminator, Type, ValueInfo};

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

    fn c(n: i64) -> Operand {
        Operand::ConstI64(n)
    }

    fn func(nvals: usize, blocks: Vec<Block>) -> Function {
        Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Void,
            blocks,
            values: vec![
                ValueInfo {
                    ty: Type::I64,
                    name: None
                };
                nvals
            ],
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

    /// The post-ctpop count-cell shape: straight-line cell reads
    /// resolve through the unique last def even though the cell is
    /// multi-def (`%1 = copy 0; %2 = add %1, ctpop; %1 = copy %2`),
    /// and the resolution survives an enclosing loop's back edge.
    #[test]
    fn straight_line_cell_resolves_through_loop() {
        // bb0: br bb1
        // bb1: %1 = copy 0
        //      %2 = ctpop %0
        //      %3 = add %1, %2
        //      %1 = copy %3
        //      cond_br %0, bb1, bb2   (back edge keeps bb1 in a loop)
        // bb2: ret
        let f = func(
            4,
            vec![
                block(
                    0,
                    vec![inst(0, InstKind::Copy(Type::I64, c(9)))],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![
                        inst(1, InstKind::Copy(Type::I64, c(0))),
                        inst(2, InstKind::Ctpop(v(0))),
                        inst(3, InstKind::BinOp(BinOp::Add, v(1), v(2))),
                        inst(1, InstKind::Copy(Type::I64, v(3))),
                    ],
                    Terminator::CondBr {
                        cond: v(0),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                ),
                block(2, vec![], Terminator::Ret(None)),
            ],
        );
        let dom = DominatorTree::compute(&f);
        let pe = PointEval::new(&f, &dom);
        // read of %1 at the add (bb1, idx 2): unique last def is the
        // copy-0 at idx 0 (the idx-3 redef re-passes it on loop-back).
        assert_eq!(pe.eval_value(ValueId(1), (1, 2)), Some(NumFact::new(0, 0)));
        // read of %1 at the terminator: last def is the idx-3 copy of
        // the add result → [0, 64].
        assert_eq!(pe.eval_value(ValueId(1), (1, 4)), Some(NumFact::new(0, 64)));
    }

    /// A loop-carried cell (defs in preheader + latch, read in the
    /// loop) must NOT resolve — the latch def reaches the read
    /// without re-passing the preheader def.
    #[test]
    fn loop_carried_cell_does_not_resolve() {
        // bb0: %0 = copy 0 ; br bb1
        // bb1: %1 = add %0, 1 ; %0 = copy %1 ; cond_br %1, bb1, bb2
        let f = func(
            2,
            vec![
                block(
                    0,
                    vec![inst(0, InstKind::Copy(Type::I64, c(0)))],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![
                        inst(1, InstKind::BinOp(BinOp::Add, v(0), c(1))),
                        inst(0, InstKind::Copy(Type::I64, v(1))),
                    ],
                    Terminator::CondBr {
                        cond: v(1),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                ),
                block(2, vec![], Terminator::Ret(None)),
            ],
        );
        let dom = DominatorTree::compute(&f);
        let pe = PointEval::new(&f, &dom);
        assert_eq!(pe.eval_value(ValueId(0), (1, 0)), None);
    }
}
