//! Backward liveness over a function: the values live into each
//! block. A side exit writes back what its target reads.
//!
//! One bit per value and block: a block's reads before its own writes
//! and its writes are taken once, and the rounds only or words
//! together until nothing changes.

use torajs_core::ssa::{BlockId, Function, ValueId};

use crate::float_demote::cfg::{successors, term_operand_value};
use crate::rc_peephole::visit_value_operands;

pub(super) struct LiveIn {
    words: usize,
    bits: Vec<u64>,
}

impl LiveIn {
    /// The values live into `b`, in id order.
    pub(super) fn of(&self, b: BlockId) -> Vec<ValueId> {
        let row = &self.bits[b.0 as usize * self.words..][..self.words];
        let mut out = Vec::new();
        for (wi, &word) in row.iter().enumerate() {
            let mut w = word;
            while w != 0 {
                out.push(ValueId((wi * 64) as u32 + w.trailing_zeros()));
                w &= w - 1;
            }
        }
        out
    }
}

/// Live-in sets of a function whose block ids are their indices.
pub(super) fn live_in(func: &Function) -> LiveIn {
    let words = func.values.len().div_ceil(64);
    let n = func.blocks.len();
    let set = |row: &mut [u64], v: ValueId| row[v.0 as usize / 64] |= 1 << (v.0 % 64);
    let has = |row: &[u64], v: ValueId| row[v.0 as usize / 64] & (1 << (v.0 % 64)) != 0;
    // per block: what it reads before writing, and what it writes
    let mut reads = vec![0u64; n * words];
    let mut writes = vec![0u64; n * words];
    for (bi, block) in func.blocks.iter().enumerate() {
        let r = &mut reads[bi * words..][..words];
        let w = &mut writes[bi * words..][..words];
        for inst in &block.insts {
            visit_value_operands(&inst.kind, |v| {
                if !has(w, v) {
                    set(r, v);
                }
            });
            if let Some(d) = inst.result {
                set(w, d);
            }
        }
        if let Some(v) = term_operand_value(&block.term)
            && !has(w, v)
        {
            set(r, v);
        }
    }
    let mut bits = reads.clone();
    let mut out = vec![0u64; words];
    let mut changed = true;
    while changed {
        changed = false;
        for bi in (0..n).rev() {
            out.fill(0);
            for s in successors(&func.blocks[bi].term) {
                let row = &bits[s.0 as usize * words..][..words];
                for (o, x) in out.iter_mut().zip(row) {
                    *o |= x;
                }
            }
            let at = bi * words;
            for k in 0..words {
                let v = reads[at + k] | (out[k] & !writes[at + k]);
                if v != bits[at + k] {
                    bits[at + k] = v;
                    changed = true;
                }
            }
        }
    }
    LiveIn { words, bits }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{
        BinOp, Block, IPred, Inst, InstKind, Operand, Terminator, Type, ValueInfo,
    };

    fn inst(r: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(r)),
            kind,
            origin: None,
        }
    }

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    /// `i = 0; while (i < n) i = i + 1; return i` with `n` the param %3:
    /// the loop cell %0 is live round the loop and out of it, the bound
    /// only while the loop runs, and a value defined and read in one
    /// block is live into none.
    #[test]
    fn a_loop_cell_is_live_round_the_loop_and_past_it() {
        let block = |id: u32, insts: Vec<Inst>, term: Terminator| Block {
            id: BlockId(id),
            insts,
            term,
        };
        let f = Function {
            name: "f".into(),
            params: vec![ValueId(3)],
            ret: Type::I64,
            blocks: vec![
                block(
                    0,
                    vec![inst(0, InstKind::Copy(Type::I64, Operand::ConstI64(0)))],
                    Terminator::Br(BlockId(1)),
                ),
                block(
                    1,
                    vec![inst(1, InstKind::ICmp(IPred::Slt, v(0), v(3)))],
                    Terminator::CondBr {
                        cond: v(1),
                        then_blk: BlockId(2),
                        else_blk: BlockId(3),
                    },
                ),
                block(
                    2,
                    vec![
                        inst(2, InstKind::BinOp(BinOp::Add, v(0), Operand::ConstI64(1))),
                        inst(0, InstKind::Copy(Type::I64, v(2))),
                    ],
                    Terminator::Br(BlockId(1)),
                ),
                block(3, vec![], Terminator::Ret(Some(v(0)))),
            ],
            values: [Type::I64, Type::Bool, Type::I64, Type::I64]
                .iter()
                .map(|ty| ValueInfo {
                    ty: *ty,
                    name: None,
                })
                .collect(),
            current_origin: None,
        };
        let live = live_in(&f);
        let ids = |b: u32| -> Vec<u32> { live.of(BlockId(b)).iter().map(|v| v.0).collect() };
        assert_eq!(ids(0), vec![3]);
        assert_eq!(ids(1), vec![0, 3]);
        assert_eq!(ids(2), vec![0, 3]);
        assert_eq!(ids(3), vec![0]);
    }
}
