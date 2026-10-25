//! Whether a value fits the slot it reaches, the type a slot has, and
//! the paths a fitting value of another static type records on the
//! way ([`Scan::lays`]).

use super::paths::elem_obj;
use super::{Scan, is_prefix};
use crate::ssa::{Module, Operand, StructId, Type, ValueId};
use crate::ssa_lower::OBJ_HEADER_SIZE;

use super::super::any_holds::Holds;

impl Scan<'_> {
    /// A slot read typed `sid`: a declared slot of a wider layout hands
    /// its cells on as `sid`'s; a declared slot of an unrelated layout
    /// (or of a narrower one, read as its extension) hands on cells
    /// `sid` does not describe.
    pub(super) fn read_as(&self, sid: StructId, p: ValueId, off: u64) -> bool {
        match self.declared_slot(p, off) {
            Some(Type::Obj(t)) => self.lays(sid, t),
            _ => true,
        }
    }

    /// A call's answer typed `sid`: the body answers for its returns
    /// as its own declared type.
    pub(super) fn answered_as(&self, sid: StructId, ret: Type) -> bool {
        match ret {
            Type::Obj(t) => self.lays(sid, t),
            _ => true,
        }
    }

    /// Two layouts read through one untyped slot: the wider one is
    /// what a store there has to fit, and unrelated ones open each
    /// other.
    pub(super) fn read_through(&mut self, base: ValueId, off: u64, sid: StructId) {
        let layouts = &self.module.struct_layouts;
        let lay = |s: StructId| &layouts[s.0 as usize];
        match self.loaded.get(&(base, off)).copied() {
            None => {
                self.loaded.insert((base, off), sid);
            }
            Some(seen) if seen == sid || is_prefix(lay(sid), lay(seen)) => {
                self.lays(sid, seen);
            }
            Some(seen) if is_prefix(lay(seen), lay(sid)) => {
                self.lays(seen, sid);
                self.loaded.insert((base, off), sid);
            }
            Some(seen) => {
                self.open[seen.0 as usize] = true;
                self.open[sid.0 as usize] = true;
            }
        }
    }

    /// A null, a cell of the slot's own layout, or one that extends
    /// it by prefix (the subclass in a base slot the sized free
    /// already accounts for); an `any` fits when everything it can
    /// hold does.
    pub(super) fn fits(&self, sid: StructId, v: &Operand) -> bool {
        match v {
            Operand::ConstPtrNull | Operand::ConstI64(0) => true,
            Operand::Value(id) => match self.f.value_type(*id) {
                Type::Obj(t) => self.lays(sid, t),
                _ => match self.holds.eval(self.fid, v) {
                    Holds::Cells(ts) => ts.into_iter().all(|t| self.lays(sid, t)),
                    Holds::Unknown => false,
                },
            },
            _ => false,
        }
    }

    /// [`fits`](Self::fits) for a slot of type `ty`: an array slot
    /// takes a null or an array of the same depth whose cells fit.
    /// The array's elements are then slots under both element types.
    pub(super) fn fits_slot(&self, ty: Type, v: &Operand) -> bool {
        match ty {
            Type::Obj(sid) => self.fits(sid, v),
            Type::Arr(a) => match v {
                Operand::ConstPtrNull | Operand::ConstI64(0) => true,
                Operand::Value(id) => match self.f.value_type(*id) {
                    Type::Arr(b) => {
                        let (x, y) = (
                            self.module.arr_layouts[a.0 as usize],
                            self.module.arr_layouts[b.0 as usize],
                        );
                        let fits = match (self.elem_obj(x), self.elem_obj(y)) {
                            (Some(s), Some(t)) => {
                                depth(self.module, x) == depth(self.module, y) && self.lays(s, t)
                            }
                            _ => false,
                        };
                        if fits {
                            self.paths.borrow_mut().align(self.module, x, y);
                        }
                        fits
                    }
                    _ => false,
                },
                _ => false,
            },
            _ => true,
        }
    }

    /// A cell of `t` fits a slot of `sid`: the same layout, or one
    /// extending it, which is recorded as a path from `t`'s slots to
    /// `sid`'s.
    pub(super) fn lays(&self, sid: StructId, t: StructId) -> bool {
        if t == sid {
            return true;
        }
        let layouts = &self.module.struct_layouts;
        let fits = is_prefix(&layouts[sid.0 as usize], &layouts[t.0 as usize]);
        if fits {
            self.paths.borrow_mut().widen(self.module, t, sid);
        }
        fits
    }

    pub(super) fn elem_obj(&self, ty: Type) -> Option<StructId> {
        elem_obj(self.module, ty)
    }

    /// `bits` are the bits of an `any` a boxed adapter's layout test
    /// (`__torajs_anyv_struct_fits` / `__torajs_anyv_arr_fits`)
    /// answered for: the adapter decodes it only on the fitting path,
    /// so the cell has the slot's layout or one extending it, and an
    /// array holds such cells in every element. Only an adapter emits
    /// the test, and it branches on the answer before the decode.
    ///
    /// Exact means no foreign layout, not the slot's layout alone: a
    /// proven decode into a slot of type `slot` records every layout
    /// extending it as a path into it, as a blind parameter does
    /// (`Paths::entered_blind`), so a base-typed slot knows the
    /// subclass cells it may hold and an array's elements are slots
    /// under both element types.
    pub(super) fn proven_fit(&self, bits: &Operand, slot: Type) -> bool {
        let Operand::Value(b) = bits else {
            return false;
        };
        let proven = self
            .f
            .name
            .starts_with(super::super::any_holds::BOXED_ENTRY_PREFIX)
            && self
                .bits_of
                .get(b)
                .is_some_and(|av| self.tested.contains(av));
        if proven {
            self.paths.borrow_mut().entered_blind(self.module, slot);
        }
        proven
    }

    pub(super) fn slot(&self, p: &Operand, off: u64) -> Option<Type> {
        let Operand::Value(id) = p else { return None };
        self.declared_slot(*id, off)
            .or_else(|| self.loaded.get(&(*id, off)).map(|sid| Type::Obj(*sid)))
    }

    pub(super) fn declared_slot(&self, id: ValueId, off: u64) -> Option<Type> {
        if off == 0 {
            if let Some(ty) = self.allocas.get(&id) {
                return Some(*ty);
            }
            if let Some(name) = self.global_refs.get(&id) {
                return self.globals.get(name).copied();
            }
        }
        if let Some(a) = self.data_of.get(&id) {
            return Some(self.module.arr_layouts[a.0 as usize]);
        }
        if let Type::Obj(sid) = self.f.value_type(id)
            && off >= OBJ_HEADER_SIZE
        {
            let i = ((off - OBJ_HEADER_SIZE) / 8) as usize;
            return self.module.struct_layouts[sid.0 as usize]
                .get(i)
                .map(|(_, ty)| *ty);
        }
        None
    }

    pub(super) fn ty(&self, v: &Operand) -> Type {
        match v {
            Operand::Value(id) => self.f.value_type(*id),
            Operand::ConstI64(_) => Type::I64,
            Operand::ConstI32(_) => Type::I32,
            Operand::ConstF64(_) => Type::F64,
            Operand::ConstBool(_) => Type::Bool,
            Operand::ConstPtrNull => Type::Ptr,
        }
    }
}

fn depth(module: &Module, ty: Type) -> usize {
    match ty {
        Type::Arr(a) => 1 + depth(module, module.arr_layouts[a.0 as usize]),
        _ => 0,
    }
}
