//! Loop-aware spill weights for the Linear Scan allocator — Round 5
//! popcount attack #1 (RFC 20260703-perf-popcount-regalloc).
//!
//! The Poletto & Sarkar "spill at active" heuristic picks the
//! furthest-*end* interval as the spill victim. That maximises the
//! freed-register window but is blind to how *hot* the victim is:
//! a loop-carried accumulator (used every iteration of a 100M-step
//! loop) and a cold prelude value that is merely rc_dec'd at
//! function exit can have near-identical `end`s, and the accumulator
//! loses the coin toss — its spill slot then costs a store→load
//! forward round-trip on every loop step (popcount fixture disasm
//! evidence in the RFC; keeping the accumulator in a register
//! measured -6% wall on popcount and -2.9% on collatz vs the
//! same-HEAD baseline).
//!
//! This module computes the textbook antidote: per-value **spill
//! weight** = Σ over each static use `4^min(loop_depth, 4)`. The
//! allocator's victim scan then prefers the *lowest* weight (fewest
//! dynamic uses lost to memory), tie-breaking on the original
//! furthest-end rule so weight-flat functions keep the exact
//! Poletto & Sarkar behaviour.
//!
//! Loop detection is the standard reducible-CFG backedge scan over
//! the lowered block order: a terminator edge from block `i` to a
//! block `j` at an earlier (or equal — self-loop) position marks the
//! index range `[start(j), end(i)]` as one loop nest level. The
//! lowerer emits natural `header → body → back-to-header` shapes, so
//! positional containment is exact for everything it produces; an
//! irreducible shape would merely misweight (never miscompile —
//! weights only steer victim choice).

use std::collections::HashMap;

use torajs_core::ssa::{Function, Terminator};

use crate::liveness::{visit_inst_operands, visit_terminator_operands};

/// Depth cap — 4^4 = 256 per use keeps the weight far away from u32
/// overflow even for pathological use counts, while still separating
/// quadruply-nested hot code from anything colder.
const DEPTH_CAP: u32 = 4;

/// Compute `ValueId.0 → spill weight` for `func` over the same
/// global inst-slot numbering as `liveness::compute_intervals`
/// (insts then one terminator slot per block).
pub fn compute_spill_weights(func: &Function) -> HashMap<u32, u32> {
    // Pass 1 — block index ranges + backedge-derived loop depth per
    // global slot index.
    let n = func.blocks.len();
    let mut block_pos: HashMap<u32, usize> = HashMap::with_capacity(n);
    for (i, b) in func.blocks.iter().enumerate() {
        block_pos.insert(b.id.0, i);
    }
    let mut block_start: Vec<u32> = vec![0; n];
    let mut block_end: Vec<u32> = vec![0; n]; // inclusive of the terminator slot
    let mut cursor: u32 = 0;
    for (i, b) in func.blocks.iter().enumerate() {
        block_start[i] = cursor;
        cursor += b.insts.len() as u32 + 1;
        block_end[i] = cursor - 1;
    }
    let total_slots = cursor as usize;
    let mut depth: Vec<u32> = vec![0; total_slots.max(1)];
    for (i, b) in func.blocks.iter().enumerate() {
        let mark = |target: u32, depth: &mut Vec<u32>| {
            if let Some(&j) = block_pos.get(&target)
                && j <= i
            {
                // Backedge i → j: everything in [start(j), end(i)]
                // sits one loop level deeper.
                for d in depth
                    .iter_mut()
                    .take(block_end[i] as usize + 1)
                    .skip(block_start[j] as usize)
                {
                    *d += 1;
                }
            }
        };
        match &b.term {
            Terminator::Br(t) => mark(t.0, &mut depth),
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => {
                mark(then_blk.0, &mut depth);
                mark(else_blk.0, &mut depth);
            }
            Terminator::Ret(_) | Terminator::Unreachable => {}
        }
    }

    // Pass 2 — accumulate 4^min(depth,cap) per static use (operand
    // reads AND the defining slot itself, so a value defined in a
    // loop counts its def-site store too).
    let mut weights: HashMap<u32, u32> = HashMap::new();
    let mut idx: u32 = 0;
    for b in &func.blocks {
        for inst in &b.insts {
            let d = depth[idx as usize].min(DEPTH_CAP);
            let unit = 1u32 << (2 * d);
            visit_inst_operands(&inst.kind, |v| {
                let w = weights.entry(v.0).or_insert(0);
                *w = w.saturating_add(unit);
            });
            if let Some(r) = inst.result {
                let w = weights.entry(r.0).or_insert(0);
                *w = w.saturating_add(unit);
            }
            idx += 1;
        }
        let d = depth[idx as usize].min(DEPTH_CAP);
        let unit = 1u32 << (2 * d);
        visit_terminator_operands(&b.term, |v| {
            let w = weights.entry(v.0).or_insert(0);
            *w = w.saturating_add(unit);
        });
        idx += 1;
    }
    weights
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{
        BinOp, Block, BlockId, Function, Inst, InstKind, Operand, Type, ValueId, ValueInfo,
    };

    fn add_inst(result: u32, a: Operand, b: Operand) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind: InstKind::BinOp(BinOp::Add, a, b),
            origin: None,
        }
    }

    fn mk_func(blocks: Vec<Block>, n_values: usize) -> Function {
        Function {
            name: "t".into(),
            params: Vec::new(),
            ret: Type::I64,
            values: (0..n_values)
                .map(|_| ValueInfo {
                    ty: Type::I64,
                    name: None,
                })
                .collect(),
            blocks,
            current_origin: None,
        }
    }

    /// Straight-line function: every use weighs 1.
    #[test]
    fn flat_function_counts_uses() {
        let f = mk_func(
            vec![Block {
                id: BlockId(0),
                insts: vec![
                    add_inst(0, Operand::ConstI64(1), Operand::ConstI64(2)),
                    add_inst(1, Operand::Value(ValueId(0)), Operand::Value(ValueId(0))),
                ],
                term: Terminator::Ret(Some(Operand::Value(ValueId(1)))),
            }],
            2,
        );
        let w = compute_spill_weights(&f);
        // v0: def + 2 operand uses = 3; v1: def + ret use = 2.
        assert_eq!(w[&0], 3);
        assert_eq!(w[&1], 2);
    }

    /// Loop-body uses weigh 4^1; a value only touched outside stays
    /// at 1/use — the loop-carried value outweighs the cold one even
    /// with fewer static mentions.
    #[test]
    fn loop_uses_outweigh_cold_uses() {
        // bb0: v0 = 1+2                                 (cold def)
        // bb1: v1 = v0+v0 ; condbr v1 ? bb1 : bb2       (self-loop)
        // bb2: ret v0
        let f = mk_func(
            vec![
                Block {
                    id: BlockId(0),
                    insts: vec![add_inst(0, Operand::ConstI64(1), Operand::ConstI64(2))],
                    term: Terminator::Br(BlockId(1)),
                },
                Block {
                    id: BlockId(1),
                    insts: vec![add_inst(
                        1,
                        Operand::Value(ValueId(0)),
                        Operand::Value(ValueId(0)),
                    )],
                    term: Terminator::CondBr {
                        cond: Operand::Value(ValueId(1)),
                        then_blk: BlockId(1),
                        else_blk: BlockId(2),
                    },
                },
                Block {
                    id: BlockId(2),
                    insts: vec![],
                    term: Terminator::Ret(Some(Operand::Value(ValueId(0)))),
                },
            ],
            2,
        );
        let w = compute_spill_weights(&f);
        // v0: cold def (1) + 2 loop uses (4+4) + cold ret use (1) = 10.
        assert_eq!(w[&0], 10);
        // v1: loop def (4) + loop condbr use (4) = 8.
        assert_eq!(w[&1], 8);
    }

    /// Nested loops multiply: depth-2 uses weigh 16.
    #[test]
    fn nested_loop_depth_compounds() {
        // bb0: br bb1
        // bb1 (outer hdr): v0 = 1+2 ; br bb2
        // bb2 (inner):     v1 = v0+v0 ; condbr v1 ? bb2 : bb3
        // bb3: condbr v0 ? bb1 : bb4
        // bb4: ret
        let f = mk_func(
            vec![
                Block {
                    id: BlockId(0),
                    insts: vec![],
                    term: Terminator::Br(BlockId(1)),
                },
                Block {
                    id: BlockId(1),
                    insts: vec![add_inst(0, Operand::ConstI64(1), Operand::ConstI64(2))],
                    term: Terminator::Br(BlockId(2)),
                },
                Block {
                    id: BlockId(2),
                    insts: vec![add_inst(
                        1,
                        Operand::Value(ValueId(0)),
                        Operand::Value(ValueId(0)),
                    )],
                    term: Terminator::CondBr {
                        cond: Operand::Value(ValueId(1)),
                        then_blk: BlockId(2),
                        else_blk: BlockId(3),
                    },
                },
                Block {
                    id: BlockId(3),
                    insts: vec![],
                    term: Terminator::CondBr {
                        cond: Operand::Value(ValueId(0)),
                        then_blk: BlockId(1),
                        else_blk: BlockId(4),
                    },
                },
                Block {
                    id: BlockId(4),
                    insts: vec![],
                    term: Terminator::Ret(None),
                },
            ],
            2,
        );
        let w = compute_spill_weights(&f);
        // v1 def + condbr use both at depth 2 (inner self-loop inside
        // outer bb1..bb3 loop): 16 + 16 = 32.
        assert_eq!(w[&1], 32);
        // v0: def at depth 1 (4) + 2 uses at depth 2 (16+16) + condbr
        // use at depth 1 (4) = 40.
        assert_eq!(w[&0], 40);
    }
}
