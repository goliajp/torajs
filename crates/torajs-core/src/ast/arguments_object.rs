//! T-11 / T-31 — `arguments` object desugar pass + its 5 yield-free
//! walkers + synthesized-local builder.
//!
//! Chunk 345 — paired with chunk 344's `arguments_object_rewrite`
//! sibling. The main pass (`desugar_arguments_object`) keeps the
//! `pub` symbol (re-exported by ast.rs for the three external
//! callers in `crates/torajs-cli/src/{cmd_build,main,lsp}.rs`), and
//! the helpers (`stmt_uses_dynamic_arguments` / `expr_uses_dynamic_arguments`
//! / `body_has_arguments_length` / `stmt_has_arguments_length` /
//! `expr_has_arguments_length` / `synth_arguments_local`) stay
//! sibling-private here.
//!
//! The cross-sibling call to the actual rewriters lives at
//! `crate::ast::arguments_object_rewrite::rewrite_arguments_in_stmt`.

use super::{Ast, Expr, ExprId, Param, Stmt};

pub fn desugar_arguments_object(ast: &mut Ast) {
    // Snapshot per-fn user-param names, indexed by FnDecl name. The
    // walk below mutates expression nodes in place using these
    // snapshots.
    use std::collections::{HashMap, HashSet};
    let mut fn_params: HashMap<String, Vec<String>> = HashMap::new();
    // T-31 — fns where `arguments.length` is referenced. Restricted to
    // free top-level FnDecls (user_start == 0; closures with `__env`
    // and class methods with `__this` keep the old declared-arity fold
    // to avoid disturbing their dispatch ABI). Each such fn gets a
    // synthetic first param `__torajs_real_argc: number`, and every
    // direct-Ident-callee Call to it gets `Number(args.len())`
    // prepended below.
    let mut uses_real_argc: HashSet<String> = HashSet::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            name, params, body, ..
        } = s
        {
            // Skip the synthetic `__env` (closure capture vector) and
            // `__this` (class instance) prefix params — they're not
            // user-visible "arguments". Everything after is the
            // user's declared param list.
            let user_start = params
                .first()
                .filter(|p| p.name == "__env" || p.name == "__this")
                .map(|_| 1)
                .unwrap_or(0);
            let names: Vec<String> = params[user_start..]
                .iter()
                .map(|p| p.name.clone())
                .collect();
            fn_params.insert(name.clone(), names);
            if user_start == 0 && body_has_arguments_length(ast, body) {
                uses_real_argc.insert(name.clone());
            }
        }
    }

    // T-31 — inject `__torajs_real_argc: number` at param[0] for each
    // uses_real_argc fn. Done before the body-rewrite walk so the
    // typechecker (which runs after desugar) sees the new signature
    // and so the recursive `arguments.length` rewrite below can resolve
    // `Ident("__torajs_real_argc")` cleanly.
    if !uses_real_argc.is_empty() {
        for s in ast.stmts.iter_mut() {
            if let Stmt::FnDecl { name, params, .. } = s
                && uses_real_argc.contains(name)
            {
                params.insert(
                    0,
                    Param {
                        name: "__torajs_real_argc".into(),
                        type_ann: Some("number".into()),
                        default: None,
                        is_rest: false,
                    },
                );
            }
        }
    }

    let stmts_clone: Vec<Stmt> = ast.stmts.clone();
    for (idx, stmt) in stmts_clone.iter().enumerate() {
        if let Stmt::FnDecl { name, body, .. } = stmt {
            let Some(params) = fn_params.get(name) else {
                continue;
            };
            let params = params.clone();
            let real_argc_here = uses_real_argc.contains(name);
            // T-11 — pre-pass: detect any dynamic `arguments[<non-
            // literal>]` use. If found, prepend a synthesized
            // `let __torajs_arguments: any[] = [p0, p1, ...]` before
            // the body and rewrite the dynamic indices to read from it.
            // Literal-index rewrites (the existing path) take priority
            // and don't materialize the array — they stay zero-cost.
            let mut needs_materialize = false;
            for s in body {
                if stmt_uses_dynamic_arguments(ast, s) {
                    needs_materialize = true;
                    break;
                }
            }
            let new_body: Vec<Stmt> = body
                .iter()
                .map(|s| {
                    crate::ast::arguments_object_rewrite::rewrite_arguments_in_stmt(
                        ast,
                        s,
                        &params,
                        real_argc_here,
                    )
                })
                .collect();
            // Synthesize the local OUTSIDE the &mut ast.stmts borrow
            // (synth_arguments_local also takes &mut ast for add_expr).
            let synth_opt = if needs_materialize {
                Some(synth_arguments_local(ast, &params))
            } else {
                None
            };
            if let Stmt::FnDecl { body: b, .. } = &mut ast.stmts[idx] {
                if let Some(synth) = synth_opt {
                    let mut full = Vec::with_capacity(new_body.len() + 1);
                    full.push(synth);
                    full.extend(new_body);
                    *b = full;
                } else {
                    *b = new_body;
                }
            }
        }
    }

    // T-31 — arena walk: every Call whose callee is a direct Ident to
    // a uses_real_argc fn gets `Number(args.len())` prepended as new
    // arg[0]. args.len() at this point is the user-passed count BEFORE
    // T-28's trailing-undef pad runs in check.rs / ssa_lower. The
    // checker sees the prepended arg, accepts the call (the remaining
    // user params are all Any and qualify for T-28 pad), and ssa_lower
    // lowers the prepended Number as ConstI64 matching the callee's
    // `: number` first param.
    if !uses_real_argc.is_empty() {
        let n = ast.exprs.len();
        for i in 0..n {
            let (callee, args_clone) = match &ast.exprs[i] {
                Expr::Call { callee, args } => (*callee, args.clone()),
                _ => continue,
            };
            let name = match ast.get_expr(callee) {
                Expr::Ident(n) => n.clone(),
                _ => continue,
            };
            if !uses_real_argc.contains(&name) {
                continue;
            }
            let argc = args_clone.len();
            let argc_lit = ast.add_expr(Expr::Number(argc as f64));
            let mut new_args = Vec::with_capacity(argc + 1);
            new_args.push(argc_lit);
            new_args.extend(args_clone);
            ast.exprs[i] = Expr::Call {
                callee,
                args: new_args,
            };
        }
    }
}

/// T-11 — returns true if any `arguments[<non-literal>]` index access
/// (or bare `arguments` reference outside the literal-index /
/// `arguments.length` / spread forms the existing rewrite handles)
/// appears in the stmt subtree. Used to gate the synthesized
/// `let __torajs_arguments: any[] = [...]` prepend.
fn stmt_uses_dynamic_arguments(ast: &Ast, s: &Stmt) -> bool {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => {
            expr_uses_dynamic_arguments(ast, *eid)
        }
        Stmt::Return(opt) => opt.is_some_and(|e| expr_uses_dynamic_arguments(ast, e)),
        Stmt::LetDecl { init, .. } => expr_uses_dynamic_arguments(ast, *init),
        Stmt::YieldInto { value, .. } => expr_uses_dynamic_arguments(ast, *value),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_uses_dynamic_arguments(ast, *cond)
                || stmt_uses_dynamic_arguments(ast, then_branch)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| stmt_uses_dynamic_arguments(ast, e))
        }
        Stmt::While { cond, body } | Stmt::DoWhile { cond, body } => {
            expr_uses_dynamic_arguments(ast, *cond) || stmt_uses_dynamic_arguments(ast, body)
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            init.as_ref()
                .is_some_and(|s| stmt_uses_dynamic_arguments(ast, s))
                || cond.is_some_and(|c| expr_uses_dynamic_arguments(ast, c))
                || step.is_some_and(|st| expr_uses_dynamic_arguments(ast, st))
                || stmt_uses_dynamic_arguments(ast, body)
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            expr_uses_dynamic_arguments(ast, *parent)
                || expr_uses_dynamic_arguments(ast, *sep)
                || stmt_uses_dynamic_arguments(ast, body)
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => expr_uses_dynamic_arguments(ast, *elem_expr) || stmt_uses_dynamic_arguments(ast, body),
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            stmts.iter().any(|s| stmt_uses_dynamic_arguments(ast, s))
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter().any(|s| stmt_uses_dynamic_arguments(ast, s))
                || catch_body
                    .iter()
                    .any(|s| stmt_uses_dynamic_arguments(ast, s))
                || finally_body
                    .as_ref()
                    .is_some_and(|fb| fb.iter().any(|s| stmt_uses_dynamic_arguments(ast, s)))
        }
        _ => false,
    }
}

fn expr_uses_dynamic_arguments(ast: &Ast, eid: ExprId) -> bool {
    match ast.get_expr(eid) {
        Expr::Index { obj, index } => {
            // Match `arguments[<non-Number-literal>]`. Number-literal
            // case is already handled inline by the existing rewrite
            // (param-name substitution; no array materialization).
            if matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "arguments") {
                if !matches!(ast.get_expr(*index), Expr::Number(_)) {
                    return true;
                }
                // Number index but out-of-range fall-through still
                // materializes — bun returns undefined; tr maps to
                // null in the boxed Any read. Conservative: treat as
                // dynamic so the array is available.
                if let Expr::Number(n) = ast.get_expr(*index)
                    && (n.fract() != 0.0 || (*n as usize) >= count_user_params(ast, eid))
                {
                    return true;
                }
            }
            expr_uses_dynamic_arguments(ast, *obj) || expr_uses_dynamic_arguments(ast, *index)
        }
        Expr::Member { obj, name } => {
            // `arguments.callee` — currently unhandled; will need its
            // own materialization later. Bare `arguments.<other>`
            // also forces materialize so stuff like
            // `arguments.length.toString()` keeps walking.
            if matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "arguments") && name != "length"
            {
                return true;
            }
            expr_uses_dynamic_arguments(ast, *obj)
        }
        Expr::Ident(n) if n == "arguments" => {
            // Bare `arguments` reference (not Index / Member / spread —
            // those have their own arms). E.g. `let xs = arguments;`
            // or passing `arguments` to a fn that's not the spread
            // form. Forces materialize.
            true
        }
        Expr::Call { callee, args } => {
            expr_uses_dynamic_arguments(ast, *callee)
                || args.iter().any(|a| {
                    // `f(...arguments)` is handled by the inline-spread
                    // rewrite — no materialize needed.
                    if let Expr::Spread { expr } = ast.get_expr(*a)
                        && let Expr::Ident(n) = ast.get_expr(*expr)
                        && n == "arguments"
                    {
                        return false;
                    }
                    expr_uses_dynamic_arguments(ast, *a)
                })
        }
        Expr::BinOp { left, right, .. } => {
            expr_uses_dynamic_arguments(ast, *left) || expr_uses_dynamic_arguments(ast, *right)
        }
        Expr::Unary { expr, .. } | Expr::TypeOf { expr } | Expr::PostIncr { target: expr, .. } => {
            expr_uses_dynamic_arguments(ast, *expr)
        }
        Expr::Assign { target, value } => {
            expr_uses_dynamic_arguments(ast, *target) || expr_uses_dynamic_arguments(ast, *value)
        }
        Expr::Array(items) => items.iter().any(|e| {
            // `[...arguments]` — handled inline by spread rewrite.
            if let Expr::Spread { expr } = ast.get_expr(*e)
                && let Expr::Ident(n) = ast.get_expr(*expr)
                && n == "arguments"
            {
                return false;
            }
            expr_uses_dynamic_arguments(ast, *e)
        }),
        Expr::ObjectLit { fields } => fields
            .iter()
            .any(|(_, e)| expr_uses_dynamic_arguments(ast, *e)),
        Expr::Spread { expr } => expr_uses_dynamic_arguments(ast, *expr),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_uses_dynamic_arguments(ast, *cond)
                || expr_uses_dynamic_arguments(ast, *then_branch)
                || expr_uses_dynamic_arguments(ast, *else_branch)
        }
        Expr::Nullish { lhs, rhs } => {
            expr_uses_dynamic_arguments(ast, *lhs) || expr_uses_dynamic_arguments(ast, *rhs)
        }
        Expr::OptChain { obj, .. } => expr_uses_dynamic_arguments(ast, *obj),
        _ => false,
    }
}

fn count_user_params(_ast: &Ast, _eid: ExprId) -> usize {
    // Caller's params count is captured during the FnDecl walk and
    // not threaded through expr_uses_dynamic_arguments today; default
    // to a large value so the literal-bounds-check arm never trips.
    // The bounds-aware materialize is a follow-up.
    usize::MAX
}

/// T-31 — returns true if the fn body references `arguments.length`
/// (i.e. an `Expr::Member { obj: Ident("arguments"), name: "length" }`)
/// anywhere. Used by `desugar_arguments_object` to decide whether to
/// inject the `__torajs_real_argc` synthetic param.
fn body_has_arguments_length(ast: &Ast, body: &[Stmt]) -> bool {
    body.iter().any(|s| stmt_has_arguments_length(ast, s))
}

fn stmt_has_arguments_length(ast: &Ast, s: &Stmt) -> bool {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => {
            expr_has_arguments_length(ast, *eid)
        }
        Stmt::Return(opt) => opt.is_some_and(|e| expr_has_arguments_length(ast, e)),
        Stmt::LetDecl { init, .. } => expr_has_arguments_length(ast, *init),
        Stmt::YieldInto { value, .. } => expr_has_arguments_length(ast, *value),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_has_arguments_length(ast, *cond)
                || stmt_has_arguments_length(ast, then_branch)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| stmt_has_arguments_length(ast, e))
        }
        Stmt::While { cond, body } | Stmt::DoWhile { cond, body } => {
            expr_has_arguments_length(ast, *cond) || stmt_has_arguments_length(ast, body)
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            init.as_ref()
                .is_some_and(|s| stmt_has_arguments_length(ast, s))
                || cond.is_some_and(|c| expr_has_arguments_length(ast, c))
                || step.is_some_and(|st| expr_has_arguments_length(ast, st))
                || stmt_has_arguments_length(ast, body)
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            expr_has_arguments_length(ast, *parent)
                || expr_has_arguments_length(ast, *sep)
                || stmt_has_arguments_length(ast, body)
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => expr_has_arguments_length(ast, *elem_expr) || stmt_has_arguments_length(ast, body),
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            stmts.iter().any(|s| stmt_has_arguments_length(ast, s))
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter().any(|s| stmt_has_arguments_length(ast, s))
                || catch_body.iter().any(|s| stmt_has_arguments_length(ast, s))
                || finally_body
                    .as_ref()
                    .is_some_and(|fb| fb.iter().any(|s| stmt_has_arguments_length(ast, s)))
        }
        // Nested FnDecl is an independent scope; its `arguments`
        // refers to the inner fn, not the outer one we're scanning.
        // desugar_nested_fns lifts these to top-level before us, so
        // this arm is mostly defensive.
        _ => false,
    }
}

fn expr_has_arguments_length(ast: &Ast, eid: ExprId) -> bool {
    match ast.get_expr(eid) {
        Expr::Member { obj, name } if name == "length" => {
            if matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "arguments") {
                return true;
            }
            expr_has_arguments_length(ast, *obj)
        }
        Expr::Member { obj, .. } => expr_has_arguments_length(ast, *obj),
        Expr::Index { obj, index } => {
            expr_has_arguments_length(ast, *obj) || expr_has_arguments_length(ast, *index)
        }
        Expr::BinOp { left, right, .. } => {
            expr_has_arguments_length(ast, *left) || expr_has_arguments_length(ast, *right)
        }
        Expr::Unary { expr, .. } | Expr::TypeOf { expr } | Expr::PostIncr { target: expr, .. } => {
            expr_has_arguments_length(ast, *expr)
        }
        Expr::Assign { target, value } => {
            expr_has_arguments_length(ast, *target) || expr_has_arguments_length(ast, *value)
        }
        Expr::Call { callee, args } => {
            expr_has_arguments_length(ast, *callee)
                || args.iter().any(|a| expr_has_arguments_length(ast, *a))
        }
        Expr::Array(items) => items.iter().any(|e| expr_has_arguments_length(ast, *e)),
        Expr::ObjectLit { fields } => fields
            .iter()
            .any(|(_, e)| expr_has_arguments_length(ast, *e)),
        Expr::Spread { expr } => expr_has_arguments_length(ast, *expr),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_has_arguments_length(ast, *cond)
                || expr_has_arguments_length(ast, *then_branch)
                || expr_has_arguments_length(ast, *else_branch)
        }
        Expr::Nullish { lhs, rhs } => {
            expr_has_arguments_length(ast, *lhs) || expr_has_arguments_length(ast, *rhs)
        }
        Expr::OptChain { obj, .. } => expr_has_arguments_length(ast, *obj),
        _ => false,
    }
}

/// T-11 — synthesize `let __torajs_arguments: any[] = [p0, p1, ...]`
/// for prepending to a fn body. Each param Ident becomes one array
/// element; the LetDecl arm in ssa_lower routes through the forced-
/// Any path because the annotation is `any[]`.
fn synth_arguments_local(ast: &mut Ast, params: &[String]) -> Stmt {
    let elems: Vec<ExprId> = params
        .iter()
        .map(|p| ast.add_expr(Expr::Ident(p.clone())))
        .collect();
    let init = ast.add_expr(Expr::Array(elems));
    Stmt::LetDecl {
        mutable: false,
        name: "__torajs_arguments".into(),
        type_ann: Some("any[]".into()),
        init,
        is_var: false,
    }
}
