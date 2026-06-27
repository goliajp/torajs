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
    let after = ctx.f.add_block();
    ctx.loop_stack.push((after, after));

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
        let v = ctx.lower_expr(c.value);
        let eq = match scrut_ty {
            Type::F64 => ctx.f.append_inst(
                cmp_blk,
                InstKind::FCmp(FPred::Oeq, scrut_val, v),
                Type::Bool,
                None,
            ),
            Type::Str | Type::Substr => {
                if let Expr::String(s) = ctx.ast.get_expr(c.value).clone() {
                    let bytes = s.into_bytes();
                    let inline_eligible = bytes.len() <= 16 && bytes.iter().all(|&b| b <= 0x7F);
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
            _ => ctx.f.append_inst(
                cmp_blk,
                InstKind::ICmp(IPred::Eq, scrut_val, v),
                Type::Bool,
                None,
            ),
        };
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
