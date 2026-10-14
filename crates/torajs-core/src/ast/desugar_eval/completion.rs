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
use super::recognize::has_use_strict_prologue;
use super::recognize::literal_eval_call;
use super::rewrite_list;
use super::source::{
    CallForm, EvalStrictness, eval_strictness, first_line, parse_eval_source, syntax_error_throw,
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
    let caller_strict = walk::caller_strict_exprs(ast);
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
        let strict = eval_strictness(form, i, &caller_strict);
        match parse_eval_source(&src, ast, super_ok, strict) {
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
                rewrite_list(&mut body, ast, true, super_ok, &caller_strict);
                let top_level = !fn_owned.get(i).copied().unwrap_or(false);
                if form == CallForm::Direct
                    && strict != EvalStrictness::Strict
                    && !has_use_strict_prologue(&body, ast)
                    && top_level
                {
                    hoist_sloppy_vars_to_script(&mut body, ast);
                }
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
                rewrite_list(&mut body, ast, true, super_ok, &caller_strict);
                let top_level = !fn_owned.get(i).copied().unwrap_or(false);
                let mut body = completion_stmt::rewrite_trailing_stmt_completion(body, ast);
                // AFTER the completion rewrite: a `var x = 2` completes
                // EMPTY (§14.3.2.1) and the assignment it becomes would
                // complete 2 — `eval('var x = 2;')` answered 2 where the
                // spec, and node, answer undefined.
                if form == CallForm::Direct
                    && strict != EvalStrictness::Strict
                    && !has_use_strict_prologue(&body, ast)
                    && top_level
                {
                    hoist_sloppy_vars_to_script(&mut body, ast);
                }
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

/// A SLOPPY direct eval's `var` belongs to the CALLER's variable
/// environment (§19.2.1.3 steps 8-10), and at a top-level call site
/// that is the global one — the IIFE's own frame, which the module
/// doc describes, is the strict rule only. `eval('var x = 2')` in a
/// `for…in` head's object literal left `x` as the arrow's local and
/// the program's `x` at 1 (test262 `scope-head-var-none`, rotation
/// 640). Every `var` site of the source becomes an assignment (an
/// init-less one is dropped), so the write lands on the program's
/// binding; a name the program declares nowhere gets a `var` of its
/// own at the head of the script, filed as an eval-declared site so
/// the hoist gives it the eval flavour (§19.2.1.3: a deletable global).
/// That declaration binds nothing (`inert_sites`): the eval binds the
/// name when it runs, at the head of its text (a placeholder in
/// `heads`), and not at program start.
/// A call site inside a function keeps the sealed frame — the
/// function's own var scope is not reachable from here (registered).
fn hoist_sloppy_vars_to_script(body: &mut Vec<Stmt>, ast: &mut Ast) {
    let mut counts = std::collections::HashMap::new();
    super::super::var_hoist::count_var_names(body, &mut counts);
    if counts.is_empty() {
        return;
    }
    let mut declared = std::collections::HashMap::new();
    super::super::var_hoist::count_var_names(&ast.stmts, &mut declared);
    let fn_names: std::collections::HashSet<String> = ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    let mut names: Vec<String> = counts.into_keys().collect();
    names.sort();
    for name in names {
        if declared.contains_key(&name) || fn_names.contains(&name) {
            continue;
        }
        let init = ast.add_expr(Expr::Uninit);
        ast.eval_var_sites
            .insert(init, super::super::var_hoist::ScriptGlobalVar::EvalVar);
        ast.eval_global_names.inert_sites.insert(init);
        let mark = ast.add_expr(Expr::Ident("undefined".to_string()));
        ast.eval_global_names.heads.insert(mark, name.clone());
        body.insert(0, Stmt::Expr(mark));
        ast.stmts.insert(
            0,
            Stmt::LetDecl {
                mutable: true,
                name,
                type_ann: None,
                init,
                is_var: true,
            },
        );
    }
    for s in body.iter_mut() {
        vars_to_assignments(s, ast);
    }
}

/// `var x = e` → `x = e`; `var x;` → nothing. Every position the var
/// hoist walks (blocks and branches, not function bodies).
fn vars_to_assignments(s: &mut Stmt, ast: &mut Ast) {
    if let Stmt::LetDecl {
        name,
        init,
        is_var: true,
        ..
    } = s
    {
        if matches!(ast.get_expr(*init), Expr::Uninit) {
            *s = Stmt::Block(Vec::new());
            return;
        }
        let target = ast.add_expr(Expr::Ident(name.clone()));
        let assign = ast.add_expr(Expr::Assign {
            target,
            value: *init,
        });
        *s = Stmt::Expr(assign);
        return;
    }
    if matches!(s, Stmt::FnDecl { .. }) {
        return;
    }
    for c in super::super::desugar_with::walk::stmt_children(s) {
        vars_to_assignments(c, ast);
    }
}
