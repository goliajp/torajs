//! What may release a reference to a cell of one class: the pieces
//! the scalar census (`pin_facts`, S6) and the call holds
//! (`call_holds`) judge a walk of functions by.

use std::collections::HashSet;

use crate::ssa::{ANY_SLOT_TAG_HEAP, FuncId, Function, InstKind, Module, StructId, Type, ValueId};

use super::cell_slot_flow::is_prefix;

/// The functions with a body `from` reach by direct calls, themselves
/// included.
pub(crate) fn reached(module: &Module, from: &[FuncId]) -> Vec<FuncId> {
    let mut seen: HashSet<FuncId> = from.iter().copied().collect();
    let mut out = from.to_vec();
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

/// Runtime functions that only read the cells they are handed: they
/// test and throw, and never release a reference.
pub(crate) const BORROWS: &[&str] = &[
    "__torajs_heap_nullish_check",
    "__torajs_arr_null_check",
    "__torajs_arr_oob_throw",
    "__torajs_arr_mark_kind",
];

/// The values of `f` that box an immediate, which no release frees.
pub(crate) fn immediates(f: &Function) -> HashSet<ValueId> {
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
/// that refers to one of those. `open` names the sids whose slots may
/// hold a cell of another layout (`cell_slot_flow`).
///
/// An expando bag (a struct cell's, an array's, a RegExp's or a
/// Date's) adds nothing: typed code reads a value out of a bag only
/// through a read of `any`, which answers it owned
/// (`LowerCtx::expr_owned_shape`), so a borrow the judges rely on is
/// held up by typed references only, and releasing a bag cannot drop
/// one of those.
pub(crate) struct MayHold<'m> {
    pub(crate) module: &'m Module,
    pub(crate) sid: StructId,
    pub(crate) open: &'m [bool],
}

impl MayHold<'_> {
    pub(crate) fn of(&self, t: Type) -> bool {
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
