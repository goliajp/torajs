//! The packed any-lane argv with its throw-live accounting, rotation
//! 550 — the value [`crate::ssa_lower_any_method_call::pack_any_argv`]
//! answers and every any-lane caller (named / index method call, bare
//! any-call, optcall, variadic, `new` dynamic, `String.raw`, super
//! builtin, uncallable) releases after its kernel returns.
//!
//! Every slot the packer boxes is an owned stake (a fresh temp's
//! reference moved into the box, or the +1 a borrow took before
//! boxing) that the kernel only borrows; the caller drops it after
//! the call. Before 550 nothing told the throw edges in between about
//! it: a later argument's lower could raise (`va(mk(i), boom())`) and
//! the earlier box was stranded — 40MB per 600k caught throws against
//! a 2MB flat band. [`AnyArgv::push_slot`] parks each boxed slot on
//! `temps.throw_live` as it is stored, so every later arg lower drops
//! it on its throw path; [`AnyArgv::release`] unparks after the call
//! and drops on the normal path. (The kernel's own throw edge comes
//! AFTER `release` in every caller — the drop-before-check order the
//! callers already kept — so the kernel-throw path releases the slots
//! through the same explicit drops, not through the parked list.)

use crate::ssa::{Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;

pub(crate) struct AnyArgv {
    pub(crate) argv: ValueId,
    /// The slots WE boxed (`Some`) — the caller's post-call release
    /// drops each; verbatim already-Any slots (`None`) are borrows.
    boxed: Vec<Option<Operand>>,
    parked: Vec<usize>,
}

impl AnyArgv {
    pub(crate) fn new(argv: ValueId, argc: usize) -> Self {
        Self {
            argv,
            boxed: Vec::with_capacity(argc),
            parked: Vec::new(),
        }
    }

    /// A value-packing entry whose slots are all borrows of values
    /// the caller already owns and releases (the HOF loop's argv-face
    /// downgrade) — nothing to park, nothing to drop.
    pub(crate) fn borrowed(argv: ValueId) -> Self {
        Self {
            argv,
            boxed: Vec::new(),
            parked: Vec::new(),
        }
    }

    /// Record one stored slot; a boxed (owned) one parks for the
    /// throw edges between here and the call.
    pub(crate) fn push_slot(&mut self, ctx: &mut LowerCtx<'_>, slot: Option<Operand>) {
        if let Some(op) = &slot {
            self.parked.push(ctx.push_throw_temp(op.clone(), Type::Any));
        }
        self.boxed.push(slot);
    }

    /// Post-call: unpark every boxed slot and drop it on the normal
    /// path. Callers run this BEFORE the kernel's throw check.
    pub(crate) fn release(self, ctx: &mut LowerCtx<'_>) {
        for t in self.parked {
            ctx.pop_throw_temp(t);
        }
        for slot in self.boxed.into_iter().flatten() {
            ctx.emit_drop_value(slot, Type::Any);
        }
    }
}
