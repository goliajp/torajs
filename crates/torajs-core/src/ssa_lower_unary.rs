//! Unary operator lowering — `!` / `-` / `~` / `+` over every operand
//! type: Any-box arithmetic, Bool / null / string ToNumber coercion,
//! BigInt helpers, IEEE -0-preserving negation and ToInt32 bit-not
//! (extracted from `ssa_lower.rs`, file-size known-debt #1).

use crate::ast::{Expr, ExprId, UnaryOp};
use crate::ssa::{BinOp as SsaBinOp, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

impl LowerCtx<'_> {
    pub(crate) fn lower_unary(&mut self, op: UnaryOp, expr: ExprId) -> Operand {
        // M1.5 — `!a` lowers to `xor a, true`. Operand is bool,
        // result is bool. (BinOp::Xor on i1/i8 flips the low bit;
        // since bools only carry 0 or 1, this is logical not.)
        // M6.1 prereq — `-x` lowers to `0 - x`. f64 path emits
        // fsub from 0.0 (no SItoFP needed since both ops are
        // f64); i64 path emits sub from 0.
        //
        // Special case for `-NumberLit(0)`: the i64 narrowing
        // path collapses both `+0` and `-0` to `ConstI64(0)`,
        // losing IEEE 754 sign. We need `-0` to survive so
        // `Object.is(0, -0) === false` and `1 / -0 === -Infinity`
        // hold. Detect the AST shape `Unary(Neg, Number(0.0))`
        // and emit `ConstF64(-0.0)` directly, bypassing the
        // i64 path entirely.
        if matches!(op, crate::ast::UnaryOp::Neg)
            && let Expr::Number(n) = self.ast.get_expr(expr)
            && *n == 0.0
            && n.fract() == 0.0
        {
            return Operand::ConstF64(-0.0);
        }
        // S135 / S136 — unary on `undefined`. ToNumber(undefined) = NaN
        // per spec §7.1.4 so `+undefined` and `-undefined` both yield
        // NaN; `~undefined` = ToInt32(NaN) ^ -1 = -1 (§7.1.6). `!`
        // already works since coerce_to_bool treats the ConstPtrNull
        // sentinel as falsy → true. Frontend Type::Undefined lowers
        // to ConstPtrNull (same as null), so expr_types is the source
        // of truth — without this fast path the Plus/Neg paths would
        // see ConstPtrNull and emit `ConstI64(0)` like `+null` does.
        if matches!(
            self.expr_types.get(&expr),
            Some(crate::check::Type::Undefined)
        ) {
            match op {
                crate::ast::UnaryOp::Plus | crate::ast::UnaryOp::Neg => {
                    return Operand::ConstF64(f64::NAN);
                }
                crate::ast::UnaryOp::BitNot => return Operand::ConstI64(-1),
                _ => {}
            }
        }
        // ToNumber(undefined) is NaN, spelled where it is free — the
        // read's own out-of-range exit. `-` flips the sign bit, which
        // the sentinel compare misses, so `-(-xs[oob])` used to
        // restore the exact pattern and read back as `undefined`.
        let neg_or_plus = matches!(op, crate::ast::UnaryOp::Neg | crate::ast::UnaryOp::Plus);
        self.binop.f64_oob_plain_for = (neg_or_plus
            && crate::ssa_lower_binop::is_direct_number_index(self, expr))
        .then_some(expr);
        let v = self.lower_expr(expr);
        self.binop.f64_oob_plain_for = None;
        // P0.9 — Any operand on unary `-` / `+`: route through
        // any_arith. See [`Self::lower_unary_any_arith`].
        if matches!(op, crate::ast::UnaryOp::Neg | crate::ast::UnaryOp::Plus)
            && matches!(self.operand_ty(&v), Type::Any)
        {
            return self.lower_unary_any_arith(op, v);
        }
        // RFC 20260716 刀 7 — Any operand on unary `~`: route
        // through any_bitnot (ToNumber → ToInt32 → xor -1). Mirror
        // of the Neg/Plus P0.9 pattern above.
        if matches!(op, crate::ast::UnaryOp::BitNot) && matches!(self.operand_ty(&v), Type::Any) {
            return self.lower_unary_any_bitnot(v);
        }
        // V3-18 m1.f / m1.h.4 — coerce Bool / null / Str before
        // unary `-`, `~`, `+`. See [`Self::coerce_unary_operand`].
        let v = self.coerce_unary_operand(op, v);
        match op {
            crate::ast::UnaryOp::Not => {
                // V3-18 m1.h.2 — coerce truthy first; the
                // existing xor-with-true path then flips.
                let raw = v.clone();
                let v = self.coerce_to_bool(v);
                // Chunk 636 — `!f()` consumes the operand by the
                // truthiness test alone; release an owned temp
                // (see ssa_lower_stmt_if.rs).
                self.release_owned_temp(expr, &raw);
                let r = self.f.append_inst(
                    self.cur_block,
                    InstKind::BinOp(SsaBinOp::Xor, v, Operand::ConstBool(true)),
                    Type::Bool,
                    None,
                );
                Operand::Value(r)
            }
            crate::ast::UnaryOp::Neg => self.lower_unary_neg(v),
            crate::ast::UnaryOp::BitNot => {
                let v_ty = self.operand_ty(&v);
                if v_ty == Type::BigInt {
                    // V3-02 — BigInt `~x` ≡ `-x - 1n`. Routes
                    // through the bigint_not runtime helper
                    // (which uses the same identity).
                    let r = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.bigint_not, vec![v]),
                        Type::BigInt,
                        None,
                    );
                    return Operand::Value(r);
                }
                // L3a-8 — `~x` is `ToInt32(x) ^ -1` per JS spec
                // §13.5.6: normalize to int32 first so
                // `~4294967295` gives 0 like v8 / jsc. The xor
                // of a sign-extended-32 value with -1 stays
                // sign-extended-32 — no post-normalization.
                let vi = self.coerce_f64_to_i64_for_bitwise(v);
                let v32 = self.emit_to_int32(vi);
                let r = self.f.append_inst(
                    self.cur_block,
                    InstKind::BinOp(SsaBinOp::Xor, v32, Operand::ConstI64(-1)),
                    Type::I64,
                    None,
                );
                Operand::Value(r)
            }
            crate::ast::UnaryOp::Plus => {
                // V3-18 m1.h.4 — `+x` is ToNumber(x). For
                // already-numeric inputs we just pass through;
                // Bool/Null get coerced via the m1.f path
                // (already applied above for Neg/BitNot we
                // mirror here). The result type is Number
                // (i64 here, since the operand is now i64
                // after coerce).
                //
                // An f64 is the one case where passing through is
                // not ToNumber. Every other numeric operator emits
                // a real FP instruction, and FPCR.DN (see
                // `cmd_build_synthesize::FPCR_DN_BIT`) makes those
                // hand back a plain NaN whatever payload the
                // operand wore — which is what turns the
                // `undefined` sentinel into the `NaN` the spec
                // asks for. `+x` alone emits nothing, so `+u` on an
                // out-of-range read still read back as `undefined`.
                // Multiplying by one is the identity on every
                // Number, ±0 and ±∞ included, and is an FP
                // instruction, so it performs the conversion the
                // operator names.
                if matches!(self.operand_ty(&v), Type::F64) {
                    let r = self.f.append_inst(
                        self.cur_block,
                        InstKind::BinOp(SsaBinOp::FMul, v, Operand::ConstF64(1.0)),
                        Type::F64,
                        None,
                    );
                    Operand::Value(r)
                } else {
                    v
                }
            }
        }
    }

    /// P0.9 — Any operand on unary `-` / `+`: route through the
    /// any_arith helper. `-x` ≡ `0 - x` so we call any_arith with
    /// op=Sub (0), LHS=ConstI64(0)+ANY_I64 tag, RHS=unboxed-from-x.
    /// Result is fresh Any-box.
    /// Step 7c: read tag/value via shim (was inline +8/+16).
    fn lower_unary_any_arith(&mut self, op: UnaryOp, v: Operand) -> Operand {
        let r_tag = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.any_unbox_tag, vec![v.clone()]),
            Type::I64,
            None,
        );
        let r_value = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.any_unbox_value, vec![v.clone()]),
            Type::I64,
            None,
        );
        let result = if matches!(op, crate::ast::UnaryOp::Neg) {
            // §13.5.5 — the pair kernel's BigInt leg negates
            // legally (§6.1.6.2.1); every other tag rides the
            // Number lane's `0 - x` inside. The raw any_arith
            // emission this replaces threw the mixed-pair
            // TypeError on a BigInt operand.
            let r = self.f.append_inst(
                self.cur_block,
                InstKind::Call(
                    self.intrinsics.any_unary_neg,
                    vec![Operand::Value(r_tag), Operand::Value(r_value)],
                ),
                Type::Any,
                None,
            );
            Operand::Value(r)
        } else {
            // Unary `+x` ≡ ToNumber(x) — call any_arith
            // op=Mul with LHS=1 to coerce to Number
            // without changing value.
            let r = self.f.append_inst(
                self.cur_block,
                InstKind::Call(
                    self.intrinsics.any_arith,
                    vec![
                        Operand::ConstI64(1), // op=Mul
                        Operand::ConstI64(2), // ANY_I64
                        Operand::ConstI64(1), // value 1
                        Operand::Value(r_tag),
                        Operand::Value(r_value),
                    ],
                ),
                Type::Any,
                None,
            );
            Operand::Value(r)
        };
        // any_arith only borrowed the pair — reclaim a ShortStr-
        // materialized temp (no-op for every other input).
        self.f.append_void(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.any_unbox_settle,
                vec![v, Operand::Value(r_value)],
            ),
        );
        // §7.1.4 records pending throws (Symbol reject, `+bigint`'s
        // mixed-pair TypeError, OrdinaryToPrimitive both-objects) —
        // without this check the undefined placeholder leaked out as
        // the result and the stranded throw poisoned the next entry
        // (rotation 370: `+b` over any(bigint) printed `undefined`).
        self.emit_throw_check(None);
        result
    }

    /// RFC 20260716 刀 7 — Any operand on unary `~`: unbox pair
    /// via shim, route through `any_bitnot` runtime helper. Mirror
    /// of [`Self::lower_unary_any_arith`] but with a 2-arg pair
    /// signature and no `any_unbox_settle` (any_bitnot borrows the
    /// pair and never materializes a ShortStr temp — the operand
    /// is ToNumber-coerced through the `any_to_number` heap path).
    fn lower_unary_any_bitnot(&mut self, v: Operand) -> Operand {
        let r_tag = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.any_unbox_tag, vec![v.clone()]),
            Type::I64,
            None,
        );
        let r_value = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.any_unbox_value, vec![v.clone()]),
            Type::I64,
            None,
        );
        let r = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.any_bitnot,
                vec![Operand::Value(r_tag), Operand::Value(r_value)],
            ),
            Type::Any,
            None,
        );
        // any_bitnot only borrowed the pair — reclaim a ShortStr-
        // materialized temp (no-op for every other input).
        self.f.append_void(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.any_unbox_settle,
                vec![v, Operand::Value(r_value)],
            ),
        );
        // §7.1.4 pending throws (Symbol reject / OrdinaryToPrimitive
        // both-objects) — same leak the arith twin had (rotation 370).
        self.emit_throw_check(None);
        Operand::Value(r)
    }

    /// V3-18 m1.f / m1.h.4 — coerce Bool / null / Str before unary
    /// `-`, `~`, `+`. For `-`, IEEE 754 -0 must survive when the
    /// operand is the falsy 0 (-false / -null = -0.0 per bun), so we
    /// route via f64 — the existing FSub-from-(-0.0) sign-preserving
    /// path picks it up. For `~` and `+` integer is fine. Strings
    /// route through `__torajs_str_to_number` per spec
    /// §13.5.{4,5,6} ToNumber.
    fn coerce_unary_operand(&mut self, op: UnaryOp, v: Operand) -> Operand {
        // §13.5.4 / §13.5.5 — unary `+` / `-` are ToNumber on the
        // operand, so an object runs its `valueOf` (NaN when it has
        // none). Both signs share this, and both want the f64 the
        // runtime kernel answers with, so it sits ahead of the
        // per-sign coercions below. A user `valueOf` can throw.
        // Every object shape takes this route, not just the struct
        // one: an array's ToNumber is its join's, a Date's is its
        // time value, and the checker used to reject the rest rather
        // than let them reach here.
        if matches!(op, UnaryOp::Plus | UnaryOp::Neg)
            && is_number_coercible_obj(self.operand_ty(&v))
        {
            let boxed = self.box_to_any(v);
            let n = self.coerce_any_to_number(boxed, Type::F64);
            self.emit_throw_check(None);
            return n;
        }
        match op {
            crate::ast::UnaryOp::Neg => {
                if matches!(v, Operand::ConstPtrNull) {
                    Operand::ConstF64(0.0)
                } else if matches!(self.operand_ty(&v), Type::Bool) {
                    // bool → i64 → f64 chain so the sign-
                    // preserving FSub path picks it up.
                    let i = self.coerce_bool_to_i64(v);
                    self.coerce_to_f64(i)
                } else if matches!(self.operand_ty(&v), Type::Str | Type::Substr) {
                    // `-s` — the F64 Neg branch picks the result
                    // up and emits the sign-preserving FSub.
                    let r = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.str_to_number, vec![v]),
                        Type::F64,
                        None,
                    );
                    Operand::Value(r)
                } else {
                    v
                }
            }
            crate::ast::UnaryOp::Plus => {
                if matches!(v, Operand::ConstPtrNull) {
                    Operand::ConstI64(0)
                } else if matches!(self.operand_ty(&v), Type::Bool) {
                    self.coerce_bool_to_i64(v)
                } else if matches!(self.operand_ty(&v), Type::Str | Type::Substr) {
                    // `+s` — result is f64 so NaN can survive
                    // parse failures.
                    let r = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.str_to_number, vec![v]),
                        Type::F64,
                        None,
                    );
                    Operand::Value(r)
                } else {
                    v
                }
            }
            crate::ast::UnaryOp::BitNot => {
                if matches!(v, Operand::ConstPtrNull) {
                    Operand::ConstI64(0)
                } else if matches!(self.operand_ty(&v), Type::Bool) {
                    self.coerce_bool_to_i64(v)
                } else if matches!(self.operand_ty(&v), Type::Str | Type::Substr) {
                    // S137 — `~s` = ToInt32(ToNumber(s)); strtod
                    // NaN on parse failure, and the post-coerce
                    // BitNot path (coerce_f64_to_i64_for_bitwise →
                    // emit_to_int32 → xor -1) already handles
                    // NaN → 0 → -1 (matches bun's `~'abc' === -1`).
                    let r = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.str_to_number, vec![v]),
                        Type::F64,
                        None,
                    );
                    Operand::Value(r)
                } else {
                    v
                }
            }
            _ => v,
        }
    }

    /// `Neg` arm — BigInt helper call / F64 const fold + FSub from
    /// -0.0 (±0 sign preservation) / I64 const fold + W3 S8
    /// sign-preserving f64 route for runtime ints.
    fn lower_unary_neg(&mut self, v: Operand) -> Operand {
        let v_ty = self.operand_ty(&v);
        match v_ty {
            Type::BigInt => {
                // T-25 — fresh +1 rc BigInt with
                // sign flipped. Drop responsibility
                // matches the rest of the BigInt
                // arithmetic path (caller side).
                let r = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.bigint_neg, vec![v]),
                    Type::BigInt,
                    None,
                );
                Operand::Value(r)
            }
            Type::F64 => {
                // V3-18 m2.d follow-up — fold Neg on
                // ConstF64 at lower-time. Otherwise
                // `-Infinity` becomes a Value via FSub
                // and downstream coerce_to_i64 sees an
                // f64 Value (FpToSi → poison for
                // non-finite, hangs / corrupts).
                if let Operand::ConstF64(n) = v {
                    return Operand::ConstF64(-n);
                }
                // Use -0.0 (not +0.0) as the LHS so the
                // ±0 sign is preserved: IEEE 754 gives
                // (+0) - (+0) = +0 but (-0) - (+0) = -0,
                // and (-0) - x = -x for all finite x.
                // Required by `Object.is(0, -0) === false`
                // and any other code that distinguishes
                // signed zeros.
                let r = self.f.append_inst(
                    self.cur_block,
                    InstKind::BinOp(SsaBinOp::FSub, Operand::ConstF64(-0.0), v),
                    Type::F64,
                    None,
                );
                Operand::Value(r)
            }
            _ => {
                // W3 C4 — fold Neg on ConstI64 at
                // lower-time (mirror of the ConstF64
                // fold above): a negative-literal
                // operand must be visible to the Mul
                // -0 float predicate (`x * -1`), which
                // matches on Operand::ConstI64.
                if let Operand::ConstI64(n) = v
                    && let Some(m) = n.checked_neg()
                {
                    return Operand::ConstI64(m);
                }
                // W3 S8 (rfc 20260611-ann-width-unification
                // §5.3) — `-x` on a runtime int mints -0
                // when x == 0 (JS spec §13.5.5 unary minus),
                // which i64 cannot represent, so a
                // non-constant negation routes through the
                // sign-preserving f64 FSub path. The
                // frem_narrow fptosi sink recovers
                // `sub(0, x)` where the -0 is unobservable
                // (i64 truncation collapses it), same
                // playbook as Mod / Mul.
                if self.operand_ty(&v) == Type::I64 {
                    let vf = self.coerce_to_f64(v);
                    let r = self.f.append_inst(
                        self.cur_block,
                        InstKind::BinOp(SsaBinOp::FSub, Operand::ConstF64(-0.0), vf),
                        Type::F64,
                        None,
                    );
                    return Operand::Value(r);
                }
                let r = self.f.append_inst(
                    self.cur_block,
                    InstKind::BinOp(SsaBinOp::Sub, Operand::ConstI64(0), v),
                    Type::I64,
                    None,
                );
                Operand::Value(r)
            }
        }
    }
}

/// The object-shaped SSA types whose ToNumber (§7.1.4 step 9) is
/// OrdinaryToPrimitive over the receiver — every one reaches the
/// runtime's any-lane coercion, and the answer is the receiver's
/// `valueOf` / `toString`, NaN when neither says a number.
///
/// `Symbol` and `BigInt` are absent because ToNumber throws for them;
/// the checker rejects those two at compile time, as TypeScript does.
///
/// Shared with the binary lanes (`ssa_lower_binop_inner`), which box
/// the same shapes for the same kernel — one list, so a shape the
/// checker starts admitting cannot reach only half of them.
pub(crate) fn is_number_coercible_obj(t: Type) -> bool {
    matches!(
        t,
        Type::Obj(_)
            | Type::Arr(_)
            | Type::Closure(_)
            | Type::FnSig(_)
            | Type::RegExp
            | Type::Date
            | Type::Promise
            | Type::Map
            | Type::Set
            | Type::MapIter
            | Type::ArrIter
            | Type::WeakMap
            | Type::WeakSet
            | Type::WeakRef
    )
}
