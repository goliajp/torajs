//! The throw register: a user function hands a throw back to its
//! caller in x21 as well as in the process-wide pending-throw flag, so
//! the probe after the call is `cbnz x21` instead of ADRP + LDR + CBNZ.
//!
//! Three roles, decided once per module:
//! - a *signalling* function returns from each block marked
//!   `ThrowSignal` (a throw it propagates) with x21 = 1, set after the
//!   epilogue; any other return leaves x21 as it found it. Only a
//!   function nothing but a direct call reaches signals: one whose
//!   address is taken (a fn value, a closure's boxed entry, a vtable
//!   or method-table row, a class drop fn) or that is a runtime helper
//!   written in SSA (`__torajs_*`) may be called from the runtime or
//!   through a pointer, whose callers read only the flag, and `main`
//!   is called by the entry wrapper. Nor does one called where the
//!   front end left the call without a probe and x21 may be read
//!   after it (compile/throw_reg_lax.rs).
//! - a *reader* calls a signalling function. x21 is no value's home in
//!   it, since a callee that throws overwrites x21, and it reads x21 to
//!   probe those calls.
//! - a *guarded* reader is one that may be entered with x21 holding
//!   something else: `main`, a function whose address is taken (or a
//!   runtime helper), or one called by a function that is not a
//!   reader; or one called where the front end left the call without
//!   a probe and x21 may be read after it, since a reader may pass a
//!   throw on with x21 set (compile/throw_reg_lax.rs). It saves x21
//!   with its callee-saved registers, zeroes it on entry and puts it
//!   back on the way out.
//!
//! The invariant: inside a reader x21 is 1 only while a throw is
//! pending, so a call made with no throw pending finds it 0. It holds
//! on entry: a guarded reader zeroes it, and an unguarded one is
//! called only by readers. A callee that leaves x21 set (a signalling
//! one, or an unguarded reader passing a throw on) does so only with
//! the throw pending, and a call to it is probed: the probe sends the
//! caller down its throw edge, and on the continue edge x21 is 0. (A
//! call the front end left without a probe is to such a callee only
//! when no probe reads x21 after it and the caller's epilogue puts x21
//! back, compile/throw_reg_lax.rs.)
//! A throw path either leaves the function or reaches a
//! `__torajs_throw_take` (catch, finally, iterator close); the reader
//! zeroes x21 on the way there, on entry to the probe's throw edge or
//! right after the take (compile/throw_reg_zero.rs). Any other callee
//! leaves x21 as it found it: a non-reader preserves it as
//! callee-saved, a guarded reader restores it. A reader may thus
//! return with x21 = 1 only with the throw pending; the callers of an
//! unguarded reader are readers, and a guarded one restores x21 on its
//! way out.
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

use super::throw_probe::ProbedCalls;
use super::throw_reg_lax::{self, LaxCall};
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
        // (caller, callee) of every direct call, and the calls not probed
        let mut edges: Vec<(usize, usize)> = Vec::new();
        let mut lax: Vec<LaxCall> = Vec::new();
        for (i, f) in module.funcs.iter().enumerate() {
            signals[i] = scan(i, f, &mut edges, &mut lax, &mut taken);
            // a runtime helper written in SSA is called by the runtime
            taken[i] |= f.name.starts_with("__torajs_");
        }
        for g in table_rows(module) {
            taken[g.0 as usize] = true;
        }
        for (s, t) in signals.iter_mut().zip(&taken) {
            *s &= !t;
        }
        let mut guarded = vec![false; n];
        loop {
            let roles = assign(module, &signals, &guarded, &edges, &taken);
            if !throw_reg_lax::settle(module, &lax, &roles, &mut signals, &mut guarded) {
                let take = (module.funcs.iter())
                    .position(|f| f.name == THROW_TAKE)
                    .map(|i| FuncId(i as u32));
                return ThrowRegPlan { roles, take };
            }
        }
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

/// The roles, given which functions signal and which a call left
/// without a probe makes guarded (compile/throw_reg_lax.rs).
fn assign(
    module: &Module,
    signals: &[bool],
    guarded: &[bool],
    edges: &[(usize, usize)],
    taken: &[bool],
) -> Vec<Role> {
    let n = signals.len();
    let mut reads = vec![false; n];
    for &(c, g) in edges {
        reads[c] |= signals[g];
    }
    let mut called_by_other = vec![false; n];
    for &(_, g) in edges.iter().filter(|&&(c, _)| !reads[c]) {
        called_by_other[g] = true;
    }
    (module.funcs.iter().enumerate())
        .map(|(i, f)| {
            let open = f.name != "main" && !taken[i] && !called_by_other[i] && !guarded[i];
            Role {
                signals: signals[i],
                reads: reads[i],
                guards: reads[i] && !open,
            }
        })
        .collect()
}

/// Whether `f`, the module's function `i`, may signal: it has a block
/// marked `ThrowSignal`, every such block returns, and it is not
/// `main`. Records its direct calls in `edges` (and in `lax` those not
/// probed) and the functions whose address it takes in `taken`.
fn scan(
    i: usize,
    f: &Function,
    edges: &mut Vec<(usize, usize)>,
    lax: &mut Vec<LaxCall>,
    taken: &mut [bool],
) -> bool {
    let (mut marked, mut all_return) = (false, true);
    let mut probes = None;
    for (bi, b) in f.blocks.iter().enumerate() {
        let mut signal = false;
        for (k, inst) in b.insts.iter().enumerate() {
            match inst.kind {
                InstKind::Call(g, _) => {
                    edges.push((i, g.0 as usize));
                    if !(probes.get_or_insert_with(|| ProbedCalls::of(f))).probed(b, k) {
                        lax.push(LaxCall {
                            caller: i,
                            callee: g.0 as usize,
                            block: bi,
                            at: k,
                        });
                    }
                }
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
/// may reach them: vtable rows, method-table adapters, class drop fns,
/// async frame descriptor rows.
fn table_rows(module: &Module) -> impl Iterator<Item = FuncId> + '_ {
    let rows = (module.vtable_globals.iter()).flat_map(|v| v.fn_ids.iter().flatten().copied());
    let methods = (module.class_layouts.iter())
        .flat_map(|c| c.methods.iter())
        .flat_map(|m| std::iter::once(m.adapter_fid).chain(m.twin_adapter_fid));
    let frames = (module.suspend_fns.iter()).flat_map(|s| [s.resume, s.drop, s.trace]);
    rows.chain(methods)
        .chain(module.class_drop_fns.iter().copied())
        .chain(frames)
}
