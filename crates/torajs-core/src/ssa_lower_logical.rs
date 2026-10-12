//! Short-circuit `&&` / `||` lowering and the ToBoolean coercion
//! their conditions share (extracted from `ssa_lower.rs`, file-size
//! known-debt #1).

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) mod join_arm;

impl LowerCtx<'_> {
    /// M1.5 — `a && b` with short-circuit. Layout:
    ///
    /// ```text
    ///   <slot> = alloca bool
    ///   av = lower(a)
    ///   cond_br av, eval_b, false_blk
    /// eval_b:
    ///   bv = lower(b)
    ///   store bv → slot
    ///   br merge
    /// false_blk:
    ///   store false → slot
    ///   br merge
    /// merge:
    ///   load slot
    /// ```
    /// V3-18 m1.g — JS spec §13.13: `a && b` returns `a` if it's
    /// falsy, otherwise `b`. Result type is the common type of
    /// both operands (typed tora gates on l == r at typecheck;
    /// implicit-any (m1.h) widens to mixed types later).
    pub(crate) fn lower_logical_and(
        &mut self,
        eid: ExprId,
        left: ExprId,
        right: ExprId,
    ) -> Operand {
        // S138 — statically-falsy lhs short-circuit. If lhs is typed
        // Type::Null / Type::Undefined (e.g. literal `null`, `undefined`,
        // or a call returning Null), `lhs && rhs` returns lhs without
        // evaluating rhs per §13.13. We still lower lhs to preserve any
        // side-effects (function call returning null), then return its
        // value; rhs is skipped entirely. Pairs with check.rs's
        // nullish-lhs LAnd arm so the result type matches lhs.
        if matches!(
            self.expr_types.get(&left),
            Some(crate::check::Type::Null) | Some(crate::check::Type::Undefined)
        ) {
            return self.lower_expr(left);
        }
        // Cluster #4 — `o[k] &&= v` fingerprint: evaluate (and
        // coerce) the shared key once up front; both the guard read
        // below and the branch-side assign consult the pin.
        // RFC 20260915 — the shared obj / index children lower once
        // here, receiver first, before the key pin coerces the key.
        let shared = crate::ssa_lower_assign::pin_logical_target(self, left, right);
        let key_pin = self.pin_logical_assign_key(left, right);
        let a = self.lower_expr(left);
        let a_ty = self.operand_ty(&a);
        let truthy = self.coerce_to_bool(a);
        // V3-18 m1.g mixed-Any case — when either side is typed
        // Any in check, widen the slot to Any and NaN-box the
        // non-Any operand. Pre-fix `alloca(a_ty)` truncated the
        // non-matching side into a typed slot and the next Load
        // decoded garbage. Right's check-time type is peeked
        // before lowering since lowering b is observable (it
        // happens inside the eval_b block); detecting the mix
        // up-front keeps the slot alloca uniform. The box is
        // expr-AWARE: an `undefined` operand is a compile-time
        // ConstPtrNull that the plain `box_to_any` tags as ANY_NULL,
        // so `anyVal && undefined` answered `null` (`typeof` printed
        // "object") — the same hole the ternary's mixed-Any join had.
        let widen_to_any =
            matches!(a_ty, Type::Any) || self.right_is_any(right) || self.result_is_any(eid);
        // Rotation 507 — two different class instances (`m || new
        // Leaf(2)`): the checker joined them to a common ancestor, so
        // the slot wears the ancestor's layout. No box — both pointers
        // carry its field prefix (see `class_join_repr`).
        let slot_ty = if widen_to_any {
            Type::Any
        } else {
            self.class_join_repr(eid).unwrap_or(a_ty)
        };
        let a_for_slot = if widen_to_any && a_ty != Type::Any {
            self.box_to_any_from_expr(left, a)
        } else {
            a
        };
        let eval_b = self.f.add_block();
        let false_blk = self.f.add_block();
        let merge = self.f.add_block();
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: truthy,
                then_blk: eval_b,
                else_blk: false_blk,
            },
        );
        self.cur_block = eval_b;
        let a_owned = slot_ty.is_refcounted() && self.join_arm_mints(left);
        // On the eval-b arm the lhs value is dead (the result is b) —
        // an owned lhs temp releases here, mirroring the ternary
        // condition release (chunk 636). The false arm keeps a's
        // stake riding into the slot.
        if a_owned {
            self.emit_drop_value(a_for_slot, slot_ty);
        }
        self.proofs.enter_cond();
        let b = crate::ssa_lower_closure_canonical::lower_slot_arm(self, slot_ty, right);
        self.proofs.leave_cond();
        let b_for_slot = if widen_to_any && self.operand_ty(&b) != Type::Any {
            self.box_to_any_from_expr(right, b)
        } else {
            b
        };
        // The slot's width is settled once both arms are in hand, so
        // its alloca waits for the rhs. Entry-block, the standard
        // shape the ternary's join already uses: the load in `merge`
        // has two predecessors sharing no dominator but entry.
        let a_ty = self.operand_ty(&a_for_slot);
        let (b_for_slot, slot_ty, b_minted) = self.join_arm_widths(slot_ty, right, b_for_slot);
        let slot = self.alloca_in_entry(slot_ty, Some("__logic"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(b_for_slot, Operand::Value(slot), 0),
        );
        // Rotation 325 — owned unification, the chunk-722 ternary
        // contract applied to `&&`: when either arm's value is owned
        // (an any-member read like `e && e.constructor`, a call), the
        // join must be owned on BOTH paths so the consumer's single
        // release balances. Off this track a DISCARDED `e && e.ctor`
        // had no release site at all — expr_owned_shape answered
        // borrow for the whole join while the rhs arm's +1 rode into
        // the slot, and the strand cut the error-prototype cycle at
        // the at-exit drain (proto-own-undefined-read-001's census
        // underflow). A join over two borrows stays a borrow.
        let b_owned = slot_ty.is_refcounted() && (b_minted || self.join_arm_mints(right));
        // a view lhs settling into a `Str` slot is copied out on its own path
        let a_owned = a_owned || (a_ty == Type::Substr && slot_ty == Type::Str);
        let join_owned = slot_ty.is_refcounted() && (a_owned || b_owned);
        if join_owned && !b_owned {
            self.emit_owned_result_inc_in(self.cur_block, b_for_slot, slot_ty);
        }
        self.f.set_term(self.cur_block, Terminator::Br(merge));
        self.cur_block = false_blk;
        let (a_for_slot, _) = self.coerce_arm_to_slot(left, a_for_slot, slot_ty);
        // a is the falsy value — return it directly (matches JS:
        // `0 && expr` returns 0, not false; `"" && expr` returns "").
        if join_owned && !a_owned {
            self.emit_owned_result_inc_in(self.cur_block, a_for_slot, slot_ty);
        }
        self.f.append_void(
            self.cur_block,
            InstKind::Store(a_for_slot, Operand::Value(slot), 0),
        );
        self.f.set_term(self.cur_block, Terminator::Br(merge));
        self.cur_block = merge;
        if let Some(owned) = key_pin {
            self.unpin_logical_assign_key(owned);
        }
        if join_owned {
            self.owned_member_reads.insert(eid);
        }
        let v = self.f.append_inst(
            self.cur_block,
            InstKind::Load(slot_ty, Operand::Value(slot), 0),
            slot_ty,
            None,
        );
        let out = Operand::Value(v);
        if let Some(shared) = shared {
            crate::ssa_lower_assign::unpin(self, shared, eid, &out);
        }
        out
    }

    /// V3-18 m1.g — JS spec §13.13: `a || b` returns `a` if truthy,
    /// otherwise `b`. Mirror of `&&`.
    pub(crate) fn lower_logical_or(&mut self, eid: ExprId, left: ExprId, right: ExprId) -> Operand {
        // S138 — statically-falsy lhs short-circuit. If lhs is typed
        // Type::Null / Type::Undefined, `lhs || rhs` is equivalent to
        // `rhs` per §13.13 (ToBoolean(null/undef) = false → eval rhs).
        // Pairs with check.rs's nullish-lhs LOr arm so the result type
        // matches rhs. Side-effects: lhs is still lowered (in case it's
        // a Call returning Null, e.g. `f(): null` followed by `|| x`).
        if matches!(
            self.expr_types.get(&left),
            Some(crate::check::Type::Null) | Some(crate::check::Type::Undefined)
        ) {
            let _ = self.lower_expr(left);
            return self.lower_expr(right);
        }
        // Cluster #4 — `o[k] ||= v` fingerprint (see the `&&` arm).
        // RFC 20260915 — the shared obj / index children lower once
        // here, receiver first, before the key pin coerces the key.
        let shared = crate::ssa_lower_assign::pin_logical_target(self, left, right);
        let key_pin = self.pin_logical_assign_key(left, right);
        let a = self.lower_expr(left);
        let a_ty = self.operand_ty(&a);
        let truthy = self.coerce_to_bool(a);
        // V3-18 m1.g mixed-Any case — mirror of `&&` above; widen
        // to Any so both short-circuit values share a uniform slot.
        let widen_to_any =
            matches!(a_ty, Type::Any) || self.right_is_any(right) || self.result_is_any(eid);
        // Rotation 507 — two different class instances (`m || new
        // Leaf(2)`): the checker joined them to a common ancestor, so
        // the slot wears the ancestor's layout. No box — both pointers
        // carry its field prefix (see `class_join_repr`).
        let slot_ty = if widen_to_any {
            Type::Any
        } else {
            self.class_join_repr(eid).unwrap_or(a_ty)
        };
        let a_for_slot = if widen_to_any && a_ty != Type::Any {
            self.box_to_any_from_expr(left, a)
        } else {
            a
        };
        let true_blk = self.f.add_block();
        let eval_b = self.f.add_block();
        let merge = self.f.add_block();
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: truthy,
                then_blk: true_blk,
                else_blk: eval_b,
            },
        );
        let a_owned = slot_ty.is_refcounted() && self.join_arm_mints(left);
        self.cur_block = eval_b;
        // On the eval-b arm the (falsy) lhs value is dead — an owned
        // lhs temp releases here; the truthy arm keeps a's stake
        // riding into the slot. Mirror of the `&&` arm above.
        if a_owned {
            self.emit_drop_value(a_for_slot, slot_ty);
        }
        self.proofs.enter_cond();
        let b = crate::ssa_lower_closure_canonical::lower_slot_arm(self, slot_ty, right);
        self.proofs.leave_cond();
        let b_for_slot = if widen_to_any && self.operand_ty(&b) != Type::Any {
            self.box_to_any_from_expr(right, b)
        } else {
            b
        };
        // Slot width settles once both arms are in hand — see the `&&`
        // arm above.
        let a_ty = self.operand_ty(&a_for_slot);
        let (b_for_slot, slot_ty, b_minted) = self.join_arm_widths(slot_ty, right, b_for_slot);
        let slot = self.alloca_in_entry(slot_ty, Some("__logic"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(b_for_slot, Operand::Value(slot), 0),
        );
        // Rotation 325 — owned unification (chunk-722 ternary
        // contract): see the `&&` arm above for the rationale.
        let b_owned = slot_ty.is_refcounted() && (b_minted || self.join_arm_mints(right));
        // a view lhs settling into a `Str` slot is copied out on its own path
        let a_owned = a_owned || (a_ty == Type::Substr && slot_ty == Type::Str);
        let join_owned = slot_ty.is_refcounted() && (a_owned || b_owned);
        if join_owned && !b_owned {
            self.emit_owned_result_inc_in(self.cur_block, b_for_slot, slot_ty);
        }
        self.f.set_term(self.cur_block, Terminator::Br(merge));
        // a is truthy — return it directly (matches JS: `5 || 0`
        // returns 5; `"x" || ""` returns "x"). Its store waits for
        // the rhs so it lands in a slot of the settled width.
        self.cur_block = true_blk;
        let (a_for_slot, _) = self.coerce_arm_to_slot(left, a_for_slot, slot_ty);
        if join_owned && !a_owned {
            self.emit_owned_result_inc_in(self.cur_block, a_for_slot, slot_ty);
        }
        self.f.append_void(
            self.cur_block,
            InstKind::Store(a_for_slot, Operand::Value(slot), 0),
        );
        self.f.set_term(self.cur_block, Terminator::Br(merge));
        self.cur_block = merge;
        if let Some(owned) = key_pin {
            self.unpin_logical_assign_key(owned);
        }
        if join_owned {
            self.owned_member_reads.insert(eid);
        }
        let v = self.f.append_inst(
            self.cur_block,
            InstKind::Load(slot_ty, Operand::Value(slot), 0),
            slot_ty,
            None,
        );
        let out = Operand::Value(v);
        if let Some(shared) = shared {
            crate::ssa_lower_assign::unpin(self, shared, eid, &out);
        }
        out
    }
}
