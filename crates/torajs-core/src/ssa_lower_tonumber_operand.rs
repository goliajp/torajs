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
use crate::ssa::{InstKind, Operand, Terminator, Type};
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

    /// Lower `eid` and bring it to an i64 INDEX operand per ES
    /// §7.1.5 `ToIntegerOrInfinity` — [`Self::lower_to_number_operand`]
    /// followed by [`Self::coerce_to_i64`], which is where `NaN → 0`
    /// and `±∞ → i64::{MAX,MIN}` are settled.
    ///
    /// Every builtin slot whose spec step is `ToIntegerOrInfinity`
    /// wants exactly this, and a lane that reaches for `lower_expr`
    /// instead is silently generating an 8-byte scalar contract over
    /// whatever the operand actually is: `a.splice(i, 1)` for an
    /// `i: any` spliced at the wrong position, no diagnostic, wrong
    /// array. Naming the pair once means a lane widens by deleting
    /// its shape gate and calling this, rather than by growing one
    /// more `Any` admission arm beside the `Number` one.
    pub(crate) fn lower_to_index_operand(&mut self, eid: ExprId) -> Operand {
        let n = self.lower_to_number_operand(eid);
        self.coerce_to_i64(n)
    }

    /// An index slot whose spec reads `undefined` as something other
    /// than ToNumber's own `NaN`: `default` when the operand IS
    /// `undefined`, [`Self::lower_to_index_operand`] otherwise.
    ///
    /// The two are different questions and the spec asks them
    /// separately all over: `(255).toString(undefined)` is decimal
    /// while `(255).toString(NaN)` is a RangeError,
    /// `'abc'.endsWith('c', undefined)` is true while
    /// `endsWith('c', NaN)` is false. So the test is on the any TAG,
    /// not on the number — and only an `Any` operand needs one, since
    /// every other static shape is either a Number or claimed by its
    /// lane's own explicit-`undefined` arm.
    pub(crate) fn lower_to_index_or_undef_default(
        &mut self,
        eid: ExprId,
        default: Operand,
        slot_name: &str,
    ) -> Operand {
        if !matches!(self.expr_types.get(&eid), Some(crate::check::Type::Any)) {
            return self.lower_to_index_operand(eid);
        }
        let (tag, _, idx) = self.any_slot_tag_number_index(eid);
        self.select_on_undef_tag(tag, default, idx, slot_name)
    }

    /// Lower an `Any` slot into its three readings at once: the box TAG,
    /// the ToNumber of it, and the ToIntegerOrInfinity of that.
    ///
    /// A slot whose spec default for `undefined` is not ToNumber's own
    /// `NaN` needs the tag; one whose default is what `NaN` means anyway
    /// needs only the number. A user `valueOf` can throw, so the same
    /// check `lower_to_number_operand` emits is emitted here.
    pub(crate) fn any_slot_tag_number_index(&mut self, a: ExprId) -> (Operand, Operand, Operand) {
        let raw = self.lower_expr(a);
        let cur = self.cur_block;
        let tag = self.f.append_inst(
            cur,
            InstKind::Call(self.intrinsics.any_unbox_tag, vec![raw.clone()]),
            Type::I64,
            None,
        );
        let n = self.f.append_inst(
            cur,
            InstKind::Call(self.intrinsics.any_to_number, vec![raw]),
            Type::F64,
            None,
        );
        self.emit_throw_check(None);
        let idx = self.coerce_to_i64(Operand::Value(n));
        (Operand::Value(tag), Operand::Value(n), idx)
    }

    /// `cond ? when_true : when_false` over two i64 operands, as a slot
    /// plus a branch rather than `InstKind::Select` — that one is
    /// introduced only after the egraph pass and its elaborator rejects an
    /// early one loudly. Both operands are computed before the branch, so
    /// neither may carry a side effect the other arm must not see.
    pub(crate) fn select_i64(
        &mut self,
        cond: Operand,
        when_true: Operand,
        when_false: Operand,
        slot_name: &str,
    ) -> Operand {
        let slot = self.alloca(Type::I64, Some(slot_name));
        let then_blk = self.f.add_block();
        let else_blk = self.f.add_block();
        let join_blk = self.f.add_block();
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond,
                then_blk,
                else_blk,
            },
        );
        self.cur_block = then_blk;
        self.f.append_void(
            then_blk,
            InstKind::Store(when_true, Operand::Value(slot), 0),
        );
        self.f.set_term(then_blk, Terminator::Br(join_blk));
        self.cur_block = else_blk;
        self.f.append_void(
            else_blk,
            InstKind::Store(when_false, Operand::Value(slot), 0),
        );
        self.f.set_term(else_blk, Terminator::Br(join_blk));
        self.cur_block = join_blk;
        Operand::Value(self.f.append_inst(
            join_blk,
            InstKind::Load(Type::I64, Operand::Value(slot), 0),
            Type::I64,
            None,
        ))
    }

    /// `<tag is undefined> ? default : idx`. Tag 5 is `undefined`.
    pub(crate) fn select_on_undef_tag(
        &mut self,
        tag: Operand,
        default: Operand,
        idx: Operand,
        slot_name: &str,
    ) -> Operand {
        let is_undef = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(crate::ssa::IPred::Eq, tag, Operand::ConstI64(5)),
            Type::Bool,
            None,
        );
        self.select_i64(Operand::Value(is_undef), default, idx, slot_name)
    }
}
