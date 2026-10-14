//! Constants forwarded through block-local cells.
//!
//! tail_dup copies a join's latch into each arm, so an arm that sets a
//! cell to a constant runs straight into the copy that hands the cell
//! on to the loop's own cell: `%t = copy 0`, then in the block it falls
//! into, `%d = copy %t`. A block entered only from the one before it
//! sees what that block left in its cells, so the read takes the
//! constant itself. Once no block reads `%t` before setting it, `%t`
//! never carries a value from one block into the next, and a set of
//! `%t` that nothing after it in its block reads goes. Codegen then
//! writes the constant straight into `%d`'s register instead of
//! through a scratch one (array-any-double-sum-1m's number arms each
//! set two such cells).
//!
//! Copy propagation along single-predecessor chains and dead-assignment
//! removal over the function's multi-def cells. Runs last, after
//! block_layout, whose tail_dup makes the shape. `TORAJS_CONST_COPY_FWD_OFF=1` skips,
//! `TORAJS_CONST_COPY_FWD_STATS=1` counts.

use torajs_core::ssa::{
    Block, Function, InstKind, Module, Operand, Terminator, ValueId, visit_value_operands,
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ConstCopyFwdStats {
    /// Copies of a cell that now copy its constant.
    pub copies_forwarded: u32,
    /// Constant sets of a cell nothing read any more.
    pub sets_removed: u32,
}

pub fn forward_constant_copies(module: &mut Module) -> ConstCopyFwdStats {
    let mut stats = ConstCopyFwdStats::default();
    for func in module.funcs.iter_mut() {
        if !func.is_declaration() {
            in_function(func, &mut stats);
        }
    }
    stats
}

fn in_function(func: &mut Function, stats: &mut ConstCopyFwdStats) {
    if !copies_a_constant_cell(func) {
        return;
    }
    let before = stats.copies_forwarded;
    forward(func, stats);
    if stats.copies_forwarded == before {
        return;
    }
    let exposed = exposed_reads(func);
    let mut live = Marks::new(func.values.len());
    for block in func.blocks.iter_mut() {
        remove_dead_sets(block, &exposed, &mut live, stats);
    }
}

/// One bit per value.
struct Bits(Vec<u64>);

impl Bits {
    fn new(values: usize) -> Self {
        Bits(vec![0; values.div_ceil(64)])
    }

    fn set(&mut self, v: ValueId) {
        self.0[v.0 as usize / 64] |= 1 << (v.0 % 64);
    }

    fn has(&self, v: ValueId) -> bool {
        self.0[v.0 as usize / 64] & (1 << (v.0 % 64)) != 0
    }
}

/// A value set cleared in O(1) by moving to the next epoch, for one
/// block at a time.
struct Marks {
    at: Vec<u32>,
    epoch: u32,
}

impl Marks {
    fn new(values: usize) -> Self {
        Marks {
            at: vec![0; values],
            epoch: 0,
        }
    }

    fn clear(&mut self) {
        self.epoch += 1;
    }

    fn insert(&mut self, v: ValueId) {
        self.at[v.0 as usize] = self.epoch;
    }

    fn remove(&mut self, v: ValueId) {
        self.at[v.0 as usize] = 0;
    }

    fn contains(&self, v: ValueId) -> bool {
        self.at[v.0 as usize] == self.epoch
    }
}

fn is_constant_set(kind: &InstKind) -> bool {
    matches!(kind, InstKind::Copy(_, k) if !matches!(k, Operand::Value(_)))
}

/// Does some copy read a cell that a constant set defines? One walk
/// over the instructions; most functions have no such copy and skip
/// the rest of the pass.
fn copies_a_constant_cell(func: &Function) -> bool {
    let mut konst = Bits::new(func.values.len());
    let mut any = false;
    for inst in func.blocks.iter().flat_map(|b| &b.insts) {
        if let Some(r) = inst.result
            && is_constant_set(&inst.kind)
        {
            konst.set(r);
            any = true;
        }
    }
    any && func
        .blocks
        .iter()
        .flat_map(|b| &b.insts)
        .any(|inst| matches!(&inst.kind, InstKind::Copy(_, Operand::Value(t)) if konst.has(*t)))
}

/// Rewrites each copy of a cell whose constant the path into it fixes.
/// A block takes the constants its only predecessor left when that one
/// comes first in the layout (and so has been walked already).
fn forward(func: &mut Function, stats: &mut ConstCopyFwdStats) {
    let n = func.blocks.len();
    let mut preds: Vec<(u32, usize)> = vec![(0, 0); n];
    for (b, block) in func.blocks.iter().enumerate() {
        let succs: &[_] = match &block.term {
            Terminator::Br(t) => &[*t],
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => &[*then_blk, *else_blk],
            _ => &[],
        };
        for t in succs {
            let e = &mut preds[t.0 as usize];
            *e = (e.0 + 1, b);
        }
    }
    let values = &func.values;
    // per block, the cells it leaves holding a constant: a handful at
    // most, so a list
    let mut out: Vec<Vec<(ValueId, Operand)>> = vec![Vec::new(); n];
    for (b, block) in func.blocks.iter_mut().enumerate() {
        let mut known = match preds[b] {
            (1, p) if p < b => out[p].clone(),
            _ => Vec::new(),
        };
        for inst in block.insts.iter_mut() {
            if let InstKind::Copy(ty, Operand::Value(t)) = &inst.kind
                && let Some(&(_, k)) = known.iter().find(|(c, _)| c == t)
                && values[t.0 as usize].ty == *ty
            {
                inst.kind = InstKind::Copy(*ty, k);
                stats.copies_forwarded += 1;
            }
            if let Some(r) = inst.result {
                known.retain(|(c, _)| *c != r);
                if let InstKind::Copy(_, k) = &inst.kind
                    && !matches!(k, Operand::Value(_))
                {
                    known.push((r, *k));
                }
            }
        }
        out[b] = known;
    }
}

/// Drops each constant set of a cell no block reads before setting
/// (`exposed`) that nothing after it in `block` reads.
fn remove_dead_sets(
    block: &mut Block,
    exposed: &Bits,
    live: &mut Marks,
    stats: &mut ConstCopyFwdStats,
) {
    let candidate = |inst: &torajs_core::ssa::Inst| {
        inst.result.is_some_and(|r| !exposed.has(r)) && is_constant_set(&inst.kind)
    };
    if !block.insts.iter().any(candidate) {
        return;
    }
    live.clear();
    term_reads(&block.term, |v| live.insert(v));
    let mut keep = vec![true; block.insts.len()];
    for (k, inst) in block.insts.iter().enumerate().rev() {
        if let Some(r) = inst.result {
            if candidate(inst) && !live.contains(r) {
                keep[k] = false;
                stats.sets_removed += 1;
                continue;
            }
            live.remove(r);
        }
        visit_value_operands(&inst.kind, |v| live.insert(v));
    }
    let mut k = 0;
    block.insts.retain(|_| {
        k += 1;
        keep[k - 1]
    });
}

/// The values some block reads before setting them: they may carry a
/// value in from another block (parameters included).
fn exposed_reads(func: &Function) -> Bits {
    let mut out = Bits::new(func.values.len());
    let mut set = Marks::new(func.values.len());
    for block in &func.blocks {
        set.clear();
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |v| {
                if !set.contains(v) {
                    out.set(v);
                }
            });
            if let Some(r) = inst.result {
                set.insert(r);
            }
        }
        term_reads(&block.term, |v| {
            if !set.contains(v) {
                out.set(v);
            }
        });
    }
    out
}

fn term_reads(term: &Terminator, mut f: impl FnMut(ValueId)) {
    match term {
        Terminator::CondBr {
            cond: Operand::Value(v),
            ..
        }
        | Terminator::Ret(Some(Operand::Value(v))) => f(*v),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{BlockId, Inst, Type, ValueInfo};

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    fn i(r: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(r)),
            kind,
            origin: None,
        }
    }

    fn func(blocks: Vec<Block>) -> Function {
        Function {
            name: "f".into(),
            params: vec![ValueId(0)],
            ret: Type::I64,
            blocks,
            values: (0..8)
                .map(|_| ValueInfo {
                    ty: Type::I64,
                    name: None,
                })
                .collect(),
            current_origin: None,
        }
    }

    /// `bb0` sets `%1 = 0` and hands it to `%2`, which `bb1` reads;
    /// `extra` goes after the hand-over in `bb0`.
    fn arm(extra: Vec<Inst>) -> Function {
        let mut insts = vec![
            i(1, InstKind::Copy(Type::I64, Operand::ConstI64(0))),
            i(2, InstKind::Copy(Type::I64, v(1))),
        ];
        insts.extend(extra);
        func(vec![
            Block {
                id: BlockId(0),
                insts,
                term: Terminator::Br(BlockId(1)),
            },
            Block {
                id: BlockId(1),
                insts: vec![],
                term: Terminator::Ret(Some(v(2))),
            },
        ])
    }

    fn run(f: &mut Function) -> ConstCopyFwdStats {
        let mut stats = ConstCopyFwdStats::default();
        in_function(f, &mut stats);
        stats
    }

    #[test]
    fn a_block_local_constant_goes_straight_to_the_copy() {
        let mut f = arm(Vec::new());
        assert_eq!(
            run(&mut f),
            ConstCopyFwdStats {
                copies_forwarded: 1,
                sets_removed: 1
            }
        );
        assert_eq!(
            f.blocks[0].insts,
            vec![i(2, InstKind::Copy(Type::I64, Operand::ConstI64(0)))]
        );
    }

    #[test]
    fn a_cell_another_block_reads_first_keeps_its_set() {
        let mut f = arm(Vec::new());
        f.blocks[1].term = Terminator::Ret(Some(v(1)));
        run(&mut f);
        assert_eq!(
            f.blocks[0].insts.first(),
            Some(&i(1, InstKind::Copy(Type::I64, Operand::ConstI64(0))))
        );
    }

    #[test]
    fn a_later_read_in_the_block_keeps_the_set() {
        let read = i(3, InstKind::Copy(Type::I64, v(1)));
        let mut f = arm(vec![read]);
        let stats = run(&mut f);
        assert_eq!(stats.copies_forwarded, 2);
        // both reads took the constant, so the set itself is dead, and
        // so is `%3`, which nothing reads
        assert_eq!(stats.sets_removed, 2);
        let use_after = i(4, InstKind::BinOp(torajs_core::ssa::BinOp::Add, v(1), v(0)));
        let mut f = arm(vec![use_after]);
        let stats = run(&mut f);
        assert_eq!(stats.sets_removed, 0);
    }

    #[test]
    fn the_constant_follows_into_the_block_it_falls_into() {
        // `bb0` sets `%1`; `bb1`, entered only from it, hands it on
        let mut f = arm(Vec::new());
        let hand = f.blocks[0].insts.remove(1);
        f.blocks[1].insts.push(hand);
        let stats = run(&mut f);
        assert_eq!(
            stats,
            ConstCopyFwdStats {
                copies_forwarded: 1,
                sets_removed: 1
            }
        );
        assert!(f.blocks[0].insts.is_empty());
        // a second way into `bb1` stops it
        f = arm(Vec::new());
        let hand = f.blocks[0].insts.remove(1);
        f.blocks[1].insts.push(hand);
        f.blocks[0].term = Terminator::CondBr {
            cond: v(0),
            then_blk: BlockId(1),
            else_blk: BlockId(1),
        };
        assert_eq!(run(&mut f), ConstCopyFwdStats::default());
    }

    #[test]
    fn a_redefinition_stops_the_forwarding() {
        let redef = i(1, InstKind::Copy(Type::I64, v(0)));
        let read = i(3, InstKind::Copy(Type::I64, v(1)));
        let mut f = arm(vec![redef, read.clone()]);
        run(&mut f);
        assert_eq!(f.blocks[0].insts.last(), Some(&read));
    }
}
