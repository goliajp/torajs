//! Short-circuit `&&` / `||` lowering and the ToBoolean coercion
//! their conditions share (extracted from `ssa_lower.rs`, file-size
//! known-debt #1).

use crate::ast::ExprId;
use crate::ssa::{FPred, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

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
    pub(crate) fn lower_logical_and(&mut self, left: ExprId, right: ExprId) -> Operand {
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
        let a = self.lower_expr(left);
        let a_ty = self.operand_ty(&a);
        let truthy = self.coerce_to_bool(a.clone());
        // V3-18 m1.g mixed-Any case — when either side is typed
        // Any in check, widen the slot to Any and NaN-box the
        // non-Any operand. Pre-fix `alloca(a_ty)` truncated the
        // non-matching side into a typed slot and the next Load
        // decoded garbage. Right's check-time type is peeked
        // before lowering since lowering b is observable (it
        // happens inside the eval_b block); detecting the mix
        // up-front keeps the slot alloca uniform.
        let widen_to_any = matches!(a_ty, Type::Any) || self.right_is_any(right);
        let slot_ty = if widen_to_any { Type::Any } else { a_ty };
        let a_for_slot = if widen_to_any && a_ty != Type::Any {
            self.box_to_any(a)
        } else {
            a
        };
        let slot = self.alloca(slot_ty, None);
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
        let b = self.lower_expr(right);
        let b_for_slot = if widen_to_any && self.operand_ty(&b) != Type::Any {
            self.box_to_any(b)
        } else {
            b
        };
        self.f.append_void(
            self.cur_block,
            InstKind::Store(b_for_slot, Operand::Value(slot), 0),
        );
        self.f.set_term(self.cur_block, Terminator::Br(merge));
        self.cur_block = false_blk;
        // a is the falsy value — return it directly (matches JS:
        // `0 && expr` returns 0, not false; `"" && expr` returns "").
        self.f.append_void(
            self.cur_block,
            InstKind::Store(a_for_slot, Operand::Value(slot), 0),
        );
        self.f.set_term(self.cur_block, Terminator::Br(merge));
        self.cur_block = merge;
        let v = self.f.append_inst(
            self.cur_block,
            InstKind::Load(slot_ty, Operand::Value(slot), 0),
            slot_ty,
            None,
        );
        Operand::Value(v)
    }

    /// Peek whether `right` was typed as Any by check.rs without
    /// lowering it (the right side of `&&` / `||` is evaluated
    /// lazily — lowering it eagerly would emit IR onto the wrong
    /// block). Used to decide whether to widen the result slot to
    /// Any so a non-Any short-circuit value and an Any
    /// continuation share a uniform slot type.
    fn right_is_any(&self, right: ExprId) -> bool {
        matches!(self.expr_types.get(&right), Some(crate::check::Type::Any))
    }

    /// V3-18 m1.g — JS spec §13.13: `a || b` returns `a` if truthy,
    /// otherwise `b`. Mirror of `&&`.
    pub(crate) fn lower_logical_or(&mut self, left: ExprId, right: ExprId) -> Operand {
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
        let a = self.lower_expr(left);
        let a_ty = self.operand_ty(&a);
        let truthy = self.coerce_to_bool(a.clone());
        // V3-18 m1.g mixed-Any case — mirror of `&&` above; widen
        // to Any so both short-circuit values share a uniform slot.
        let widen_to_any = matches!(a_ty, Type::Any) || self.right_is_any(right);
        let slot_ty = if widen_to_any { Type::Any } else { a_ty };
        let a_for_slot = if widen_to_any && a_ty != Type::Any {
            self.box_to_any(a)
        } else {
            a
        };
        let slot = self.alloca(slot_ty, None);
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
        self.cur_block = true_blk;
        // a is truthy — return it directly (matches JS: `5 || 0`
        // returns 5; `"x" || ""` returns "x").
        self.f.append_void(
            self.cur_block,
            InstKind::Store(a_for_slot, Operand::Value(slot), 0),
        );
        self.f.set_term(self.cur_block, Terminator::Br(merge));
        self.cur_block = eval_b;
        let b = self.lower_expr(right);
        let b_for_slot = if widen_to_any && self.operand_ty(&b) != Type::Any {
            self.box_to_any(b)
        } else {
            b
        };
        self.f.append_void(
            self.cur_block,
            InstKind::Store(b_for_slot, Operand::Value(slot), 0),
        );
        self.f.set_term(self.cur_block, Terminator::Br(merge));
        self.cur_block = merge;
        let v = self.f.append_inst(
            self.cur_block,
            InstKind::Load(slot_ty, Operand::Value(slot), 0),
            slot_ty,
            None,
        );
        Operand::Value(v)
    }

    /// V3-18 m1.g — JS spec §7.1.2 ToBoolean. Coerces `op` to a
    /// Type::Bool for branch conditions in `&&` / `||` / `if` /
    /// ternary on non-bool inputs.
    ///   undefined → false  (post-V3-18 m1.h)
    ///   null      → false
    ///   Bool      → as-is
    ///   Number i64 → 0 = false, else true
    ///   F64       → 0/-0/NaN = false, else true
    ///   String / Substr → empty = false, else true
    ///   Object / Array / Closure / etc → always true (non-null heap)
    pub(crate) fn coerce_to_bool(&mut self, op: Operand) -> Operand {
        let ty = self.operand_ty(&op);
        match ty {
            Type::Bool => op,
            Type::I64 => self.cmp(IPred::Ne, op, Operand::ConstI64(0)),
            Type::F64 => {
                // ToBoolean(NaN) = false, ToBoolean(+0/-0) = false,
                // else true. FPred::One ("ordered, not equal") is
                // true iff both operands are non-NaN AND unequal —
                // exactly NaN→false, ±0→false, others→true.
                self.fcmp(FPred::One, op, Operand::ConstF64(0.0))
            }
            Type::Str | Type::Substr => {
                // ToBoolean(string) per spec §7.1.2 — falsy iff "" or
                // null. Pre-fix tora unconditionally loaded len at
                // offset 8 (segfault on null). Truthy-narrow on
                // Nullable<String> in check.rs hands us a Str operand
                // that may be NULL when the runtime branch isn't taken
                // (e.g. `if (s)` on `s: string | null`), so guard the
                // length load with an explicit null-check.
                let is_null = self.f.append_inst(
                    self.cur_block,
                    InstKind::ICmp(IPred::Eq, op.clone(), Operand::ConstPtrNull),
                    Type::Bool,
                    None,
                );
                let null_blk = self.f.add_block();
                let nn_blk = self.f.add_block();
                let merge = self.f.add_block();
                let result_slot = self.alloca_in_entry(Type::Bool, Some("__strbool"));
                let cb = self.cur_block;
                self.f.set_term(
                    cb,
                    Terminator::CondBr {
                        cond: Operand::Value(is_null),
                        then_blk: null_blk,
                        else_blk: nn_blk,
                    },
                );
                // null branch: store false.
                self.f.append_void(
                    null_blk,
                    InstKind::Store(Operand::ConstBool(false), Operand::Value(result_slot), 0),
                );
                self.f.set_term(null_blk, Terminator::Br(merge));
                // non-null branch: load len via the encoding-aware
                // helper, compare > 0.
                self.cur_block = nn_blk;
                let len_op = crate::ssa_lower_str::load_str_or_substr_length(self, op, ty);
                let nz = self.cmp(IPred::Sgt, len_op, Operand::ConstI64(0));
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(nz, Operand::Value(result_slot), 0),
                );
                let cb = self.cur_block;
                self.f.set_term(cb, Terminator::Br(merge));
                self.cur_block = merge;
                let r = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::Bool, Operand::Value(result_slot), 0),
                    Type::Bool,
                    None,
                );
                Operand::Value(r)
            }
            Type::Ptr => {
                // null literal or any raw pointer — null = false.
                self.cmp(IPred::Ne, op, Operand::ConstPtrNull)
            }
            // P0.4 — ToBoolean(Any) per JS spec §7.1.2 routes through
            // __torajs_any_to_bool which unboxes the tag + payload
            // and applies spec rules: NULL → false, BOOL → value,
            // I64 → !=0, F64 → !=0 && !NaN, HEAP/Str → len>0, other
            // HEAP → true. Other heap-pointer types continue to use
            // the simple null-check fallback (still correct because
            // they're statically non-Any objects: object → true,
            // null → false).
            Type::Any => {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.any_to_bool, vec![op]),
                    Type::Bool,
                    None,
                );
                Operand::Value(v)
            }
            // Heap-typed values (Obj / Arr / Closure / Symbol /
            // RegExp / Date / BigInt / ...) lower to a single pointer
            // at codegen, so ToBoolean per spec §7.1.2 is exactly
            // `ptr != null` — null is the only falsy value for an
            // object / heap value. The previous shortcut returned
            // ConstBool(true) under the assumption that these values
            // always come from `new` / literal alloc; the truthy-
            // narrow wedge breaks that (a Nullable<Obj> binding can
            // legitimately carry NULL through `if (b) ...`), so the
            // fallback now does the explicit null-check.
            _ => self.cmp(IPred::Ne, op, Operand::ConstPtrNull),
        }
    }
}
