//! Which values a frame cell may back: the mint sites, and the set
//! of values joined to each by copies with every use checked against
//! the rules in the parent module's doc.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    BinOp, Function, Inst, InstKind, Operand, StringId, Terminator, ValueId, visit_value_operands,
};

use super::{HDR_BYTES, I64_DIGITS_MAX, Kernels, Mint, VIEW_LIT_CELL};

#[derive(Clone, Copy)]
pub(super) enum Use {
    Inst(usize, usize),
    Term,
}

pub(super) struct Index<'f> {
    pub(super) f: &'f Function,
    pub(super) defs: HashMap<ValueId, Vec<(usize, usize)>>,
    pub(super) uses: HashMap<ValueId, Vec<Use>>,
}

impl<'f> Index<'f> {
    pub(super) fn of(f: &'f Function) -> Self {
        let mut defs: HashMap<ValueId, Vec<(usize, usize)>> = HashMap::default();
        let mut uses: HashMap<ValueId, Vec<Use>> = HashMap::default();
        for (b, block) in f.blocks.iter().enumerate() {
            for (i, inst) in block.insts.iter().enumerate() {
                if let Some(r) = inst.result {
                    defs.entry(r).or_default().push((b, i));
                }
                visit_value_operands(&inst.kind, |v| {
                    uses.entry(v).or_default().push(Use::Inst(b, i))
                });
            }
            match &block.term {
                Terminator::Ret(Some(Operand::Value(v)))
                | Terminator::CondBr {
                    cond: Operand::Value(v),
                    ..
                } => uses.entry(*v).or_default().push(Use::Term),
                _ => {}
            }
        }
        Index { f, defs, uses }
    }

    pub(super) fn inst(&self, b: usize, i: usize) -> &'f Inst {
        &self.f.blocks[b].insts[i]
    }
}

/// The slot a mint call needs, or `None` when `inst` is not one.
pub(super) fn mint_of(inst: &Inst, k: &Kernels, ix: &Index<'_>) -> Option<(u64, Mint)> {
    let InstKind::Call(fid, args) = &inst.kind else {
        return None;
    };
    inst.result?;
    if Some(*fid) == k.to_str && args.len() == 1 {
        let bytes = (HDR_BYTES + I64_DIGITS_MAX).next_multiple_of(8);
        return Some((bytes, Mint::ToStr));
    }
    if args.len() != 2 {
        return None;
    }
    if Some(*fid) == k.view_concat {
        let (bits, meta) = (*k.lit_imms.get(static_sid(&args[1], ix)?.0 as usize)?)?;
        return Some((VIEW_LIT_CELL, Mint::ViewLit { bits, meta }));
    }
    if Some(*fid) != k.concat {
        return None;
    }
    let sid = static_sid(&args[0], ix)?;
    Some(((*k.lit_cells.get(sid.0 as usize)?)?, Mint::Concat))
}

/// The static literal `op` names, when its one def is a
/// `static_str_ref`.
fn static_sid(op: &Operand, ix: &Index<'_>) -> Option<StringId> {
    let Operand::Value(a) = op else {
        return None;
    };
    let [(b, i)] = ix.defs.get(a)?.as_slice() else {
        return None;
    };
    let InstKind::StaticStrRef(sid) = ix.inst(*b, *i).kind else {
        return None;
    };
    Some(sid)
}

pub(super) enum Role {
    Copy(ValueId),
    Drop,
    Read,
    /// The cell's address taken as an integer: a read, provided every
    /// value derived from it is only an address loads go through.
    Address(ValueId),
    Escape,
}

pub(super) fn role(inst: &Inst, v: ValueId, k: &Kernels) -> Role {
    let is_v = |op: &Operand| *op == Operand::Value(v);
    match &inst.kind {
        InstKind::Copy(_, op) if is_v(op) => inst.result.map_or(Role::Escape, Role::Copy),
        InstKind::Call(fid, args) if *fid == k.drop => {
            if inst.result.is_none() && args.len() == 1 {
                Role::Drop
            } else {
                Role::Escape
            }
        }
        InstKind::Call(fid, args) => match k.readers.get(fid) {
            Some(slots)
                if args
                    .iter()
                    .enumerate()
                    .all(|(p, a)| !is_v(a) || slots.contains(&p)) =>
            {
                Role::Read
            }
            _ => Role::Escape,
        },
        InstKind::PtrToInt(op) if is_v(op) => inst.result.map_or(Role::Escape, Role::Address),
        InstKind::Load(_, base, _) if is_v(base) => Role::Read,
        InstKind::LoadDyn(_, base, idx)
        | InstKind::LoadU8Dyn(base, idx)
        | InstKind::LoadU16Dyn(base, idx)
            if is_v(base) && !is_v(idx) =>
        {
            Role::Read
        }
        _ => Role::Escape,
    }
}

/// Whether the bytes behind `v` at inst `j` of block `b` are those
/// of `v`'s own latest def: its one def is a mint, or the latest def
/// before `j` in the block is one. Every call def of a value in the
/// set is a mint, which [`web`] checks.
fn reads_own_bytes(ix: &Index<'_>, b: usize, j: usize, v: ValueId) -> bool {
    let is_call = |inst: &Inst| matches!(inst.kind, InstKind::Call(..));
    if let Some([(db, di)]) = ix.defs.get(&v).map(|d| d.as_slice())
        && is_call(ix.inst(*db, *di))
    {
        return true;
    }
    ix.f.blocks[b].insts[..j]
        .iter()
        .rev()
        .find(|inst| inst.result == Some(v))
        .is_some_and(is_call)
}

/// The set of values joined to the mint result `r` by copies, and
/// whether every one of them stays readable only while its slot's
/// bytes are the ones written.
pub(super) fn web(r: ValueId, k: &Kernels, ix: &Index<'_>) -> (HashSet<ValueId>, bool) {
    let mut set = HashSet::from_iter([r]);
    let mut queue = vec![r];
    let mut ok = !ix.f.params.contains(&r);
    while let Some(v) = queue.pop() {
        let mut join = |w: ValueId, set: &mut HashSet<ValueId>| {
            if set.insert(w) {
                queue.push(w);
            }
        };
        if ix.f.params.contains(&v) {
            ok = false;
        }
        for &(b, i) in ix.defs.get(&v).map_or(&[][..], |d| d.as_slice()) {
            let inst = ix.inst(b, i);
            match &inst.kind {
                InstKind::Copy(_, Operand::Value(u)) => join(*u, &mut set),
                _ if mint_of(inst, k, ix).is_some() => {}
                _ => ok = false,
            }
        }
        for &u in ix.uses.get(&v).map_or(&[][..], |u| u.as_slice()) {
            let Use::Inst(b, j) = u else {
                ok = false;
                continue;
            };
            match role(ix.inst(b, j), v, k) {
                Role::Copy(w) => join(w, &mut set),
                Role::Drop => {}
                Role::Read => ok &= reads_own_bytes(ix, b, j, v),
                Role::Address(a) => ok &= reads_own_bytes(ix, b, j, v) && only_loads_through(a, ix),
                Role::Escape => ok = false,
            }
        }
    }
    (set, ok)
}

/// Every value derived from the address `a` — by adding to it or
/// turning it back into a pointer — has one def and is used only as
/// the base a load reads through, or compared. One def means every use sees the
/// address taken in its own iteration, after that iteration's mint: a
/// value carried round a loop would need a copy, which is refused.
fn only_loads_through(a: ValueId, ix: &Index<'_>) -> bool {
    let mut seen = HashSet::from_iter([a]);
    let mut queue = vec![a];
    while let Some(v) = queue.pop() {
        if ix.defs.get(&v).map_or(0, Vec::len) != 1 {
            return false;
        }
        let is_v = |op: &Operand| *op == Operand::Value(v);
        for &u in ix.uses.get(&v).map_or(&[][..], |u| u.as_slice()) {
            let Use::Inst(b, j) = u else {
                return false;
            };
            let inst = ix.inst(b, j);
            let derived = match &inst.kind {
                InstKind::BinOp(BinOp::Add, x, y) if is_v(x) != is_v(y) => inst.result,
                InstKind::IntToPtr(op) if is_v(op) => inst.result,
                InstKind::Load(_, base, _) if is_v(base) => None,
                // a null test reads the address, not the bytes
                InstKind::ICmp(_, x, y) if is_v(x) || is_v(y) => None,
                InstKind::LoadDyn(_, base, idx)
                | InstKind::LoadU8Dyn(base, idx)
                | InstKind::LoadU16Dyn(base, idx)
                    if is_v(base) && !is_v(idx) =>
                {
                    None
                }
                _ => return false,
            };
            if let Some(d) = derived
                && seen.insert(d)
            {
                queue.push(d);
            }
        }
    }
    true
}
