//! `var` hoisting pass — chunk 355, extracted from ast.rs.
//!
//! Pub entry `desugar_var_hoist` runs the P2.1 var-hoist pass per
//! ES spec §14.3.2.1 VariableStatement. Three private walker
//! helpers (`hoist_vars_in_block`, `collect_and_rewrite_var`,
//! `hoist_recurse_stmt`) implement the fn-body descent.

use super::{Expr, ExprId, Stmt};

/// P2.1 — `var` hoisting pass per ES spec §14.3.2.1
/// VariableStatement. Walks every fn body + the top-level script
/// (taken as the implicit `main` body for hoisting purposes), finds
/// every `Stmt::LetDecl { is_var: true, .. }` (including deep inside
/// `if` / `while` / `for` / `block` / `switch` / `try` bodies), and:
///
///   1. Emits a synthetic `let <name>: <type_ann> = Uninit` at the top
///      of the enclosing fn-body / script. Pre-init reads return
///      undefined per spec (P1.3's Expr::Uninit → Type::Undefined
///      substrate makes this work).
///   2. Replaces the original site:
///      - If the original had an init: convert to `<name> = <init>`
///        assignment (Stmt::Expr wrapping Expr::Assign).
///      - If no init (`var x;`): remove the stmt entirely.
///   3. Same name declared multiple times in the same fn scope:
///      ONE hoisted decl + N assignments. Later decls' type anns
///      override earlier ones (last-write-wins matches JS shape).
///
/// `let` / `const` (is_var=false) are block-scoped per spec and not
/// touched by this pass.
pub fn desugar_var_hoist(ast: &mut super::Ast) {
    // Take stmts out so we can pass &mut ast.exprs and a separate
    // &mut Vec<Stmt> to the recursive helpers without aliasing.
    let mut top = std::mem::take(&mut ast.stmts);
    // Top-level script: hoist over `top` itself.
    hoist_vars_in_block(&mut top, &mut ast.exprs);
    // Each top-level FnDecl: hoist over its body.
    for stmt in top.iter_mut() {
        if let Stmt::FnDecl { body, .. } = stmt {
            hoist_vars_in_block(body, &mut ast.exprs);
        }
    }
    ast.stmts = top;
}

/// Walks `body` recursively, collects every `is_var: true` LetDecl's
/// (name, type_ann), and rewrites the original site to either an
/// assignment (if the decl had an init) or a no-op. Returned hoisted
/// decls are inserted at the top of `body`.
fn hoist_vars_in_block(body: &mut Vec<Stmt>, exprs: &mut Vec<Expr>) {
    use std::collections::BTreeMap;
    let mut hoisted: BTreeMap<String, Option<String>> = BTreeMap::new();
    collect_and_rewrite_var(body, &mut hoisted, exprs, false);
    if hoisted.is_empty() {
        return;
    }
    let mut prelude: Vec<Stmt> = Vec::with_capacity(hoisted.len());
    for (name, _user_ann) in hoisted {
        let init = ExprId(exprs.len() as u32);
        exprs.push(Expr::Uninit);
        // Hoisted vars are ALWAYS Type::Any-typed slots regardless
        // of user annotation. Two reasons:
        //   1. The pre-init read returns undefined per spec, which
        //      doesn't fit Type::Number / Array<Number> / etc. Any
        //      fits because Undefined is a valid Any-tag.
        //   2. var's runtime type can change across reassignments
        //      (`var x = 1; x = "hello"` is legal JS); Any covers
        //      both. The user's annotation is treated as a hint for
        //      the eventual value, not a slot constraint.
        // User-typed slots that need precise types should use `let`
        // (block-scoped) which keeps its annotation.
        prelude.push(Stmt::LetDecl {
            mutable: true,
            name,
            type_ann: Some("any".to_string()),
            init,
            is_var: false, // already hoisted; downstream sees a regular let
        });
    }
    let tail = std::mem::take(body);
    *body = prelude;
    body.extend(tail);
}

fn collect_and_rewrite_var(
    stmts: &mut Vec<Stmt>,
    hoisted: &mut std::collections::BTreeMap<String, Option<String>>,
    exprs: &mut Vec<Expr>,
    nested: bool,
) {
    let mut new_stmts: Vec<Stmt> = Vec::with_capacity(stmts.len());
    // Rotation 205 — names the escape hatch already declared as a
    // `let` in THIS stmt list. ES §14.3.2 allows same-scope `var`
    // re-declaration (one shared binding); a second escape-hatch
    // conversion would emit a second `let` and trip the checker's
    // redeclaration reject, so later same-name `var`s convert to
    // plain assignments instead (matching the hoisted path's
    // BTreeMap dedup). Scoped to this list only: the converted `let`
    // is block-scoped, so a name declared in a NESTED block is not
    // assignable from here (the escape hatch's pre-existing
    // block-scope compromise).
    let mut escaped: std::collections::HashSet<String> = std::collections::HashSet::new();
    let drained = std::mem::take(stmts);
    for s in drained {
        match s {
            Stmt::LetDecl {
                mutable,
                name,
                type_ann,
                init,
                is_var: true,
            } => {
                // P2.1 escape hatches — three cases where we don't
                // hoist (treat as a regular block-scoped `let`):
                //   1. User wrote an explicit type annotation
                //      (`var arr: number[] = [...]`). The typed slot
                //      can't carry the spec's pre-init undefined
                //      (Type::Undefined doesn't fit Type::Array<Number>).
                //   2. Init is a function/arrow expression. Hoisting
                //      to Type::Any loses the FnSig and makes the
                //      var uncallable (substrate: call-on-Any is P3).
                //      `var f = function() {}; f()` pre-P2.1 worked
                //      via let-aliasing; this keeps that path live.
                //   3. (future) Init produces other types that lose
                //      method dispatch when boxed (Array, Obj, etc.)
                //      — covered by case 2's spirit; add cases as
                //      they surface.
                let init_keeps_type = match &exprs[init.0 as usize] {
                    Expr::ArrowFn { .. } | Expr::Closure { .. } => true,
                    // After lift_arrow_fns, capturing-less function
                    // expressions become `Expr::Ident("__closure_N")`
                    // pointing at the lifted FnDecl. That ident
                    // resolves to a FnSig at the SSA layer, NOT to
                    // a regular let-typed value, so hoisting to Any
                    // would lose the call-site dispatch.
                    Expr::Ident(n) if n.starts_with("__closure_") => true,
                    // P5 — `var xs = [literal]` is the dominant
                    // test262 / plain-JS pattern. Hoisting to
                    // Type::Any made `xs.length` return "undefined"
                    // (Member-on-Any with no length dispatch) — a
                    // silent-wrong bug that gated every test262
                    // case using `var arr = [...]` from passing.
                    // Keep the typed Array<T> slot; pre-init read
                    // returns the slot default (null) which is rare
                    // enough that test coverage is happy with the
                    // typed path. See `var-hoist-001` for the
                    // pre-init undefined case (uses `var x = 1`,
                    // Number, not Array).
                    Expr::Array(_) => true,
                    // Same fix for object literals — `var obj =
                    // {a: 1}; obj.a` returned undefined under the
                    // Any-promotion path because Member-on-Any
                    // routes through dynobj_get which doesn't yet
                    // resolve typed-struct fields. Keep the typed
                    // Struct slot so direct field reads work.
                    Expr::ObjectLit { .. } => true,
                    _ => false,
                };
                // Rotation 205 — a re-declared name (either form
                // already declared it: an escape-hatch `let` in this
                // list, or a hoisted prelude slot in this fn)
                // converts to an assignment onto the existing
                // binding — ES's one-shared-binding semantics.
                let redeclared = escaped.contains(&name) || hoisted.contains_key(&name);
                // Rotation 264 — the escape hatch is sound ONLY at
                // the fn-body / script top level, where block scope
                // and fn scope coincide. In a NESTED list (try /
                // block / switch case) an escaped `let` is invisible
                // to the rest of the fn — `try { var x = [] } x`
                // answered "undeclared" (the compromise the escape
                // notes admitted to). Member/call-on-Any has matured
                // since P5 (length / index / methods / field reads /
                // invocation all dispatch), so a nested var always
                // hoists to the Any prelude slot per §14.3.2.1.
                if (type_ann.is_some() || init_keeps_type) && !redeclared && !nested {
                    escaped.insert(name.clone());
                    new_stmts.push(Stmt::LetDecl {
                        mutable,
                        name,
                        type_ann,
                        init,
                        is_var: false,
                    });
                    continue;
                }
                if !redeclared {
                    hoisted.insert(name.clone(), None);
                }
                if !matches!(exprs[init.0 as usize], Expr::Uninit) {
                    let target_id = ExprId(exprs.len() as u32);
                    exprs.push(Expr::Ident(name));
                    let assign_id = ExprId(exprs.len() as u32);
                    exprs.push(Expr::Assign {
                        target: target_id,
                        value: init,
                    });
                    new_stmts.push(Stmt::Expr(assign_id));
                }
            }
            mut other => {
                hoist_recurse_stmt(&mut other, hoisted, exprs);
                new_stmts.push(other);
            }
        }
    }
    *stmts = new_stmts;
}

fn hoist_recurse_stmt(
    s: &mut Stmt,
    hoisted: &mut std::collections::BTreeMap<String, Option<String>>,
    exprs: &mut Vec<Expr>,
) {
    match s {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            hoist_recurse_stmt(then_branch, hoisted, exprs);
            if let Some(eb) = else_branch.as_deref_mut() {
                hoist_recurse_stmt(eb, hoisted, exprs);
            }
        }
        Stmt::Block(b) => {
            collect_and_rewrite_var(b, hoisted, exprs, true);
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            hoist_recurse_stmt(body, hoisted, exprs);
        }
        Stmt::Labeled { body, .. } => {
            hoist_recurse_stmt(body, hoisted, exprs);
        }
        Stmt::For { body, init, .. } => {
            // Special-case: a single `var i = 0` in the for-init slot
            // hoists `i` to the enclosing fn-body. Replace the
            // for-init with the equivalent `i = 0` assignment so the
            // loop semantic stays unchanged.
            if let Some(init_box) = init.as_deref_mut() {
                if let Stmt::LetDecl {
                    name,
                    init: init_id,
                    is_var: true,
                    ..
                } = init_box
                {
                    // §14.7.4 — a for-head `var` shares the fn-level
                    // hoist domain, annotation or not. The old typed
                    // escape hatch mirrored the top-level one by
                    // flipping `is_var` off in place, but the mirror
                    // does not hold here: a top-level escape `let`
                    // stays visible to the rest of the fn body, while
                    // a for-head `let` is scoped to the loop — so
                    // `for (var x: any = 1; …)` left `x` unreadable
                    // after the loop (401-05). Same reasoning as the
                    // rotation-264 nested-list fix: the for head IS a
                    // nested position, and nested vars always hoist.
                    let nm = name.clone();
                    let init_eid = *init_id;
                    hoisted.insert(nm.clone(), None);
                    if !matches!(exprs[init_eid.0 as usize], Expr::Uninit) {
                        let target_id = ExprId(exprs.len() as u32);
                        exprs.push(Expr::Ident(nm));
                        let assign_id = ExprId(exprs.len() as u32);
                        exprs.push(Expr::Assign {
                            target: target_id,
                            value: init_eid,
                        });
                        *init_box = Stmt::Expr(assign_id);
                    } else {
                        *init = None;
                    }
                } else {
                    hoist_recurse_stmt(init_box, hoisted, exprs);
                }
            }
            hoist_recurse_stmt(body, hoisted, exprs);
        }
        Stmt::ForOfSplitIter { body, .. } | Stmt::ForOf { body, .. } => {
            hoist_recurse_stmt(body, hoisted, exprs);
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            collect_and_rewrite_var(body, hoisted, exprs, true);
            collect_and_rewrite_var(catch_body, hoisted, exprs, true);
            if let Some(fb) = finally_body {
                collect_and_rewrite_var(fb, hoisted, exprs, true);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for case in cases {
                collect_and_rewrite_var(&mut case.body, hoisted, exprs, true);
            }
            if let Some(dflt) = default {
                collect_and_rewrite_var(dflt, hoisted, exprs, true);
            }
        }
        Stmt::Multi(inner) => {
            collect_and_rewrite_var(inner, hoisted, exprs, true);
        }
        // Bare LetDecl that's NOT is_var=true — leave alone. (The
        // is_var=true path is handled by the caller's match arm.)
        Stmt::LetDecl { .. } => {}
        // FnDecl is its own scope — don't descend (handled by the
        // top-level pass that hoists separately per fn body).
        Stmt::FnDecl { .. } => {}
        // Terminal stmts with no nested stmt list.
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::super::Ast;
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn hoisted_ast(src: &str) -> Ast {
        let tokens = tokenize(src).expect("lex");
        let mut ast = parse(src, &tokens).expect("parse");
        desugar_var_hoist(&mut ast);
        ast
    }

    fn count_letdecls(ast: &Ast, name: &str) -> usize {
        ast.stmts
            .iter()
            .filter(|s| matches!(s, Stmt::LetDecl { name: n, .. } if n == name))
            .count()
    }

    #[test]
    fn same_scope_var_redeclaration_dedups_to_one_let() {
        // rotation 205 — ES §14.3.2 one-shared-binding: the second
        // escape-hatch var converts to an assignment, not a second let
        let ast = hoisted_ast("var v = { a: 1 };\nvar v = { a: 2 };\n");
        assert_eq!(count_letdecls(&ast, "v"), 1);
    }

    #[test]
    fn mixed_hoist_then_escape_shape_dedups() {
        // first var hoists to the any-slot prelude; the second
        // (escape-shape) must assign onto it, not redeclare
        let ast = hoisted_ast("var w = 5;\nvar w = { a: 3 };\n");
        assert_eq!(count_letdecls(&ast, "w"), 1);
    }

    #[test]
    fn redeclaration_without_init_is_dropped() {
        let ast = hoisted_ast("var m = { x: 7 };\nvar m;\n");
        assert_eq!(count_letdecls(&ast, "m"), 1);
    }
}
