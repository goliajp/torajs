//! One clause's IsStrictlyEqual compare against the scrutinee, split
//! from the switch lowering (file-size line).

use crate::ast::Expr;
use crate::ssa::{BlockId, FPred, IPred, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// `true` when this clause value is the `undefined` literal — the
/// global, not a local of that name, which would shadow it and
/// would not lower to the null pointer anyway. Same test the any
/// boundary makes (`box_to_any_from_expr`).
fn is_undefined_literal(ctx: &LowerCtx, value: crate::ast::ExprId) -> bool {
    matches!(ctx.ast.get_expr(value), Expr::Ident(n) if n == "undefined")
        && !ctx.locals.contains_key("undefined")
}

/// One clause's IsStrictlyEqual compare against the scrutinee
/// (§14.12.4), specialized on the scrutinee's static type — see the
/// module doc for the per-type shapes.
pub(super) fn emit_case_compare(
    ctx: &mut LowerCtx,
    scrutinee: crate::ast::ExprId,
    scrut_val: Operand,
    scrut_ty: Type,
    value: crate::ast::ExprId,
    cmp_blk: BlockId,
) -> crate::ssa::ValueId {
    let v = ctx.lower_expr(value);
    let is_null = matches!(ctx.ast.get_expr(value), Expr::Null);
    let is_undef = is_undefined_literal(ctx, value);
    let v_ty = ctx.operand_ty(&v);
    if let Some(hit) = settled_compare(ctx, scrutinee, scrut_ty, v_ty, is_null, is_undef) {
        let pred = if hit { IPred::Eq } else { IPred::Ne };
        return ctx.f.append_inst(
            cmp_blk,
            InstKind::ICmp(pred, Operand::ConstI64(0), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
    }
    match scrut_ty {
        // The clause value takes the scrutinee's width: a widened
        // scrutinee (`i % 2` under the f64 class) against a literal
        // `0` handed `FCmp` a `ConstI64` operand, which the FPR
        // materializer cannot hold ("FPR materialization can't
        // hold ConstI64(0)" at build).
        // An f64 slot spells `undefined` with its NaN sentinel, so
        // `case undefined` compares that bit pattern; converted to a
        // number it would be compared as a value.
        Type::F64 if is_undef => {
            let bits = ctx.f.append_inst(
                cmp_blk,
                InstKind::BitCastF64ToI64(scrut_val),
                Type::I64,
                None,
            );
            let sentinel = crate::ssa_lower_undef_f64_source::F64_UNDEF_SENTINEL_BITS as i64;
            ctx.f.append_inst(
                cmp_blk,
                InstKind::ICmp(IPred::Eq, Operand::Value(bits), Operand::ConstI64(sentinel)),
                Type::Bool,
                None,
            )
        }
        Type::F64 => {
            let v = ctx.coerce_to_f64(v);
            ctx.f.append_inst(
                cmp_blk,
                InstKind::FCmp(FPred::Oeq, scrut_val, v),
                Type::Bool,
                None,
            )
        }
        Type::Str | Type::Substr => {
            if let Expr::String(s) = ctx.ast.get_expr(value).clone() {
                let bytes = s.into_bytes();
                // RFC 20260707-undefined-sentinel-repr chunk 1 —
                // a nullable-str scrutinee (missed exec/match
                // capture may be NULL) declines the inline byte
                // walk; the runtime `str_eq` has the null guard.
                let inline_eligible = bytes.len() <= 16
                    && bytes.iter().all(|&b| b <= 0x7F)
                    && !crate::ssa_lower_nullable_guard::is_nullable_str_source(ctx, scrutinee);
                if inline_eligible {
                    let r = ctx.emit_inline_str_eq_bytes(scrut_val, &bytes);
                    if let Operand::Value(vid) = r {
                        vid
                    } else {
                        unreachable!("emit_inline_str_eq_bytes returns Value")
                    }
                } else {
                    let intrinsic = if scrut_ty == Type::Substr {
                        ctx.intrinsics.substr_eq_str
                    } else {
                        ctx.intrinsics.str_eq
                    };
                    ctx.f.append_inst(
                        cmp_blk,
                        InstKind::Call(intrinsic, vec![scrut_val, v]),
                        Type::Bool,
                        None,
                    )
                }
            } else {
                let intrinsic = if scrut_ty == Type::Substr {
                    ctx.intrinsics.substr_eq_str
                } else {
                    ctx.intrinsics.str_eq
                };
                ctx.f.append_inst(
                    cmp_blk,
                    InstKind::Call(intrinsic, vec![scrut_val, v]),
                    Type::Bool,
                    None,
                )
            }
        }
        // §14.12.4 selects a clause with IsStrictlyEqual, which is
        // what `===` means — so an `any` on either side has to take
        // the same runtime path `===` takes. A raw `ICmp` compares
        // a boxed word against a bare one and is never equal, so
        // every case fell through to `default`: a wrong answer
        // where the checker used to raise a loud one.
        _ if scrut_ty == Type::Any || ctx.operand_ty(&v) == Type::Any => {
            // The helper tells a concrete `undefined` from `null` by the
            // per-binop state the `===` arm records, which no clause
            // passes through; boxing the clause value by its expression
            // carries that in the tag instead.
            let v = if is_null || is_undef {
                ctx.box_to_any_from_expr(value, v)
            } else {
                v
            };
            match crate::ssa_lower_binop_inner_strict_eq::try_lower(
                ctx,
                crate::ast::BinOp::Eq,
                scrut_val,
                v,
            ) {
                Some(Operand::Value(vid)) => vid,
                // That helper folds to a constant only when BOTH
                // sides are concrete, which this guard has already
                // ruled out; the Any path always emits a call.
                _ => unreachable!("strict-eq Any path returns a value"),
            }
        }
        // §14.12.4 selects with IsStrictlyEqual, and a
        // pointer-shaped slot spells its undefined with the
        // per-type immortal cell, not NULL. The `undefined` clause
        // value lowers to `ConstPtrNull`, so the raw `ICmp` at the
        // bottom compared the cell against NULL — never equal. The
        // clause was never selected and `switch (o) { case
        // undefined: }` on a slot holding undefined fell through to
        // `default`, which is the answer for the one value the
        // clause exists to catch.
        //
        // The `===` spelling of this exact test has compared
        // against the sentinel ADDRESS since RFC 20260710
        // (`ssa_lower_binop_null_undef`). This site could not reuse
        // it: that lowering reads the per-binop id state the
        // `Expr::Binary` arm records, and a case clause never goes
        // through that arm. So the comparison is spelled here, from
        // the same helper that mints the address.
        //
        // `case null:` needs nothing — NULL is how null is spelled
        // in the slot, and the bottom arm already compares it.
        // Self-gating on the checker's nullable typing.
        _ if is_undefined_literal(ctx, value)
            && crate::ssa_lower_nullable_guard::holds_undef_cell(ctx, scrutinee, scrut_ty) =>
        {
            let sentinel = ctx
                .str_undef_sentinel_for(scrut_ty)
                .expect("guard admits only sentinel-capable slot types");
            ctx.f.append_inst(
                cmp_blk,
                InstKind::ICmp(IPred::Eq, scrut_val, sentinel),
                Type::Bool,
                None,
            )
        }
        // The mirror: an integer scrutinee against a fractional
        // clause value (`case 1.5`) compares in f64 — IsStrictlyEqual
        // on Numbers is numeric, and `ICmp` would hand the GPR
        // materializer an f64 constant.
        _ if scrut_ty == Type::I64 && ctx.operand_ty(&v) == Type::F64 => {
            let l = ctx.coerce_to_f64(scrut_val);
            ctx.f
                .append_inst(cmp_blk, InstKind::FCmp(FPred::Oeq, l, v), Type::Bool, None)
        }
        _ => ctx.f.append_inst(
            cmp_blk,
            InstKind::ICmp(IPred::Eq, scrut_val, v),
            Type::Bool,
            None,
        ),
    }
}

/// The clause compare's answer when it is known without comparing: a
/// scrutinee the checker typed as exactly `undefined` or `null` equals
/// only that clause, whatever its slot spells it with; and two
/// different representations are two different ES types — a number,
/// boolean or string against a value of another family, as in
/// `switch (null) { case 0: }` or `switch ("0") { case 0: }` — for
/// which IsStrictlyEqual answers false. The raw arms would compare
/// bits (NULL equals 0) or call a string kernel on a non-string. An
/// `any` side, and a `null` / `undefined` clause against a slot that
/// can hold it, are the pairs whose answer depends on the value. The
/// clause value has been evaluated either way.
fn settled_compare(
    ctx: &LowerCtx,
    scrutinee: crate::ast::ExprId,
    scrut_ty: Type,
    v_ty: Type,
    is_null: bool,
    is_undef: bool,
) -> Option<bool> {
    let nullish = is_null || is_undef;
    match ctx.expr_types.get(&scrutinee) {
        Some(crate::check::Type::Undefined) if nullish => return Some(is_undef),
        Some(crate::check::Type::Null) if nullish => return Some(is_null),
        _ => {}
    }
    if scrut_ty == Type::Any || v_ty == Type::Any {
        return None;
    }
    // pointer-shaped slots hold both nullish values, an f64 slot
    // spells `undefined` with its sentinel, integer and boolean
    // slots hold neither
    let fits = match scrut_ty {
        Type::I64 | Type::I32 | Type::Bool => false,
        Type::F64 => is_undef,
        _ => true,
    };
    let differ = if nullish {
        !fits
    } else {
        value_kind(scrut_ty) != value_kind(v_ty)
    };
    differ.then_some(false)
}

/// Which ES type family a compare operand belongs to: Number,
/// Boolean, String, or anything else pointer-shaped.
fn value_kind(ty: Type) -> u8 {
    match ty {
        Type::I64 | Type::I32 | Type::F64 => 1,
        Type::Bool => 2,
        Type::Str | Type::Substr => 3,
        _ => 0,
    }
}
