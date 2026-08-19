//! The completion value of a multi-statement DIRECT source, placed
//! exactly — the general case the single-expression collapse in the
//! parent module cannot reach. §14.5.1: when the final statement of
//! the source is an ExpressionStatement, its value IS the source's
//! completion, overriding whatever the earlier statements completed
//! with. The call becomes an IIFE at the call site:
//!
//!   eval("f(); 2;")   →   (() => { f(); return 2; })()
//!
//! The arrow is not an approximation of the direct-eval environment;
//! it IS that environment. An arrow transmits `this`, `arguments` and
//! `super` from the enclosing context — exactly what a direct eval
//! sees — and its body is its own VariableEnvironment, so the source's
//! `var` and function declarations die with the eval, which is the
//! strict-mode rule tr is under. A trailing declaration would leave
//! the completion on an *earlier* statement (declarations complete
//! empty), so only the trailing-expression shape rewrites; the rest
//! keep the honest reject.

use super::super::{Ast, Expr, ExprId, Stmt, free_vars};
use super::completion_stmt;
use super::rewrite_list;
use super::source::{
    CallForm, DeleteSites, first_line, literal_eval_call, parse_eval_source, syntax_error_throw,
};
use super::walk;

/// This pass runs LAST in the desugar deliberately: by then the
/// statement walks have inlined every statement-position direct eval
/// as a sealed block, so any literal direct call still standing is in
/// value position — no parent-tracking is needed to know that, and
/// the established sealed-block shape for statement evals stays
/// untouched.
///
/// A source that does NOT parse gets the same IIFE treatment with a
/// `throw` for a body: §19.2.1.1 step 12 wants the SyntaxError at
/// evaluation time, JavaScript has no throw *expression*, and the
/// arrow IIFE is exactly tr's statement-in-value-position carrier.
/// Scope cannot matter to a throw, so this applies to both call forms.
pub(super) fn rewrite_completion_value_evals(ast: &mut Ast) {
    // An INDIRECT source belongs to the global scope, and an IIFE sits
    // in the caller's — the two agree only when scope cannot tell them
    // apart: a source that binds nothing (an IIFE body is a function
    // scope, so a sloppy global `var` would die in it) and whose free
    // variables are either none or resolved at a top-level call site
    // that no nested lexical name shadows. The same two-track rule the
    // single-expression collapse already applies, extended to the
    // multi-statement shape.
    let fn_owned = walk::fn_owned_exprs(ast);
    let class_owned = walk::class_owned_exprs(ast);
    let nested_lexical = walk::nested_lexical_names(ast);
    let mut i = 0;
    while i < ast.exprs.len() {
        let eid = ExprId(i as u32);
        let Some((src, form)) = literal_eval_call(eid, ast) else {
            i += 1;
            continue;
        };
        // §19.2.1.1 steps 4-6 (r334 blade 6) — `super` in the text is
        // legal only for a direct eval at a class-member site; the
        // parser refuses it everywhere else, which routes the call
        // into the None arm below: the whole source becomes a
        // SyntaxError throw and none of it runs.
        let super_ok = form == CallForm::Direct && class_owned.get(i).copied().unwrap_or(false);
        match parse_eval_source(&src, ast, super_ok, DeleteSites::Strict) {
            Err(_) => {
                let throw = syntax_error_throw(format!("eval: {}", first_line(&src)), ast);
                wrap_iife(i, vec![throw], ast);
            }
            Ok(mut body)
                if body.len() >= 2
                    && matches!(body.last(), Some(Stmt::Expr(_)))
                    && (form == CallForm::Direct
                        || scope_transparent(&body, ast, i, &fn_owned, &nested_lexical)) =>
            {
                let Some(Stmt::Expr(tail)) = body.pop() else {
                    unreachable!()
                };
                // A nested statement-position eval inside the source is
                // an eval like any other; the statement walk no longer
                // runs, so give the new body its own pass.
                rewrite_list(&mut body, ast, true, super_ok);
                body.push(Stmt::Return(Some(tail)));
                wrap_iife(i, body, ast);
            }
            // A trailing statement (switch / loop / if / try / block)
            // leaves the completion inside its own V accumulation —
            // the UpdateEmpty desugar in `completion_stmt.rs`.
            Ok(mut body)
                if !body.is_empty()
                    && (form == CallForm::Direct
                        || scope_transparent(&body, ast, i, &fn_owned, &nested_lexical)) =>
            {
                rewrite_list(&mut body, ast, true, super_ok);
                let body = completion_stmt::rewrite_trailing_stmt_completion(body, ast);
                wrap_iife(i, body, ast);
            }
            Ok(_) => {}
        }
        i += 1;
    }
}

/// An INDIRECT source may take the IIFE only when scope cannot tell
/// the arrow's frame apart from the global one: the source binds
/// nothing, and its free variables are either none or resolved at a
/// top-level call site no nested lexical name shadows — the same
/// two-track rule the single-expression collapse applies.
fn scope_transparent(
    body: &[Stmt],
    ast: &Ast,
    i: usize,
    fn_owned: &[bool],
    nested_lexical: &std::collections::HashSet<String>,
) -> bool {
    !binds_anything(body) && {
        let fv = free_vars::free_vars_of_body(ast, &[], body);
        fv.is_empty()
            || (!fn_owned.get(i).copied().unwrap_or(false)
                && fv.iter().all(|n| !nested_lexical.contains(n)))
    }
}

/// Any binding at any depth. A binding is what separates the IIFE from
/// a real indirect eval: the eval's `var` lands on the GLOBAL and
/// survives the call, while an IIFE's dies with the arrow's own scope.
/// A declaration-free source never asks the question. Function bodies
/// need no descent — the declaration itself already answers.
fn binds_anything(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::LetDecl { .. } | Stmt::FnDecl { .. } | Stmt::ClassDecl { .. } => true,
        Stmt::Block(b) | Stmt::Multi(b) => binds_anything(b),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            binds_anything(std::slice::from_ref(then_branch))
                || else_branch
                    .as_deref()
                    .is_some_and(|e| binds_anything(std::slice::from_ref(e)))
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
            binds_anything(std::slice::from_ref(body))
        }
        // The loop variable is itself a binding.
        Stmt::ForOf { .. } => true,
        Stmt::For { init, body, .. } => {
            init.as_deref()
                .is_some_and(|i| binds_anything(std::slice::from_ref(i)))
                || binds_anything(std::slice::from_ref(body))
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            catch_param,
            ..
        } => {
            catch_param.is_some()
                || binds_anything(body)
                || binds_anything(catch_body)
                || finally_body.as_deref().is_some_and(binds_anything)
        }
        Stmt::Switch { cases, default, .. } => {
            cases.iter().any(|c| binds_anything(&c.body))
                || default.as_deref().is_some_and(binds_anything)
        }
        _ => false,
    })
}

/// Overwrite slot `i` with `(() => { body })()`.
pub(super) fn wrap_iife(i: usize, body: Vec<Stmt>, ast: &mut Ast) {
    let arrow = ast.add_expr(Expr::ArrowFn {
        params: Vec::new(),
        return_type: None,
        body,
    });
    ast.exprs[i] = Expr::Call {
        callee: arrow,
        args: Vec::new(),
    };
}
