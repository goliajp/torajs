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
        let key_pin = self.pin_logical_assign_key(left, right);
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
            self.class_join_repr(eid).unwrap_or(a_ty.clone())
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
        let a_owned = slot_ty.is_refcounted() && self.expr_owned_shape(left);
        // On the eval-b arm the lhs value is dead (the result is b) —
        // an owned lhs temp releases here, mirroring the ternary
        // condition release (chunk 636). The false arm keeps a's
        // stake riding into the slot.
        if a_owned {
            self.emit_drop_value(a_for_slot.clone(), slot_ty.clone());
        }
        let b = self.lower_expr(right);
        let b_for_slot = if widen_to_any && self.operand_ty(&b) != Type::Any {
            self.box_to_any_from_expr(right, b)
        } else {
            b
        };
        // The slot's width is settled once both arms are in hand, so
        // its alloca waits for the rhs. Entry-block, the standard
        // shape the ternary's join already uses: the load in `merge`
        // has two predecessors sharing no dominator but entry.
        let (b_for_slot, slot_ty) = self.join_arm_widths(slot_ty, b_for_slot);
        let slot = self.alloca_in_entry(slot_ty, Some("__logic"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(b_for_slot.clone(), Operand::Value(slot), 0),
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
        let b_owned = slot_ty.is_refcounted() && self.expr_owned_shape(right);
        let join_owned = slot_ty.is_refcounted() && (a_owned || b_owned);
        if join_owned && !b_owned {
            self.emit_owned_result_inc_in(self.cur_block, b_for_slot, slot_ty.clone());
        }
        self.f.set_term(self.cur_block, Terminator::Br(merge));
        self.cur_block = false_blk;
        let a_for_slot = self.coerce_arm_to_slot(a_for_slot, slot_ty);
        // a is the falsy value — return it directly (matches JS:
        // `0 && expr` returns 0, not false; `"" && expr` returns "").
        if join_owned && !a_owned {
            self.emit_owned_result_inc_in(self.cur_block, a_for_slot.clone(), slot_ty.clone());
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
        Operand::Value(v)
    }

    /// W3 S8's rule, stated next door in [`crate::ssa_lower_ternary`]:
    /// a number is ONE type to the checker and TWO widths here, so a
    /// join whose arms land on different widths has to settle at
    /// `f64` rather than let the slot wear whichever arm the lowering
    /// happened to see first. `&&` / `||` reconciled nothing, and an
    /// `f64` operand stored into an `i64` slot is not a truncation
    /// the backend performs quietly -- it refuses outright
    /// (`materialize_operand_gpr called on ... holding Fpr`), so
    /// `xs.length && xs[0]` (an i64 length beside an f64 element) was
    /// not a wrong answer but a compile error.
    ///
    /// Answers the joined slot type plus the rhs converted into it.
    /// The lhs lives in a different block, so its half runs at its
    /// own store site through [`Self::coerce_arm_to_slot`].
    fn join_arm_widths(&mut self, slot_ty: Type, b: Operand) -> (Operand, Type) {
        let joined = match (slot_ty, self.operand_ty(&b)) {
            (Type::I64, Type::F64) | (Type::F64, Type::I64) => Type::F64,
            _ => slot_ty,
        };
        let b = self.coerce_arm_to_slot(b, joined);
        (b, joined)
    }

    /// One arm's half of [`Self::join_arm_widths`], emitted in that
    /// arm's own block -- which is the whole reason it is not folded
    /// into the join itself.
    fn coerce_arm_to_slot(&mut self, op: Operand, slot_ty: Type) -> Operand {
        if slot_ty == Type::F64 && self.operand_ty(&op) == Type::I64 {
            return self.coerce_to_f64(op);
        }
        op
    }

    /// Peek whether check.rs answered Any for the JOIN itself while
    /// the operands stayed typed — §13.13 makes `a && b` one of its
    /// two operands, and with no general union type to spell `A | B`
    /// the checker names that Any. The two peeks beside this one look
    /// at the operands and so miss it, and the slot would then take
    /// the lhs's shape and truncate the rhs into it.
    fn result_is_any(&self, eid: ExprId) -> bool {
        matches!(self.expr_types.get(&eid), Some(crate::check::Type::Any))
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
        let key_pin = self.pin_logical_assign_key(left, right);
        let a = self.lower_expr(left);
        let a_ty = self.operand_ty(&a);
        let truthy = self.coerce_to_bool(a.clone());
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
            self.class_join_repr(eid).unwrap_or(a_ty.clone())
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
        let a_owned = slot_ty.is_refcounted() && self.expr_owned_shape(left);
        self.cur_block = eval_b;
        // On the eval-b arm the (falsy) lhs value is dead — an owned
        // lhs temp releases here; the truthy arm keeps a's stake
        // riding into the slot. Mirror of the `&&` arm above.
        if a_owned {
            self.emit_drop_value(a_for_slot.clone(), slot_ty.clone());
        }
        let b = self.lower_expr(right);
        let b_for_slot = if widen_to_any && self.operand_ty(&b) != Type::Any {
            self.box_to_any_from_expr(right, b)
        } else {
            b
        };
        // Slot width settles once both arms are in hand — see the `&&`
        // arm above.
        let (b_for_slot, slot_ty) = self.join_arm_widths(slot_ty, b_for_slot);
        let slot = self.alloca_in_entry(slot_ty, Some("__logic"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(b_for_slot.clone(), Operand::Value(slot), 0),
        );
        // Rotation 325 — owned unification (chunk-722 ternary
        // contract): see the `&&` arm above for the rationale.
        let b_owned = slot_ty.is_refcounted() && self.expr_owned_shape(right);
        let join_owned = slot_ty.is_refcounted() && (a_owned || b_owned);
        if join_owned && !b_owned {
            self.emit_owned_result_inc_in(self.cur_block, b_for_slot, slot_ty.clone());
        }
        self.f.set_term(self.cur_block, Terminator::Br(merge));
        // a is truthy — return it directly (matches JS: `5 || 0`
        // returns 5; `"x" || ""` returns "x"). Its store waits for
        // the rhs so it lands in a slot of the settled width.
        self.cur_block = true_blk;
        let a_for_slot = self.coerce_arm_to_slot(a_for_slot, slot_ty);
        self.f.append_void(
            true_blk,
            InstKind::Store(a_for_slot.clone(), Operand::Value(slot), 0),
        );
        self.f.set_term(true_blk, Terminator::Br(merge));
        if join_owned && !a_owned {
            self.emit_owned_result_inc_in(true_blk, a_for_slot, slot_ty.clone());
        }
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
                // ToBoolean(string) per spec §7.1.2 — falsy iff "",
                // null, or undefined. A Str slot may hold NULL (JS
                // null via a Nullable<String> truthy-narrow) or the
                // undefined sentinel cell (missed exec/match capture,
                // RFC 20260707 chunk 2 — its payload is "undefined"
                // so the len>0 walk would wrongly answer true); the
                // runtime nullish probe covers both before the
                // length load.
                let is_null = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.str_is_nullish, vec![op.clone()]),
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
            Type::BigInt => self.bigint_to_bool(op),
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
            // RegExp / Date / BigInt / FnSig / ...) lower to a single
            // pointer at codegen, so ToBoolean per spec §7.1.2 is a
            // nullish check: NULL (JS null) and the slot type's
            // immortal undefined sentinel (RFC 20260710 C2a FnSig →
            // Str-shaped oddball; C2b Obj/Arr/Closure → the generic
            // Tag::Undefined cell) are the two falsy pointers;
            // everything else is a live object → true. Two inline
            // cmps + and, zero calls. Slot types with no sentinel
            // yet (Symbol / RegExp / Date / ...) keep the plain
            // null check.
            _ => {
                let ne_null = self.cmp(IPred::Ne, op.clone(), Operand::ConstPtrNull);
                let Some(sentinel) = self.str_undef_sentinel_for(ty) else {
                    return ne_null;
                };
                let ne_undef = self.cmp(IPred::Ne, op, sentinel);
                let r = self.f.append_inst(
                    self.cur_block,
                    InstKind::BinOp(crate::ssa::BinOp::And, ne_null, ne_undef),
                    Type::Bool,
                    None,
                );
                Operand::Value(r)
            }
        }
    }

    /// ES §7.1.4 ToBoolean(BigInt) — `0n` is falsy, every other
    /// BigInt is truthy. The heap layout keeps `len == 0` iff the
    /// value is `0n`, so a runtime probe answers it without leaking
    /// LEN_OFF into SSA.
    ///
    /// The probe reads the payload, so a slot that can hold the
    /// generic undefined cell (a read past the end of a `bigint[]`,
    /// a `find` miss) has to be sorted out first — the cell is a
    /// bare header with no words behind it, and asking it for its
    /// length answered `true` for a value that is `undefined`. Same
    /// two falsy pointers as the pointer-slot arm below, only with a
    /// branch rather than an `and` because the probe must not run on
    /// the sentinel at all.
    fn bigint_to_bool(&mut self, op: Operand) -> Operand {
        let sentinel = self
            .str_undef_sentinel_for(Type::BigInt)
            .expect("BigInt spells undefined with the generic cell");
        let ne_null = self.cmp(IPred::Ne, op.clone(), Operand::ConstPtrNull);
        let ne_undef = self.cmp(IPred::Ne, op.clone(), sentinel);
        let live = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(crate::ssa::BinOp::And, ne_null, ne_undef),
            Type::Bool,
            None,
        );
        let result_slot = self.alloca_in_entry(Type::Bool, Some("__bigbool"));
        let live_blk = self.f.add_block();
        let merge = self.f.add_block();
        let cb = self.cur_block;
        self.f.append_void(
            cb,
            InstKind::Store(Operand::ConstBool(false), Operand::Value(result_slot), 0),
        );
        self.f.set_term(
            cb,
            Terminator::CondBr {
                cond: Operand::Value(live),
                then_blk: live_blk,
                else_blk: merge,
            },
        );
        self.cur_block = live_blk;
        let v = self.f.append_inst(
            live_blk,
            InstKind::Call(self.intrinsics.bigint_is_nonzero, vec![op]),
            Type::I64,
            None,
        );
        let nz = self.cmp(IPred::Ne, Operand::Value(v), Operand::ConstI64(0));
        let cb = self.cur_block;
        self.f
            .append_void(cb, InstKind::Store(nz, Operand::Value(result_slot), 0));
        self.f.set_term(cb, Terminator::Br(merge));
        self.cur_block = merge;
        let r = self.f.append_inst(
            merge,
            InstKind::Load(Type::Bool, Operand::Value(result_slot), 0),
            Type::Bool,
            None,
        );
        Operand::Value(r)
    }
}
