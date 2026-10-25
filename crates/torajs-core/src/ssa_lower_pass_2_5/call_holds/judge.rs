//! Whether something a hold spans may free its cell.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use crate::ssa::{CallHold, FuncId, Function, InstKind, Module, Operand, Type, ValueId};

use super::super::cell_release::{BORROWS, MayHold, Slots, immediates, reached};
use super::super::pin_facts::grow::READ_ONLY;
use super::runtime;
use super::span::{preds, span};

/// Per walk (the functions it starts from, the held cell's type,
/// whether it is of what the runtime can enter), its answer.
type Walks = HashMap<(Vec<FuncId>, Type, bool), Option<String>>;

pub(super) struct Judge<'m> {
    pub(super) module: &'m Module,
    pub(super) slots: Slots<'m>,
    /// Per function with holds, the blocks that run their pieces.
    pub(super) pieces: &'m HashMap<FuncId, Vec<bool>>,
    pub(super) drops: HashSet<FuncId>,
    pub(super) asleep: Option<&'m [bool]>,
    /// Per held cell type, why what the runtime can enter may free one
    /// such cell, or `None` when nothing does.
    pub(super) entered: RefCell<HashMap<Type, Option<String>>>,
    pub(super) walks: RefCell<Walks>,
    /// The same per function a walk reaches.
    pub(super) scans: RefCell<HashMap<(FuncId, Type, bool), Option<String>>>,
}

/// What every hold of one function reads of it.
pub(super) struct Body<'f> {
    fid: FuncId,
    f: &'f Function,
    preds: Vec<Vec<usize>>,
    immediates: HashSet<ValueId>,
}

impl<'f> Body<'f> {
    pub(super) fn of(module: &'f Module, fid: FuncId) -> Self {
        let f = &module.funcs[fid.0 as usize];
        Body {
            fid,
            f,
            preds: preds(f),
            immediates: immediates(f),
        }
    }
}

impl Judge<'_> {
    /// Why hold `h` of `body` is needed, or `None` when its span
    /// releases nothing that could free the cell.
    pub(super) fn needed(&self, body: &Body<'_>, h: &CallHold) -> Option<String> {
        let done = h
            .done
            .expect("a hold's call releases it once the call is lowered");
        let f = body.f;
        let may = self.may(h.cell);
        let skip = &self.pieces[&body.fid];
        let Some(span) = span(f, &body.preds, skip, (h.take.join, h.drops[done].at)) else {
            return Some("its release is not reached from its retain".to_string());
        };
        let mut callees = Vec::new();
        let blocks = span.into_iter();
        if let Some(why) = self.scan(f, &body.immediates, blocks, &may, &mut callees, false) {
            return Some(format!("{why} in {}", f.name));
        }
        self.walk(callees, &may, false)
    }

    fn may(&self, cell: Type) -> MayHold<'_> {
        MayHold {
            module: self.module,
            cell,
            slots: self.slots,
        }
    }

    /// Why a function `from` reach may free a cell `may` asks about;
    /// `inside` when the walk is of what the runtime can enter.
    fn walk(&self, mut from: Vec<FuncId>, may: &MayHold<'_>, inside: bool) -> Option<String> {
        from.sort_unstable_by_key(|f| f.0);
        from.dedup();
        let key = (from, may.cell, inside);
        if let Some(why) = self.walks.borrow().get(&key) {
            return why.clone();
        }
        let why = (reached(self.module, &key.0).into_iter())
            .filter(|g| !self.drops.contains(g))
            .find_map(|g| self.frees(g, may, inside));
        self.walks.borrow_mut().insert(key, why.clone());
        why
    }

    /// Why the body of `g`, but its pieces, may free a cell `may` asks
    /// about.
    fn frees(&self, g: FuncId, may: &MayHold<'_>, inside: bool) -> Option<String> {
        let key = (g, may.cell, inside);
        if let Some(why) = self.scans.borrow().get(&key) {
            return why.clone();
        }
        let gf = &self.module.funcs[g.0 as usize];
        let skip = self.pieces.get(&g);
        let blocks = (0..gf.blocks.len()).filter(|b| skip.is_none_or(|s| !s[*b]));
        let imm = immediates(gf);
        let why = (self.scan(gf, &imm, blocks, may, &mut Vec::new(), inside))
            .map(|why| format!("{why} in {}", gf.name));
        self.scans.borrow_mut().insert(key, why.clone());
        why
    }

    /// Why a function the runtime can enter may free a cell `may` asks
    /// about. While the walk is of those functions, a reentrant read in
    /// them enters nothing new.
    fn entered_frees(&self, may: &MayHold<'_>) -> Option<String> {
        if let Some(why) = self.entered.borrow().get(&may.cell) {
            return why.clone();
        }
        let all_awake = vec![false; self.module.funcs.len()];
        let asleep = self.asleep.unwrap_or(&all_awake);
        let why = self.walk(runtime::entered(self.module, asleep), may, true);
        self.entered.borrow_mut().insert(may.cell, why.clone());
        why
    }

    /// The first instruction of `blocks` of `f` that may free the cell;
    /// the user functions they call directly go to `callees`.
    /// `immediates` are `f`'s.
    fn scan(
        &self,
        f: &Function,
        immediates: &HashSet<ValueId>,
        blocks: impl Iterator<Item = usize>,
        may: &MayHold<'_>,
        callees: &mut Vec<FuncId>,
        inside: bool,
    ) -> Option<String> {
        for b in blocks {
            for inst in &f.blocks[b].insts {
                let why = match &inst.kind {
                    InstKind::Call(t, args) => self.call(f, (*t, args), may, callees, inside),
                    InstKind::CallIndirect(..) => Some("an indirect call".to_string()),
                    InstKind::RcDecAny(Operand::Value(x)) if immediates.contains(x) => None,
                    InstKind::RcDecAny(_) => Some("an untyped release".to_string()),
                    InstKind::Suspend(..) => Some("a suspension".to_string()),
                    _ => None,
                };
                if why.is_some() {
                    return why;
                }
            }
        }
        None
    }

    fn call(
        &self,
        f: &Function,
        (t, args): (FuncId, &[Operand]),
        may: &MayHold<'_>,
        callees: &mut Vec<FuncId>,
        inside: bool,
    ) -> Option<String> {
        let g = &self.module.funcs[t.0 as usize];
        let runtime = g.is_declaration();
        if !runtime && !self.drops.contains(&t) {
            callees.push(t);
            return None;
        }
        let name = g.name.as_str();
        if BORROWS.contains(&name) {
            return None;
        }
        if args.iter().any(|a| may.of(operand_ty(f, a))) {
            return Some(format!("{name} may release one"));
        }
        let quiet = READ_ONLY.contains(&name) || runtime::ALLOCS.contains(&name);
        if !runtime || quiet || name.starts_with(runtime::ARR_DROP_PREFIX) {
            return None;
        }
        if !runtime::REENTRANT_READS.contains(&name) {
            return Some(format!("{name} may run user code"));
        }
        if inside {
            return None;
        }
        (self.entered_frees(may)).map(|why| format!("{name} may run user code: {why}"))
    }
}

fn operand_ty(f: &Function, op: &Operand) -> Type {
    match op {
        Operand::Value(v) => f.value_type(*v),
        Operand::ConstI64(_) => Type::I64,
        Operand::ConstI32(_) => Type::I32,
        Operand::ConstF64(_) => Type::F64,
        Operand::ConstBool(_) => Type::Bool,
        Operand::ConstPtrNull => Type::Ptr,
    }
}
