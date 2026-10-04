//! Appending blocks and instructions to a function under construction.

use torajs_core::ssa::{
    BinOp, Block, BlockId, Function, IPred, Inst, InstKind, Operand, Terminator, Type, ValueId,
    ValueInfo, VecInst, VecOp, VecShape,
};

use super::guard::Test;

pub(super) const VEC: Type = Type::Vec(VecShape::I64x2);

pub(super) struct Emit<'f> {
    pub func: &'f mut Function,
    pub block: BlockId,
}

impl Emit<'_> {
    pub fn new_block(&mut self) -> BlockId {
        let id = BlockId(self.func.blocks.len() as u32);
        self.func.blocks.push(Block {
            id,
            insts: Vec::new(),
            term: Terminator::Unreachable,
        });
        id
    }

    pub fn at(&mut self, b: BlockId) {
        self.block = b;
    }

    pub fn def(&mut self, r: ValueId, kind: InstKind) {
        self.func.blocks[self.block.0 as usize].insts.push(Inst {
            result: Some(r),
            kind,
            origin: None,
        });
    }

    pub fn value(&mut self, ty: Type) -> ValueId {
        let id = ValueId(self.func.values.len() as u32);
        self.func.values.push(ValueInfo { ty, name: None });
        id
    }

    pub fn push(&mut self, kind: InstKind, ty: Type) -> Operand {
        let r = self.value(ty);
        self.def(r, kind);
        Operand::Value(r)
    }

    pub fn bin(&mut self, op: BinOp, a: Operand, b: Operand) -> Operand {
        self.push(InstKind::BinOp(op, a, b), Type::I64)
    }

    pub fn icmp(&mut self, pred: IPred, a: Operand, b: Operand) -> Operand {
        self.push(InstKind::ICmp(pred, a, b), Type::Bool)
    }

    pub fn vec(&mut self, vi: VecInst) -> Operand {
        self.push(InstKind::Vec(vi), VEC)
    }

    pub fn vbin(&mut self, op: VecOp, a: Operand, b: Operand) -> Operand {
        self.vec(VecInst::BinOp(op, a, b))
    }

    pub fn copy(&mut self, cell: ValueId, ty: Type, src: Operand) {
        self.def(cell, InstKind::Copy(ty, src));
    }

    pub fn term(&mut self, t: Terminator) {
        self.func.blocks[self.block.0 as usize].term = t;
    }

    pub fn cond_br(&mut self, cond: Operand, then_blk: BlockId, else_blk: BlockId) {
        self.term(Terminator::CondBr {
            cond,
            then_blk,
            else_blk,
        });
    }

    /// The bool that holds when `v` fails `test`.
    pub fn violates(&mut self, v: Operand, test: Test) -> Operand {
        match test {
            Test::HighBits(k) => {
                let high = self.bin(BinOp::LShr, v, Operand::ConstI64(k));
                self.icmp(IPred::Ne, high, Operand::ConstI64(0))
            }
            Test::Above(c) => self.icmp(IPred::Sgt, v, Operand::ConstI64(c)),
            Test::Below(c) => self.icmp(IPred::Slt, v, Operand::ConstI64(c)),
        }
    }

    pub fn any(&mut self, conds: Vec<Operand>) -> Operand {
        let mut it = conds.into_iter();
        let first = it.next().expect("at least one check");
        it.fold(first, |acc, c| {
            self.push(InstKind::BinOp(BinOp::Or, acc, c), Type::Bool)
        })
    }

    /// Fold `vs` pairwise with `op`, then into one i64.
    pub fn reduce(&mut self, op: VecOp, vs: &[Operand]) -> Operand {
        let mut level: Vec<Operand> = vs.to_vec();
        while level.len() > 1 {
            level = level
                .chunks(2)
                .map(|p| match p {
                    [a, b] => self.vbin(op, *a, *b),
                    [a] => *a,
                    _ => unreachable!(),
                })
                .collect();
        }
        self.push(InstKind::Vec(VecInst::Reduce(op, level[0])), Type::I64)
    }
}
