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
    "__torajs_str_null_check",
    "__torajs_arr_set_length_validate",
];

/// The values of `f` that box an immediate, which no release frees.
pub(crate) fn immediates(f: &Function) -> HashSet<ValueId> {
    (f.blocks.iter().flat_map(|b| &b.insts))
        .filter(|i| matches!(i.kind, InstKind::BoxAny(tag, _) if tag != ANY_SLOT_TAG_HEAP))
        .filter_map(|i| i.result)
        .collect()
}

/// What the slots of each layout may hold besides a cell of their own
/// layout, over the functions a judge reads (`cell_slot_flow`): `open`
/// names the sids whose slots may hold a cell of another layout, and
/// `wider` gives, per sid, the layouts extending it whose cells reach
/// one of its slots ([`wider_cells`]).
#[derive(Clone, Copy)]
pub(crate) struct Slots<'m> {
    pub(crate) open: &'m [bool],
    pub(crate) wider: &'m [Vec<StructId>],
}

impl<'m> Slots<'m> {
    pub(crate) fn new(open: &'m [bool], wider: &'m [Vec<StructId>]) -> Self {
        Slots { open, wider }
    }
}

/// Per sid, the layouts whose cells a slot of it may hold by prefix:
/// every `(wide, narrow)` path a cell took into a slot of `narrow`,
/// followed back through the slots of `wide` in turn.
pub(crate) fn wider_cells(n: usize, widens: &[(StructId, StructId)]) -> Vec<Vec<StructId>> {
    let mut into = vec![Vec::new(); n];
    for &(wide, narrow) in widens {
        into[narrow.0 as usize].push(wide);
    }
    (0..n)
        .map(|s| {
            let mut out: Vec<StructId> = Vec::new();
            let mut stack = into[s].clone();
            while let Some(w) = stack.pop() {
                if w.0 as usize != s && !out.contains(&w) {
                    out.push(w);
                    stack.extend_from_slice(&into[w.0 as usize]);
                }
            }
            out
        })
        .collect()
}

/// Which types may hold a reference to a cell of type `cell`.
///
/// For a class instance (`Obj(sid)`): the class itself and any layout
/// that is a prefix of its own by field name (the types a cell of it
/// widens to). For a string: a string, a substring view (which keeps
/// its parent) and the cells that keep their source text (a RegExp, a
/// Symbol's description). For an array: any array, since a slot of one
/// array layout can hold a cell of another. For a Map or a Set: the
/// types that may hold a cell of any kind (a Map, a Set, a closure, an
/// `any`). For every kind: a layout whose slots may hold a cell of
/// another layout (an interface a class instance enters, a slot an
/// `any` crosses into), a class slot whose wider cells (a subclass
/// instance in a base-typed slot, including one a boxed adapter
/// decoded behind its layout test) may hold one in a field of their
/// own, anything untyped, and every type that refers to one of those.
///
/// An expando bag (a struct cell's, an array's, a RegExp's or a
/// Date's) adds nothing: typed code reads a value out of a bag only
/// through a read of `any`, which answers it owned
/// (`LowerCtx::expr_owned_shape`), so a borrow the judges rely on is
/// held up by typed references only, and releasing a bag cannot drop
/// one of those.
pub(crate) struct MayHold<'m> {
    pub(crate) module: &'m Module,
    pub(crate) cell: Type,
    pub(crate) slots: Slots<'m>,
}

impl MayHold<'_> {
    pub(crate) fn of(&self, t: Type) -> bool {
        self.walk(t, &mut HashSet::new())
    }

    fn walk(&self, t: Type, seen: &mut HashSet<Type>) -> bool {
        let layouts = &self.module.struct_layouts;
        let str_cell = self.cell == Type::Str;
        match t {
            Type::I64 | Type::F64 | Type::I32 | Type::Bool | Type::Void => false,
            Type::Str | Type::Substr | Type::RegExp | Type::Symbol => str_cell,
            Type::Date | Type::BigInt | Type::FnSig(_) | Type::Vec(_) => false,
            Type::Obj(s) | Type::Inline(s) => {
                let fields = &layouts[s.0 as usize];
                let widens = match self.cell {
                    Type::Obj(sid) => is_prefix(fields, &layouts[sid.0 as usize]),
                    _ => false,
                };
                // an inline struct is a copy of its own layout, never
                // a wider cell
                let wider: &[StructId] = match t {
                    Type::Obj(_) => &self.slots.wider[s.0 as usize],
                    _ => &[],
                };
                seen.insert(t)
                    && (self.slots.open[s.0 as usize]
                        || widens
                        || fields.iter().any(|(_, t)| self.walk(*t, seen))
                        || wider.iter().any(|w| self.walk(Type::Obj(*w), seen)))
            }
            Type::Arr(a) => {
                matches!(self.cell, Type::Arr(_)) || self.walk(self.module.arr_elem(a), seen)
            }
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::PropKey;

    fn layout(fields: &[(&str, Type)]) -> Vec<(PropKey, Type)> {
        (fields.iter())
            .map(|(n, t)| (PropKey::from(*n), *t))
            .collect()
    }

    /// Layouts 0 `Base {k}`, 1 `Sub {k, name: string, o: Inner}`
    /// (extends 0), 2 `Inner {v}`, 3 `Holder {b: Base}`.
    fn module() -> Module {
        let inner = Type::Obj(StructId(2));
        Module {
            struct_layouts: vec![
                layout(&[("k", Type::I64)]),
                layout(&[("k", Type::I64), ("name", Type::Str), ("o", inner)]),
                layout(&[("v", Type::I64)]),
                layout(&[("b", Type::Obj(StructId(0)))]),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn wider_cells_follow_paths_back() {
        let s = StructId;
        let wider = wider_cells(4, &[(s(1), s(0)), (s(2), s(1)), (s(0), s(0))]);
        assert_eq!(wider[0], vec![s(1), s(2)]);
        assert_eq!(wider[1], vec![s(2)]);
        assert!(wider[2].is_empty() && wider[3].is_empty());
    }

    #[test]
    fn a_base_slot_holds_what_a_subclass_cell_in_it_holds() {
        let m = module();
        let open = vec![false; 4];
        let none = vec![Vec::new(); 4];
        let sub_in_base = wider_cells(4, &[(StructId(1), StructId(0))]);
        let inner = Type::Obj(StructId(2));
        for (cell, ty) in [
            (Type::Str, Type::Obj(StructId(0))),
            (inner, Type::Obj(StructId(0))),
            (inner, Type::Obj(StructId(3))),
        ] {
            let may = |wider| MayHold {
                module: &m,
                cell,
                slots: Slots { open: &open, wider },
            };
            assert!(!may(&none).of(ty), "{cell:?} in {ty:?} with no path");
            assert!(may(&sub_in_base).of(ty), "{cell:?} in {ty:?}, Sub in Base");
        }
        // an inline struct is a copy of its own layout
        let may = MayHold {
            module: &m,
            cell: Type::Str,
            slots: Slots {
                open: &open,
                wider: &sub_in_base,
            },
        };
        assert!(!may.of(Type::Inline(StructId(0))));
    }
}
