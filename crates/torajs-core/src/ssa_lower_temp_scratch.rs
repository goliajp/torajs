//! Parked owned-temp scratch threaded through `LowerCtx` — operands
//! whose mint site and release site straddle other emission (a runtime
//! helper call, a may-throw lower of a sibling subexpression), moved
//! out of `ssa_lower_ctx_struct.rs` as their own state family.

use crate::ssa::{Operand, Type, ValueId};

#[derive(Default)]
pub(crate) struct TempScratch {
    /// RFC 20260712 chunk B — fresh-owned operands parked in a str
    /// method's argv (ToString-coerced searchValue/replaceValue,
    /// fresh temp args) that must drop AFTER the runtime helper
    /// call consumes them. `populate_argv` pushes; `dispatch_
    /// intrinsic` drains right after the emit. Pre-B these leaked
    /// (300k `replace(n.slice(0,1), ..)` churned 16MB). Rotation
    /// 550 — each entry also carries its `throw_live` token
    /// (`LowerCtx::park_argv_owned`): a later argument's lower or the
    /// kernel's own check can raise while the temp is parked here,
    /// and the drain unparks before it drops.
    pub(crate) argv_owned: Vec<(Operand, Type, usize)>,
    /// Rotation 549 — owned temps alive across a may-throw region.
    /// A consumer holding an owned temp while it emits anything
    /// that can raise parks it (`LowerCtx::push_throw_temp`) and
    /// unparks (`pop_throw_temp`) at its normal-path release site;
    /// every `emit_throw_check`'s throw path drops the live slots
    /// (newest first) before branching to the catch / propagate
    /// destination — neither destination can know about a value
    /// that never reached a local. Slots are `Option` so unpark
    /// order need not mirror park order (Object.create releases its
    /// proto temp while the fresh dynobj parked after it is still
    /// live). Pre-549 every such temp leaked on the throw path:
    /// 600k of `try { Object.defineProperty({} as any, "p",
    /// badDesc) } catch {}` churned 175MB against a 1.9MB flat
    /// baseline.
    pub(crate) throw_live: Vec<Option<ThrowTemp>>,
}

/// One parked temp: a value with its drop type, or a relocation slot
/// whose CURRENT pointee is the temp — an object literal's dynobj
/// resizes under its own per-field sets, so a parked pointer would go
/// stale; the slot always holds the live block and the throw path
/// loads it there (no hot-path cost: nothing is read on the normal
/// path).
#[derive(Clone)]
pub(crate) enum ThrowTemp {
    Value(Operand, Type),
    DynobjSlot(ValueId),
}
