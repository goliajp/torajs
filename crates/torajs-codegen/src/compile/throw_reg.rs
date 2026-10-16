//! The throw register: a user function hands a throw back to its
//! caller in x21 as well as in the process-wide pending-throw flag, so
//! the probe after the call is `cbnz x21` instead of ADRP + LDR + CBNZ.
//!
//! Three roles, decided once per module:
//! - a *signalling* function returns from each block marked
//!   `ThrowSignal` (a throw it propagates) with x21 = 1, set after the
//!   epilogue; any other return leaves x21 as it found it, unless a
//!   call it did not probe threw (see below). Only a
//!   function nothing but a direct call reaches signals: one whose
//!   address is taken (a fn value, a closure's boxed entry, a vtable
//!   or method-table row, a class drop fn) or that is a runtime helper
//!   written in SSA (`__torajs_*`) may be called from the runtime or
//!   through a pointer, whose callers read only the flag, and `main`
//!   is called by the entry wrapper.
//! - a *reader* calls a signalling function. x21 is no value's home in
//!   it, since a callee that throws overwrites x21, and it reads x21 to
//!   probe those calls.
//! - a *guarded* reader is one that may be entered with x21 holding
//!   something else: `main`, a function whose address is taken (or a
//!   runtime helper), or one called by a function that is not a
//!   reader. It saves x21 with its callee-saved registers and zeroes
//!   it on entry.
//!
//! The invariant: inside a reader x21 is 1 only while a throw is
//! pending, so a call made with no throw pending finds it 0. It holds
//! on entry: a guarded reader zeroes it, and an unguarded one is
//! called only by readers. A callee that returns normally leaves x21
//! as it found it: a non-reader preserves it as callee-saved, a
//! guarded reader restores it, an unguarded reader keeps the invariant
//! itself. A callee that throws may leave it 1 (a signalling one
//! always does), and then the caller is on a throw path, which either
//! leaves the function or reaches a `__torajs_throw_take` (catch,
//! finally, iterator close); the reader zeroes x21 on the way there,
//! on entry to the probe's throw edge or right after the take
//! (compile/throw_reg_zero.rs). A reader may thus return with x21 = 1
//! and the throw pending, a normal return included when a call it did
//! not probe threw; the callers of an unguarded reader are readers,
//! which count the call as one that may leave x21 set, and a guarded
//! one restores x21 on its way out.
//!
//! A probe reads x21 when the call before it is to a signalling
//! function with nothing in between but copies, on every way in
//! (compile/throw_probe.rs).
//!
//! Sibling calls (compile/sibcall.rs) keep this: the callee returns
//! straight to this function's caller, so it signals exactly when this
//! function does; and a guarded reader, whose epilogue puts back the
//! x21 it was entered with, makes none to an unguarded reader.

use torajs_core::ssa::{Block, FuncId, Function, InstKind, Module, Terminator};

use crate::reg::Gpr;

pub(crate) const THROW_REG: Gpr = Gpr::X21;

const THROW_TAKE: &str = "__torajs_throw_take";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Role {
    pub(crate) signals: bool,
    pub(crate) reads: bool,
    pub(crate) guards: bool,
}

/// The roles of a module's functions, by `FuncId`.
#[derive(Debug, Clone, Default)]
pub(crate) struct ThrowRegPlan {
    roles: Vec<Role>,
    take: Option<FuncId>,
}

impl ThrowRegPlan {
    pub(crate) fn of(module: &Module) -> Self {
        if crate::switches::get().throw_reg_off {
            return ThrowRegPlan::default();
        }
        let n = module.funcs.len();
        let mut taken = vec![false; n];
        let mut signals = vec![false; n];
        // (caller, callee) of every direct call
        let mut edges: Vec<(usize, usize)> = Vec::new();
        for (i, f) in module.funcs.iter().enumerate() {
            signals[i] = scan(i, f, &mut edges, &mut taken);
            // a runtime helper written in SSA is called by the runtime
            taken[i] |= f.name.starts_with("__torajs_");
        }
        for g in table_rows(module) {
            taken[g.0 as usize] = true;
        }
        for (s, t) in signals.iter_mut().zip(&taken) {
            *s &= !t;
        }
        let mut reads = vec![false; n];
        for &(c, g) in &edges {
            reads[c] |= signals[g];
        }
        let mut called_by_other = vec![false; n];
        for &(_, g) in edges.iter().filter(|&&(c, _)| !reads[c]) {
            called_by_other[g] = true;
        }
        let roles = (module.funcs.iter().enumerate())
            .map(|(i, f)| {
                let open = f.name != "main" && !taken[i] && !called_by_other[i];
                Role {
                    signals: signals[i],
                    reads: reads[i],
                    guards: reads[i] && !open,
                }
            })
            .collect();
        let take = (module.funcs.iter())
            .position(|f| f.name == THROW_TAKE)
            .map(|i| FuncId(i as u32));
        ThrowRegPlan { roles, take }
    }

    pub(crate) fn role(&self, f: FuncId) -> Role {
        self.roles.get(f.0 as usize).copied().unwrap_or_default()
    }

    pub(crate) fn signals(&self, f: FuncId) -> bool {
        self.role(f).signals
    }

    /// Whether the call `kind` may be a sibling call of a function in
    /// `role` (compile/sibcall.rs). The callee returns to this
    /// function's caller, which reads x21 after a call here exactly
    /// when this function signals; and an unguarded reader is entered
    /// with x21 = 0, which the epilogue of a guarded one does not leave.
    pub(crate) fn sibcall_allowed(&self, role: Role, kind: &InstKind) -> bool {
        let callee = match kind {
            InstKind::Call(g, _) => self.role(*g),
            _ => Role::default(),
        };
        let enters_open = callee.reads && !callee.guards;
        callee.signals == role.signals && !(role.guards && enters_open)
    }

    /// Whether `kind` calls a function that may return with x21 = 1:
    /// one that signals, or an unguarded reader passing a throw on.
    pub(crate) fn may_leave_set(&self, kind: &InstKind) -> bool {
        let InstKind::Call(f, _) = kind else {
            return false;
        };
        let r = self.role(*f);
        r.signals || (r.reads && !r.guards)
    }

    /// Whether `kind` takes the pending throw, after which a reader
    /// zeroes x21 (compile/throw_reg_zero.rs).
    pub(crate) fn is_take(&self, kind: &InstKind) -> bool {
        matches!(kind, InstKind::Call(f, _) if Some(*f) == self.take)
    }
}

/// Whether `f`, the module's function `i`, may signal: it has a block
/// marked `ThrowSignal`, every such block returns, and it is not
/// `main`. Records its direct calls in `edges` and the functions whose
/// address it takes in `taken`.
fn scan(i: usize, f: &Function, edges: &mut Vec<(usize, usize)>, taken: &mut [bool]) -> bool {
    let (mut marked, mut all_return) = (false, true);
    for b in &f.blocks {
        let mut signal = false;
        for inst in &b.insts {
            match inst.kind {
                InstKind::Call(g, _) => edges.push((i, g.0 as usize)),
                InstKind::FnAddr(g) | InstKind::BoxedEntryAddr(g) => taken[g.0 as usize] = true,
                InstKind::ThrowSignal => signal = true,
                _ => {}
            }
        }
        if signal {
            marked = true;
            all_return &= matches!(b.term, Terminator::Ret(_));
        }
    }
    marked && all_return && f.name != "main"
}

pub(crate) fn has_signal(block: &Block) -> bool {
    (block.insts.iter()).any(|i| matches!(i.kind, InstKind::ThrowSignal))
}

/// The functions a table names, so that the runtime or a pointer call
/// may reach them: vtable rows, method-table adapters, class drop fns.
fn table_rows(module: &Module) -> impl Iterator<Item = FuncId> + '_ {
    let rows = (module.vtable_globals.iter()).flat_map(|v| v.fn_ids.iter().flatten().copied());
    let methods = (module.class_layouts.iter())
        .flat_map(|c| c.methods.iter())
        .flat_map(|m| std::iter::once(m.adapter_fid).chain(m.twin_adapter_fid));
    rows.chain(methods)
        .chain(module.class_drop_fns.iter().copied())
}
