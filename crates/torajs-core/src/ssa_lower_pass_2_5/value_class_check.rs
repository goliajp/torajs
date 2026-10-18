//! The finished SSA's word on the value classes of pass 0.5
//! (`ssa_lower_pass_0_5::value_class_census`). The census reads syntax
//! and this reads what lowering emitted, so the two fail in different
//! ways; where they disagree, the census has let in a class whose
//! instances some code can tell apart from a copy of their fields, and
//! the compile fails with an internal error instead of going on.
//!
//! In a live function, a cell of a value class, or an array of them,
//! is never:
//! - boxed into `any` or taken apart into raw bits (`BoxAny`,
//!   `PtrToInt`);
//! - passed where the callee takes an `any`, a cell or array of
//!   another layout, an integer (but the element word of a push onto
//!   an array of its own class), or a pointer the runtime reads
//!   through (all but [`ARRAY_ENTRIES`] / [`CELL_ENTRIES`]);
//! - compared with another cell (`ICmp`: identity; a test against a
//!   sentinel the module names is not one);
//! - stored anywhere but a local slot of its own type, nor returned
//!   but by its own factory (a cell, not an array, in both cases);
//!
//! and no slot of its layout is open to a cell of another
//! ([`super::cell_slot_flow::open_sids`]).
//! `TORAJS_VALUE_CLASS_STATS=1` prints each disagreement, then how
//! many there were over how many live functions.

use std::collections::{HashMap, HashSet};

use crate::ast::Ast;
use crate::ssa::{
    FuncId, Function, InstKind, Module, Operand, StructId, Terminator, Type, ValueId,
};
use crate::ssa_lower::proofs::ProgramFacts;

use super::any_holds::dead_adapters;

#[cfg(test)]
mod tests;

/// Runtime entries an array of a value class may be handed to through
/// a pointer: the element push (an inline array builds the element in
/// its slot instead; codegen expands this call with 8-byte slots), the
/// per-element release (an inline array is dropped as a scalar one),
/// and the element-kind mark a struct field store writes (an inline
/// array is marked with its own kind). Every other runtime entry that
/// takes one — sort, search, join, print, JSON, iteration, the `any`
/// lanes — reads its slots as 8-byte words, and is a disagreement.
const ARRAY_ENTRIES: &[&str] = &[
    "__torajs_arr_push",
    "__torajs_arr_drop_leaf_instances",
    "__torajs_arr_mark_kind",
];

/// Runtime entries a cell of a value class may be handed to through a
/// pointer: the release of the cell the factory built (an element
/// built in its slot has no cell to release) and the nullish test of a
/// receiver, which never reads through it (an inline element is tested
/// by its bounds instead). Every other one reads the cell as an object.
const CELL_ENTRIES: &[&str] = &[
    "__torajs_obj_drop_sized",
    "__torajs_obj_props_drop_slow",
    "__torajs_obj_buffer_slow",
    "__torajs_weakref_target_dying",
    "__torajs_heap_nullish_check",
];

/// Fails the compile when the SSA disagrees with the census. A method
/// twin the census proved no receiver enters is not live, whatever
/// references it.
pub(crate) fn report(module: &Module, ast: &Ast, facts: &ProgramFacts, open: &[bool]) {
    if facts.value_classes.is_empty() {
        return;
    }
    let dead_twin =
        |name: &str| facts.twins_unreachable && ast.cmany_twins.values().any(|t| t == name);
    let (found, live) = disagreements(module, &facts.value_classes, open, &dead_twin);
    if std::env::var_os("TORAJS_VALUE_CLASS_STATS").is_some() {
        for d in &found {
            eprintln!("[value_class] census and SSA disagree: {d}");
        }
        eprintln!(
            "[value_class] SSA check: {} disagreements over {live} live functions",
            found.len()
        );
    }
    if let Some(first) = found.first() {
        panic!(
            "internal error: the value-class census and the SSA disagree ({} in all): {first}",
            found.len()
        );
    }
}

/// Every disagreement, and how many live functions were read.
pub(super) fn disagreements(
    module: &Module,
    value_classes: &HashSet<StructId>,
    open: &[bool],
    skip: &dyn Fn(&str) -> bool,
) -> (Vec<String>, usize) {
    let mut sids: Vec<&StructId> = value_classes.iter().collect();
    sids.sort_by_key(|s| s.0);
    let mut found: Vec<String> = (sids.iter())
        .filter(|s| open[s.0 as usize])
        .map(|s| format!("sid {} / open slot", s.0))
        .collect();
    let dead = dead_adapters(module);
    let mut live = 0;
    for (i, f) in module.funcs.iter().enumerate() {
        if dead[i] || skip(&f.name) {
            continue;
        }
        live += 1;
        let r = Reader::new(module, f, value_classes);
        for b in &f.blocks {
            for inst in &b.insts {
                for (sid, what) in r.inst(&inst.kind) {
                    found.push(format!("sid {} / {} / {what}", sid.0, f.name));
                }
            }
            if let Some((sid, what)) = r.ret(&b.term) {
                found.push(format!("sid {} / {} / {what}", sid.0, f.name));
            }
        }
    }
    (found, live)
}

struct Reader<'a> {
    module: &'a Module,
    f: &'a Function,
    value_classes: &'a HashSet<StructId>,
    /// The type of each local slot of the function.
    allocas: HashMap<ValueId, Type>,
    /// Values that are the address of a named global (a sentinel).
    globals: HashSet<ValueId>,
}

impl<'a> Reader<'a> {
    fn new(module: &'a Module, f: &'a Function, value_classes: &'a HashSet<StructId>) -> Self {
        let mut r = Reader {
            module,
            f,
            value_classes,
            allocas: HashMap::new(),
            globals: HashSet::new(),
        };
        for i in f.blocks.iter().flat_map(|b| &b.insts) {
            match (&i.kind, i.result) {
                (InstKind::Alloca(t), Some(v)) => {
                    r.allocas.insert(v, *t);
                }
                (InstKind::GlobalRef(_), Some(v)) => {
                    r.globals.insert(v);
                }
                _ => {}
            }
        }
        r
    }

    /// The value class an operand holds a cell or an array of, with
    /// the operand's type.
    fn held(&self, o: &Operand) -> Option<(StructId, Type)> {
        let Operand::Value(v) = o else { return None };
        // a sentinel typed as the cell is not an instance
        if self.globals.contains(v) {
            return None;
        }
        let t = self.f.value_type(*v);
        let sid = match t {
            Type::Obj(sid) => sid,
            Type::Arr(a) => match self.module.arr_layouts[a.0 as usize] {
                Type::Obj(sid) => sid,
                _ => return None,
            },
            _ => return None,
        };
        self.value_classes.contains(&sid).then_some((sid, t))
    }

    /// A cell (not an array) of a value class.
    fn cell(&self, o: &Operand) -> Option<StructId> {
        self.held(o)
            .filter(|(_, t)| matches!(t, Type::Obj(_)))
            .map(|(s, _)| s)
    }

    fn inst(&self, kind: &InstKind) -> Vec<(StructId, String)> {
        match kind {
            InstKind::BoxAny(_, v) => self
                .held(v)
                .map(|(s, _)| (s, "boxed".into()))
                .into_iter()
                .collect(),
            InstKind::PtrToInt(v) => self
                .held(v)
                .map(|(s, _)| (s, "bits".into()))
                .into_iter()
                .collect(),
            InstKind::Call(fid, a) => self.call(*fid, a),
            InstKind::CallIndirect(sig, _, a) => {
                let params = (self.module.signatures.get(sig.0 as usize)).map(|s| s.0.as_slice());
                self.args(params.unwrap_or_default(), a, "an indirect call", |_| false)
            }
            InstKind::ICmp(_, a, b) => {
                let other_cell = |o: &Operand| match o {
                    Operand::Value(v) => {
                        matches!(self.f.value_type(*v), Type::Obj(_)) && !self.globals.contains(v)
                    }
                    _ => false,
                };
                let hit = (self.cell(a).filter(|_| other_cell(b)))
                    .or_else(|| self.cell(b).filter(|_| other_cell(a)));
                hit.map(|s| (s, format!("compared: {kind:?}")))
                    .into_iter()
                    .collect()
            }
            InstKind::Store(v, slot, _) => {
                let own = |s: StructId| match slot {
                    Operand::Value(p) => self.allocas.get(p) == Some(&Type::Obj(s)),
                    _ => false,
                };
                (self.cell(v).filter(|s| !own(*s)))
                    .map(|s| (s, format!("stored: {kind:?}")))
                    .into_iter()
                    .collect()
            }
            InstKind::StoreDyn(v, _, _) | InstKind::StoreDynScaled8(v, _, _) => self
                .cell(v)
                .map(|s| (s, format!("stored: {kind:?}")))
                .into_iter()
                .collect(),
            _ => Vec::new(),
        }
    }

    fn call(&self, fid: FuncId, args: &[Operand]) -> Vec<(StructId, String)> {
        let callee = &self.module.funcs[fid.0 as usize];
        let params: Vec<Type> = (callee.params.iter())
            .map(|p| callee.value_type(*p))
            .collect();
        let name = callee.name.as_str();
        let entry = |own: Type| {
            let list = match own {
                Type::Obj(_) => CELL_ENTRIES,
                _ => ARRAY_ENTRIES,
            };
            callee.is_declaration() && list.contains(&name)
        };
        self.args(&params, args, name, entry)
    }

    /// The arguments handed somewhere their layout is not what the
    /// parameter reads; `entry` says a pointer parameter taking a value
    /// of this type does not read through it.
    fn args(
        &self,
        params: &[Type],
        args: &[Operand],
        callee: &str,
        entry: impl Fn(Type) -> bool,
    ) -> Vec<(StructId, String)> {
        // an integer parameter takes the cell as an element word when
        // another argument is an array of its own class (a push)
        let elem_home = |own: Type| {
            matches!(own, Type::Obj(_))
                && args.iter().any(|o| {
                    matches!(o, Operand::Value(v) if matches!(self.f.value_type(*v),
                        Type::Arr(a) if self.module.arr_layouts[a.0 as usize] == own))
                })
        };
        (args.iter().enumerate())
            .filter_map(|(i, a)| {
                let (sid, own) = self.held(a)?;
                let escapes = match params.get(i).copied() {
                    Some(p) if p == own => false,
                    Some(Type::Obj(_) | Type::Arr(_) | Type::Any) | None => true,
                    Some(Type::I64) => !elem_home(own),
                    Some(Type::Ptr) => !entry(own),
                    Some(_) => false,
                };
                escapes.then(|| (sid, format!("passed to {callee} as argument {i}")))
            })
            .collect()
    }

    /// A cell returned by anything but its own factory.
    fn ret(&self, term: &Terminator) -> Option<(StructId, String)> {
        let Terminator::Ret(Some(v)) = term else {
            return None;
        };
        let sid = self.cell(v)?;
        let factory = self.f.name.starts_with("__new_") && self.f.ret == Type::Obj(sid);
        (!factory).then(|| (sid, "returned".to_string()))
    }
}
