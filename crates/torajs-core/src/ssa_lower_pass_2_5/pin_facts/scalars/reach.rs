//! What the members reach (the walk F2 makes), and what it must not
//! do: enter the cluster again (S5) or free a cell of the class (S6).

use std::collections::HashSet;

use crate::ssa::{FuncId, InstKind, Module, Operand, StructId, Type};

pub(super) use super::super::super::cell_release::reached;
use super::super::super::cell_release::{BORROWS, MayHold, immediates};
use super::ty;

/// S5: the first function of `reach` that calls a member from outside
/// the cluster or takes a member's address.
pub(super) fn reentered(module: &Module, cluster: &[FuncId], reach: &[FuncId]) -> Option<String> {
    reach.iter().find_map(|fid| {
        let f = &module.funcs[fid.0 as usize];
        let inside = cluster.contains(fid);
        f.blocks
            .iter()
            .flat_map(|b| &b.insts)
            .find_map(|i| match &i.kind {
                InstKind::Call(t, _) if !inside && cluster.contains(t) => Some(format!(
                    "{} called by {}",
                    module.funcs[t.0 as usize].name, f.name
                )),
                InstKind::FnAddr(t) | InstKind::BoxedEntryAddr(t) if cluster.contains(t) => {
                    Some(format!(
                        "{}'s address taken by {}",
                        module.funcs[t.0 as usize].name, f.name
                    ))
                }
                _ => None,
            })
    })
}

/// S6: the first function of `reach` that may release a reference to a
/// cell of class sid `sid`. An entry borrows its receiver from its
/// caller and writes the scalars back into it after the body returns,
/// so nothing the body runs may free the cell. A release is a call
/// handed a value whose type may hold such a cell, to a runtime
/// function not in `BORROWS` or to a class's release fn (whose body is
/// judged by that type, not walked), an untyped release (but of a boxed
/// immediate), an indirect call or a suspension. `open` names the sids
/// whose slots may hold a cell of another layout (`cell_slot_flow`).
pub(super) fn released(
    module: &Module,
    sid: StructId,
    reach: &[FuncId],
    open: &[bool],
) -> Option<String> {
    let drops: HashSet<FuncId> = module.class_drop_fns.iter().copied().collect();
    let holds = MayHold {
        module,
        cell: Type::Obj(sid),
        open,
    };
    for fid in reach.iter().filter(|f| !drops.contains(f)) {
        let f = &module.funcs[fid.0 as usize];
        let immediates = immediates(f);
        for inst in f.blocks.iter().flat_map(|b| &b.insts) {
            let why = match &inst.kind {
                InstKind::Call(t, args) => {
                    let g = &module.funcs[t.0 as usize];
                    let runs = g.is_declaration() || drops.contains(t);
                    let held = |a: &Operand| holds.of(ty(f, a));
                    (runs && !BORROWS.contains(&g.name.as_str()) && args.iter().any(held))
                        .then(|| format!("{} may release one", g.name))
                }
                InstKind::CallIndirect(..) => Some("an indirect call".to_string()),
                InstKind::RcDecAny(Operand::Value(x)) if immediates.contains(x) => None,
                InstKind::RcDecAny(_) => Some("an untyped release".to_string()),
                InstKind::Suspend(..) => Some("a suspension".to_string()),
                _ => None,
            };
            if let Some(why) = why {
                return Some(format!("{why} of its cells in {}", f.name));
            }
        }
    }
    None
}
