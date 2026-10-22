//! Array-field header loads read once per call — the method twin of
//! `load_licm`. A recursive-descent parser reads `this.toks[this.pos]`
//! in every loop test, and every read loads the field, the array's
//! length and its data pointer again, because a `Call` sits between
//! two reads and nothing tells the egraph the call left them alone.
//!
//! `Module::pins` says it: the field is written by the constructor
//! alone, each member of the cluster hands its own receiver on to the
//! others, and nothing that runs while a member is on the stack grows,
//! shrinks or reallocates any array. So within one call of a member,
//! `this.<field>`, its length (+8) and its data pointer (+32) are the
//! same on every read. Each member keeps one load of each, at the head
//! of its entry block, and every other read uses that value; keeping it
//! across the calls is the register allocator's job (a callee-saved
//! register, or a spill).
//!
//! Moving a load to the entry runs it on paths that never read it.
//! That is safe: the field is never empty (the pin needs it live) and
//! the array cell it names stays allocated while the receiver holds
//! it. It is not free: the entry pays the three loads on every call,
//! and the values held across a call take callee-saved registers the
//! frame saves and restores. So a member is rewritten only when one
//! read of the field dominates another, a second read on one path. A
//! lone read in a loop test is left alone: when the loop does not
//! iterate it runs once per call, and the hoist only adds the saves.
//!
//! `TORAJS_FIELD_PIN_OFF=1` skips, `TORAJS_FIELD_PIN_STATS=1` counts.

pub(crate) mod cluster;

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::dominator::DominatorTree;
use torajs_core::ssa::{
    BlockId, Function, Inst, InstKind, Module, Operand, Terminator, Type, ValueId,
    rewrite_value_operands,
};

use crate::load_licm::{ARR_DATA_PTR_OFF, ARR_LEN_OFF};

#[derive(Debug, Default)]
pub struct FieldPinStats {
    /// (member, field) pairs rewritten.
    pub members: u32,
    /// Loads replaced by the entry block's.
    pub loads_merged: u32,
}

pub fn pin_field_loads(module: &mut Module) -> FieldPinStats {
    let mut stats = FieldPinStats::default();
    let mut seen: HashSet<(usize, u64)> = HashSet::default();
    let members: Vec<(usize, u64)> = (module.pins.iter())
        .flat_map(|p| p.cluster.iter().map(|m| (m.0 as usize, p.field_off)))
        .filter(|m| seen.insert(*m))
        .collect();
    for (i, off) in members {
        pin_member(&mut module.funcs[i], off, &mut stats);
    }
    stats
}

/// The three loads a read of the field goes through.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Word {
    Field,
    Len,
    Data,
}

/// A load of one word: its block, its index there, its result, its type.
type Read = (usize, usize, ValueId, Type);

fn pin_member(func: &mut Function, field_off: u64, stats: &mut FieldPinStats) {
    let Some(&this) = func.params.first() else {
        return;
    };
    let found = reads(func, this, field_off);
    let Some(fields) = found.get(&Word::Field) else {
        return;
    };
    // one type for the field, or the loads are not one value
    if fields.iter().any(|l| l.3 != fields[0].3) || !read_twice(func, fields) {
        return;
    }
    let field = fields[0].2;
    let mut canon: HashMap<ValueId, ValueId> = HashMap::default();
    let mut gone: HashSet<(usize, usize)> = HashSet::default();
    let mut head = Vec::new();
    for word in [Word::Field, Word::Len, Word::Data] {
        let Some(loads) = found.get(&word) else {
            continue;
        };
        let (_, _, keep, ty) = loads[0];
        let (base, off) = match word {
            Word::Field => (this, field_off),
            Word::Len => (field, ARR_LEN_OFF),
            Word::Data => (field, ARR_DATA_PTR_OFF),
        };
        head.push(Inst {
            result: Some(keep),
            kind: InstKind::Load(ty, Operand::Value(base), off),
            origin: None,
        });
        for &(bi, ii, r, _) in loads {
            gone.insert((bi, ii));
            canon.insert(r, keep);
        }
        stats.loads_merged += loads.len() as u32 - 1;
    }
    stats.members += 1;
    for (bi, block) in func.blocks.iter_mut().enumerate() {
        let insts = std::mem::take(&mut block.insts);
        block.insts = (insts.into_iter().enumerate())
            .filter(|(ii, _)| !gone.contains(&(bi, *ii)))
            .map(|(_, mut inst)| {
                rewrite_value_operands(&mut inst.kind, |v| canon_of(&canon, v));
                inst
            })
            .collect();
        rewrite_term(&mut block.term, &canon);
    }
    func.blocks[0].insts.splice(0..0, head);
}

/// The loads of each word in `func`, in block order: the field at
/// `field_off` of the receiver `this`, and the length and data pointer
/// of what such a load gives.
fn reads(func: &Function, this: ValueId, field_off: u64) -> HashMap<Word, Vec<Read>> {
    let alias = identities(func);
    let mut found: HashMap<Word, Vec<Read>> = HashMap::default();
    let mut fields: HashSet<ValueId> = HashSet::default();
    for (bi, block) in func.blocks.iter().enumerate() {
        for (ii, inst) in block.insts.iter().enumerate() {
            let (InstKind::Load(ty, Operand::Value(base), off), Some(r)) =
                (&inst.kind, inst.result)
            else {
                continue;
            };
            let base = *alias.get(base).unwrap_or(base);
            let word = if base == this && *off == field_off {
                fields.insert(r);
                Word::Field
            } else if fields.contains(&base) && *off == ARR_LEN_OFF && *ty == Type::I64 {
                Word::Len
            } else if fields.contains(&base) && *off == ARR_DATA_PTR_OFF && *ty == Type::Ptr {
                Word::Data
            } else {
                continue;
            };
            found.entry(word).or_default().push((bi, ii, r, *ty));
        }
    }
    found
}

fn canon_of(canon: &HashMap<ValueId, ValueId>, v: ValueId) -> ValueId {
    *canon.get(&v).unwrap_or(&v)
}

fn rewrite_term(term: &mut Terminator, canon: &HashMap<ValueId, ValueId>) {
    let op = match term {
        Terminator::CondBr { cond, .. } => cond,
        Terminator::Ret(Some(op)) => op,
        Terminator::Br(_) | Terminator::Ret(None) | Terminator::Unreachable => return,
    };
    if let Operand::Value(v) = op {
        *v = canon_of(canon, *v);
    }
}

/// Each `id` result, resolved to the value at the end of its chain:
/// the inliner binds a spliced callee's receiver through one.
fn identities(func: &Function) -> HashMap<ValueId, ValueId> {
    let mut direct: HashMap<ValueId, ValueId> = HashMap::default();
    for inst in func.blocks.iter().flat_map(|b| &b.insts) {
        if let (InstKind::Identity(Operand::Value(x)), Some(r)) = (&inst.kind, inst.result) {
            direct.insert(r, *x);
        }
    }
    let mut out = HashMap::default();
    for &r in direct.keys() {
        let mut v = r;
        while let Some(x) = direct.get(&v) {
            v = *x;
        }
        out.insert(r, v);
    }
    out
}

/// Whether one read of the field runs after another on some path: a
/// read that dominates a second one.
fn read_twice(func: &Function, reads: &[Read]) -> bool {
    if reads.len() < 2 {
        return false;
    }
    let dom = DominatorTree::compute(func);
    reads.iter().any(|a| {
        reads.iter().any(|b| {
            (a.0 == b.0 && a.1 < b.1)
                || (a.0 != b.0 && dom.dominates(BlockId(a.0 as u32), BlockId(b.0 as u32)))
        })
    })
}

#[cfg(test)]
mod tests;
