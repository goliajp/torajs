//! Split-for-i → for-of iterator rewrite pass (P-iter Phase 3).
//!
//! Chunk 353 — extracted from ast.rs. Orchestrates the AST pass:
//! `rewrite_split_for_i_to_iter` walks every stmt list looking for the
//! `let X = E.split(LIT); for (let I = 0; I < X.length; ...)` pair,
//! verifies escape constraints via the `sfi_safe` sibling, and rewrites
//! the pair into a `Stmt::Block([mut-counter, ForOfSplitIter])` shape
//! whose body is materialised via the `sfi_rewrite` sibling.
//!
//! The four pattern-matchers (`for_i_x_length_match`,
//! `match_split_call_with_lit_sep`, `is_zero_lit`, `is_i_plus_eq_1`) +
//! the two walkers (`rewrite_sfi_walk_list`, `rewrite_sfi_walk_stmt`)
//! + the `SplitForICtx` counter live here. The pub entry stays
//! reachable at `torajs_core::ast::rewrite_split_for_i_to_iter` via the
//! `pub use` in ast.rs.

use super::{Ast, BinOp, Expr, ExprId, Stmt};

/// P-iter Phase 3 — rewrite `let X = E.split(LIT); for (let I = 0; I <
/// X.length; I = I + 1) { ... X[I] ... }` into a `for-of E.split(LIT)`
/// shape so the SplitIter substrate (Phase 1+2) eliminates the
/// per-call Array<Substr> allocation. Conservative: bails to the
/// untouched original if the body or trailing stmts could see X as a
/// random-access Array.
///
/// Pattern (must hold over an adjacent stmt pair):
///   1. `Stmt::LetDecl { name: X, init: Call { Member { obj: E,
///      name: "split" }, args: [SEP] } }` where SEP is `Expr::String(_)`
///      (literal sep — guarantees lifetime via STATIC_LITERAL globals).
///   2. `Stmt::For { init: Some(LetDecl { name: I, init: Number(0),
///      mutable: true }), cond: Some(BinOp::Lt(Ident(I), Member { obj:
///      Ident(X), name: "length" })), step: Some(Assign { target:
///      Ident(I), value: BinOp::Add(Ident(I), Number(1)) }), body: ... }`.
///
/// Escape verification on body + trailing stmts:
///   - Every read of X must be `Index { obj: Ident(X), index: Ident(I) }`
///     (sequential access only). X used elsewhere — `X[i+1]`, `X.length`
///     inside body, `X` as fn arg, `X` stored to outer slot, X read
///     after the loop — disqualifies the rewrite.
///   - The `I` counter may be read freely (e.g. as a position argument);
///     the rewrite preserves it via a manual mut counter.
///
/// Rewrite emits:
///   Stmt::Block([
///     LetDecl mutable I = 0,
///     ForOfSplitIter { var: <fresh>, parent: E, sep: SEP, body: BODY' },
///   ])
///
/// where BODY' is the original body with `X[I]` index reads replaced
/// by `Ident(<fresh>)` and a trailing `I = I + 1` appended so the
/// counter still advances per iter.
pub fn rewrite_split_for_i_to_iter(ast: &mut Ast) {
    let mut ctx = SplitForICtx { counter: 0 };
    let mut top = std::mem::take(&mut ast.stmts);
    rewrite_sfi_walk_list(ast, &mut top, &mut ctx);
    ast.stmts = top;
}

struct SplitForICtx {
    counter: u32,
}

fn rewrite_sfi_walk_list(ast: &mut Ast, stmts: &mut Vec<Stmt>, ctx: &mut SplitForICtx) {
    // Forward scan: for each Stmt::For matching the for-i+.length
    // pattern, look back through prior stmts in the same block for
    // the matching `let X = E.split(LIT)` declaration. Intermediate
    // stmts (e.g. unrelated `let total = 0`) are allowed as long as
    // they don't reference X. Once both endpoints are found and all
    // escape conditions hold, splice: drop X-decl at j and replace
    // For at i with the rewritten Block.
    let mut to_remove: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut replacements: std::collections::HashMap<usize, Stmt> = std::collections::HashMap::new();
    for i in 0..stmts.len() {
        if to_remove.contains(&i) {
            continue;
        }
        // Try to extract i_name + x_name from the For pattern at
        // stmts[i]. Bails on any non-canonical shape.
        let (i_name, x_name) = match for_i_x_length_match(ast, &stmts[i]) {
            Some(t) => t,
            None => continue,
        };
        // Look back for `let X = E.split(LIT)` in this block. Scan
        // i-1 → 0; bail at the first stmt that references X with a
        // shape other than the X-declaration.
        let mut found_j: Option<usize> = None;
        let mut hit_blocker = false;
        for j in (0..i).rev() {
            if to_remove.contains(&j) {
                hit_blocker = true;
                break;
            }
            // Is this the matching X-decl?
            if let Stmt::LetDecl { name, init, .. } = &stmts[j]
                && name == &x_name
            {
                if match_split_call_with_lit_sep(ast, *init).is_some() {
                    found_j = Some(j);
                }
                break;
            }
            // Otherwise, intermediate stmt must not reference X.
            if crate::ast::sfi_safe::sfi_stmt_uses_ident(ast, &stmts[j], &x_name) {
                hit_blocker = true;
                break;
            }
        }
        let _ = hit_blocker;
        let Some(j) = found_j else { continue };
        // Verify body / trailing escape constraints.
        let (parent_eid, sep_eid) = match &stmts[j] {
            Stmt::LetDecl { init, .. } => match_split_call_with_lit_sep(ast, *init).unwrap(),
            _ => unreachable!(),
        };
        let body_box = match &stmts[i] {
            Stmt::For { body, .. } => body.clone(),
            _ => continue,
        };
        if !crate::ast::sfi_safe::sfi_body_x_safe(ast, &body_box, &x_name, &i_name) {
            continue;
        }
        if stmts
            .iter()
            .skip(i + 1)
            .any(|s| crate::ast::sfi_safe::sfi_stmt_uses_ident(ast, s, &x_name))
        {
            continue;
        }
        // Build the rewrite: Block([let mut I = 0, ForOfSplitIter])
        let id = ctx.counter;
        ctx.counter += 1;
        let v_name = format!("__sfi_v_{id}");
        let new_body =
            crate::ast::sfi_rewrite::sfi_rewrite_body(ast, &body_box, &x_name, &i_name, &v_name);
        // Append `I = I + 1` step so the body's manual counter ticks.
        let i_ref_step_l = ast.add_expr(Expr::Ident(i_name.clone()));
        let i_ref_step_r = ast.add_expr(Expr::Ident(i_name.clone()));
        let one_eid = ast.add_expr(Expr::Number(1.0));
        let inc_eid = ast.add_expr(Expr::BinOp {
            op: BinOp::Add,
            left: i_ref_step_r,
            right: one_eid,
        });
        let assign_eid = ast.add_expr(Expr::Assign {
            target: i_ref_step_l,
            value: inc_eid,
        });
        let body_with_inc = match new_body {
            Stmt::Block(mut inner) => {
                inner.push(Stmt::Expr(assign_eid));
                Stmt::Block(inner)
            }
            other => Stmt::Block(vec![other, Stmt::Expr(assign_eid)]),
        };
        let zero_eid = ast.add_expr(Expr::Number(0.0));
        let counter_decl = Stmt::LetDecl {
            mutable: true,
            name: i_name.clone(),
            type_ann: Some("number".into()),
            init: zero_eid,
            is_var: false,
        };
        let forof = Stmt::ForOfSplitIter {
            var_name: v_name,
            parent: parent_eid,
            sep: sep_eid,
            body: Box::new(body_with_inc),
        };
        let new_block = Stmt::Block(vec![counter_decl, forof]);
        to_remove.insert(j);
        replacements.insert(i, new_block);
    }
    // Apply: keep stmts not in to_remove; for indices in replacements,
    // swap to the new Stmt; otherwise clone original.
    if !to_remove.is_empty() || !replacements.is_empty() {
        let mut out: Vec<Stmt> = Vec::with_capacity(stmts.len());
        for (idx, s) in stmts.iter().enumerate() {
            if to_remove.contains(&idx) {
                continue;
            }
            if let Some(rw) = replacements.remove(&idx) {
                out.push(rw);
            } else {
                out.push(s.clone());
            }
        }
        *stmts = out;
    }
    // Recurse into each child stmt's nested lists.
    for s in stmts.iter_mut() {
        rewrite_sfi_walk_stmt(ast, s, ctx);
    }
}

/// Returns Some((i_name, x_name)) if the Stmt is a canonical
/// `for (let mut I = 0; I < X.length; I = I + 1) BODY`. None on any
/// shape mismatch.
fn for_i_x_length_match(ast: &Ast, s: &Stmt) -> Option<(String, String)> {
    let Stmt::For {
        init, cond, step, ..
    } = s
    else {
        return None;
    };
    // init: let mut I = 0
    let i_name = match init.as_deref() {
        Some(Stmt::LetDecl {
            mutable: true,
            name,
            init: init_eid,
            ..
        }) if is_zero_lit(ast, *init_eid) => name.clone(),
        _ => return None,
    };
    let cond_eid = (*cond)?;
    // cond: I < X.length
    let x_name = match ast.get_expr(cond_eid) {
        Expr::BinOp {
            op: BinOp::Lt,
            left,
            right,
        } if matches!(ast.get_expr(*left), Expr::Ident(n) if *n == i_name) => {
            match ast.get_expr(*right) {
                Expr::Member { obj, name } if name == "length" => match ast.get_expr(*obj) {
                    Expr::Ident(n) => n.clone(),
                    _ => return None,
                },
                _ => return None,
            }
        }
        _ => return None,
    };
    let step_eid = (*step)?;
    if !is_i_plus_eq_1(ast, step_eid, &i_name) {
        return None;
    }
    Some((i_name, x_name))
}

fn rewrite_sfi_walk_stmt(ast: &mut Ast, s: &mut Stmt, ctx: &mut SplitForICtx) {
    match s {
        Stmt::Block(inner) | Stmt::Multi(inner) => {
            rewrite_sfi_walk_list(ast, inner, ctx);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_sfi_walk_stmt(ast, then_branch, ctx);
            if let Some(eb) = else_branch {
                rewrite_sfi_walk_stmt(ast, eb, ctx);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            rewrite_sfi_walk_stmt(ast, body, ctx);
        }
        Stmt::Labeled { body, .. } => {
            rewrite_sfi_walk_stmt(ast, body, ctx);
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                rewrite_sfi_walk_stmt(ast, i, ctx);
            }
            rewrite_sfi_walk_stmt(ast, body, ctx);
        }
        Stmt::ForOfSplitIter { body, .. } | Stmt::ForOf { body, .. } => {
            rewrite_sfi_walk_stmt(ast, body, ctx);
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases.iter_mut() {
                rewrite_sfi_walk_list(ast, &mut c.body, ctx);
            }
            if let Some(db) = default {
                rewrite_sfi_walk_list(ast, db, ctx);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            rewrite_sfi_walk_list(ast, body, ctx);
            rewrite_sfi_walk_list(ast, catch_body, ctx);
            if let Some(fb) = finally_body {
                rewrite_sfi_walk_list(ast, fb, ctx);
            }
        }
        Stmt::FnDecl { body, .. } => {
            rewrite_sfi_walk_list(ast, body, ctx);
        }
        Stmt::ClassDecl { methods, .. } => {
            for m in methods.iter_mut() {
                rewrite_sfi_walk_list(ast, &mut m.body, ctx);
            }
        }
        Stmt::ExportDecl { inner, .. } => {
            if let Some(inner) = inner {
                rewrite_sfi_walk_stmt(ast, inner, ctx);
            }
        }
        _ => {}
    }
}

fn match_split_call_with_lit_sep(ast: &Ast, eid: ExprId) -> Option<(ExprId, ExprId)> {
    if let Expr::Call { callee, args } = ast.get_expr(eid)
        && args.len() == 1
        && let Expr::Member { obj: parent, name } = ast.get_expr(*callee)
        && name == "split"
        && matches!(ast.get_expr(args[0]), Expr::String(_))
    {
        return Some((*parent, args[0]));
    }
    None
}

fn is_zero_lit(ast: &Ast, eid: ExprId) -> bool {
    matches!(ast.get_expr(eid), Expr::Number(n) if *n == 0.0)
}

fn is_i_plus_eq_1(ast: &Ast, eid: ExprId, i_name: &str) -> bool {
    // Either `I = I + 1` (Assign) or `I++` (PostIncr) — accept both.
    if let Expr::Assign { target, value } = ast.get_expr(eid)
        && matches!(ast.get_expr(*target), Expr::Ident(n) if n == i_name)
        && let Expr::BinOp {
            op: BinOp::Add,
            left,
            right,
        } = ast.get_expr(*value)
        && matches!(ast.get_expr(*left), Expr::Ident(n) if n == i_name)
        && matches!(ast.get_expr(*right), Expr::Number(n) if *n == 1.0)
    {
        return true;
    }
    if let Expr::PostIncr { target, .. } = ast.get_expr(eid)
        && matches!(ast.get_expr(*target), Expr::Ident(n) if n == i_name)
    {
        return true;
    }
    false
}
