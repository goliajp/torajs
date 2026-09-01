//! `Stmt::Switch` arm of `LowerCtx::lower_stmt` extracted from
//! [`crate::ssa_lower`] (chunk 146).
//!
//! Pre-extract this arm was 189 LOC inline inside `lower_stmt`
//! (over the 200-line god-fn hard limit when counted with the
//! surrounding match — though the lower_stmt body itself is the
//! biggest god-fn left in ssa_lower.rs at ~2700 LOC; arm-by-arm
//! extraction same pattern as chunk 145).
//!
//! Lowers `switch (scrutinee) { case v: body; default: body }` as
//! a chain of strict-eq compares with shared fall-through bodies:
//!
//! ```text
//! eval scrutinee → cmp_0 → (body_0 | cmp_1) → cmp_1 →
//!   (body_1 | … | default | after) → after
//! ```
//!
//! Each body falls through to the next body's entry unless
//! interrupted by `break` (loop_stack supplies the break target =
//! `after`). Per-arm compare specializes on scrutinee type:
//!
//! - `F64` → `FCmp::Oeq`.
//! - `Str` / `Substr` with ASCII-only ≤16-byte literal case value
//!   → inline byte-cmp fast path (`emit_inline_str_eq_bytes`,
//!   skips `__torajs_str_eq` / `__torajs_substr_eq_str` C call).
//!   Non-literal or non-ASCII falls back to the runtime helper
//!   (substr_eq_str when scrutinee is Substr; str_eq otherwise).
//! - Everything else → `ICmp::Eq`.
//!
//! Empty-cases edge case (`switch (x) { default: ... }` or
//! `switch (x) {}`) wires `switch_entry` directly to the default
//! body (or to `after` when there's no default either).

use crate::ast::{Expr, Stmt, SwitchCase};
use crate::ssa::{BlockId, FPred, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower(
    ctx: &mut LowerCtx,
    scrutinee: crate::ast::ExprId,
    cases: &[SwitchCase],
    default: &Option<Vec<Stmt>>,
) {
    let scrut_val = ctx.lower_expr(scrutinee);
    let scrut_ty = ctx.operand_ty(&scrut_val);
    // Chunk 750 — a fresh-owned scrutinee (call result / concat) has
    // no release site: every per-case compare only borrows, and the
    // multi-exit shape (fall-through / break / body return) has no
    // single post-compare point. Park it in an anonymous slot on the
    // enclosing scope frame so the existing scope-close / fn-exit
    // drop walks release it on every path (`switch (pick(i))` leaked
    // one cell per execution — 15.9MB/300k churn).
    if scrut_ty.is_refcounted() && ctx.expr_is_fresh_owned(scrutinee) {
        let name = format!("__switch_scrut_{}", scrutinee.0);
        let slot = ctx.alloca_in_entry(scrut_ty, Some(&name));
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(scrut_val.clone(), Operand::Value(slot), 0),
        );
        ctx.locals.insert(
            name.clone(),
            crate::ssa_lower::LocalInfo {
                slot,
                ty: scrut_ty,
                moved: false,
                borrowed: false,
                scope_depth: ctx.scope_stack.len() - 1,
            },
        );
        if let Some(top) = ctx.scope_stack.last_mut() {
            top.push(name);
        }
    }
    let after = ctx.f.add_block();
    // RFC 20260901-scope-exit-drops — case bodies lower into the
    // enclosing frame (no switch frame); a `break` owes only the
    // block frames a case body opened: depth = `len()`.
    ctx.loop_stack
        .push(crate::ssa_lower_scope_exit::LoopTargets {
            cont: after,
            brk: after,
            scope_depth: ctx.scope_stack.len(),
            teardown_depth: ctx.for_of_teardown_stack.len(),
        });

    let switch_entry = ctx.cur_block;

    let body_blks: Vec<BlockId> = cases.iter().map(|_| ctx.f.add_block()).collect();
    let default_blk = if default.is_some() {
        Some(ctx.f.add_block())
    } else {
        None
    };

    for (i, c) in cases.iter().enumerate() {
        let cmp_blk = ctx.cur_block;
        let _ = i;
        let eq = emit_case_compare(ctx, scrutinee, scrut_val, scrut_ty, c.value, cmp_blk);
        let next_cmp_or_default = if i + 1 < cases.len() {
            ctx.f.add_block()
        } else {
            default_blk.unwrap_or(after)
        };
        let _ = cmp_blk;
        ctx.f.set_term(
            ctx.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(eq),
                then_blk: body_blks[i],
                else_blk: next_cmp_or_default,
            },
        );
        let fall_through = if i + 1 < body_blks.len() {
            body_blks[i + 1]
        } else {
            default_blk.unwrap_or(after)
        };
        ctx.cur_block = body_blks[i];
        for s in &c.body {
            ctx.lower_stmt(s);
            if !ctx.cur_open() {
                break;
            }
        }
        if ctx.cur_open() {
            ctx.f.set_term(ctx.cur_block, Terminator::Br(fall_through));
        }
        if i + 1 < cases.len() {
            ctx.cur_block = next_cmp_or_default;
        }
    }

    if let (Some(db), Some(default_body)) = (default_blk, default) {
        ctx.cur_block = db;
        for s in default_body {
            ctx.lower_stmt(s);
            if !ctx.cur_open() {
                break;
            }
        }
        if ctx.cur_open() {
            ctx.f.set_term(ctx.cur_block, Terminator::Br(after));
        }
    }
    if cases.is_empty() {
        let target = default_blk.unwrap_or(after);
        ctx.f.set_term(switch_entry, Terminator::Br(target));
    }

    ctx.loop_stack.pop();
    ctx.cur_block = after;
}

/// One clause's IsStrictlyEqual compare against the scrutinee
/// (§14.12.4), specialized on the scrutinee's static type — see the
/// module doc for the per-type shapes.
fn emit_case_compare(
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
