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
//!
//! Every place a cell changes static type (it fits a slot of a prefix
//! of its layout, or one slot is read under two types) must go through
//! `Scan::lays`, which records the path (`paths`); one that does not is
//! a slot left closed while it holds a foreign cell.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::ast::PropKey;
use crate::ssa::{
    ArrId, FuncId, Function, InstKind, Module, Operand, StructId, Terminator, Type, ValueId,
};
use crate::ssa_lower::{ARR_DATA_PTR_OFF, CLOSURE_FN_ADDR_OFF, OBJ_HEADER_SIZE};

mod env;
mod fit;
mod paths;

use super::any_holds::AnyHolds;
pub(crate) use env::{EnvLayouts, env_layouts};
use paths::Paths;

const OBJ_ALLOC: &str = "__torajs_obj_alloc";
/// A capture box answers the pointer to its value slot; the payload
/// goes in as the call's argument.
const BOX_ALLOC: &str = "__torajs_capture_box_alloc";

/// Per sid, whether a slot of that sid can hold a cell of another
/// layout. A slot of a prefix of an open sid is open too once a cell
/// read out of an open slot can reach it (`paths`). A boxed adapter
/// nothing references never runs, and what its body hands on reaches
/// no slot.
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
    let paths = RefCell::new(Paths::default());
    for fid in holds.address_taken() {
        for p in &module.funcs[fid.0 as usize].params {
            let ty = module.funcs[fid.0 as usize].value_type(*p);
            paths.borrow_mut().entered_blind(module, ty);
        }
    }
    for (i, f) in module.funcs.iter().enumerate() {
        if holds.is_dead(FuncId(i as u32)) {
            continue;
        }
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
            paths: &paths,
        };
        scan.run();
    }
    paths.into_inner().settle(&mut open);
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
    paths: &'a RefCell<Paths>,
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
                let callee = &self.module.funcs[fid.0 as usize];
                Some(fid.0 as usize) == self.alloc
                    || (!callee.blocks.is_empty() && self.answered_as(sid, callee.ret))
            }
            InstKind::Load(_, Operand::Value(p), off) => self.read_as(sid, *p, *off),
            InstKind::LoadDyn(_, Operand::Value(p), _)
            | InstKind::LoadDynScaled8(_, Operand::Value(p), _) => self.read_as(sid, *p, 0),
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

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_paths;
