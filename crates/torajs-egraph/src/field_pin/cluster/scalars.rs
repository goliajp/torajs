//! A cluster's scalars (`PinFact::scalars`) in their registers: the
//! `k`th, a field of the receiver or a module global, lives in the
//! register of `PinWord::Scalar(k)` while a member runs.
//!
//! In a body, each load of the field through the receiver, or of the
//! global through its `global_ref`, becomes `pinned k` and each store
//! `pin_set k`; a `global_ref` nothing reads any longer goes. A
//! `pinned` value names the register, not a copy of it, so it holds
//! what the load read only until the scalar is next written: by a
//! `pin_set k`, or by a call to a member, which may write it. A use
//! past such a point, or in another block, reads a copy made right
//! after the `pinned`.
//!
//! The entry loads each scalar from memory before it calls the body
//! and stores it back right after, before the throw probe: a body that
//! throws returns like one that does not, so the field holds what the
//! program had stored by then either way.
//!
//! `TORAJS_PIN_SCALAR_STRICT` (set to anything) checks the protocol at
//! run time: a body also keeps each store, so the field in memory
//! always holds what the register should, and right after each
//! `pinned` it compares the two and traps (`brk`) when they differ.
//! It catches the register changed or memory written behind the
//! cluster's back; a stale read outside the cluster stays invisible,
//! since memory is current in that mode.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    BlockId, FuncId, Function, Inst, InstKind, Module, Operand, PinFact, PinScalar, PinWord, Type,
    ValueId,
};

use super::Receivers;
use cells::foreign;
pub(super) use cells::receivers;
use flow::copy_past_writes;
use globals::{drop_unread_refs, global_refs, kept, reentered};
use strict::check;

/// Set (to anything), each read of a scalar is checked against memory.
pub const STRICT_ENV: &str = "TORAJS_PIN_SCALAR_STRICT";

pub(super) fn strict() -> bool {
    std::env::var_os(STRICT_ENV).is_some()
}

/// The runtime allocation a member's own new cell comes from.
const OBJ_ALLOC: &str = "__torajs_obj_alloc";

/// Where a scalar lives in memory: the field at an offset of the
/// receiver's cell, or a module global.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Slot {
    Field(u64),
    Global(String),
}

/// The scalars of `fact` its `members` keep in registers, at most two,
/// in their order; the `k`th is `PinWord::Scalar(k)`. None when
/// something the members reach enters the cluster again; no field when
/// a member reaches one on a cell `foreign` finds; a global only while
/// it holds on the module as it is now (`globals::kept`). `quiet` names
/// the functions that never run and call a member or take its address
/// (`PinFact::quiet`); `recv` the values that hold each member's
/// receiver.
pub(super) fn held(
    module: &Module,
    fact: &PinFact,
    members: &[FuncId],
    quiet: &HashSet<FuncId>,
    recv: &Receivers,
) -> Vec<(Slot, Type)> {
    if reentered(module, members, quiet, recv) {
        return Vec::new();
    }
    let fields: Vec<(u64, Type)> = (fact.scalars.iter())
        .filter_map(|s| match s {
            PinScalar::Field { off, ty } => Some((*off, *ty)),
            PinScalar::Global { .. } => None,
        })
        .collect();
    let alloc = (module.funcs.iter().position(|f| f.name == OBJ_ALLOC)).map(|i| FuncId(i as u32));
    let clash = members.iter().any(|m| {
        let f = &module.funcs[m.0 as usize];
        foreign(f, fact.sid, &fields, &recv[m], alloc)
    });
    (fact.scalars.iter())
        .filter_map(|s| match s {
            PinScalar::Field { off, ty } => (!clash).then_some((Slot::Field(*off), *ty)),
            PinScalar::Global { sym, ty } => {
                (kept(module, members, quiet, sym, *ty)).then(|| (Slot::Global(sym.clone()), *ty))
            }
        })
        .take(2)
        .collect()
}

/// What the rewrite of a body did.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct Rewritten {
    /// Loads and stores of a scalar replaced.
    pub accesses: u32,
    /// Copies made of a `pinned` value used past a write.
    pub copies: u32,
}

/// A scalar access of the receiver's field or of a global.
enum Access {
    Read(u8, Type),
    Write(u8, Operand),
}

/// The access `kind` makes of a scalar of `held`: through a receiver
/// of `recv` at a field's offset, or through a `global_ref` of `refs`.
fn access(
    kind: &InstKind,
    held: &[(Slot, Type)],
    recv: &HashSet<ValueId>,
    refs: &HashMap<ValueId, String>,
) -> Option<Access> {
    let (InstKind::Load(_, Operand::Value(b), off) | InstKind::Store(_, Operand::Value(b), off)) =
        kind
    else {
        return None;
    };
    let k = (held.iter()).position(|(s, _)| match s {
        Slot::Field(o) => o == off && recv.contains(b),
        Slot::Global(sym) => *off == 0 && refs.get(b) == Some(sym),
    })? as u8;
    match kind {
        InstKind::Load(ty, ..) => Some(Access::Read(k, *ty)),
        InstKind::Store(v, ..) => Some(Access::Write(k, *v)),
        _ => None,
    }
}

/// Rewrite the scalar accesses of body `f`; `writes` are the functions
/// a call to which may write a scalar (the members and their bodies).
pub(super) fn body(
    f: &mut Function,
    held: &[(Slot, Type)],
    writes: &HashSet<FuncId>,
    strict: bool,
) -> Rewritten {
    let mut out = Rewritten::default();
    let pinned = rewrite_accesses(f, held, strict, &mut out);
    drop_unread_refs(f, held);
    for &p in &pinned {
        out.copies += copy_past_writes(f, p, writes);
    }
    if strict {
        for &p in &pinned {
            check(f, p, held);
        }
    }
    out
}

/// Turn each scalar load into `pinned` and each store into `pin_set`,
/// keeping the store too when `strict`; the `pinned` values made, in
/// order.
fn rewrite_accesses(
    f: &mut Function,
    held: &[(Slot, Type)],
    strict: bool,
    out: &mut Rewritten,
) -> Vec<ValueId> {
    let recv = receivers(f);
    let refs: HashMap<ValueId, String> = (global_refs(f).into_iter())
        .map(|(r, s)| (r, s.to_string()))
        .collect();
    let defs = def_counts(f);
    let mut pinned: Vec<ValueId> = Vec::new();
    for b in 0..f.blocks.len() {
        let mut i = 0;
        while i < f.blocks[b].insts.len() {
            let inst = &f.blocks[b].insts[i];
            let (Some(a), result) = (access(&inst.kind, held, &recv, &refs), inst.result) else {
                i += 1;
                continue;
            };
            out.accesses += 1;
            match a {
                Access::Read(k, ty) => {
                    let r = result.expect("a load has a result");
                    let word = InstKind::Pinned(PinWord::Scalar(k));
                    if defs.get(&r) == Some(&1) {
                        f.blocks[b].insts[i].kind = word;
                        pinned.push(r);
                    } else {
                        // `r` has other defs, so a new value names the register
                        // and this def of `r` becomes a copy of it
                        let p = f.insert_inst(BlockId(b as u32), i, word, ty);
                        f.blocks[b].insts[i + 1].kind = InstKind::Copy(ty, Operand::Value(p));
                        pinned.push(p);
                        i += 1;
                    }
                }
                Access::Write(k, v) => {
                    let set = InstKind::PinSet(PinWord::Scalar(k), v);
                    let store = std::mem::replace(&mut f.blocks[b].insts[i].kind, set);
                    if strict {
                        f.blocks[b].insts.insert(i + 1, void(store));
                        i += 1;
                    }
                }
            }
            i += 1;
        }
    }
    pinned
}

/// Load each scalar of entry `f` from memory before the call to the
/// body and store it back right after it.
pub(super) fn entry(f: &mut Function, held: &[(Slot, Type)]) {
    let call = (f.blocks[0].insts.iter())
        .position(|i| matches!(i.kind, InstKind::Call(..)))
        .expect("an entry calls its body");
    let mut at = call + 1;
    for (k, (slot, ty)) in held.iter().enumerate() {
        let word = PinWord::Scalar(k as u8);
        let q = f.insert_inst(BlockId(0), at, InstKind::Pinned(word), *ty);
        let (base, off, n) = address(f, BlockId(0), at + 1, slot);
        let store = InstKind::Store(Operand::Value(q), base, off);
        f.blocks[0].insts.insert(at + 1 + n, void(store));
        at += 2 + n;
    }
    let mut at = call;
    for (k, (slot, ty)) in held.iter().enumerate() {
        let (base, off, n) = address(f, BlockId(0), at, slot);
        let l = f.insert_inst(BlockId(0), at + n, InstKind::Load(*ty, base, off), *ty);
        let set = InstKind::PinSet(PinWord::Scalar(k as u8), Operand::Value(l));
        f.blocks[0].insts.insert(at + n + 1, void(set));
        at += 2 + n;
    }
}

/// The base and offset of `slot`'s word in memory, for an access at
/// `at` in block `b` of `f`: the receiver, or a `global_ref` of the
/// global inserted at `at`; and the number of instructions inserted.
pub(super) fn address(
    f: &mut Function,
    b: BlockId,
    at: usize,
    slot: &Slot,
) -> (Operand, u64, usize) {
    match slot {
        Slot::Field(off) => (Operand::Value(f.params[0]), *off, 0),
        Slot::Global(sym) => {
            let g = f.insert_inst(b, at, InstKind::GlobalRef(sym.clone()), Type::Ptr);
            (Operand::Value(g), 0, 1)
        }
    }
}

fn void(kind: InstKind) -> Inst {
    Inst {
        result: None,
        kind,
        origin: None,
    }
}

fn def_counts(f: &Function) -> HashMap<ValueId, u32> {
    let mut defs: HashMap<ValueId, u32> = HashMap::default();
    for r in f
        .blocks
        .iter()
        .flat_map(|b| &b.insts)
        .filter_map(|i| i.result)
    {
        *defs.entry(r).or_default() += 1;
    }
    defs
}

mod cells;
mod flow;
mod globals;
mod strict;
#[cfg(test)]
mod tests;
