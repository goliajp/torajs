//! The pinned registers: in the members of a cluster (`Module::pin_regs`,
//! the e-graph's `field_pin::cluster`) x27 holds the pinned field's
//! array length and x28 its data pointer; a cluster with scalars
//! (`PinFact::scalars`) keeps the first in x26 and the second in x25.
//!
//! Two roles, read once per module, each with the cluster's number of
//! scalars:
//! - an *inner* body keeps its registers out of its register pool and
//!   never saves them; each `Pinned` value is homed in its register
//!   and emits nothing, and only a `PinSet` writes a scalar's. A body
//!   may make sibling calls: whatever it jumps to keeps x25–x28 as
//!   callee-saved.
//! - an *entry* saves them with its callee-saved registers, loads x27 /
//!   x28 from its receiver's field right after the prologue, while x0
//!   still holds the receiver, and calls its body; its epilogue puts
//!   back the caller's. It makes no sibling call, which would skip that
//!   restore.

use torajs_arr::layout::{ARR_DATA_PTR_OFF, ARR_LEN_OFF};
use torajs_core::ssa::{FuncId, Function, InstKind, Module, Operand, PinWord};

use super::operand::materialize_operand_gpr;
use super::write_u32;
use crate::enc::{ldr_x_imm12, mov_x_reg};
use crate::reg::{Gpr, Reg};
use crate::regalloc::Assignment;
use std::borrow::Cow;
use torajs_core::ssa::id_hash::{HashMap, HashSet};

pub(crate) const PIN_LEN: Gpr = Gpr::X27;
pub(crate) const PIN_DATA: Gpr = Gpr::X28;
/// The registers of a cluster's scalars, by `PinWord::Scalar` index.
pub(crate) const PIN_SCALARS: [Gpr; 2] = [Gpr::X26, Gpr::X25];

/// Every pinned register, in the order the roles reserve them.
const PINNED: [Gpr; 4] = [PIN_LEN, PIN_DATA, PIN_SCALARS[0], PIN_SCALARS[1]];

/// A role's `u8` is the number of the cluster's scalars.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum PinRole {
    #[default]
    None,
    Inner(u8),
    /// The field's offset in the receiver's cell.
    Entry(u32, u8),
}

/// The pin roles of a module's functions, by `FuncId`.
#[derive(Debug, Clone, Default)]
pub(crate) struct PinPlan {
    roles: Vec<PinRole>,
}

impl PinPlan {
    pub(crate) fn of(module: &Module) -> Self {
        let regs = &module.pin_regs;
        if regs.inner.is_empty() {
            return PinPlan::default();
        }
        let mut roles = vec![PinRole::None; module.funcs.len()];
        let scalars =
            |f: FuncId| (regs.scalars.iter().find(|(m, _)| *m == f)).map_or(0, |(_, n)| *n);
        for f in &regs.inner {
            roles[f.0 as usize] = PinRole::Inner(scalars(*f));
        }
        for &(entry, _, off) in &regs.entries {
            roles[entry.0 as usize] = PinRole::Entry(off as u32, scalars(entry));
        }
        PinPlan { roles }
    }

    pub(crate) fn role(&self, f: FuncId) -> PinRole {
        self.roles.get(f.0 as usize).copied().unwrap_or_default()
    }
}

impl PinRole {
    /// The registers no value of the function may take.
    pub(crate) fn reserved(self) -> &'static [Gpr] {
        match self {
            PinRole::None => &[],
            PinRole::Inner(n) | PinRole::Entry(_, n) => &PINNED[..2 + n as usize],
        }
    }

    /// The callee-saved registers the frame saves for the role.
    pub(crate) fn saved_mask(self) -> u32 {
        match self {
            PinRole::Entry(..) => (self.reserved().iter()).fold(0, |m, g| m | (1 << g.idx())),
            PinRole::None | PinRole::Inner(_) => 0,
        }
    }

    pub(crate) fn is_entry(self) -> bool {
        matches!(self, PinRole::Entry(..))
    }

    /// An entry's loads, after the prologue: the field into x28, then
    /// its length into x27 and its data pointer over the field.
    pub(crate) fn emit_entry_loads(self, bytes: &mut Vec<u8>) {
        if let PinRole::Entry(off, _) = self {
            write_u32(bytes, ldr_x_imm12(PIN_DATA, Gpr::X0, off));
            write_u32(bytes, ldr_x_imm12(PIN_LEN, PIN_DATA, ARR_LEN_OFF as u32));
            write_u32(
                bytes,
                ldr_x_imm12(PIN_DATA, PIN_DATA, ARR_DATA_PTR_OFF as u32),
            );
        }
    }
}

/// The register a `Pinned` value of `word` lives in.
pub(crate) fn home(word: PinWord) -> Gpr {
    match word {
        PinWord::Len => PIN_LEN,
        PinWord::Data => PIN_DATA,
        PinWord::Scalar(k) => PIN_SCALARS[k as usize],
    }
}

/// `pin_set <word>, %v`: the value into the scalar's register.
pub(crate) fn emit_pin_set(bytes: &mut Vec<u8>, word: PinWord, v: &Operand, alloc: &Assignment) {
    let home = home(word);
    let src = materialize_operand_gpr(bytes, v, home, alloc);
    if src != home {
        write_u32(bytes, mov_x_reg(home, src));
    }
}

/// Each `Pinned` result of `func` with its register, and each value a
/// cast that moves no bits makes of one: a pointer taken as an integer
/// or back, the same bits in the same register. Each such value has one
/// def, and none of them is coalesced with a copy, so nothing else is
/// written to the register.
fn pinned_values(func: &Function) -> Vec<(u32, Gpr)> {
    let insts = || func.blocks.iter().flat_map(|b| &b.insts);
    let mut out: Vec<(u32, Gpr)> = (insts())
        .filter_map(|i| match (&i.kind, i.result) {
            (InstKind::Pinned(w), Some(r)) => Some((r.0, home(*w))),
            _ => None,
        })
        .collect();
    let mut grew = !out.is_empty();
    while grew {
        grew = false;
        for i in insts() {
            let (
                InstKind::PtrToInt(Operand::Value(v)) | InstKind::IntToPtr(Operand::Value(v)),
                Some(r),
            ) = (&i.kind, i.result)
            else {
                continue;
            };
            let home = out.iter().find(|(p, _)| *p == v.0).map(|(_, g)| *g);
            if let Some(g) = home
                && !out.iter().any(|(p, _)| *p == r.0)
            {
                out.push((r.0, g));
                grew = true;
            }
        }
    }
    out
}

/// The pinned values of `func` when the pool holds back the pinned
/// registers, which only a cluster member's does.
pub(crate) fn pinned_homes(func: &Function, reserved: &[Gpr]) -> Vec<(u32, Gpr)> {
    match reserved.contains(&PIN_LEN) {
        true => pinned_values(func),
        false => Vec::new(),
    }
}

/// The values copy coalescing keeps apart: the returned ones, and a
/// pinned value, which shares its register with no copy of it.
pub(crate) fn copy_fixed<'a>(
    ret_vids: &'a HashSet<u32>,
    pinned: &[(u32, Gpr)],
) -> Cow<'a, HashSet<u32>> {
    let mut fixed = Cow::Borrowed(ret_vids);
    if !pinned.is_empty() {
        fixed.to_mut().extend(pinned.iter().map(|(v, _)| *v));
    }
    fixed
}

/// Home each pinned value in its register and take it out of the sweep.
pub(crate) fn home_pinned(
    pinned: &[(u32, Gpr)],
    by_value: &mut HashMap<u32, Reg>,
    order: &mut Vec<u32>,
) {
    if pinned.is_empty() {
        return;
    }
    for &(v, home) in pinned {
        by_value.insert(v, Reg::Gpr(home));
    }
    order.retain(|v| !pinned.iter().any(|(p, _)| p == v));
}
