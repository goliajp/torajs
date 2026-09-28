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
//! Each body falls through to the entry of the body written after it
//! (a `default` between cases falls into the next case) unless
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

use crate::ast::{Stmt, SwitchCase};
use crate::ssa::{BlockId, InstKind, Operand, Terminator};
use crate::ssa_lower::LowerCtx;

mod compare;
use compare::emit_case_compare;

pub(crate) fn lower(
    ctx: &mut LowerCtx,
    scrutinee: crate::ast::ExprId,
    cases: &[SwitchCase],
    default: &Option<Vec<Stmt>>,
    default_at: usize,
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
                boxed: false,
                scope_depth: ctx.scope_stack.len() - 1,
            },
        );
        if let Some(top) = ctx.scope_stack.last_mut() {
            top.push(name);
        }
    }
    let after = ctx.f.add_block();
    // §14.12.4 — the CaseBlock is ONE declarative environment
    // spanning every clause and the default. It opens here, before
    // the first clause value is evaluated, and closes at `after`.
    // Without it a clause's `let` stayed in `locals` past the switch
    // (`switch (0) { default: let f = 1; } typeof f` answered
    // "number") and, worse, permanently replaced an outer binding of
    // the same name — the shadow frame is what restores it.
    ctx.scope_stack.push(Vec::new());
    ctx.shadow_stack.push(Vec::new());
    ctx.case_block_depth += 1;
    for c in cases.iter() {
        crate::ssa_lower_stmt_let_decl_recursive::hoist_forward_boxes(ctx, c.body.iter());
    }
    if let Some(db) = default {
        crate::ssa_lower_stmt_let_decl_recursive::hoist_forward_boxes(ctx, db.iter());
    }
    // A clause-declared binding some closure in the CaseBlock captures
    // gets its box HERE, before the compare chain — the clause bodies
    // are siblings, so the block holding the declaration dominates
    // none of the others (see `open_case_block_box`).
    let captured: Vec<String> = cases
        .iter()
        .flat_map(|c| crate::ast::stmts_flat(&c.body))
        .chain(default.iter().flat_map(|d| crate::ast::stmts_flat(d)))
        .filter_map(|s| match s {
            Stmt::LetDecl { name, .. } if ctx.escape_captured_lets.contains(name) => {
                Some(name.clone())
            }
            _ => None,
        })
        .collect();
    for name in captured {
        crate::ssa_lower_stmt_let_decl_recursive::open_case_block_box(ctx, &name);
    }
    // RFC 20260901-scope-exit-drops — `break` leaves the case body's
    // own block frames but lands on `after`, which is where the
    // CaseBlock frame itself is released: depth = `len()` AFTER the
    // push, so every edge into `after` drops that frame exactly once.
    ctx.loop_stack
        .push(crate::ssa_lower_scope_exit::LoopTargets {
            cont: None,
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

    // Clause bodies lower in source order, so a closure in a clause
    // after a mid-switch `default` sees the bindings the default
    // declares.
    for (i, c) in cases.iter().enumerate() {
        let cmp_blk = ctx.cur_block;
        let eq = emit_case_compare(ctx, scrutinee, scrut_val, scrut_ty, c.value, cmp_blk);
        let next_cmp_or_default = if i + 1 < cases.len() {
            ctx.f.add_block()
        } else {
            default_blk.unwrap_or(after)
        };
        ctx.f.set_term(
            ctx.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(eq),
                then_blk: body_blks[i],
                else_blk: next_cmp_or_default,
            },
        );
        if i == default_at
            && let (Some(db), Some(d)) = (default_blk, default)
        {
            lower_clause_body(ctx, db, d, body_blks[i]);
        }
        // Bodies fall through in source order, where the default
        // sits `default_at` clauses in.
        let fall_through = match default_blk {
            Some(db) if i + 1 == default_at => db,
            _ if i + 1 < body_blks.len() => body_blks[i + 1],
            _ => after,
        };
        lower_clause_body(ctx, body_blks[i], &c.body, fall_through);
        if i + 1 < cases.len() {
            ctx.cur_block = next_cmp_or_default;
        }
    }
    if default_at == cases.len()
        && let (Some(db), Some(d)) = (default_blk, default)
    {
        lower_clause_body(ctx, db, d, after);
    }
    if cases.is_empty() {
        let target = default_blk.unwrap_or(after);
        ctx.f.set_term(switch_entry, Terminator::Br(target));
    }

    ctx.loop_stack.pop();
    ctx.case_block_depth -= 1;
    ctx.cur_block = after;
    ctx.close_scope_frame();
}

/// Lower one clause body into `blk`, falling through to `next` unless
/// it already jumped away.
fn lower_clause_body(ctx: &mut LowerCtx, blk: BlockId, body: &[Stmt], next: BlockId) {
    ctx.cur_block = blk;
    for s in body {
        ctx.lower_stmt(s);
        if !ctx.cur_open() {
            break;
        }
    }
    if ctx.cur_open() {
        ctx.f.set_term(ctx.cur_block, Terminator::Br(next));
    }
}
