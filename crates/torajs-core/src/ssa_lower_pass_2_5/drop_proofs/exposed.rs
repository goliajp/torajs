//! Which cell layouts can reach the runtime as JS values.
//!
//! The runtime attaches an expando bag to a cell, registers it as a
//! WeakRef target, or hands it to user code through an `any` lane only
//! when the cell crossed into its hands as a value: boxed into `any`,
//! taken apart into bits, passed where a function takes something else
//! than a cell (a runtime entry, a parameter of another type), returned
//! as something else, or held in a field or element of a container
//! that crossed. A cell that only moves between cell-typed slots, is
//! read, written, compared, counted and released stays with typed code.
//! Each such use is whitelisted below; every other use of an operand
//! holding cells exposes the layout of what it holds.

use crate::ssa::{FuncId, Function, InstKind, Module, Operand, StructId, Terminator, Type};
use crate::ssa_lower::OBJ_HEADER_SIZE;

use super::super::cell_slot_flow::elem_obj;

/// Runtime entries that take a cell and keep it to themselves: they
/// release it, buffer it for the cycle collector (which walks it by its
/// tag's layout row and runs no user code), test it for nullish, or
/// store it as an element of a typed array.
const CELL_ENTRIES: &[&str] = &[
    "__torajs_obj_drop_sized",
    "__torajs_obj_free_cell",
    "__torajs_obj_drop_rc",
    "__torajs_obj_props_drop_slow",
    "__torajs_obj_buffer_slow",
    "__torajs_cycle_buffer",
    "__torajs_weakref_target_dying",
    "__torajs_heap_nullish_check",
    "__torajs_drop_release",
    "__torajs_arr_push",
];

/// Runtime entries that take an array of cells and never hand an
/// element out: allocation, growth, the element-kind mark, the bounds
/// and null checks, the element push and the releases.
const ARRAY_ENTRIES: &[&str] = &[
    "__torajs_arr_alloc",
    "__torajs_arr_reserve",
    "__torajs_arr_push",
    "__torajs_arr_mark_kind",
    "__torajs_arr_null_check",
    "__torajs_arr_index_check_store",
    "__torajs_arr_index_revive_idx",
    "__torajs_arr_typed_set_grow",
    "__torajs_arr_set_length_validate",
];
const ARRAY_DROP_PREFIX: &str = "__torajs_arr_drop";

/// Per sid, whether its cells can reach the runtime as values, over
/// the functions `skip` does not rule out; closed over containment (a
/// crossed container exposes what its fields and elements hold).
pub(super) fn exposed_sids(module: &Module, skip: &[bool]) -> Vec<bool> {
    let mut out = vec![false; module.struct_layouts.len()];
    for (i, f) in module.funcs.iter().enumerate() {
        if skip[i] || f.blocks.is_empty() {
            continue;
        }
        let mut s = Scan {
            module,
            f,
            out: &mut out,
        };
        s.run();
    }
    let mut work: Vec<usize> = (0..out.len()).filter(|s| out[*s]).collect();
    while let Some(s) = work.pop() {
        for (_, fty) in &module.struct_layouts[s] {
            if let Some(n) = elem_obj(module, *fty)
                && !std::mem::replace(&mut out[n.0 as usize], true)
            {
                work.push(n.0 as usize);
            }
        }
    }
    out
}

struct Scan<'a> {
    module: &'a Module,
    f: &'a Function,
    out: &'a mut Vec<bool>,
}

/// What an operand holds: a cell (or a pointer read out of a cell
/// field), or an array of cells.
#[derive(Clone, Copy, PartialEq)]
enum Held {
    Cell(StructId),
    Array(StructId),
}

impl Scan<'_> {
    fn held(&self, o: &Operand) -> Option<Held> {
        let Operand::Value(v) = o else { return None };
        match self.f.value_type(*v) {
            Type::Obj(s) => Some(Held::Cell(s)),
            t @ Type::Arr(_) => elem_obj(self.module, t).map(Held::Array),
            _ => None,
        }
    }

    fn expose(&mut self, h: Option<Held>) {
        if let Some(Held::Cell(s) | Held::Array(s)) = h {
            self.out[s.0 as usize] = true;
        }
    }

    fn run(&mut self) {
        let f = self.f;
        // a bare pointer read out of a cell's cell-typed field holds
        // that field's cells
        let mut field_ptrs = std::collections::HashMap::new();
        for inst in f.blocks.iter().flat_map(|b| &b.insts) {
            if let (InstKind::Load(Type::Ptr, Operand::Value(b), off), Some(v)) =
                (&inst.kind, inst.result)
                && let Type::Obj(s) = f.value_type(*b)
                && *off >= OBJ_HEADER_SIZE
                && let Some((_, fty)) = self.module.struct_layouts[s.0 as usize]
                    .get(((off - OBJ_HEADER_SIZE) / 8) as usize)
                && let Some(n) = elem_obj(self.module, *fty)
            {
                field_ptrs.insert(v, n);
            }
        }
        for b in &f.blocks {
            for inst in &b.insts {
                self.inst(
                    &inst.kind,
                    inst.result.map(|r| f.value_type(r)),
                    &field_ptrs,
                );
            }
            if let Terminator::Ret(Some(v)) = &b.term
                && !same_kind(self.held(v), f.ret)
            {
                let h = self.held(v);
                self.expose(h);
            }
        }
    }

    fn inst(
        &mut self,
        kind: &InstKind,
        result: Option<Type>,
        field_ptrs: &std::collections::HashMap<crate::ssa::ValueId, StructId>,
    ) {
        match kind {
            InstKind::Load(..)
            | InstKind::LoadDyn(..)
            | InstKind::LoadDynScaled8(..)
            | InstKind::LoadU8Dyn(..)
            | InstKind::LoadU16Dyn(..)
            | InstKind::Store(..)
            | InstKind::StoreDyn(..)
            | InstKind::StoreDynScaled8(..)
            | InstKind::ICmp(..)
            | InstKind::RcInc(_) => {}
            InstKind::Copy(_, o) | InstKind::Identity(o) => {
                if !same_kind(self.held(o), result.unwrap_or(Type::Void)) {
                    let h = self.held(o);
                    self.expose(h);
                }
            }
            InstKind::Select(_, _, a, b) => {
                for o in [a, b] {
                    if !same_kind(self.held(o), result.unwrap_or(Type::Void)) {
                        let h = self.held(o);
                        self.expose(h);
                    }
                }
            }
            InstKind::Call(fid, args) => self.call(*fid, args, field_ptrs),
            InstKind::CallIndirect(sig, _, args) => {
                let params = (self.module.signatures.get(sig.0 as usize)).map(|s| s.0.clone());
                let params = params.unwrap_or_default();
                for (i, a) in args.iter().enumerate() {
                    let pt = params.get(i).copied().unwrap_or(Type::Void);
                    if !same_kind(self.held(a), pt) {
                        let h = self.held(a);
                        self.expose(h);
                    }
                }
            }
            _ => crate::ssa::visit_value_operands(kind, |v| {
                let h = self.held(&Operand::Value(v));
                self.expose(h);
                if let Some(s) = field_ptrs.get(&v) {
                    self.out[s.0 as usize] = true;
                }
            }),
        }
    }

    fn call(
        &mut self,
        fid: FuncId,
        args: &[Operand],
        field_ptrs: &std::collections::HashMap<crate::ssa::ValueId, StructId>,
    ) {
        let callee = &self.module.funcs[fid.0 as usize];
        let name = callee.name.as_str();
        let runtime = callee.blocks.is_empty();
        let release = self.module.class_drop_fns.contains(&fid);
        for (i, a) in args.iter().enumerate() {
            let held = self.held(a);
            let ok = match held {
                None => {
                    // a field pointer handed to a runtime entry
                    let ptr = matches!(a, Operand::Value(v) if field_ptrs.contains_key(v));
                    if ptr && runtime && !CELL_ENTRIES.contains(&name) {
                        let Operand::Value(v) = a else { unreachable!() };
                        let s = field_ptrs[v];
                        self.out[s.0 as usize] = true;
                    }
                    continue;
                }
                Some(Held::Cell(_)) if runtime => CELL_ENTRIES.contains(&name),
                Some(Held::Array(_)) if runtime => {
                    ARRAY_ENTRIES.contains(&name) || name.starts_with(ARRAY_DROP_PREFIX)
                }
                Some(Held::Cell(_)) if release => true,
                Some(h) => {
                    let pt = (callee.params.get(i)).map(|p| callee.value_type(*p));
                    same_kind(Some(h), pt.unwrap_or(Type::Void))
                }
            };
            if !ok {
                self.expose(held);
            }
        }
    }
}

/// Whether a slot of type `ty` takes what `h` holds as what it is: a
/// cell as a cell, an array as an array. Nothing held always fits.
fn same_kind(h: Option<Held>, ty: Type) -> bool {
    match h {
        None => true,
        Some(Held::Cell(_)) => matches!(ty, Type::Obj(_)),
        Some(Held::Array(_)) => matches!(ty, Type::Arr(_)),
    }
}
