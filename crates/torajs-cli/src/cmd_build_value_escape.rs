//! Whether a typed heap value can leave the typed lanes.
//!
//! The any world reaches a heap cell only through a value that left
//! the typed lanes: a box (`BoxAny`), a store into a heap slot (a
//! field, an array element, a closure env, a generator frame) whose
//! reader may box it later, a runtime kernel that takes it, a cast to
//! an untyped word. A value whose every use stays typed — a local or
//! global slot of its own type, a user fn parameter of its own type,
//! a return of its own type, a comparison (of the pointer, or of its
//! integer view when that view is only ever compared — a null test),
//! refcount traffic, a load or store through it as the address, and a
//! kernel the caller names as one that keeps it ([`escapes`]'s
//! `sink`) — never reaches the any world, whatever the rest of the
//! program does.
//!
//! The walk is type-driven, not flow-driven: every SSA value of a
//! tracked type is checked, wherever it came from, so a parameter is
//! covered at its call sites and its uses alike. Anything not named
//! above is an escape.

use std::collections::HashMap;
use std::hash::Hash;

use torajs_core::ssa::{Function, InstKind, Module, Operand, Terminator, Type, ValueId};

use crate::cmd_build_value_escape_slots::{int_views, slot_types, slot_value};

/// The first escape found for each tracked key, as a readable reason.
/// `key_of` picks the tracked types (and keys them); `sink` names the
/// runtime kernels a tracked value may be handed to.
pub(crate) fn escapes<K: Copy + Eq + Hash>(
    module: &Module,
    key_of: impl Fn(Type) -> Option<K>,
    sink: impl Fn(&str) -> bool,
) -> HashMap<K, String> {
    let mut out = HashMap::new();
    let global_ty: HashMap<&str, Type> = module
        .data_globals
        .iter()
        .map(|g| (g.name.as_str(), g.ty))
        .collect();
    for f in &module.funcs {
        let slots = slot_types(f, &global_ty);
        let views = int_views(f, &key_of);
        FnScan {
            module,
            f,
            key_of: &key_of,
            sink: &sink,
            slots,
            views,
            out: &mut out,
        }
        .run();
    }
    out
}

struct FnScan<'a, K, KF, SF> {
    module: &'a Module,
    f: &'a Function,
    key_of: &'a KF,
    sink: &'a SF,
    slots: HashMap<ValueId, Option<Type>>,
    views: HashMap<ValueId, K>,
    out: &'a mut HashMap<K, String>,
}

impl<K, KF, SF> FnScan<'_, K, KF, SF>
where
    K: Copy + Eq + Hash,
    KF: Fn(Type) -> Option<K>,
    SF: Fn(&str) -> bool,
{
    fn ty(&self, op: &Operand) -> Option<Type> {
        match op {
            Operand::Value(v) => Some(self.f.values[v.0 as usize].ty),
            _ => None,
        }
    }

    fn key(&self, op: &Operand) -> Option<K> {
        self.ty(op).and_then(self.key_of)
    }

    /// The tracked key of a slot address, when `op` is one.
    fn slot_key(&self, op: &Operand) -> Option<K> {
        match op {
            Operand::Value(v) => self.slots.get(v).copied().flatten().and_then(self.key_of),
            _ => None,
        }
    }

    fn is_slot(&self, op: &Operand) -> bool {
        matches!(op, Operand::Value(v) if self.slots.contains_key(v))
    }

    fn escape(&mut self, key: K, why: impl FnOnce() -> String) {
        let name = &self.f.name;
        self.out
            .entry(key)
            .or_insert_with(|| format!("{name}: {}", why()));
    }

    /// Every operand of `kind` that is a tracked value or a tracked
    /// slot's address escapes.
    fn escape_all(&mut self, kind: &InstKind, what: &str) {
        let mut ops = Vec::new();
        torajs_core::ssa::visit_value_operands(kind, |v| ops.push(Operand::Value(v)));
        for op in &ops {
            if let Some(k) = self.key(op).or_else(|| self.slot_key(op)) {
                self.escape(k, || format!("{what} {kind:?}"));
            }
        }
    }

    fn run(&mut self) {
        for b in &self.f.blocks {
            for i in &b.insts {
                if !matches!(i.kind, InstKind::ICmp(..)) {
                    self.view_uses(&i.kind);
                }
                self.inst(&i.kind, i.result);
            }
            if let Terminator::Ret(Some(op)) = &b.term {
                if let Some(k) = self.key(op)
                    && self.ty(op) != Some(self.f.ret)
                {
                    self.escape(k, || "returned as another type".into());
                }
                if let Operand::Value(v) = op
                    && let Some(&k) = self.views.get(v)
                {
                    self.escape(k, || "integer view returned".into());
                }
            }
        }
    }

    /// An integer view used anywhere but a comparison escapes.
    fn view_uses(&mut self, kind: &InstKind) {
        let mut hit = Vec::new();
        torajs_core::ssa::visit_value_operands(kind, |v| hit.extend(self.views.get(&v).copied()));
        for k in hit {
            self.escape(k, || format!("integer view used by {kind:?}"));
        }
    }

    fn inst(&mut self, kind: &InstKind, result: Option<ValueId>) {
        match kind {
            InstKind::Call(fid, args) => self.call(*fid, args),
            InstKind::CallIndirect(sig, callee, args) => self.call_indirect(sig.0, callee, args),
            InstKind::Store(..)
            | InstKind::Load(..)
            | InstKind::LoadDyn(..)
            | InstKind::LoadDynScaled8(..)
            | InstKind::LoadU8Dyn(..)
            | InstKind::LoadU16Dyn(..)
            | InstKind::StoreDyn(..)
            | InstKind::StoreDynScaled8(..)
            | InstKind::PtrToInt(_) => self.mem(kind),
            InstKind::ICmp(_, a, b) => {
                for op in [a, b] {
                    if let Some(k) = self.slot_key(op) {
                        self.escape(k, || "slot address compared".into());
                    }
                }
            }
            InstKind::RcInc(_)
            | InstKind::RcIncAny(_)
            | InstKind::RcDecAny(_)
            | InstKind::Alloca(_)
            | InstKind::AllocaBytes(_)
            | InstKind::GlobalRef(_) => {}
            InstKind::Copy(_, op) | InstKind::Identity(op) => self.same_type(op, result),
            InstKind::Select(_, c, a, b) => {
                if let Some(k) = self.key(c).or_else(|| self.slot_key(c)) {
                    self.escape(k, || "used as a condition".into());
                }
                self.same_type(a, result);
                self.same_type(b, result);
            }
            _ => self.escape_all(kind, "operand of"),
        }
    }

    /// A load or store, by slot or through a cell, and a pointer's
    /// integer view.
    fn mem(&mut self, kind: &InstKind) {
        match kind {
            InstKind::Store(val, ptr, _) => self.store(val, ptr),
            InstKind::Load(ty, ptr, _) => {
                if let Some(k) = self.slot_key(ptr)
                    && self.slots.get(&slot_value(ptr)).copied().flatten() != Some(*ty)
                {
                    self.escape(k, || format!("slot read as {ty:?}"));
                }
            }
            // reading through the cell or comparing it keeps it typed
            InstKind::LoadDyn(_, ptr, idx)
            | InstKind::LoadDynScaled8(_, ptr, idx)
            | InstKind::LoadU8Dyn(ptr, idx)
            | InstKind::LoadU16Dyn(ptr, idx) => {
                for op in [ptr, idx] {
                    if let Some(k) = self.slot_key(op) {
                        self.escape(k, || "slot read through a dynamic index".into());
                    }
                }
                if let Some(k) = self.key(idx) {
                    self.escape(k, || "used as an index".into());
                }
            }
            InstKind::StoreDyn(val, ptr, idx) | InstKind::StoreDynScaled8(val, ptr, idx) => {
                for op in [val, ptr, idx] {
                    if let Some(k) = self.slot_key(op) {
                        self.escape(k, || "slot written through a dynamic index".into());
                    }
                }
                for op in [val, idx] {
                    if let Some(k) = self.key(op) {
                        self.escape(k, || "stored through a dynamic index".into());
                    }
                }
            }
            InstKind::PtrToInt(op) => {
                if let Some(k) = self.slot_key(op) {
                    self.escape(k, || "slot address as an integer".into());
                }
            }
            _ => {}
        }
    }

    fn call_indirect(&mut self, sig: u32, callee: &Operand, args: &[Operand]) {
        if let Some(k) = self.key(callee).or_else(|| self.slot_key(callee)) {
            self.escape(k, || "called".into());
        }
        let params = &self.module.signatures[sig as usize].0;
        for (i, a) in args.iter().enumerate() {
            if let Some(k) = self.slot_key(a) {
                self.escape(k, || format!("slot handed to an indirect call arg {i}"));
            }
            if let Some(k) = self.key(a)
                && params.get(i).copied() != self.ty(a)
            {
                self.escape(k, || format!("indirect call arg {i} of another type"));
            }
        }
    }

    /// A store keeps a tracked value only in a slot of its own type.
    fn store(&mut self, val: &Operand, ptr: &Operand) {
        if let Some(k) = self.slot_key(val) {
            self.escape(k, || "slot address stored".into());
        }
        if let Some(k) = self.key(val)
            && self.slots.get(&slot_value(ptr)).copied().flatten() != self.ty(val)
        {
            self.escape(k, || format!("stored through {ptr:?}"));
        }
        if let Some(k) = self.key(ptr).filter(|_| self.is_slot(ptr)) {
            self.escape(k, || "slot of a tracked value".into());
        }
    }

    /// A derivation (`Copy`, `Identity`, a `Select` arm) keeps the
    /// value tracked only when the result has the operand's type.
    fn same_type(&mut self, op: &Operand, result: Option<ValueId>) {
        if let Some(k) = self.slot_key(op) {
            self.escape(k, || "slot address derived".into());
        }
        let res_ty = result.map(|r| self.f.values[r.0 as usize].ty);
        if let Some(k) = self.key(op)
            && res_ty != self.ty(op)
        {
            self.escape(k, || format!("derived as {res_ty:?}"));
        }
    }

    fn call(&mut self, fid: torajs_core::ssa::FuncId, args: &[Operand]) {
        let callee = &self.module.funcs[fid.0 as usize];
        let kept = (self.sink)(&callee.name);
        for (i, a) in args.iter().enumerate() {
            if let Some(k) = self.slot_key(a) {
                self.escape(k, || format!("slot handed to `{}` arg {i}", callee.name));
            }
            let Some(k) = self.key(a) else { continue };
            let typed_param = !callee.blocks.is_empty()
                && callee
                    .params
                    .get(i)
                    .is_some_and(|p| Some(callee.values[p.0 as usize].ty) == self.ty(a));
            if !kept && !typed_param {
                self.escape(k, || format!("handed to `{}` arg {i}", callee.name));
            }
        }
    }
}
