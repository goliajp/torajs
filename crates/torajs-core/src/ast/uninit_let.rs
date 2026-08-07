//! Uninit-let init-splice pass — chunk 359, extracted from ast.rs.
//!
//! Pub entry `desugar_uninit_let` promotes the first matching
//! same-scope follow-up assignment into a bare `let x;`'s init slot
//! (rewriting `let x; x = EXPR;` → `let x = EXPR;`). Any `let`
//! without a matching follow-up keeps the `Expr::Uninit` sentinel so
//! the typechecker can report a clean "let declared but never
//! assigned" error. Private helper `rewrite_uninit_in_stmts` walks
//! each declaring scope (FnDecl body, block, if/while/for/try body).

use std::collections::HashMap;

use super::{Ast, Expr, ExprId, Stmt};

/// `let x;` (the `var x;` shape after the test262 runner's `var → let`
/// rewrite) parses to `Stmt::LetDecl { init: Expr::Uninit }`. This
/// pass walks each declaring scope, finds the first
/// `Stmt::Expr(Assign { Ident(x), value })` after the let, splices
/// `value` into the let's init, and removes the assignment. Anything
/// that doesn't have a matching follow-up assignment keeps the
/// `Uninit` sentinel; the typechecker reports it with a clear "let
/// declared but never assigned" message — better than the previous
/// `expected `=`, got Semi` parse error.
///
/// Limitations of the search:
///   - same scope only — won't promote an inner-block assignment to
///     the outer let's init, since that would change scope semantics
///   - first matching assignment wins — chains like
///     `let x; if (...) x = 1; else x = "two";` don't unify; only the
///     first branch's value lifts in, the second stays an assign and
///     the regular type checker handles the agreement check
///   - top-level vs fn-body scopes are walked uniformly; nested
///     control-flow children don't bubble assignments across their
///     boundary (we only splice within the same `Vec<Stmt>`)
pub fn desugar_uninit_let(ast: &mut Ast) {
    // One shared `undefined` Ident for every implicit-any conversion
    // below (RFC 20260727-dstr-assignment 刀 0); ExprId reuse across
    // statements is established form (destr_defaults does the same).
    let undef_eid = ast.add_expr(Expr::Ident("undefined".into()));
    // mem::take so the statement vec can be mutated while the arena
    // stays available for the reference walker and the orphan
    // tombstoning below.
    let mut stmts = std::mem::take(&mut ast.stmts);
    rewrite_uninit_in_stmts(&mut stmts, ast, undef_eid);
    ast.stmts = stmts;
    // FnDecl bodies live inside the vec already; the recursive
    // walk handles them when it descends into Stmt::FnDecl variants.
}

fn rewrite_uninit_in_stmts(stmts: &mut Vec<Stmt>, ast: &mut Ast, undef_eid: ExprId) {
    let mut i = 0;
    while i < stmts.len() {
        // Recurse into nested scopes first so each scope's lets see
        // their own follow-up assignments.
        match &mut stmts[i] {
            Stmt::FnDecl { body, .. } => {
                rewrite_uninit_in_stmts(body, ast, undef_eid);
            }
            Stmt::Block(inner) => {
                rewrite_uninit_in_stmts(inner, ast, undef_eid);
            }
            Stmt::Multi(_) => {
                // A Multi is a FLAT statement sequence (the parser's
                // multi-name `var a, b;` line), not a scope — recursing
                // treated it as one, so a declarator inside could never
                // see its follow-up assignment in the surrounding list
                // (`var F, o; F = function () {…};` kept the Uninit
                // sentinel while the single-name spelling spliced — the
                // S13.2.2 constructed-fn-expr family writes exactly the
                // multi-name form). Splice the members into this list
                // and re-examine from the same index (a nested Multi
                // re-enters this arm).
                let Stmt::Multi(inner) = std::mem::replace(&mut stmts[i], Stmt::Break(None)) else {
                    unreachable!()
                };
                stmts.splice(i..=i, inner);
                continue;
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                if let Stmt::Block(b) | Stmt::Multi(b) = then_branch.as_mut() {
                    rewrite_uninit_in_stmts(b, ast, undef_eid);
                }
                if let Some(eb) = else_branch
                    && let Stmt::Block(b) | Stmt::Multi(b) = eb.as_mut()
                {
                    rewrite_uninit_in_stmts(b, ast, undef_eid);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                if let Stmt::Block(b) | Stmt::Multi(b) = body.as_mut() {
                    rewrite_uninit_in_stmts(b, ast, undef_eid);
                }
            }
            Stmt::For { body, .. } => {
                if let Stmt::Block(b) | Stmt::Multi(b) = body.as_mut() {
                    rewrite_uninit_in_stmts(b, ast, undef_eid);
                }
            }
            Stmt::Labeled { body, .. } => {
                // Process the labeled statement as a one-element scope so
                // nested lets inside a labeled loop / block are rewritten
                // like any other (the wrapped stmt re-enters this walker).
                let mut tmp = vec![std::mem::replace(body.as_mut(), Stmt::Break(None))];
                rewrite_uninit_in_stmts(&mut tmp, ast, undef_eid);
                // The Multi flatten above can split a labeled multi-name
                // line (`L: var x=0, y=0;`) into several statements — a
                // bare pop would DROP all but the last declarator.
                // Rewrap; a Multi carries the same flat-sequence
                // semantics the label's body had.
                *body = Box::new(if tmp.len() == 1 {
                    tmp.pop().unwrap()
                } else {
                    Stmt::Multi(tmp)
                });
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                rewrite_uninit_in_stmts(body, ast, undef_eid);
                rewrite_uninit_in_stmts(catch_body, ast, undef_eid);
                if let Some(fb) = finally_body {
                    rewrite_uninit_in_stmts(fb, ast, undef_eid);
                }
            }
            _ => {}
        }
        // Now, if this stmt is an Uninit let, scan forward for the
        // first matching `name = EXPR;` and splice.
        let (name, init_eid) = match &stmts[i] {
            Stmt::LetDecl { name, init, .. } => (name.clone(), *init),
            _ => {
                i += 1;
                continue;
            }
        };
        let is_uninit = matches!(ast.get_expr(init_eid), Expr::Uninit);
        if !is_uninit {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let mut found: Option<(usize, ExprId, ExprId, ExprId)> = None;
        while j < stmts.len() {
            if let Stmt::Expr(eid) = &stmts[j]
                && let Expr::Assign { target, value } = ast.get_expr(*eid)
                && let Expr::Ident(n) = ast.get_expr(*target)
                && n == &name
            {
                found = Some((j, *eid, *target, *value));
                break;
            }
            // Don't reach into non-flat control-flow: an assignment in
            // a sibling block / if-branch doesn't lift to the outer
            // scope. Only adjacent flat stmts in the SAME Vec<Stmt>
            // count.
            j += 1;
        }
        if let Some((stmt_idx, assign_eid, target_eid, value)) = found {
            // Execution-order fix (rotation 133): splicing the
            // assignment's VALUE into the earlier let REORDERED its
            // side effects across every intermediate statement —
            // `let r; log(d.getTime()); r = d.setDate(28);` ran the
            // setter before the log (test262 Date time-clip family
            // read post-mutation state). Instead MOVE THE
            // DECLARATION DOWN to the assignment site: same
            // spliced-init typing, effects stay in program order. A
            // TDZ-ish intermediate reference of the name keeps the
            // Uninit sentinel (loud checker reject beats the silent
            // reorder).
            let referenced = {
                let mut refs: HashMap<String, usize> = HashMap::new();
                for s in &stmts[i + 1..stmt_idx] {
                    crate::linter::refs::count_refs_stmt(ast, s, &mut refs);
                }
                refs.get(&name).copied().unwrap_or(0) > 0
            };
            if !referenced {
                let mut decl = stmts.remove(i);
                if let Stmt::LetDecl { init, .. } = &mut decl {
                    *init = value;
                }
                // stmt_idx shifted down by the remove above.
                stmts[stmt_idx - 1] = decl;
                // The replaced assignment's Assign node and its
                // target Ident are now arena orphans — no statement
                // reaches them, but whole-arena name scans (the
                // arguments-face binding-safety walk, the HOF
                // direct-call gate) still see the orphan Ident and
                // kill the binding's chain. Tombstone both slots;
                // `value` stays live as the spliced init.
                ast.exprs[assign_eid.0 as usize] = Expr::Uninit;
                ast.exprs[target_eid.0 as usize] = Expr::Uninit;
                // Re-examine the slot that slid into position i.
                continue;
            }
        }
        // ES §14.3.1 / TS: an un-annotated `let x;` the splice could
        // not resolve is an implicit-any binding holding `undefined`
        // (RFC 20260727-dstr-assignment 刀 0). The Uninit sentinel it
        // used to keep typed the binding Undefined, which rejected
        // every later cross-scope / cross-Multi assignment (`let v;
        // function f() { v = 1; }` / `let a, b; a = 1;`). Annotated
        // survivors keep their sentinel (parser-let-no-init-001
        // asserts their Null semantics); `var`-machinery decls
        // (for-of `var k;`, var_hoist) keep their own path.
        if let Stmt::LetDecl {
            type_ann,
            init,
            is_var: false,
            ..
        } = &mut stmts[i]
            && type_ann.is_none()
        {
            *type_ann = Some("any".into());
            *init = undef_eid;
        }
        i += 1;
    }
}
