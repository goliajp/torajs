//! ES §7.1.4 `ToNumber` at a builtin's numeric parameter position.
//!
//! A builtin parameter whose spec step is `ToNumber` /
//! `ToIntegerOrInfinity` / `ToIndex` reaches EVERY value — there is no
//! input shape it refuses. `"abcd".charAt("   +00200.0000E-0002   ")`
//! is `'c'`, `String.fromCharCode("65")` is `"A"`, and
//! `(1.5).toFixed("1")` is `"1.5"`. tr used to answer those three with
//! a compile-time `argument 0: expected Number, got String`, because
//! each such lane had grown a per-shape admission wedge (Number, then
//! `Any`, then `Undefined`) instead of a coercion.
//!
//! This helper is the single lowering-side answer, so a lane widens by
//! deleting its shape gate rather than by growing one more arm:
//!
//! - already a Number (`I64` / `F64`) → straight through, no boxing,
//!   no call — the typed-tier fast path is untouched;
//! - anything else → box to `Any` (the literal ShortStr fast path in
//!   [`LowerCtx::box_to_any_from_expr`] keeps a string literal off the
//!   heap), then `__torajs_any_to_number`.
//!
//! The `any_to_number` kernel is the runtime's own `ToNumber`, so
//! `ToPrimitive` on an object receiver records a pending `TypeError`
//! (§7.1.1) rather than answering; [`LowerCtx::emit_throw_check`]
//! propagates it here — before the NaN placeholder could become a
//! position, and before a stale pending leaks into an unrelated later
//! check.
//!
//! `undefined` is NOT special-cased: `ToNumber(undefined)` is `NaN`,
//! and the callers that want `ToIntegerOrInfinity`'s `NaN → 0` get it
//! from [`LowerCtx::coerce_to_i64`] downstream. A lane that needs a
//! different default for a *missing* argument decides that before
//! calling this — an absent argument is not the same thing as one that
//! evaluates to `undefined`.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// Lower `eid` and bring it to a Number operand (`I64` or `F64`)
    /// per ES §7.1.4, whatever its static type.
    pub(crate) fn lower_to_number_operand(&mut self, eid: ExprId) -> Operand {
        let raw = self.lower_expr(eid);
        match self.operand_ty(&raw) {
            Type::I64 | Type::I32 | Type::F64 => raw,
            _ => {
                let boxed = self.box_to_any_from_expr(eid, raw);
                let n = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.any_to_number, vec![boxed]),
                    Type::F64,
                    None,
                );
                self.emit_throw_check(None);
                Operand::Value(n)
            }
        }
    }
}
