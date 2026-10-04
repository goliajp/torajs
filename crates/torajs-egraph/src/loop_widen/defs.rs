//! Where each value is defined — the widening pass asks for the one
//! def of a loop's data pointer, array and hoisted constants.

use std::collections::HashMap;

use torajs_core::ssa::{Function, Inst, InstKind, Operand, ValueId};

pub(super) struct DefSites {
    /// `(block, index)` of every def, in block order; a multi-def
    /// cell has several.
    sites: HashMap<ValueId, Vec<(u32, u32)>>,
}

impl DefSites {
    pub(super) fn of(func: &Function) -> Self {
        let mut sites: HashMap<ValueId, Vec<(u32, u32)>> = HashMap::new();
        for (b, block) in func.blocks.iter().enumerate() {
            for (i, inst) in block.insts.iter().enumerate() {
                if let Some(r) = inst.result {
                    sites.entry(r).or_default().push((b as u32, i as u32));
                }
            }
        }
        Self { sites }
    }

    pub(super) fn single_def<'f>(&self, func: &'f Function, v: ValueId) -> Option<&'f Inst> {
        match self.sites.get(&v)?[..] {
            [(b, i)] => Some(&func.blocks[b as usize].insts[i as usize]),
            _ => None,
        }
    }

    pub(super) fn has_def(&self, v: ValueId) -> bool {
        self.sites.contains_key(&v)
    }

    /// An i64 constant, inline or hoisted into a single-def copy.
    pub(super) fn const_i64(&self, func: &Function, op: &Operand) -> Option<i64> {
        match op {
            Operand::ConstI64(c) => Some(*c),
            Operand::Value(v) => match self.single_def(func, *v)?.kind {
                InstKind::Copy(_, Operand::ConstI64(c)) => Some(c),
                _ => None,
            },
            _ => None,
        }
    }
}
