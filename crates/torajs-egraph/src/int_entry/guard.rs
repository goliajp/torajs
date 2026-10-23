//! The check chain in front of a versioned loop: one pass through it
//! decides, for every f64 bound the loop compares its counter with,
//! whether the comparison can be made between integers.
//!
//! For an integer `i` and a real `n`, `i < n` is `i < ceil(n)` and
//! `i <= n` is `i <= floor(n)`. So the bound need not be an integer,
//! only a number — NaN compares false with everything, and rounds to
//! 0 — whose rounding lands inside the window. The window stays
//! inside 2^53 on both sides: at 2^53 itself `sitofp` has started
//! rounding the counter, and the f64 comparison the slow loop keeps
//! is no longer the integer one.
//!
//! A failed check leaves for the loop as it was; the chain falls
//! through into the copy.
//!
//! A bound the loop also reads as a value, or a value it only reads,
//! is not rounded: it passes when truncating it and converting back
//! gives the same bits — which NaN, -0, a fraction, an infinity and
//! anything past 2^63 do not — and the copy reads the converted
//! integer back as an f64 wherever it read the value. That f64 is
//! bit-identical to the value on the passing path, and it is a
//! `sitofp` of an integer with a fact, which is what every consumer
//! downstream wants to see.
//!
//! Each rounded bound leaves the chain through a `copy`. The window
//! comparisons refine the rounded value only along the passing path;
//! the copy is evaluated on that path, so what it stores is the
//! refined interval, and the loop reads it as the copy's own.

use crate::id_hash::HashMap;

use torajs_core::ssa::{
    BinOp, Block, BlockId, FPred, Function, IPred, Inst, InstKind, Operand, Terminator, Type,
    ValueId,
};

use crate::float_demote::push_value;

/// Which way a bound rounds to become the integer comparand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Round {
    Ceil,
    Floor,
    /// the value itself, when it is an integer
    Exact,
}

/// One rounded bound the chain must produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Need {
    pub(crate) source: ValueId,
    pub(crate) round: Round,
    pub(crate) lo: i64,
    pub(crate) hi: i64,
}

struct Chain<'f> {
    func: &'f mut Function,
    slow: BlockId,
    /// the block being filled; its terminator is set when it closes
    open: BlockId,
    checks: Vec<(ValueId, bool)>,
}

impl Chain<'_> {
    fn new_block(&mut self) -> BlockId {
        let id = BlockId(self.func.blocks.len() as u32);
        self.func.blocks.push(Block {
            id,
            insts: Vec::new(),
            term: Terminator::Unreachable,
        });
        id
    }

    fn emit(&mut self, ty: Type, kind: InstKind) -> Operand {
        let result = push_value(&mut self.func.values, ty);
        self.func.blocks[self.open.0 as usize].insts.push(Inst {
            result: Some(result),
            kind,
            origin: None,
        });
        Operand::Value(result)
    }

    /// Close the open block on `cond`: the slow loop when it holds
    /// (or, with `pass_when`, when it does not), a fresh block
    /// otherwise.
    fn leave_when(&mut self, cond: Operand, pass_when: bool) {
        let next = self.new_block();
        let (then_blk, else_blk) = if pass_when {
            (next, self.slow)
        } else {
            (self.slow, next)
        };
        if let Operand::Value(v) = cond {
            self.checks.push((v, !pass_when));
        }
        self.func.blocks[self.open.0 as usize].term = Terminator::CondBr {
            cond,
            then_blk,
            else_blk,
        };
        self.open = next;
    }
}

pub(crate) struct Built {
    pub(crate) head: BlockId,
    /// the last block, entered only when every check passed
    pub(crate) pass: BlockId,
    pub(crate) rounded: HashMap<(ValueId, Round), ValueId>,
    /// source → the f64 the copy reads in its place: `sitofp` of the
    /// exact integer's `copy`
    pub(crate) values: HashMap<ValueId, ValueId>,
    /// each check's condition, in chain order, and whether the slow
    /// loop is its `then` side
    pub(crate) checks: Vec<(ValueId, bool)>,
}

/// Build the chain; `needs` holds each (source, round) once.
pub(crate) fn build_chain(
    func: &mut Function,
    needs: &[Need],
    slow: BlockId,
    fast: BlockId,
) -> Built {
    let mut chain = Chain {
        func,
        slow,
        open: BlockId(0),
        checks: Vec::new(),
    };
    let head = chain.new_block();
    chain.open = head;
    let mut rounded: Vec<(ValueId, Round, Operand)> = Vec::new();
    let mut converted: HashMap<ValueId, (Operand, Operand)> = HashMap::default();
    for need in needs {
        let v = Operand::Value(need.source);
        let (truncated, back) = match converted.get(&need.source) {
            Some(tb) => *tb,
            None => {
                let ordered = chain.emit(Type::Bool, InstKind::FCmp(FPred::Oeq, v, v));
                chain.leave_when(ordered, true);
                let t = chain.emit(Type::I64, InstKind::FpToSi(v));
                let b = chain.emit(Type::F64, InstKind::SiToFp(t));
                converted.insert(need.source, (t, b));
                (t, b)
            }
        };
        let value = match need.round {
            Round::Exact => {
                let bits = chain.emit(Type::I64, InstKind::BitCastF64ToI64(v));
                let back_bits = chain.emit(Type::I64, InstKind::BitCastF64ToI64(back));
                let differs = chain.emit(Type::Bool, InstKind::ICmp(IPred::Ne, bits, back_bits));
                chain.leave_when(differs, false);
                truncated
            }
            // truncation went toward zero: a bound with a fraction sits
            // above it when positive, below it when negative
            Round::Ceil | Round::Floor => {
                let (pred, op) = match need.round {
                    Round::Ceil => (FPred::Olt, BinOp::Add),
                    _ => (FPred::Ogt, BinOp::Sub),
                };
                let off = chain.emit(Type::Bool, InstKind::FCmp(pred, back, v));
                let off = chain.emit(Type::I64, InstKind::ZExtBoolToI64(off));
                chain.emit(Type::I64, InstKind::BinOp(op, truncated, off))
            }
        };
        // both ends, always: a saturated truncation wraps on the
        // adjustment and lands at the opposite end
        let above = chain.emit(
            Type::Bool,
            InstKind::ICmp(IPred::Sgt, value, Operand::ConstI64(need.hi)),
        );
        chain.leave_when(above, false);
        let below = chain.emit(
            Type::Bool,
            InstKind::ICmp(IPred::Slt, value, Operand::ConstI64(need.lo)),
        );
        chain.leave_when(below, false);
        rounded.push((need.source, need.round, value));
    }
    let mut out = HashMap::default();
    let mut values = HashMap::default();
    for (source, round, value) in rounded {
        let Operand::Value(pi) = chain.emit(Type::I64, InstKind::Copy(Type::I64, value)) else {
            unreachable!("emit answers a value");
        };
        out.insert((source, round), pi);
        if round == Round::Exact {
            let Operand::Value(vs) = chain.emit(Type::F64, InstKind::SiToFp(Operand::Value(pi)))
            else {
                unreachable!("emit answers a value");
            };
            values.insert(source, vs);
        }
    }
    let pass = chain.open;
    chain.func.blocks[pass.0 as usize].term = Terminator::Br(fast);
    Built {
        head,
        pass,
        rounded: out,
        values,
        checks: chain.checks,
    }
}
