//! Which values of a function are a primitive on every definition:
//! the stored value of a run-time keyed write that cannot install a
//! ToPrimitive hook ([`crate::cmd_build_dispatch_judge_coerce_hooks`]).

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{
    BinOp, DOUBLE_ENCODE_OFFSET, FPred, Function, InstKind, Module, Operand, TAG_TYPE_NUMBER,
    Terminator, ValueId,
};

/// Entries whose answer is always a primitive: the `+` of two decoded
/// operands answers a number, a string or a bigint.
const PRIMITIVE_ANSWERS: [&str; 1] = ["__torajs_anyv_add_pair"];

/// Every definition of each value of a function: the IR keeps a
/// φ-merged value as one id defined by a `copy` on each predecessor.
pub(crate) struct Defs {
    defs: HashMap<ValueId, Vec<(usize, usize)>>,
}

impl Defs {
    pub(crate) fn of(f: &Function) -> Self {
        let mut defs: HashMap<ValueId, Vec<(usize, usize)>> = HashMap::new();
        for (bi, b) in f.blocks.iter().enumerate() {
            for (ii, inst) in b.insts.iter().enumerate() {
                if let Some(r) = inst.result {
                    defs.entry(r).or_default().push((bi, ii));
                }
            }
        }
        Self { defs }
    }

    /// True when `v` is a primitive on every definition.
    pub(crate) fn primitive(&self, module: &Module, f: &Function, v: &Operand) -> bool {
        let Operand::Value(v) = v else {
            return false;
        };
        let mut seen = HashSet::new();
        self.primitive_value(module, f, *v, &mut seen)
    }

    /// A copy cycle adds no value of its own, so a value already on
    /// the walk answers true.
    fn primitive_value(
        &self,
        module: &Module,
        f: &Function,
        v: ValueId,
        seen: &mut HashSet<ValueId>,
    ) -> bool {
        if !seen.insert(v) {
            return true;
        }
        let Some(sites) = self.defs.get(&v) else {
            return false;
        };
        sites.iter().all(|&(bi, ii)| {
            match &f.blocks[bi].insts[ii].kind {
                InstKind::Copy(_, Operand::Value(src)) => {
                    self.primitive_value(module, f, *src, seen)
                }
                // tag 4 is the heap pointer; every other tag encodes a
                // primitive (a double through the canonicalizing box)
                InstKind::BoxAny(tag, _) => *tag != 4,
                InstKind::BoxInt32(_) | InstKind::BoxF64(_) => true,
                InstKind::IntToPtr(bits) => self.number_bits(f, bits),
                InstKind::Call(fid, _) => {
                    PRIMITIVE_ANSWERS.contains(&module.funcs[fid.0 as usize].name.as_str())
                }
                _ => false,
            }
        })
    }

    /// True when `bits` encode a number: a
    /// constant outside the cell range, an int32 under the number tag,
    /// or a double's bits plus the encode offset on the arm where the
    /// double is not a NaN (only a NaN payload wraps into the cell
    /// range, which is why the box tests for one).
    fn number_bits(&self, f: &Function, bits: &Operand) -> bool {
        const TAG: i64 = TAG_TYPE_NUMBER as i64;
        const OFFSET: i64 = DOUBLE_ENCODE_OFFSET as i64;
        let v = match bits {
            Operand::ConstI64(c) => return (*c as u64) & TAG_TYPE_NUMBER != 0,
            Operand::Value(v) => *v,
            _ => return false,
        };
        let Some(&[(bi, ii)]) = self.defs.get(&v).map(Vec::as_slice) else {
            return false;
        };
        match &f.blocks[bi].insts[ii].kind {
            InstKind::BinOp(BinOp::Or, _, Operand::ConstI64(t)) => t & TAG == TAG,
            InstKind::BinOp(BinOp::Add, Operand::Value(raw), Operand::ConstI64(OFFSET)) => {
                self.not_nan_arm(f, bi, *raw)
            }
            _ => false,
        }
    }

    /// True when `raw` is `bitcast` of a double that block `bi` holds
    /// only on the false arm of that double's `f != f` test.
    fn not_nan_arm(&self, f: &Function, bi: usize, raw: ValueId) -> bool {
        let Some(&[(rb, ri)]) = self.defs.get(&raw).map(Vec::as_slice) else {
            return false;
        };
        let InstKind::BitCastF64ToI64(Operand::Value(x)) = &f.blocks[rb].insts[ri].kind else {
            return false;
        };
        let id = f.blocks[bi].id;
        let mut preds = f.blocks.iter().filter(|b| match &b.term {
            Terminator::Br(t) => *t == id,
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => *then_blk == id || *else_blk == id,
            _ => false,
        });
        let (Some(p), None) = (preds.next(), preds.next()) else {
            return false;
        };
        let Terminator::CondBr {
            cond: Operand::Value(c),
            then_blk,
            else_blk,
        } = &p.term
        else {
            return false;
        };
        if *else_blk != id || *then_blk == id {
            return false;
        }
        let Some(&[(cb, ci)]) = self.defs.get(c).map(Vec::as_slice) else {
            return false;
        };
        let nan_test = Operand::Value(*x);
        matches!(
            &f.blocks[cb].insts[ci].kind,
            InstKind::FCmp(FPred::Une, a, b) if *a == nan_test && *b == nan_test
        )
    }
}
