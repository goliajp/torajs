//! Which struct-typed slots can hold a cell of another layout.
//!
//! The checker admits `any` into every slot, and lowering carries such
//! a value across unchanged: a constructor's answered object behind
//! `const v: C = new C()`, an `as any` round trip, an `any` argument to
//! a typed parameter, a field, an array element, a declared return.
//! Each crossing is visible in the finished SSA as a value whose type
//! is not the slot's reaching that slot, and a sid any such value
//! reaches is *open*: its slots hold cells of unknown layout, so
//! nothing about the layout may size them. [`open_sids`] finds those
//! sids from the SSA alone, so a new crossing in lowering needs no
//! registration here. An `any` that provably carries only cells of
//! fitting layouts (`any_holds`) is not a crossing.
//!
//! A slot's type is read off the SSA: a `let` slot from its `Alloca`,
//! a field from the base's layout, a global from the module table, an
//! element from the array's layout (behind the cell's data pointer),
//! a parameter from the callee, a return from the function. An array
//! is a slot of slots: a value typed as an array of another layout
//! reaching an array-typed slot, or an array made by a runtime call
//! out of an `any`, opens the element layout. A slot behind a bare
//! pointer (a capture box, a frame reached through a closure env) has
//! the type the function loads from it, or the type the closure's
//! capture list gives the box, and what is stored there must fit that.

use std::collections::HashMap;

use crate::ast::PropKey;
use crate::ssa::{
    ArrId, FuncId, Function, InstKind, Module, Operand, StructId, Terminator, Type, ValueId,
};
use crate::ssa_lower::{ARR_DATA_PTR_OFF, CLOSURE_FN_ADDR_OFF, OBJ_HEADER_SIZE};

mod env;

use super::any_holds::{AnyHolds, Holds};
pub(crate) use env::{EnvLayouts, env_layouts};

const OBJ_ALLOC: &str = "__torajs_obj_alloc";
/// A capture box answers the pointer to its value slot; the payload
/// goes in as the call's argument.
const BOX_ALLOC: &str = "__torajs_capture_box_alloc";

/// of that sid. Closed under prefix subtyping: an open sid's cells go
/// wherever its prefixes' do.
pub(crate) fn open_sids(module: &Module, envs: &EnvLayouts) -> Vec<bool> {
    let layouts = &module.struct_layouts;
    let globals: HashMap<&str, Type> = module
        .data_globals
        .iter()
        .map(|g| (g.name.as_str(), g.ty))
        .collect();
    let alloc = module.funcs.iter().position(|f| f.name == OBJ_ALLOC);
    let box_alloc = module.funcs.iter().position(|f| f.name == BOX_ALLOC);
    let holds = AnyHolds::of(module);
    let mut open = vec![false; layouts.len()];
    for (i, f) in module.funcs.iter().enumerate() {
        let mut scan = Scan {
            module,
            f,
            fid: FuncId(i as u32),
            holds: &holds,
            globals: &globals,
            envs,
            alloc,
            box_alloc,
            allocas: HashMap::new(),
            global_refs: HashMap::new(),
            data_of: HashMap::new(),
            fn_addrs: HashMap::new(),
            env_of: HashMap::new(),
            loaded: HashMap::new(),
            open: &mut open,
        };
        scan.run();
    }
    for sid in 0..layouts.len() {
        if !open[sid] {
            continue;
        }
        for (base, lay) in layouts.iter().enumerate() {
            if lay.len() < layouts[sid].len() && is_prefix(lay, &layouts[sid]) {
                open[base] = true;
            }
        }
    }
    open
}

/// `base` is `wide`'s layout by field name, field for field.
pub(crate) fn is_prefix(base: &[(PropKey, Type)], wide: &[(PropKey, Type)]) -> bool {
    base.len() <= wide.len() && base.iter().zip(wide).all(|((a, _), (b, _))| a == b)
}

struct Scan<'a> {
    module: &'a Module,
    f: &'a Function,
    fid: FuncId,
    holds: &'a AnyHolds<'a>,
    globals: &'a HashMap<&'a str, Type>,
    envs: &'a EnvLayouts,
    alloc: Option<usize>,
    box_alloc: Option<usize>,
    allocas: HashMap<ValueId, Type>,
    global_refs: HashMap<ValueId, &'a str>,
    /// Each array cell's data pointer, by the array it was read from.
    data_of: HashMap<ValueId, ArrId>,
    fn_addrs: HashMap<ValueId, FuncId>,
    /// Each closure env cell this function builds, by its closure.
    env_of: HashMap<ValueId, FuncId>,
    /// The struct layout the function reads through each `(base,
    /// offset)` that no declaration types.
    loaded: HashMap<(ValueId, u64), StructId>,
    open: &'a mut Vec<bool>,
}

impl Scan<'_> {
    fn run(&mut self) {
        let f = self.f;
        for inst in f.blocks.iter().flat_map(|b| &b.insts) {
            match (&inst.kind, inst.result) {
                (InstKind::Alloca(ty), Some(v)) => {
                    self.allocas.insert(v, *ty);
                }
                (InstKind::GlobalRef(name), Some(v)) => {
                    self.global_refs.insert(v, name);
                }
                (InstKind::FnAddr(fid), Some(v)) => {
                    self.fn_addrs.insert(v, *fid);
                }
                (InstKind::Load(Type::Ptr, Operand::Value(arr), ARR_DATA_PTR_OFF), Some(v)) => {
                    if let Type::Arr(a) = f.value_type(*arr) {
                        self.data_of.insert(v, a);
                    }
                }
                (
                    InstKind::Store(Operand::Value(fa), Operand::Value(env), CLOSURE_FN_ADDR_OFF),
                    _,
                ) => {
                    if let Some(fid) = self.fn_addrs.get(fa) {
                        self.env_of.insert(*env, *fid);
                    }
                }
                _ => {}
            }
        }
        self.type_box_slots();
        for block in &f.blocks {
            for inst in &block.insts {
                self.inst(inst.result, &inst.kind);
            }
            if let Terminator::Ret(Some(v)) = &block.term {
                self.reaches_slot(f.ret, v);
            }
        }
    }

    fn inst(&mut self, result: Option<ValueId>, kind: &InstKind) {
        let module = self.module;
        if let Some(v) = result {
            match self.f.value_type(v) {
                Type::Obj(sid) => self.defined(sid, kind),
                Type::Arr(a) => self.defined_arr(a, kind),
                _ => {}
            }
        }
        match kind {
            InstKind::Store(v, p, off) => {
                if let Some(ty) = self.slot(p, *off) {
                    self.reaches_slot(ty, v);
                }
            }
            InstKind::StoreDyn(v, base, _) | InstKind::StoreDynScaled8(v, base, _) => {
                let data = match base {
                    Operand::Value(b) => self.data_of.get(b).copied(),
                    _ => None,
                };
                let arr = data.or(match self.ty(base) {
                    Type::Arr(a) => Some(a),
                    _ => None,
                });
                if let Some(a) = arr {
                    self.reaches_slot(module.arr_layouts[a.0 as usize], v);
                }
            }
            InstKind::Call(fid, args) => {
                if Some(fid.0 as usize) == self.box_alloc
                    && let (Some(b), [payload]) = (result, args.as_slice())
                    && let Some(&sid) = self.loaded.get(&(b, 0))
                {
                    self.reaches(sid, payload);
                }
                let callee = &module.funcs[fid.0 as usize];
                for (p, arg) in callee.params.iter().zip(args) {
                    self.reaches_slot(callee.value_type(*p), arg);
                }
                if callee.blocks.is_empty() {
                    self.runtime_array_call(args);
                }
            }
            InstKind::CallIndirect(sig, _, args) => {
                let (params, _) = &module.signatures[sig.0 as usize];
                for (p, arg) in params.iter().zip(args) {
                    self.reaches_slot(*p, arg);
                }
            }
            _ => {}
        }
    }

    /// A runtime helper handed an array of cells may store any other
    /// argument into it (`push`, `unshift`, `fill`, `splice`): each
    /// such argument has to fit the element layout, and an array
    /// argument's elements have to.
    fn runtime_array_call(&mut self, args: &[Operand]) {
        for (i, arr) in args.iter().enumerate() {
            let Type::Arr(a) = self.ty(arr) else { continue };
            let elem = self.module.arr_layouts[a.0 as usize];
            if self.elem_obj(elem).is_none() {
                continue;
            }
            for (j, arg) in args.iter().enumerate() {
                match self.ty(arg) {
                    _ if j == i => {}
                    Type::Arr(_) => self.reaches_slot(Type::Arr(a), arg),
                    Type::Any | Type::Ptr | Type::Obj(_) => self.reaches_slot(elem, arg),
                    _ => {}
                }
            }
        }
    }

    /// A value typed `Obj(sid)` is born here: from the cell allocation
    /// itself, a slot read, or a call whose body answers for its own
    /// returns. Anything else (a decode out of `any`, a select over
    /// foreign operands, a runtime declaration) opens the sid.
    fn defined(&mut self, sid: StructId, kind: &InstKind) {
        let exact = match kind {
            InstKind::Call(fid, _) => {
                Some(fid.0 as usize) == self.alloc
                    || !self.module.funcs[fid.0 as usize].blocks.is_empty()
            }
            InstKind::Alloca(_)
            | InstKind::AllocaBytes(_)
            | InstKind::GlobalRef(_)
            | InstKind::Load(..)
            | InstKind::LoadDyn(..)
            | InstKind::LoadDynScaled8(..) => true,
            InstKind::Copy(_, v) | InstKind::Identity(v) => self.fits(sid, v),
            InstKind::Select(_, _, a, b) => self.fits(sid, a) && self.fits(sid, b),
            _ => false,
        };
        if !exact {
            self.open[sid.0 as usize] = true;
        }
    }

    /// A value typed as an array of cells is born here. A runtime
    /// call that takes an `any`, or an array whose elements do not fit
    /// this array's, builds the array out of it and the cells it holds
    /// are of any layout (`__torajs_arr_any_to_typed` takes the `any[]`
    /// itself, typed `Arr` of `Any` elements, not an `Any`); one whose
    /// arguments all fit (an allocation, a slice of a typed array)
    /// holds what typed arrays held.
    fn defined_arr(&mut self, a: ArrId, kind: &InstKind) {
        let elem = self.module.arr_layouts[a.0 as usize];
        let Some(sid) = self.elem_obj(elem) else {
            return;
        };
        let exact = match kind {
            InstKind::Call(fid, args) => {
                !self.module.funcs[fid.0 as usize].blocks.is_empty()
                    || args.iter().all(|v| match self.ty(v) {
                        Type::Any => false,
                        Type::Arr(_) => self.fits_slot(Type::Arr(a), v),
                        _ => true,
                    })
            }
            InstKind::Alloca(_)
            | InstKind::AllocaBytes(_)
            | InstKind::GlobalRef(_)
            | InstKind::Load(..)
            | InstKind::LoadDyn(..)
            | InstKind::LoadDynScaled8(..) => true,
            InstKind::Copy(_, v) | InstKind::Identity(v) => self.fits_slot(elem, v),
            InstKind::Select(_, _, x, y) => self.fits_slot(elem, x) && self.fits_slot(elem, y),
            _ => false,
        };
        if !exact {
            self.open[sid.0 as usize] = true;
        }
    }

    fn reaches(&mut self, sid: StructId, v: &Operand) {
        if !self.fits(sid, v) {
            self.open[sid.0 as usize] = true;
        }
    }

    /// `v` enters a slot of type `ty`: a cell slot, or an array slot
    /// whose elements (at any depth) are cells.
    fn reaches_slot(&mut self, ty: Type, v: &Operand) {
        if let Some(sid) = self.elem_obj(ty)
            && !self.fits_slot(ty, v)
        {
            self.open[sid.0 as usize] = true;
        }
    }

    /// Two layouts read through one untyped slot: the wider one is
    /// what a store there has to fit, and unrelated ones open each
    /// other.
    fn read_through(&mut self, base: ValueId, off: u64, sid: StructId) {
        let layouts = &self.module.struct_layouts;
        let lay = |s: StructId| &layouts[s.0 as usize];
        match self.loaded.get(&(base, off)).copied() {
            None => {
                self.loaded.insert((base, off), sid);
            }
            Some(seen) if seen == sid || is_prefix(lay(sid), lay(seen)) => {}
            Some(seen) if is_prefix(lay(seen), lay(sid)) => {
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
    fn fits(&self, sid: StructId, v: &Operand) -> bool {
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
    fn fits_slot(&self, ty: Type, v: &Operand) -> bool {
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
                        match (self.elem_obj(x), self.elem_obj(y)) {
                            (Some(s), Some(t)) => {
                                depth(self.module, x) == depth(self.module, y) && self.lays(s, t)
                            }
                            _ => false,
                        }
                    }
                    _ => false,
                },
                _ => false,
            },
            _ => true,
        }
    }

    fn lays(&self, sid: StructId, t: StructId) -> bool {
        let layouts = &self.module.struct_layouts;
        t == sid || is_prefix(&layouts[sid.0 as usize], &layouts[t.0 as usize])
    }

    /// The cell layout at the bottom of a (nested) array type.
    fn elem_obj(&self, ty: Type) -> Option<StructId> {
        match ty {
            Type::Obj(sid) => Some(sid),
            Type::Arr(a) => self.elem_obj(self.module.arr_layouts[a.0 as usize]),
            _ => None,
        }
    }

    fn slot(&self, p: &Operand, off: u64) -> Option<Type> {
        let Operand::Value(id) = p else { return None };
        self.declared_slot(*id, off)
            .or_else(|| self.loaded.get(&(*id, off)).map(|sid| Type::Obj(*sid)))
    }

    fn declared_slot(&self, id: ValueId, off: u64) -> Option<Type> {
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

    fn ty(&self, v: &Operand) -> Type {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssa::{BlockId, DataGlobal, FuncId};
    use crate::ssa_lower::CLOSURE_CAP_BASE_OFF;

    fn layout(names: &[&str]) -> Vec<(PropKey, Type)> {
        names
            .iter()
            .map(|n| (PropKey::from(*n), Type::I64))
            .collect()
    }

    /// Layouts 0 `{a}`, 1 `{a, b}` (extends 0), 2 `{x, y}`; funcs 0
    /// and 1 are the cell and capture-box allocator declarations, 2 a
    /// declaration answering `any`, 3 one answering a pointer.
    fn module() -> Module {
        let mut m = Module::default();
        m.struct_layouts = vec![layout(&["a"]), layout(&["a", "b"]), layout(&["x", "y"])];
        for name in [OBJ_ALLOC, BOX_ALLOC] {
            let mut alloc = Function::new(name, Type::Ptr);
            alloc.add_param(Type::I64, "n");
            m.funcs.push(alloc);
        }
        m.funcs.push(Function::new("opaque", Type::Any));
        let mut helper = Function::new("helper", Type::Ptr);
        helper.add_param(Type::Ptr, "p");
        helper.add_param(Type::I64, "v");
        m.funcs.push(helper);
        m
    }

    fn open(m: &Module) -> Vec<bool> {
        open_sids(m, &EnvLayouts::new())
    }

    fn body(m: &mut Module, build: impl FnOnce(&mut Function, BlockId)) -> FuncId {
        let mut f = Function::new("f", Type::Void);
        let entry = f.add_block();
        build(&mut f, entry);
        f.set_term(entry, Terminator::Ret(None));
        let id = FuncId(m.funcs.len() as u32);
        m.funcs.push(f);
        id
    }

    /// An `any` nothing in the module accounts for: a declaration's
    /// answer.
    fn any(f: &mut Function, entry: BlockId) -> Operand {
        Operand::Value(f.append_inst(entry, InstKind::Call(FuncId(2), vec![]), Type::Any, None))
    }

    fn cell(f: &mut Function, entry: BlockId, sid: u32) -> Operand {
        Operand::Value(f.append_inst(
            entry,
            InstKind::Call(FuncId(0), vec![Operand::ConstI64(40)]),
            Type::Obj(StructId(sid)),
            None,
        ))
    }

    /// An array cell of `arr` and its data pointer, read as lowering
    /// reads it.
    fn arr_data(f: &mut Function, entry: BlockId, arr: ArrId) -> (ValueId, Operand) {
        let xs = f.append_inst(entry, InstKind::AllocaBytes(8), Type::Arr(arr), None);
        let data = f.append_inst(
            entry,
            InstKind::Load(Type::Ptr, Operand::Value(xs), ARR_DATA_PTR_OFF),
            Type::Ptr,
            None,
        );
        (xs, Operand::Value(data))
    }

    #[test]
    fn an_any_stored_into_a_typed_let_slot_opens_it_and_its_prefixes() {
        let mut m = module();
        body(&mut m, |f, entry| {
            let slot = f.append_inst(
                entry,
                InstKind::Alloca(Type::Obj(StructId(1))),
                Type::Ptr,
                None,
            );
            let v = any(f, entry);
            f.append_void(entry, InstKind::Store(v, Operand::Value(slot), 0));
        });
        assert_eq!(open(&m), vec![true, true, false]);
    }

    #[test]
    fn a_cell_of_the_slots_own_or_wider_layout_keeps_it_closed() {
        let mut m = module();
        body(&mut m, |f, entry| {
            let slot = f.append_inst(
                entry,
                InstKind::Alloca(Type::Obj(StructId(0))),
                Type::Ptr,
                None,
            );
            let wide = cell(f, entry, 1);
            f.append_void(entry, InstKind::Store(wide, Operand::Value(slot), 0));
            f.append_void(
                entry,
                InstKind::Store(Operand::ConstPtrNull, Operand::Value(slot), 0),
            );
        });
        assert_eq!(open(&m), vec![false, false, false]);
    }

    #[test]
    fn a_foreign_layout_in_a_field_a_global_and_an_element_opens_each() {
        let mut m = module();
        m.struct_layouts
            .push(vec![(PropKey::from("child"), Type::Obj(StructId(2)))]);
        m.data_globals.push(DataGlobal {
            name: "g".into(),
            ty: Type::Obj(StructId(1)),
        });
        let arr = m.intern_arr(Type::Obj(StructId(0)));
        body(&mut m, |f, entry| {
            let holder = cell(f, entry, 3);
            let foreign = cell(f, entry, 1);
            f.append_void(
                entry,
                InstKind::Store(foreign.clone(), holder, OBJ_HEADER_SIZE),
            );
            let g = f.append_inst(entry, InstKind::GlobalRef("g".into()), Type::Ptr, None);
            let v = any(f, entry);
            f.append_void(entry, InstKind::Store(v, Operand::Value(g), 0));
            let (_, data) = arr_data(f, entry, arr);
            let v = any(f, entry);
            f.append_void(entry, InstKind::StoreDyn(v, data, Operand::ConstI64(0)));
        });
        assert_eq!(open(&m), vec![true, true, true, false]);
    }

    #[test]
    fn the_four_element_channels_open_the_element_layout() {
        let arr_of = |m: &mut Module, sid: u32| m.intern_arr(Type::Obj(StructId(sid)));
        // a literal's element store, through the data pointer
        let mut m = module();
        let arr = arr_of(&mut m, 2);
        body(&mut m, |f, entry| {
            let (_, data) = arr_data(f, entry, arr);
            let v = any(f, entry);
            f.append_void(entry, InstKind::Store(v, data, 8));
        });
        assert_eq!(open(&m), vec![false, false, true]);
        // a runtime push of an `any` into a typed array
        let mut m = module();
        let arr = arr_of(&mut m, 2);
        body(&mut m, |f, entry| {
            let (xs, _) = arr_data(f, entry, arr);
            let v = any(f, entry);
            f.append_inst(
                entry,
                InstKind::Call(FuncId(3), vec![Operand::Value(xs), v]),
                Type::Ptr,
                None,
            );
        });
        assert_eq!(open(&m), vec![false, false, true]);
        // a typed array built by a runtime call out of an `any[]`:
        // the argument is the array of `any` elements itself, as the
        // typed declaration's conversion passes it
        let mut m = module();
        let arr = arr_of(&mut m, 2);
        let any_arr = ArrId(m.arr_layouts.len() as u32);
        m.arr_layouts.push(Type::Any);
        body(&mut m, |f, entry| {
            let v = f.append_inst(entry, InstKind::AllocaBytes(8), Type::Arr(any_arr), None);
            f.append_inst(
                entry,
                InstKind::Call(FuncId(3), vec![Operand::Value(v), Operand::ConstI64(4)]),
                Type::Arr(arr),
                None,
            );
        });
        assert_eq!(open(&m), vec![false, false, true]);
        // the same call handed a fitting typed array (a slice) keeps
        // the element layout exact
        let mut m = module();
        let arr = arr_of(&mut m, 2);
        body(&mut m, |f, entry| {
            let v = f.append_inst(entry, InstKind::AllocaBytes(8), Type::Arr(arr), None);
            f.append_inst(
                entry,
                InstKind::Call(FuncId(3), vec![Operand::Value(v), Operand::ConstI64(4)]),
                Type::Arr(arr),
                None,
            );
        });
        assert_eq!(open(&m), vec![false, false, false]);
        // an array of another layout stored into an array-typed slot
        let mut m = module();
        let arr = arr_of(&mut m, 2);
        let other = arr_of(&mut m, 0);
        body(&mut m, |f, entry| {
            let slot = f.append_inst(entry, InstKind::Alloca(Type::Arr(arr)), Type::Ptr, None);
            let ys = f.append_inst(entry, InstKind::AllocaBytes(8), Type::Arr(other), None);
            f.append_void(
                entry,
                InstKind::Store(Operand::Value(ys), Operand::Value(slot), 0),
            );
        });
        assert_eq!(open(&m), vec![false, false, true]);
    }

    #[test]
    fn a_runtime_call_over_typed_arrays_and_scalars_keeps_them_closed() {
        let mut m = module();
        let arr = m.intern_arr(Type::Obj(StructId(0)));
        body(&mut m, |f, entry| {
            let (xs, _) = arr_data(f, entry, arr);
            let wide = cell(f, entry, 1);
            f.append_inst(
                entry,
                InstKind::Call(FuncId(3), vec![Operand::Value(xs), wide]),
                Type::Ptr,
                None,
            );
            let n = f.append_inst(
                entry,
                InstKind::Call(FuncId(3), vec![Operand::Value(xs), Operand::ConstI64(3)]),
                Type::I64,
                None,
            );
            f.append_inst(
                entry,
                InstKind::Call(FuncId(3), vec![Operand::Value(xs), Operand::Value(n)]),
                Type::Arr(arr),
                None,
            );
        });
        assert_eq!(open(&m), vec![false, false, false]);
    }

    #[test]
    fn a_typed_parameter_and_a_declared_return_open_on_an_any() {
        let mut m = module();
        let mut callee = Function::new("take", Type::Obj(StructId(2)));
        callee.add_param(Type::Obj(StructId(1)), "b");
        let entry = callee.add_block();
        let v = any(&mut callee, entry);
        callee.set_term(entry, Terminator::Ret(Some(v)));
        let take = FuncId(m.funcs.len() as u32);
        m.funcs.push(callee);
        body(&mut m, |f, entry| {
            let v = any(f, entry);
            f.append_void(entry, InstKind::Call(take, vec![v]));
        });
        assert_eq!(open(&m), vec![true, true, true]);
    }

    #[test]
    fn a_decode_out_of_any_typed_as_a_struct_opens_it() {
        let mut m = module();
        body(&mut m, |f, entry| {
            f.append_inst(
                entry,
                InstKind::IntToPtr(Operand::ConstI64(0)),
                Type::Obj(StructId(2)),
                None,
            );
        });
        assert_eq!(open(&m), vec![false, false, true]);
    }

    #[test]
    fn a_slot_behind_a_pointer_is_typed_by_what_is_loaded_from_it() {
        let mut m = module();
        body(&mut m, |f, entry| {
            let v = any(f, entry);
            let bx = f.append_inst(entry, InstKind::Call(FuncId(1), vec![v]), Type::Ptr, None);
            f.append_inst(
                entry,
                InstKind::Load(Type::Obj(StructId(2)), Operand::Value(bx), 0),
                Type::Obj(StructId(2)),
                None,
            );
            let frame = f.append_inst(entry, InstKind::AllocaBytes(16), Type::Ptr, None);
            f.append_inst(
                entry,
                InstKind::Load(Type::Obj(StructId(0)), Operand::Value(frame), 8),
                Type::Obj(StructId(0)),
                None,
            );
            let own = cell(f, entry, 1);
            f.append_void(entry, InstKind::Store(own, Operand::Value(frame), 8));
        });
        assert_eq!(open(&m), vec![false, false, true]);
        body(&mut m, |f, entry| {
            let frame = f.append_inst(entry, InstKind::AllocaBytes(16), Type::Ptr, None);
            f.append_inst(
                entry,
                InstKind::Load(Type::Obj(StructId(1)), Operand::Value(frame), 0),
                Type::Obj(StructId(1)),
                None,
            );
            let v = any(f, entry);
            f.append_void(entry, InstKind::Store(v, Operand::Value(frame), 0));
        });
        assert_eq!(open(&m), vec![true, true, true]);
    }

    /// A boxed capture of layout 2 neither function reads as a cell:
    /// the box is typed by the closure's capture list on both sides.
    #[test]
    fn a_capture_box_is_typed_by_the_closures_capture_list() {
        let mut m = module();
        // the closure body: loads the box from its env and writes an
        // `any` into it
        let mut clo = Function::new("__closure_0", Type::Void);
        let p0 = clo.add_param(Type::Ptr, "__env");
        let entry = clo.add_block();
        let slot = clo.append_inst(entry, InstKind::Alloca(Type::Ptr), Type::Ptr, None);
        clo.append_void(
            entry,
            InstKind::Store(Operand::Value(p0), Operand::Value(slot), 0),
        );
        let env = clo.append_inst(
            entry,
            InstKind::Load(Type::Ptr, Operand::Value(slot), 0),
            Type::Ptr,
            None,
        );
        let bx = clo.append_inst(
            entry,
            InstKind::Load(Type::Ptr, Operand::Value(env), CLOSURE_CAP_BASE_OFF),
            Type::Ptr,
            None,
        );
        let v = any(&mut clo, entry);
        clo.append_void(entry, InstKind::Store(v, Operand::Value(bx), 0));
        clo.set_term(entry, Terminator::Ret(None));
        let clo_id = FuncId(m.funcs.len() as u32);
        m.funcs.push(clo);
        let mut envs = EnvLayouts::new();
        envs.insert(clo_id, vec![(Type::Obj(StructId(2)), true)]);
        assert_eq!(open_sids(&m, &envs), vec![false, false, true]);
        // the declaring side: boxes an `any` payload and stores the
        // box into the env it builds for the closure
        let mut m = module();
        m.funcs.push(Function::new("__closure_0", Type::Void));
        body(&mut m, |f, entry| {
            let v = any(f, entry);
            let bx = f.append_inst(entry, InstKind::Call(FuncId(1), vec![v]), Type::Ptr, None);
            let env = f.append_inst(
                entry,
                InstKind::Call(FuncId(0), vec![Operand::ConstI64(56)]),
                Type::Ptr,
                None,
            );
            let fa = f.append_inst(entry, InstKind::FnAddr(clo_id), Type::Ptr, None);
            f.append_void(
                entry,
                InstKind::Store(Operand::Value(fa), Operand::Value(env), CLOSURE_FN_ADDR_OFF),
            );
            f.append_void(
                entry,
                InstKind::Store(
                    Operand::Value(bx),
                    Operand::Value(env),
                    CLOSURE_CAP_BASE_OFF,
                ),
            );
        });
        assert_eq!(open_sids(&m, &envs), vec![false, false, true]);
        assert_eq!(open(&m), vec![false, false, false]);
    }
}
