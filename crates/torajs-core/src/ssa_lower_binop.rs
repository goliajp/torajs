//! `Expr::BinOp { op, left, right }` lowering pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s `Expr::BinOp` match arm
//! as chunk-81 of the decomp.
//!
//! Dispatch ladder:
//!
//! 1. **Short-circuit logical** (`&&` / `||`) — route to
//!    `lower_logical_and` / `lower_logical_or` BEFORE eager binop
//!    lowering (control flow vs. eager evaluation).
//! 2. **AST-level fold** for `Eq` / `Neq`:
//!    - V3-18 Phase D — `undefined === null` / `=== undefined` /
//!      `null === null` etc. compile-time fold (both lower to
//!      `ConstPtrNull` at runtime layer but JS spec distinguishes
//!      them).
//!    - V3-18 m2.e — `<prim>.constructor === <Ctor>` fold via
//!      `try_fold_constructor_eq`.
//!    - Perf fast-path — `s === "literal"` (short literal ≤16 bytes)
//!      via `try_inline_str_eq_with_literal` (skips `__torajs_str_eq`
//!      C-runtime call, LLVM can't inline cross-module).
//! 3. **Eager evaluation + `lower_binop_with_ids`** (passes operand
//!    ExprIds so Eq/Neq Any-side packing picks `ANY_UNDEF=5` vs
//!    `ANY_NULL=0` correctly).
//! 4. **Refcount drop dance** — TS-shape: `a + b` (string concat)
//!    does NOT consume operands; concat runtime allocs fresh without
//!    freeing inputs. Drop refcounted fresh-owned operands left over
//!    from BinOp on Str / Substr (Eq / Neq / Add). `Ident` / `Member`
//!    / `Index` source means we don't own the heap.
//! 5. **bigint throw-check** — `P7.4-a-b`: a bigint Div/Mod/Pow/Shl/
//!    Shr helper may have called `__torajs_throw_range_error`. Emit
//!    `emit_throw_check(None)` AFTER operands are dropped (block
//!    split before drops would strand the refcounted a/b).

use crate::ast::{BinOp as AstBinOp, Expr, ExprId};
use crate::ssa::Operand;
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    op: AstBinOp,
    left: ExprId,
    right: ExprId,
) -> Operand {
    // M1.5 — short-circuit `&&` / `||` need control flow, not eager
    // evaluation. Route through their own lowering before calling
    // lower_binop (which assumes both operands are already lowered).
    if matches!(op, AstBinOp::LAnd) {
        return ctx.lower_logical_and(eid, left, right);
    }
    if matches!(op, AstBinOp::LOr) {
        return ctx.lower_logical_or(eid, left, right);
    }
    if matches!(op, AstBinOp::Eq | AstBinOp::Neq) {
        if let Some(r) = try_fold_undef_null_eq(ctx, op, left, right) {
            return r;
        }
        // V3-18 m2.e — `<prim>.constructor === <Ctor>` /
        // `<Ctor> === <prim>.constructor` compile-time fold.
        if let Some(r) = ctx.try_fold_constructor_eq(op, left, right) {
            return r;
        }
        // Perf fast-path — `s === "literal"` / `s !== "literal"`
        // (short literal ≤16 bytes) inline len-eq + byte-eq chain,
        // skipping `__torajs_str_eq` C-runtime call (LLVM can't inline
        // cross-module). Critical for switch-on-string hot loops.
        if let Some(r) = ctx.try_inline_str_eq_with_literal(op, left, right) {
            return r;
        }
    }
    // RFC 20260719-fn-tostring-source B6 — `"" + Math.max`: a
    // namespace-static builtin fn member has no value form to lower;
    // in the Add-concat face fold that side to the JSC named native
    // form (§13.15.3 ToPrimitive on a fn is its toString), mirroring
    // the `.toString()` / `String()` folds. Non-Add ops fall through
    // to the operand lower's loud reject.
    let lower_operand = |ctx: &mut LowerCtx<'_>, eid: ExprId| {
        if matches!(op, AstBinOp::Add)
            && let Some(text) =
                crate::ssa_lower_call_fn_tostring::namespace_static_native_form(ctx, eid)
        {
            let s = ctx.intern_string_literal(&text);
            return Operand::Value(s);
        }
        ctx.lower_expr(eid)
    };
    // ToNumber(undefined) is NaN — for an operand that IS the index
    // read, the cheapest place to say so is the read's own
    // out-of-range exit (`f64_oob_plain_for`): the branch already
    // exists, only the constant in it changes, so the value stays
    // exactly the shape `float_demote` was demoting. Undoing the
    // payload afterwards instead cost `array-sum-1m` 54% by putting a
    // branch in the middle of a demotable chain.
    let plain_ok = numeric_only(ctx, op, left, right);
    let a = {
        let hit = plain_ok && is_direct_number_index(ctx, left);
        ctx.binop.f64_oob_plain_for = hit.then_some(left);
        let v = lower_operand(ctx, left);
        ctx.binop.f64_oob_plain_for = None;
        ctx.binop.left_lowered_plain = hit;
        v
    };
    let b = {
        let hit = plain_ok && is_direct_number_index(ctx, right);
        ctx.binop.f64_oob_plain_for = hit.then_some(right);
        let v = lower_operand(ctx, right);
        ctx.binop.f64_oob_plain_for = None;
        ctx.binop.right_lowered_plain = hit;
        v
    };
    // TS-shape: `a + b` (string concat) does NOT consume the
    // operands — both `a` and `b` keep their heaps and remain
    // readable + droppable afterwards. The concat runtime produces
    // a fresh allocation without freeing inputs.
    // P1.5/P1.8 — pass the operand ExprIds so the Eq/Neq Any-side
    // packing can pick ANY_UNDEF=5 vs ANY_NULL=0.
    let result = ctx.lower_binop_with_ids(op, a, b, Some(left), Some(right));
    // Drop fresh-owned refcounted operands left over from BinOp on
    // Str / Substr (Eq / Neq / Add). lower_binop doesn't consume —
    // every concat / str_eq path keeps the inputs live. If the
    // source-level expr was an `Ident` / `Member` / `Index` we don't
    // own the heap (the binding does), so leave it alone. Anything
    // else (`String` literal, `Call` returning Str, sub-BinOp concat
    // result, etc.) was a fresh alloc whose ownership ends here.
    let a_ty = ctx.operand_ty(&a);
    if a_ty.is_refcounted() && ctx.expr_is_fresh_owned(left) {
        ctx.emit_drop_value(a, a_ty);
    }
    let b_ty = ctx.operand_ty(&b);
    if b_ty.is_refcounted() && ctx.expr_is_fresh_owned(right) {
        ctx.emit_drop_value(b, b_ty);
    }
    // P7.4-a-b — a bigint Div/Mod/Pow/Shl/Shr helper may have called
    // __torajs_throw_range_error. Emit the throw-check AFTER the
    // operands are dropped (a block split before the drops would
    // strand the refcounted a/b). Last action on the normal path;
    // #13's binding-slot entry-hoist keeps a `let c = …` slot in
    // the entry block so the post-split scope-end drop's load still
    // resolves.
    if std::mem::take(&mut ctx.bigint_op_may_throw) {
        ctx.emit_throw_check(None);
    }
    result
}

/// True when the operator can only be numeric, so an `undefined`
/// operand is ToNumber'd rather than shown. `+` is excluded whenever
/// either side could be a string or an `any`, because concatenation
/// spells `undefined` out (`xs[oob] + "!"` is `"undefined!"`) and
/// wants the sentinel intact. Comparisons and `===` are excluded for
/// the same reason: they read the bits to answer the question.
fn numeric_only(ctx: &LowerCtx<'_>, op: AstBinOp, left: ExprId, right: ExprId) -> bool {
    match op {
        AstBinOp::Sub | AstBinOp::Mul | AstBinOp::Div | AstBinOp::Mod | AstBinOp::Pow => true,
        AstBinOp::Add => [left, right].iter().all(|e| {
            matches!(
                ctx.expr_types.get(e),
                Some(crate::check::Type::Number) | Some(crate::check::Type::Boolean)
            )
        }),
        _ => false,
    }
}

/// True when this expression *is* the `number[]` index read, so the
/// out-of-range exit about to be emitted for it is the one that
/// should answer a plain NaN. Deliberately not
/// `is_undef_f64_source`: an alias (`const u = xs[oob]; u * 2`) has
/// already been materialised as the sentinel, and `at` / `find` /
/// `pop` reach the exit through paths this hook does not cover —
/// both keep the general
/// [`crate::ssa_lower_f64_sentinel_canon`] route.
pub(crate) fn is_direct_number_index(ctx: &LowerCtx<'_>, eid: ExprId) -> bool {
    matches!(ctx.ast.get_expr(eid), Expr::Index { obj, .. }
    if matches!(
        ctx.expr_types.get(obj),
        Some(crate::check::Type::Array(elem)) if **elem == crate::check::Type::Number
    ))
}

fn try_fold_undef_null_eq(
    ctx: &LowerCtx<'_>,
    op: AstBinOp,
    left: ExprId,
    right: ExprId,
) -> Option<Operand> {
    // V3-18 Phase D — `undefined === null` and friends. Both lower
    // to ConstPtrNull at the runtime layer (no Type::Undefined
    // sentinel yet), but JS spec distinguishes them. Pattern-match
    // the Ident form and fold at AST level: undefined === null →
    // false, undefined === undefined → true, etc.
    let l_is_undef = matches!(ctx.ast.get_expr(left), Expr::Ident(n) if n == "undefined");
    let l_is_null = matches!(ctx.ast.get_expr(left), Expr::Null);
    let r_is_undef = matches!(ctx.ast.get_expr(right), Expr::Ident(n) if n == "undefined");
    let r_is_null = matches!(ctx.ast.get_expr(right), Expr::Null);
    if (l_is_undef || l_is_null) && (r_is_undef || r_is_null) {
        let same = (l_is_undef && r_is_undef) || (l_is_null && r_is_null);
        let result = if matches!(op, AstBinOp::Eq) {
            same
        } else {
            !same
        };
        return Some(Operand::ConstBool(result));
    }
    None
}
