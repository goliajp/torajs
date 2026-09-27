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
        && ctx.locals.get("undefined").is_none()
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
    match scrut_ty {
        // The clause value takes the scrutinee's width: a widened
        // scrutinee (`i % 2` under the f64 class) against a literal
        // `0` handed `FCmp` a `ConstI64` operand, which the FPR
        // materializer cannot hold ("FPR materialization can't
        // hold ConstI64(0)" at build).
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
            match crate::ssa_lower_binop_inner_strict_eq::try_lower(
                ctx,
                crate::ast::BinOp::Eq,
                scrut_val.clone(),
                v.clone(),
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
