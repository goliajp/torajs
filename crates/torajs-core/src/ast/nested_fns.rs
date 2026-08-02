//! Nested-fn hoisting pass — chunk 358, extracted from ast.rs.
//!
//! Pub entry `desugar_nested_fns` implements ES Annex B §B.3.3
//! (FunctionDeclarations in IfStatement Blocks). Walks every top-
//! level FnDecl body + module-top-level non-FnDecl stmts, finds
//! nested `function f() {...}` decls inside any block-shape stmt,
//! lifts them to top-level with mangled names
//! `__nested_<parent>_<name>_<uid>`, rewrites references. The
//! private helpers (`collect_nested_fns_in_stmt/to_lift`,
//! `rewrite_idents_in_body/stmt/expr` in the idents sibling) do the
//! tree walk — both passes share ONE stmt collector since rotation
//! 269 (the pass-1-only sibling lacked the bare-FnDecl-branch arm).

use super::nested_fns_idents::{rewrite_idents_in_body, rewrite_idents_in_stmt};
use super::{Ast, Stmt};
use std::collections::{HashMap, HashSet};

/// P3.4 — block-scoped / nested function declaration hoisting per
/// ES spec Annex B §B.3.3 (FunctionDeclarations in IfStatement
/// Blocks). Walks every top-level FnDecl body, finds nested
/// `function f() {...}` declarations inside any block-shape stmt,
/// lifts them to top-level with mangled names
/// `__nested_<parent>_<name>_<uid>`. References to the original
/// name within the parent body get rewritten to the mangled name.
///
/// Captures: this pass only handles the no-capture case. If the
/// nested fn body references parent locals, ssa_lower will fail at
/// the lifted FnDecl's body lower (lifted = top-level scope, can't
/// see parent locals). Real closure-capturing nested fns need the
/// same treatment as arrow fns (lift_arrow_fns adds an `__env`
/// first param + Closure shape) — substrate followup if test262
/// surfaces it.
pub fn desugar_nested_fns(ast: &mut Ast) {
    // r283 — fixpoint over the single-round pass: a lifted FnDecl can
    // itself contain a nested FnDecl (`function a() { function b() {
    // function c() {} } }` — the t262 async-case tail's `assertions`
    // / `$DONE` pair, 120-case cluster). Each round only walks
    // top-level decls, so freshly-lifted bodies get their own round;
    // a round that lifts nothing is the (idempotent) fixed point.
    loop {
        let before = ast.stmts.len();
        desugar_nested_fns_once(ast);
        if ast.stmts.len() == before {
            break;
        }
    }
}

fn desugar_nested_fns_once(ast: &mut Ast) {
    let mut top = std::mem::take(&mut ast.stmts);
    let mut new_top: Vec<Stmt> = Vec::new();
    let mut counter: u32 = 0;
    // Pass 1 — top-level FnDecl bodies. Walk nested fns inside parent
    // FnDecl bodies (the original P3.4 scope).
    for stmt in top.iter_mut() {
        if let Stmt::FnDecl {
            name: parent_name,
            body,
            ..
        } = stmt
        {
            let parent = parent_name.clone();
            let mut renames: HashMap<String, String> = HashMap::new();
            let mut branch_scoped: HashSet<String> = HashSet::new();
            let mut lifted: Vec<Stmt> = Vec::new();
            collect_nested_fns_to_lift(
                body,
                &parent,
                &mut renames,
                &mut branch_scoped,
                true,
                &mut lifted,
                &mut counter,
            );
            if !renames.is_empty() {
                // A bare-branch decl (`if (c) function f() {}`) is
                // block-scoped in TS semantics: outer references keep
                // the ORIGINAL name (they resolve nowhere and ride the
                // catchable-ReferenceError undeclared-read lane, which
                // is exactly what bun answers). Only the lifted body
                // itself sees the mangled name (self-recursion).
                let outer_map: HashMap<String, String> = renames
                    .iter()
                    .filter(|(k, _)| !branch_scoped.contains(*k))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                if !outer_map.is_empty() {
                    rewrite_idents_in_body(ast, body, &outer_map);
                }
                for lf in lifted.iter_mut() {
                    if let Stmt::FnDecl { body: lb, .. } = lf {
                        rewrite_idents_in_body(ast, lb, &renames);
                    }
                }
            }
            new_top.extend(lifted);
        }
    }
    // P3.4-followup-A — module-top-level blocks. `{ function f() {} }`
    // at module scope (outside any FnDecl) puts a FnDecl inside a
    // Stmt::Block, which the synthesized `main` body walks. Without
    // this pass, ssa_lower's lower_top_stmt catch-all panicked on
    // every such case (annexB function-statement hoisting tests).
    // Same shape as Pass 1 but the "parent" is the synthetic "__top"
    // namespace for mangling. Also handles If / While / DoWhile /
    // For / ForOf / ForIn / Try / Switch nested at module top.
    let parent = "__top".to_string();
    let mut top_renames: HashMap<String, String> = HashMap::new();
    let mut top_branch_scoped: HashSet<String> = HashSet::new();
    let mut top_lifted: Vec<Stmt> = Vec::new();
    for stmt in top.iter_mut() {
        match stmt {
            Stmt::FnDecl { .. } => {} // handled by Pass 1
            other => {
                collect_nested_fns_in_stmt(
                    other,
                    &parent,
                    &mut top_renames,
                    &mut top_branch_scoped,
                    false,
                    &mut top_lifted,
                    &mut counter,
                );
            }
        }
    }
    if !top_renames.is_empty() {
        // Rewrite ident references in the entire top-level (excluding
        // the lifted decls themselves; those get rewritten below).
        // Branch-scoped names stay un-rewritten outside — see pass 1.
        let outer_map: HashMap<String, String> = top_renames
            .iter()
            .filter(|(k, _)| !top_branch_scoped.contains(*k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if !outer_map.is_empty() {
            for stmt in top.iter_mut() {
                rewrite_idents_in_stmt(ast, stmt, &outer_map);
            }
        }
        for lf in top_lifted.iter_mut() {
            if let Stmt::FnDecl { body: lb, .. } = lf {
                rewrite_idents_in_body(ast, lb, &top_renames);
            }
        }
    }
    new_top.extend(top_lifted);
    top.extend(new_top);
    ast.stmts = top;
}

/// P3.4-followup-A — recursive helper that descends into a single stmt
/// looking for nested FnDecls (block/if/while/for/try/switch
/// children). When found, lift to `lifted` with mangled name and
/// drop from the original site (replaced with a no-op Block).
fn collect_nested_fns_in_stmt(
    stmt: &mut Stmt,
    parent_name: &str,
    renames: &mut HashMap<String, String>,
    branch_scoped: &mut HashSet<String>,
    strict_branch: bool,
    lifted: &mut Vec<Stmt>,
    counter: &mut u32,
) {
    // Bare-FnDecl-as-statement: `if (cond) function f() {}` parses
    // the then-branch as Stmt::FnDecl directly (no enclosing Block).
    // Same lift-and-mangle as the Block case but in-place.
    if let Stmt::FnDecl { name, .. } = stmt {
        if !name.starts_with("__closure_")
            && !name.starts_with("__cm_")
            && !name.starts_with("__sm_")
            && !name.starts_with("__nested_")
        {
            let mangled = format!("__nested_{parent_name}_{name}_{counter}");
            *counter += 1;
            renames.insert(name.clone(), mangled.clone());
            // TS semantics INSIDE a fn body (strict_branch): a
            // bare-branch decl is block-scoped, so the rename must
            // NOT redirect references outside the lifted body (bun
            // answers ReferenceError there). MODULE-TOP global code
            // keeps the historical sloppy leak (annexB B.3.3 — the
            // 20 global-code `if-decl-*-update` cases assert exactly
            // that hoist; rotation 269's first cut flipped them to
            // strict and regressed all 20). The split is an S5.7
            // boundary-decision entry. Block-shaped
            // `{ function f() {} }` keeps the historical
            // leak-to-parent behavior on both sides.
            if strict_branch {
                branch_scoped.insert(name.clone());
            }
            // Take the FnDecl out of the slot, replace with empty
            // Block, push the renamed decl into `lifted`.
            let taken = std::mem::replace(stmt, Stmt::Block(Vec::new()));
            if let Stmt::FnDecl {
                type_params,
                params,
                return_type,
                body,
                is_generator,
                span,
                ..
            } = taken
            {
                lifted.push(Stmt::FnDecl {
                    name: mangled,
                    type_params,
                    params,
                    return_type,
                    body,
                    is_generator,
                    // lift keeps the user's declaration text (B1b)
                    span,
                });
            }
            return;
        }
    }
    match stmt {
        Stmt::Block(body) | Stmt::Multi(body) => {
            collect_nested_fns_to_lift(
                body,
                parent_name,
                renames,
                branch_scoped,
                strict_branch,
                lifted,
                counter,
            );
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_nested_fns_in_stmt(
                then_branch,
                parent_name,
                renames,
                branch_scoped,
                strict_branch,
                lifted,
                counter,
            );
            if let Some(eb) = else_branch {
                collect_nested_fns_in_stmt(
                    eb,
                    parent_name,
                    renames,
                    branch_scoped,
                    strict_branch,
                    lifted,
                    counter,
                );
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            collect_nested_fns_in_stmt(
                body,
                parent_name,
                renames,
                branch_scoped,
                strict_branch,
                lifted,
                counter,
            );
        }
        Stmt::For { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::Labeled { body, .. } => {
            collect_nested_fns_in_stmt(
                body,
                parent_name,
                renames,
                branch_scoped,
                strict_branch,
                lifted,
                counter,
            );
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            collect_nested_fns_to_lift(
                body,
                parent_name,
                renames,
                branch_scoped,
                strict_branch,
                lifted,
                counter,
            );
            collect_nested_fns_to_lift(
                catch_body,
                parent_name,
                renames,
                branch_scoped,
                strict_branch,
                lifted,
                counter,
            );
            if let Some(fb) = finally_body {
                collect_nested_fns_to_lift(
                    fb,
                    parent_name,
                    renames,
                    branch_scoped,
                    strict_branch,
                    lifted,
                    counter,
                );
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for case in cases.iter_mut() {
                collect_nested_fns_to_lift(
                    &mut case.body,
                    parent_name,
                    renames,
                    branch_scoped,
                    strict_branch,
                    lifted,
                    counter,
                );
            }
            if let Some(d) = default {
                collect_nested_fns_to_lift(
                    d,
                    parent_name,
                    renames,
                    branch_scoped,
                    strict_branch,
                    lifted,
                    counter,
                );
            }
        }
        _ => {} // leaf stmts have no nested FnDecl children
    }
}

fn collect_nested_fns_to_lift(
    body: &mut Vec<Stmt>,
    parent_name: &str,
    renames: &mut HashMap<String, String>,
    branch_scoped: &mut HashSet<String>,
    strict_branch: bool,
    lifted: &mut Vec<Stmt>,
    counter: &mut u32,
) {
    let drained = std::mem::take(body);
    let mut new_body: Vec<Stmt> = Vec::with_capacity(drained.len());
    for s in drained {
        match s {
            Stmt::FnDecl { ref name, .. }
                if !name.starts_with("__closure_")
                    && !name.starts_with("__cm_")
                    && !name.starts_with("__sm_")
                    && !name.starts_with("__nested_") =>
            {
                let mangled = format!("__nested_{parent_name}_{name}_{counter}");
                *counter += 1;
                renames.insert(name.clone(), mangled.clone());
                let new_decl = match s {
                    Stmt::FnDecl {
                        type_params,
                        params,
                        return_type,
                        body: fbody,
                        is_generator,
                        span,
                        ..
                    } => Stmt::FnDecl {
                        name: mangled,
                        type_params,
                        params,
                        return_type,
                        body: fbody,
                        is_generator,
                        // lift keeps the user's declaration text (B1b)
                        span,
                    },
                    _ => unreachable!(),
                };
                lifted.push(new_decl);
                // Original site: drop entirely.
            }
            mut other => {
                // RFC 20260801 (rotation 269) — pass 1 now walks the
                // same collector as the module-top pass: the old
                // sibling walk (`collect_nested_fns_in_other`,
                // deleted) lacked the bare-FnDecl-branch arm, so a
                // braceless `if (c) function f() {}` INSIDE a fn
                // body (annexB B.3.2's most common shape, usually
                // under an IIFE) reached the lowering catch-all
                // panic while the identical shape at module top
                // lifted fine.
                collect_nested_fns_in_stmt(
                    &mut other,
                    parent_name,
                    renames,
                    branch_scoped,
                    strict_branch,
                    lifted,
                    counter,
                );
                new_body.push(other);
            }
        }
    }
    *body = new_body;
}
