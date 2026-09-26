//! Frequency-aware spill weights for the Linear Scan allocator.
//!
//! The Poletto & Sarkar "spill at active" heuristic picks the
//! furthest-*end* interval as the spill victim. That maximises the
//! freed-register window but is blind to how *hot* the victim is:
//! a loop-carried accumulator (used every iteration of a 100M-step
//! loop) and a cold prelude value that is merely rc_dec'd at
//! function exit can have near-identical `end`s, and the accumulator
//! loses the coin toss — its spill slot then costs a store→load
//! forward round-trip on every loop step (keeping popcount's
//! accumulator in a register measured -6% wall, collatz -2.9%).
//!
//! The textbook antidote: per-value **spill weight** = Σ over each
//! static use (and the def) of the estimated execution frequency of
//! its block, from `torajs_core::ssa::block_freq` (loop depth, with
//! recognized cold arms such as throw paths and grow calls scaled
//! down). The allocator's
//! victim scan prefers the *lowest* weight (fewest dynamic uses lost
//! to memory), tie-breaking on the furthest-end rule so weight-flat
//! functions keep the exact Poletto & Sarkar behaviour. The same
//! per-slot frequencies price the save / reload around a call.
//!
//! A loop level stands for 16 iterations: a per-level factor of 4 let
//! a chatty prelude value (one use per Error-class registration call,
//! ten or more mentions) outweigh a loop accumulator with two
//! mentions — popcount's `total` lost the victim scan to values that
//! run once per process. Frequencies come from the dominator tree's
//! natural loops, so a cold arm that layout sank past its loop's
//! latch is not mistaken for another loop level.

use std::collections::HashMap;

use torajs_core::ssa::Function;
use torajs_core::ssa::block_freq::block_freqs;

use crate::liveness::{visit_inst_operands, visit_terminator_operands};

/// `ValueId.0 → spill weight` for `func`, and the per-slot frequencies
/// it was built from, over the global inst-slot numbering of
/// `liveness::compute_intervals` (insts then one terminator slot per
/// block).
pub fn compute_spill_weights(func: &Function) -> (HashMap<u32, u64>, Vec<u64>) {
    let units = slot_units(func);
    // accumulate the slot's unit per static use (operand reads AND the
    // defining slot itself, so a value defined in a loop counts its
    // def-site store too)
    let mut weights: HashMap<u32, u64> = HashMap::new();
    let mut idx: usize = 0;
    for b in &func.blocks {
        for inst in &b.insts {
            let unit = units[idx];
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
        let unit = units[idx];
        visit_terminator_operands(&b.term, |v| {
            let w = weights.entry(v.0).or_insert(0);
            *w = w.saturating_add(unit);
        });
        idx += 1;
    }
    (weights, units)
}

/// Per global slot, the estimated execution frequency of its block.
fn slot_units(func: &Function) -> Vec<u64> {
    let freqs = block_freqs(func);
    func.blocks
        .iter()
        .flat_map(|b| std::iter::repeat_n(freqs[b.id.0 as usize], b.insts.len() + 1))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::block_freq::ENTRY_FREQ as E;
    use torajs_core::ssa::{
        BinOp, Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId,
        ValueInfo,
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

    /// Straight-line function: every use weighs one entry.
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
        let (w, _) = compute_spill_weights(&f);
        // v0: def + 2 operand uses = 3; v1: def + ret use = 2.
        assert_eq!(w[&0], 3 * E);
        assert_eq!(w[&1], 2 * E);
    }

    /// Loop-body uses weigh 16 entries; a value only touched outside
    /// stays at one entry per use — the loop-carried value outweighs the cold one even
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
        let (w, _) = compute_spill_weights(&f);
        // v0: cold def (1) + 2 loop uses (16+16) + cold ret use (1) = 34.
        assert_eq!(w[&0], 34 * E);
        // v1: loop def (16) + loop condbr use (16) = 32.
        assert_eq!(w[&1], 32 * E);
    }

    /// Nested loops multiply: depth-2 uses weigh 256.
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
        let (w, _) = compute_spill_weights(&f);
        // v1 def + condbr use both at depth 2 (inner self-loop inside
        // outer bb1..bb3 loop): 256 + 256 = 512.
        assert_eq!(w[&1], 512 * E);
        // v0: def at depth 1 (16) + 2 uses at depth 2 (256+256) +
        // condbr use at depth 1 (16) = 544.
        assert_eq!(w[&0], 544 * E);
    }

    /// An arm placed after its loop's latch branches backwards in
    /// block order without being a loop; it sits one loop level deep,
    /// not two.
    #[test]
    fn a_sunk_arm_is_not_another_loop_level() {
        // bb0: br bb1
        // bb1 (hdr): condbr v9 ? bb2 : bb5
        // bb2: condbr v9 ? bb4 : bb3
        // bb3 (latch): br bb1
        // bb4 (sunk arm): v0 = 1+2 ; br bb3
        // bb5: ret
        let cond = |t: u32, e: u32| Terminator::CondBr {
            cond: Operand::Value(ValueId(9)),
            then_blk: BlockId(t),
            else_blk: BlockId(e),
        };
        let blk = |id: u32, insts: Vec<Inst>, term: Terminator| Block {
            id: BlockId(id),
            insts,
            term,
        };
        let f = mk_func(
            vec![
                blk(0, vec![], Terminator::Br(BlockId(1))),
                blk(1, vec![], cond(2, 5)),
                blk(2, vec![], cond(4, 3)),
                blk(3, vec![], Terminator::Br(BlockId(1))),
                blk(
                    4,
                    vec![add_inst(0, Operand::ConstI64(1), Operand::ConstI64(2))],
                    Terminator::Br(BlockId(3)),
                ),
                blk(5, vec![], Terminator::Ret(None)),
            ],
            10,
        );
        let (w, _) = compute_spill_weights(&f);
        // one loop level, not two
        assert_eq!(w[&0], 16 * E);
    }
}
