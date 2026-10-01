//! The capture-box side of `cell_slot_flow`: what a closure's env
//! layout says about the boxes stored in it, read from lowering's
//! capture side channel rather than from the SSA, where a box is a
//! bare pointer on both sides of the env.

use std::collections::HashMap;

use crate::ssa::{FuncId, InstKind, Module, Operand, StructId, Type};
use crate::ssa_lower::CLOSURE_CAP_BASE_OFF;

use super::Scan;
/// Per lifted closure, the `(type, boxed)` of each env capture slot,
/// as the construction site and the body preamble lay the env out.
pub(crate) type EnvLayouts = HashMap<FuncId, Vec<(Type, bool)>>;

/// The env layouts from lowering's capture side channel, keyed by
/// the function that reads each env: an any-widened clone of a lifted
/// body enters with the original's env (`clone_origin`).
pub(crate) fn env_layouts(
    module: &Module,
    captures: &HashMap<String, Vec<(String, Type, bool)>>,
    clone_origin: &HashMap<String, String>,
) -> EnvLayouts {
    module
        .funcs
        .iter()
        .enumerate()
        .filter_map(|(i, f)| {
            let origin = clone_origin.get(&f.name).unwrap_or(&f.name);
            let caps = captures.get(origin)?;
            Some((
                FuncId(i as u32),
                caps.iter().map(|(_, t, b)| (*t, *b)).collect(),
            ))
        })
        .collect()
}

impl Scan<'_> {
    /// Give each slot behind a bare pointer the type the function
    /// reads from it, and each capture box the type its closure's
    /// capture list gives it: the box this function builds an env
    /// around, and the box a closure body loads from its env.
    pub(super) fn type_box_slots(&mut self) {
        let f = self.f;
        let env_slot = f.params.first().and_then(|p0| {
            f.blocks
                .iter()
                .flat_map(|b| &b.insts)
                .find_map(|inst| match &inst.kind {
                    InstKind::Store(Operand::Value(v), Operand::Value(s), 0)
                        if v == p0 && self.allocas.contains_key(s) =>
                    {
                        Some(*s)
                    }
                    _ => None,
                })
        });
        let envs = self.envs;
        let own_layout = envs.get(&self.fid);
        let mut env_ptrs = Vec::new();
        for inst in f.blocks.iter().flat_map(|b| &b.insts) {
            match (&inst.kind, inst.result) {
                (InstKind::Load(Type::Obj(sid), Operand::Value(base), off), _)
                    if self.declared_slot(*base, *off).is_none() =>
                {
                    self.read_through(*base, *off, *sid);
                }
                (InstKind::Load(Type::Ptr, Operand::Value(s), 0), Some(v))
                    if Some(*s) == env_slot && own_layout.is_some() =>
                {
                    env_ptrs.push(v);
                }
                (InstKind::Load(Type::Ptr, Operand::Value(e), off), Some(bx))
                    if env_ptrs.contains(e) =>
                {
                    if let Some(sid) = boxed_obj(own_layout, *off) {
                        self.read_through(bx, 0, sid);
                    }
                }
                (InstKind::Store(Operand::Value(bx), Operand::Value(env), off), _) => {
                    if let Some(fid) = self.env_of.get(env)
                        && let Some(sid) = boxed_obj(envs.get(fid), *off)
                    {
                        self.read_through(*bx, 0, sid);
                    }
                }
                _ => {}
            }
        }
    }
}

/// The cell layout a boxed capture at env offset `off` holds, if
/// the layout says the slot is a box of cells.
fn boxed_obj(layout: Option<&Vec<(Type, bool)>>, off: u64) -> Option<StructId> {
    let i = off.checked_sub(CLOSURE_CAP_BASE_OFF)? / 8;
    match layout?.get(i as usize) {
        Some((Type::Obj(sid), true)) => Some(*sid),
        _ => None,
    }
}
