//! Return-site closure-forwarder pass — chunk 354, extracted from ast.rs.
//!
//! Pub entry `synthesize_forwarders` synths `__forward_<name>` shims
//! for `Stmt::Return(Ident(top_fn))` in closure-typed-ret positions.
//! The 7 private helpers (`body_has_closure_return`,
//! `stmt_has_closure_return`, `collect_fnsig_ident_returns` +
//! `_stmt`, `collect_return_ident_rewrites`,
//! `rewrite_returns_to_forwarders`, `collect_return_replacements`)
//! walk stmt trees to detect and rewrite the Return sites.
//!
//! Sibling `ast/forwarders_object.rs` holds the ObjectLit-store-site
//! variant (`synthesize_fn_to_closure_forwarders`) + the field-type
//! tagger (`tag_struct_field_closure_types`).

use super::{Ast, Expr, ExprId, Param, Stmt};

/// Knife 4d (RFC 20260801-arguments-method-face) — an argv-consuming
/// generator factory carries a trailing GEN_ARGV_PARAM (the obj-gen
/// parser's argv channel). Its forwarder must NOT declare that slot:
/// it fills it with its own `[...arguments]` instead, so the relay
/// carries an arguments touch, the argv/static faces adopt the relay,
/// and the true call-site argv reaches the factory however the relay
/// is invoked (member call or escaped alias). Answers the params the
/// forwarder declares plus whether the tail was trimmed — the caller
/// then pushes the spread arg via [`push_gen_argv_spread`].
pub(crate) fn split_gen_argv_tail(params: &[Param]) -> (&[Param], bool) {
    if params
        .last()
        .is_some_and(|p| p.name == crate::ast::GEN_ARGV_PARAM)
    {
        (&params[..params.len() - 1], true)
    } else {
        (params, false)
    }
}

/// Second half of [`split_gen_argv_tail`]: append `[...arguments]`
/// as the factory call's argv argument.
pub(crate) fn push_gen_argv_spread(ast: &mut Ast, arg_eids: &mut Vec<ExprId>) {
    let src = ast.add_expr(Expr::Ident("arguments".into()));
    let spread = ast.add_expr(Expr::Spread { expr: src });
    arg_eids.push(ast.add_expr(Expr::Array(vec![spread])));
}

/// Synthesize forwarder closures for `Stmt::Return(Ident(global_fn))`
/// in functions whose declared ret type is a closure type
/// (`(...) => R`). Without this, ssa_lower's `effective_ret_ty`
/// upgrades the fn's ret to Closure (because the body also returns
/// a capturing arrow somewhere) but the bare-fn-name branch returns
/// a Type::FnSig value — calling-convention mismatch SIGSEGVs at
/// the call site.
///
/// Fix: each such Return(Ident(name)) is rewritten to
///   `return Closure { fn_name: "__forward_<name>", captures: [] }`
/// where `__forward_<name>(__env: ptr, args...) { return name(args...); }`
/// is a synthesized FnDecl appended to ast.stmts. The forwarder has
/// a `__env` first param so ssa_lower treats it as closure-shaped
/// (env-first calling convention); the body just discards env and
/// forwards to the wrapped fn. Both branches now emit Closure
/// values and the caller's CallIndirect dispatches uniformly.
///
/// Runs after `lift_arrow_fns` so capturing arrows are already
/// `Expr::Closure` and we can detect mixed shapes.
pub fn synthesize_forwarders(ast: &mut Ast) {
    use std::collections::{HashMap, HashSet};

    // Snapshot all FnDecls' (params, return type) for forwarder body
    // synthesis. Filter out "closure-shaped" fns (first param `__env`):
    // those are already closures and shouldn't be wrapped.
    let mut fn_sigs: HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)> =
        HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            name,
            params,
            return_type,
            span,
            ..
        } = s
        {
            let is_closure_shaped = params.first().is_some_and(|p| p.name == "__env");
            if !is_closure_shaped {
                fn_sigs.insert(name.clone(), (params.clone(), return_type.clone(), *span));
            }
        }
    }
    if fn_sigs.is_empty() {
        return;
    }

    // Walk fns whose declared ret type is closure-typed. For each,
    // collect Return(Ident(global_fn)) pairs that need a forwarder.
    let mut targets: HashSet<String> = HashSet::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            return_type,
            body,
            params,
            ..
        } = s
        {
            // Skip closure-shaped fns (their bodies were already lifted).
            let is_closure_shaped = params.first().is_some_and(|p| p.name == "__env");
            if is_closure_shaped {
                continue;
            }
            let Some(rt) = return_type.as_deref() else {
                continue;
            };
            // Quick sniff: ret type looks like a fn type ann
            // (`(args) => R` parser shape, or `__fn(...)->R` lifted
            // shape).
            let looks_like_fn = rt.starts_with('(') || rt.contains("=>") || rt.starts_with("__fn(");
            if !looks_like_fn {
                continue;
            }
            // Body has any Return(Closure-producing expr)? Mirrors
            // ssa_lower's `body_returns_closure` heuristic.
            if !body_has_closure_return(ast, body) {
                continue;
            }
            // Collect Return(Ident(name)) where name is FnSig-shaped.
            collect_fnsig_ident_returns(ast, body, &fn_sigs, &mut targets);
        }
    }

    if targets.is_empty() {
        return;
    }

    // Synthesize one forwarder per target.
    let mut new_decls: Vec<Stmt> = Vec::new();
    let mut renames: HashMap<String, String> = HashMap::new();
    for target in &targets {
        let (params, return_type, target_span) = fn_sigs.get(target).unwrap().clone();
        let forward_name = format!("__forward_{target}");
        // params: __env: ptr, ...orig params
        let mut fwd_params: Vec<Param> = Vec::with_capacity(params.len() + 1);
        fwd_params.push(Param {
            name: "__env".into(),
            type_ann: Some(format!("__env({})", "")),
            default: None,
            is_rest: false,
        });
        // A bind_this_param-promoted target carries a hidden `__this`
        // first param. The forwarder's public face skips it (callers
        // pass the user-visible arity) and the forwarding call feeds
        // `undefined` — same plain-call receiver the direct-call
        // rewrite in this_param.rs supplies.
        let takes_this = params.first().is_some_and(|p| p.name == "__this");
        let user_params = if takes_this {
            &params[1..]
        } else {
            &params[..]
        };
        // Knife 4d — a trailing GEN_ARGV_PARAM never enters the
        // forwarder's declared face; the call below feeds it
        // `[...arguments]` (see split_gen_argv_tail).
        let (user_params, takes_gen_argv) = split_gen_argv_tail(user_params);
        fwd_params.extend(user_params.iter().cloned());
        // body: return target(p0, p1, ...);
        let mut arg_eids: Vec<ExprId> = Vec::with_capacity(params.len());
        if takes_this {
            arg_eids.push(ast.add_expr(Expr::Ident("undefined".into())));
        }
        for p in user_params {
            arg_eids.push(ast.add_expr(Expr::Ident(p.name.clone())));
        }
        if takes_gen_argv {
            push_gen_argv_spread(ast, &mut arg_eids);
        }
        let callee_id = ast.add_expr(Expr::Ident(target.clone()));
        let call_id = ast.add_expr(Expr::Call {
            callee: callee_id,
            args: arg_eids,
        });
        let body = vec![Stmt::Return(Some(call_id))];
        new_decls.push(Stmt::FnDecl {
            name: forward_name.clone(),
            type_params: Vec::new(),
            params: fwd_params,
            return_type,
            body,
            is_generator: false,
            // B4 — carry the TARGET's source span: the forwarder IS
            // the user fn as far as toString is concerned.
            span: target_span,
        });
        renames.insert(target.clone(), forward_name);
    }

    // Rewrite Return(Ident(target)) → Return(Closure { fn_name:
    // __forward_<target>, captures: [] }). Done by adding new exprs;
    // existing ExprIds stay valid (just point at unused old idents).
    let n = ast.exprs.len();
    let mut return_rewrites: Vec<(usize, ExprId)> = Vec::new();
    for i in 0..ast.stmts.len() {
        collect_return_ident_rewrites(ast, i, &renames, &mut return_rewrites);
    }
    for (stmt_visit_idx, eid_to_replace) in return_rewrites {
        let _ = stmt_visit_idx;
        let _ = eid_to_replace;
    }
    // Walk stmts and rewrite Returns directly.
    rewrite_returns_to_forwarders(ast, &renames);

    let _ = n;
    ast.stmts.extend(new_decls);
}

fn body_has_closure_return(ast: &Ast, body: &[Stmt]) -> bool {
    body.iter().any(|s| stmt_has_closure_return(ast, s))
}

fn stmt_has_closure_return(ast: &Ast, s: &Stmt) -> bool {
    match s {
        Stmt::Return(Some(eid)) => {
            matches!(ast.get_expr(*eid), Expr::Closure { .. })
                || matches!(ast.get_expr(*eid), Expr::Ident(n) if n.starts_with("__closure_"))
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            stmt_has_closure_return(ast, then_branch)
                || else_branch
                    .as_deref()
                    .is_some_and(|s| stmt_has_closure_return(ast, s))
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => stmt_has_closure_return(ast, body),
        Stmt::Labeled { body, .. } => stmt_has_closure_return(ast, body),
        Stmt::For { body, .. } => stmt_has_closure_return(ast, body),
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            stmts.iter().any(|s| stmt_has_closure_return(ast, s))
        }
        Stmt::Switch { cases, default, .. } => {
            cases
                .iter()
                .any(|c| c.body.iter().any(|s| stmt_has_closure_return(ast, s)))
                || default
                    .as_ref()
                    .is_some_and(|d| d.iter().any(|s| stmt_has_closure_return(ast, s)))
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter().any(|s| stmt_has_closure_return(ast, s))
                || catch_body.iter().any(|s| stmt_has_closure_return(ast, s))
                || finally_body
                    .as_ref()
                    .is_some_and(|fb| fb.iter().any(|s| stmt_has_closure_return(ast, s)))
        }
        _ => false,
    }
}

fn collect_fnsig_ident_returns(
    ast: &Ast,
    body: &[Stmt],
    fn_sigs: &std::collections::HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>,
    out: &mut std::collections::HashSet<String>,
) {
    for s in body {
        collect_fnsig_ident_returns_stmt(ast, s, fn_sigs, out);
    }
}

fn collect_fnsig_ident_returns_stmt(
    ast: &Ast,
    s: &Stmt,
    fn_sigs: &std::collections::HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>,
    out: &mut std::collections::HashSet<String>,
) {
    match s {
        Stmt::Return(Some(eid)) => {
            if let Expr::Ident(name) = ast.get_expr(*eid)
                && fn_sigs.contains_key(name)
            {
                out.insert(name.clone());
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_fnsig_ident_returns_stmt(ast, then_branch, fn_sigs, out);
            if let Some(eb) = else_branch {
                collect_fnsig_ident_returns_stmt(ast, eb, fn_sigs, out);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            collect_fnsig_ident_returns_stmt(ast, body, fn_sigs, out);
        }
        Stmt::Labeled { body, .. } => {
            collect_fnsig_ident_returns_stmt(ast, body, fn_sigs, out);
        }
        Stmt::For { body, .. } => {
            collect_fnsig_ident_returns_stmt(ast, body, fn_sigs, out);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                collect_fnsig_ident_returns_stmt(ast, s, fn_sigs, out);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                for s in &c.body {
                    collect_fnsig_ident_returns_stmt(ast, s, fn_sigs, out);
                }
            }
            if let Some(d) = default {
                for s in d {
                    collect_fnsig_ident_returns_stmt(ast, s, fn_sigs, out);
                }
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for s in body {
                collect_fnsig_ident_returns_stmt(ast, s, fn_sigs, out);
            }
            for s in catch_body {
                collect_fnsig_ident_returns_stmt(ast, s, fn_sigs, out);
            }
            if let Some(fb) = finally_body {
                for s in fb {
                    collect_fnsig_ident_returns_stmt(ast, s, fn_sigs, out);
                }
            }
        }
        _ => {}
    }
}

#[allow(unused)]
fn collect_return_ident_rewrites(
    _ast: &Ast,
    _stmt_idx: usize,
    _renames: &std::collections::HashMap<String, String>,
    _out: &mut Vec<(usize, ExprId)>,
) {
    // Placeholder — kept so the synthesize_forwarders body compiles
    // while the rewrite walker is the actual mutating pass below.
}

fn rewrite_returns_to_forwarders(
    ast: &mut Ast,
    renames: &std::collections::HashMap<String, String>,
) {
    // Two-phase: collect (eid, forward_name) replacements, then apply.
    // Walk every FnDecl's body — top-level stmts are mostly FnDecls
    // and the actual Returns we need to rewrite live in their bodies.
    let mut replacements: Vec<(ExprId, String)> = Vec::new();
    let bodies: Vec<Vec<Stmt>> = ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { body, .. } => Some(body.clone()),
            _ => None,
        })
        .collect();
    for body in &bodies {
        for s in body {
            collect_return_replacements(ast, s, renames, &mut replacements);
        }
    }
    for (eid, forward_name) in replacements {
        let new_expr = Expr::Closure {
            fn_name: forward_name,
            captures: Vec::new(),
        };
        ast.exprs[eid.0 as usize] = new_expr;
    }
}

fn collect_return_replacements(
    ast: &Ast,
    s: &Stmt,
    renames: &std::collections::HashMap<String, String>,
    out: &mut Vec<(ExprId, String)>,
) {
    match s {
        Stmt::Return(Some(eid)) => {
            if let Expr::Ident(name) = ast.get_expr(*eid)
                && let Some(forward_name) = renames.get(name)
            {
                out.push((*eid, forward_name.clone()));
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_return_replacements(ast, then_branch, renames, out);
            if let Some(eb) = else_branch {
                collect_return_replacements(ast, eb, renames, out);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            collect_return_replacements(ast, body, renames, out);
        }
        Stmt::Labeled { body, .. } => {
            collect_return_replacements(ast, body, renames, out);
        }
        Stmt::For { body, .. } => {
            collect_return_replacements(ast, body, renames, out);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                collect_return_replacements(ast, s, renames, out);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                for s in &c.body {
                    collect_return_replacements(ast, s, renames, out);
                }
            }
            if let Some(d) = default {
                for s in d {
                    collect_return_replacements(ast, s, renames, out);
                }
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for s in body {
                collect_return_replacements(ast, s, renames, out);
            }
            for s in catch_body {
                collect_return_replacements(ast, s, renames, out);
            }
            if let Some(fb) = finally_body {
                for s in fb {
                    collect_return_replacements(ast, s, renames, out);
                }
            }
        }
        _ => {}
    }
}
