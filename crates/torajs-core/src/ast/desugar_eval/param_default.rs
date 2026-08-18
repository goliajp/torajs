//! Parameter-default direct evals that declare `arguments` — the
//! §19.2.1.3 EvalDeclarationInstantiation special case.
//!
//! Every `*declare-arguments*` test262 case puts the eval in default
//! parameter position: `function f(p = eval("var arguments")) {}` must
//! throw a SyntaxError **when f is called** (an async function turns
//! that into a rejection). The verdict is goal-independent: in strict
//! code `arguments` is not a legal BindingIdentifier, so the source
//! fails to parse (§19.2.1.1 step 12 → SyntaxError at evaluation
//! time); in sloppy code EvalDeclarationInstantiation refuses a
//! var-scoped `arguments` declaration when the eval's variable
//! environment is a function's parameter scope. Either way the call
//! site evaluates to a SyntaxError throw, so the rewrite needs no
//! goal gate.
//!
//! Arrow functions take a different path — an arrow has no
//! `arguments` binding of its own, so the declaration is legal there
//! (the binding lands in the arrow's parameter scope) unless a
//! parameter is itself named `arguments`. The ownership walk here
//! therefore skips true-arrow defaults; the sibling
//! `param_default_arrow` pass owns both arrow verdicts. `mark_subtree`
//! stops at any nested function boundary: a direct eval inside an
//! arrow that sits in a default gets the ARROW's variable
//! environment, not the parameter scope, so the special case does not
//! apply to it.
//!
//! This pass must run BEFORE the value-position collapse: under the
//! strict assumption that collapse folds a declarations-only source
//! to `undefined` (bindings die with the eval's environment), which
//! silently swallows the required throw in exactly this position.
//!
//! What stays out of scope (RFC 20260810-eval-declare-arguments):
//! the legal arrow-position semantics (eval's `var` landing in the
//! parameter scope) keep their honest current behavior; indirect
//! evals never take this path (their variable environment is the
//! global, where a `var arguments` is legal).

use super::super::{Ast, Expr, ExprId, Param, Stmt};
use super::completion;
use super::source::{
    CallForm, DeleteSites, first_line, literal_eval_call, parse_eval_source, syntax_error_throw,
};

/// Rewrite every direct literal eval in non-arrow default-parameter
/// position whose source var-declares `arguments` into a SyntaxError
/// throw IIFE (the statement-in-value-position carrier).
pub(super) fn rewrite_param_default_arguments_evals(ast: &mut Ast) {
    let owned = param_default_owned_exprs(ast);
    let mut i = 0;
    while i < ast.exprs.len() {
        if !owned.get(i).copied().unwrap_or(false) {
            i += 1;
            continue;
        }
        let eid = ExprId(i as u32);
        let Some((src, CallForm::Direct)) = literal_eval_call(eid, ast) else {
            i += 1;
            continue;
        };
        // A source that fails to parse is not this pass's problem —
        // the completion pass already turns it into the same throw.
        let Some(body) = parse_eval_source(&src, ast, false, DeleteSites::Strict) else {
            i += 1;
            continue;
        };
        if declares_var_arguments(&body) {
            let throw = syntax_error_throw(format!("eval: {}", first_line(&src)), ast);
            completion::wrap_iife(i, vec![throw], ast);
        }
        i += 1;
    }
}

/// Var-scoped declarations of the name `arguments` in the parsed eval
/// source: `var arguments` (`LetDecl { is_var }`) and hoisted
/// function declarations both hit the EvalDeclarationInstantiation
/// check; `let`/`const` spellings die in the eval's own lexical
/// environment and take a different (lexical) error path — left out.
pub(super) fn declares_var_arguments(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::LetDecl {
            name, is_var: true, ..
        } => name == "arguments",
        Stmt::FnDecl { name, .. } => name == "arguments",
        Stmt::Multi(list) => declares_var_arguments(list),
        _ => false,
    })
}

/// One flag per arena slot: does this expression sit in a NON-ARROW
/// function's default-parameter expression, reached without crossing
/// another function boundary?
///
/// Expression-position functions all live in the flat arena, so a
/// linear scan reaches every `ArrowFn` node — including ones nested
/// inside other bodies — and the parser side-tables
/// (`fn_expr_exprs` / `gen_fn_exprs`) say which of those are really
/// function expressions. Statement-position functions (`FnDecl`,
/// class members) come from the statement walk; an `ArrowFn` body's
/// nested `FnDecl`s are covered by walking every arena `ArrowFn`'s
/// body too (re-marking a slot is idempotent).
fn param_default_owned_exprs(ast: &Ast) -> Vec<bool> {
    let mut owned = vec![false; ast.exprs.len()];
    for (i, e) in ast.exprs.iter().enumerate() {
        if let Expr::ArrowFn { params, body, .. } = e {
            let eid = ExprId(i as u32);
            // An object-literal METHOD parses as an arena ArrowFn too
            // (`objlit_method_exprs` is what remembers the difference),
            // and a method has its own `arguments` binding — §19.2.1.3
            // applies to it exactly like a function expression (the
            // t262 meth-*-declare-arguments family).
            if ast.fn_expr_exprs.contains(&eid)
                || ast.gen_fn_exprs.contains_key(&eid)
                || ast.objlit_method_exprs.contains(&eid)
            {
                mark_defaults(ast, params, &mut owned);
            }
            mark_stmts(ast, body, &mut owned);
        }
    }
    mark_stmts(ast, &ast.stmts, &mut owned);
    owned
}

pub(super) fn mark_defaults(ast: &Ast, params: &[Param], owned: &mut [bool]) {
    for p in params {
        if let Some(d) = p.default {
            mark_subtree(ast, d, owned);
        }
    }
}

fn mark_stmts(ast: &Ast, stmts: &[Stmt], owned: &mut [bool]) {
    for s in stmts {
        mark_stmt(ast, s, owned);
    }
}

/// Statement walk that only hunts for function declarations and class
/// members — their defaults get marked, their bodies walked for more.
/// Expression positions need no walk here: arena scan covers them.
fn mark_stmt(ast: &Ast, s: &Stmt, owned: &mut [bool]) {
    match s {
        Stmt::FnDecl { params, body, .. } => {
            mark_defaults(ast, params, owned);
            mark_stmts(ast, body, owned);
        }
        Stmt::ClassDecl {
            static_init,
            ctor,
            methods,
            static_methods,
            ..
        } => {
            for si in static_init {
                if let super::super::StaticInit::Block(b) = si {
                    mark_stmts(ast, b, owned);
                }
            }
            if let Some(c) = ctor {
                mark_defaults(ast, &c.params, owned);
                mark_stmts(ast, &c.body, owned);
            }
            for m in methods.iter().chain(static_methods) {
                mark_defaults(ast, &m.params, owned);
                mark_stmts(ast, &m.body, owned);
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            mark_stmt(ast, then_branch, owned);
            if let Some(e) = else_branch.as_deref() {
                mark_stmt(ast, e, owned);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
            mark_stmt(ast, body, owned)
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init.as_deref() {
                mark_stmt(ast, i, owned);
            }
            mark_stmt(ast, body, owned);
        }
        Stmt::ForOf { body, .. } | Stmt::ForOfSplitIter { body, .. } => mark_stmt(ast, body, owned),
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                mark_stmts(ast, &c.body, owned);
            }
            if let Some(d) = default {
                mark_stmts(ast, d, owned);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            mark_stmts(ast, body, owned);
            mark_stmts(ast, catch_body, owned);
            if let Some(f) = finally_body {
                mark_stmts(ast, f, owned);
            }
        }
        Stmt::Block(b) | Stmt::Multi(b) => mark_stmts(ast, b, owned),
        Stmt::ExportDecl { inner, .. } => {
            if let Some(i) = inner.as_deref() {
                mark_stmt(ast, i, owned);
            }
        }
        _ => {}
    }
}

/// Flag `eid`'s whole expression sub-tree, STOPPING at any nested
/// function boundary — an eval inside a nested function's body takes
/// that function's variable environment, not the parameter scope.
fn mark_subtree(ast: &Ast, eid: ExprId, owned: &mut [bool]) {
    let Some(slot) = owned.get_mut(eid.0 as usize) else {
        return;
    };
    *slot = true;
    let Some(e) = ast.exprs.get(eid.0 as usize) else {
        return;
    };
    match e {
        Expr::ArrowFn { .. } => {}
        Expr::BinOp { left, right, .. }
        | Expr::Sequence { left, right }
        | Expr::Nullish {
            lhs: left,
            rhs: right,
        } => {
            mark_subtree(ast, *left, owned);
            mark_subtree(ast, *right, owned);
        }
        Expr::Unary { expr, .. }
        | Expr::As { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::Spread { expr } => mark_subtree(ast, *expr, owned),
        Expr::InstanceOf { expr, rhs } => {
            mark_subtree(ast, *expr, owned);
            mark_subtree(ast, *rhs, owned);
        }
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => mark_subtree(ast, *obj, owned),
        Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
            mark_subtree(ast, *callee, owned);
            for a in args {
                mark_subtree(ast, *a, owned);
            }
        }
        Expr::Assign { target, value } => {
            mark_subtree(ast, *target, owned);
            mark_subtree(ast, *value, owned);
        }
        Expr::Index { obj, index } | Expr::OptIndex { obj, index } => {
            mark_subtree(ast, *obj, owned);
            mark_subtree(ast, *index, owned);
        }
        Expr::Array(elems) => {
            for el in elems {
                mark_subtree(ast, *el, owned);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, v) in fields {
                mark_subtree(ast, *v, owned);
            }
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            for a in args {
                mark_subtree(ast, *a, owned);
            }
        }
        Expr::NewDynamic { callee, args } => {
            mark_subtree(ast, *callee, owned);
            for a in args {
                mark_subtree(ast, *a, owned);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            mark_subtree(ast, *cond, owned);
            mark_subtree(ast, *then_branch, owned);
            mark_subtree(ast, *else_branch, owned);
        }
        Expr::PostIncr { target, .. } => mark_subtree(ast, *target, owned),
        _ => {}
    }
}
