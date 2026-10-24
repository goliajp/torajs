//! What the members reach (the walk F2 makes), and what it must not
//! do: enter the cluster again (S5) or free a cell of the class (S6).

use std::collections::HashSet;

use crate::ssa::{
    ANY_SLOT_TAG_HEAP, FuncId, Function, InstKind, Module, Operand, StructId, Type, ValueId,
};

use super::super::super::cell_slot_flow::is_prefix;
use super::ty;

/// The functions with a body the members reach by direct calls,
/// themselves included: the walk F2 makes.
pub(super) fn reached(module: &Module, cluster: &[FuncId]) -> Vec<FuncId> {
    let mut seen: HashSet<FuncId> = cluster.iter().copied().collect();
    let mut out = cluster.to_vec();
    let mut i = 0;
    while let Some(&fid) = out.get(i) {
        i += 1;
        for inst in module.funcs[fid.0 as usize]
            .blocks
            .iter()
            .flat_map(|b| &b.insts)
        {
            if let InstKind::Call(t, _) = &inst.kind
                && !module.funcs[t.0 as usize].is_declaration()
                && seen.insert(*t)
            {
                out.push(*t);
            }
        }
    }
    out
}

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

/// Runtime functions that only read the cells they are handed: they
/// test and throw, and never release a reference.
const BORROWS: &[&str] = &[
    "__torajs_heap_nullish_check",
    "__torajs_arr_null_check",
    "__torajs_arr_oob_throw",
    "__torajs_arr_mark_kind",
];

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
    let holds = Holds { module, sid, open };
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

/// The values of `f` that box an immediate, which no release frees.
fn immediates(f: &Function) -> HashSet<ValueId> {
    (f.blocks.iter().flat_map(|b| &b.insts))
        .filter(|i| matches!(i.kind, InstKind::BoxAny(tag, _) if tag != ANY_SLOT_TAG_HEAP))
        .filter_map(|i| i.result)
        .collect()
}

/// Which types may hold a reference to a cell of class sid `sid`: the
/// class itself and any layout that is a prefix of its own by field
/// name (the types a cell of it widens to), a layout whose slots may
/// hold a cell of another layout (an interface a class instance enters,
/// a slot an `any` crosses into), anything untyped, and every type
/// that refers to one of those.
struct Holds<'m> {
    module: &'m Module,
    sid: StructId,
    open: &'m [bool],
}

impl Holds<'_> {
    fn of(&self, t: Type) -> bool {
        self.walk(t, &mut HashSet::new())
    }

    fn walk(&self, t: Type, seen: &mut HashSet<StructId>) -> bool {
        let layouts = &self.module.struct_layouts;
        match t {
            Type::I64 | Type::F64 | Type::I32 | Type::Bool | Type::Void => false,
            Type::Str | Type::Substr | Type::RegExp | Type::Date | Type::Symbol => false,
            Type::BigInt | Type::FnSig(_) | Type::Vec(_) => false,
            Type::Obj(s) | Type::Inline(s) => {
                let fields = &layouts[s.0 as usize];
                seen.insert(s)
                    && (self.open[s.0 as usize]
                        || is_prefix(fields, &layouts[self.sid.0 as usize])
                        || fields.iter().any(|(_, t)| self.walk(*t, seen)))
            }
            Type::Arr(a) => self.walk(self.module.arr_elem(a), seen),
            _ => true,
        }
    }
}
