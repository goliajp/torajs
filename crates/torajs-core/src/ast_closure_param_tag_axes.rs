//! Usage-axis seeds for [`crate::ast_closure_param_tag`] — marking
//! rounds that originate from HOW an `__fn(`-annotated param is
//! consumed, rather than from the value shape reaching it at a call
//! site.
//!
//! chunk 631 — replace-cb axis: `s.replace(pat, cb)` /
//! `s.replaceAll(pat, cb)` where `cb` names a fn-typed param. The
//! functional-replaceValue runtime lane invokes its callback through
//! the closure boxed entry (+32); a bare FnSig has neither env nor
//! boxed entry, so the param must carry the env-first closure shape
//! (chunk 617 residual — the AST forwarder wrap there only covers
//! bare top-FnDecl Ident arguments, not FnSig-typed params).
//!
//! Attribution is per-name program-wide (no scope precision), the
//! same approximation as the parent pass's closure-ident collection:
//! a name collision over-marks a param, costing that one cold param
//! its direct-dispatch ABI; correctness is preserved by the forwarder
//! wrap at its call sites.

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use std::collections::{HashMap, HashSet};

/// Seed marks for fn-typed params whose name appears as the
/// functional replaceValue of a `replace` / `replaceAll` member call
/// anywhere in the program.
pub(crate) fn replace_cb_param_seeds(
    ast: &Ast,
    fn_params: &HashMap<String, Vec<(usize, String)>>,
) -> HashSet<(String, usize)> {
    let mut cb_names: HashSet<&str> = HashSet::new();
    for e in &ast.exprs {
        if let Expr::Call { callee, args } = e
            && let Expr::Member { name, .. } = ast.get_expr(*callee)
            && matches!(name.as_str(), "replace" | "replaceAll")
            && args.len() >= 2
            && let Expr::Ident(n) = ast.get_expr(args[1])
        {
            cb_names.insert(n);
        }
    }
    let mut seeds = HashSet::new();
    if cb_names.is_empty() {
        return seeds;
    }
    for (fname, fps) in fn_params {
        for (idx, pname) in fps {
            if cb_names.contains(pname.as_str()) {
                seeds.insert((fname.clone(), *idx));
            }
        }
    }
    seeds
}

/// Namespace-static member values (`Math.sin` handed into a marked
/// slot, or returned through a ret-marked lane) need the forwarder
/// treatment too: the RFC 20260719 dispatcher cell answers the boxed
/// lane only — its `fn_addr` is the RFC B4 loud TypeError — so the
/// `__cls(` typed CallIndirect cannot take the cell directly. The
/// wrap mirrors the named-fn one, except the signature comes from the
/// receiving slot's own `__fn(`/`__cls(` annotation (the ns table
/// records arity, not types) and the forwarder body is a direct
/// member call, which compiles through the typed kernel — no
/// boxed-lane detour on the invoke path. The first slot's signature
/// wins for a member reaching differently-annotated slots — the same
/// per-name approximation grade as the parent pass (plan-state L3b
/// tracks the residue).
pub(crate) fn wrap_ns_static_values(
    ast: &mut Ast,
    fn_params: &HashMap<String, Vec<(usize, String)>>,
    fn_returns: &HashMap<String, Vec<ExprId>>,
    marked: &HashSet<(String, usize)>,
    ret_marked: &HashSet<String>,
    existing_forwarders: &mut HashSet<String>,
) {
    // (arg/return ExprId, namespace, member, slot signature ann).
    let mut sites: Vec<(ExprId, String, String, String)> = Vec::new();
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Ident(fname) = ast.get_expr(*callee) else {
            continue;
        };
        for (idx, _) in fn_params.get(fname).into_iter().flatten() {
            if !marked.contains(&(fname.clone(), *idx)) {
                continue;
            }
            if let Some(arg) = args.get(*idx)
                && let Some((ns, m)) = ns_static_member(ast, *arg)
                && let Some(ann) = param_ann_of(ast, fname, *idx)
            {
                sites.push((*arg, ns, m, ann));
            }
        }
    }
    // name order, not hash order: the synthesized decls land in this order
    let mut returns: Vec<_> = fn_returns.iter().collect();
    returns.sort_by(|a, b| a.0.cmp(b.0));
    for (fname, rets) in returns {
        if !ret_marked.contains(fname) {
            continue;
        }
        for r in rets {
            if let Some((ns, m)) = ns_static_member(ast, *r)
                && let Some(ann) = fn_return_ann_of(ast, fname)
            {
                sites.push((*r, ns, m, ann));
            }
        }
    }
    if sites.is_empty() {
        return;
    }
    for (eid, ns, m, sig_ann) in sites {
        let forward_name = format!("__forward_ns_{ns}_{m}");
        if !existing_forwarders.contains(&forward_name) {
            let Some((param_anns, ret_ann)) = split_sig_ann(&sig_ann) else {
                // Slot ann is not a fn signature — leave the site as
                // it is (the parent pass never marked such a slot).
                continue;
            };
            let mut fwd_params: Vec<Param> = Vec::with_capacity(param_anns.len() + 1);
            fwd_params.push(Param {
                name: "__env".into(),
                type_ann: Some("__env()".to_string()),
                default: None,
                is_rest: false,
            });
            let mut arg_eids: Vec<ExprId> = Vec::with_capacity(param_anns.len());
            for (i, pann) in param_anns.iter().enumerate() {
                let pname = format!("__nsa{i}");
                fwd_params.push(Param {
                    name: pname.clone(),
                    type_ann: Some(pann.clone()),
                    default: None,
                    is_rest: false,
                });
                arg_eids.push(ast.add_expr(Expr::Ident(pname)));
            }
            let ns_eid = ast.add_expr(Expr::Ident(ns.clone()));
            let mem_eid = ast.add_expr(Expr::Member {
                obj: ns_eid,
                name: m.clone(),
            });
            let call_eid = ast.add_expr(Expr::Call {
                callee: mem_eid,
                args: arg_eids,
            });
            ast.stmts.push(Stmt::FnDecl {
                name: forward_name.clone(),
                type_params: Vec::new(),
                params: fwd_params,
                return_type: Some(ret_ann),
                body: vec![Stmt::Return(Some(call_eid))],
                is_generator: false,
                // No user source to point at — toString of a wrapped
                // builtin answers the empty span.
                span: crate::lexer::Span { start: 0, end: 0 },
            });
            existing_forwarders.insert(forward_name.clone());
        }
        ast.exprs[eid.0 as usize] = Expr::Closure {
            fn_name: forward_name,
            captures: Vec::new(),
        };
    }
}

/// `(ns, member)` when `eid` is a namespace-static member read.
fn ns_static_member(ast: &Ast, eid: ExprId) -> Option<(String, String)> {
    if let Expr::Member { obj, name } = ast.get_expr(eid)
        && let Expr::Ident(ns) = ast.get_expr(*obj)
        && torajs_rc::ns_static_id(ns, name) != torajs_rc::NS_STATIC_UNKNOWN
    {
        Some((ns.clone(), name.clone()))
    } else {
        None
    }
}

/// The type annotation of `fname`'s param `idx`, wherever the decl
/// nests.
fn param_ann_of(ast: &Ast, fname: &str, idx: usize) -> Option<String> {
    let mut stack: Vec<&Stmt> = ast.stmts.iter().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::FnDecl { name, params, .. } = s
            && name == fname
        {
            return params.get(idx).and_then(|p| p.type_ann.clone());
        }
        crate::ast_closure_param_tag::push_child_stmts(s, &mut stack);
    }
    None
}

/// The return-type annotation of `fname`, wherever the decl nests.
fn fn_return_ann_of(ast: &Ast, fname: &str) -> Option<String> {
    let mut stack: Vec<&Stmt> = ast.stmts.iter().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::FnDecl {
            name, return_type, ..
        } = s
            && name == fname
        {
            return return_type.clone();
        }
        crate::ast_closure_param_tag::push_child_stmts(s, &mut stack);
    }
    None
}

/// Split `__fn(P1|P2)->(R)` (or its retagged `__cls(` twin) into the
/// param ann strings and the return ann — the string-level twin of
/// `ssa_lower_parse_fn_type::try_parse_fn_type`'s depth-aware walk.
fn split_sig_ann(ann: &str) -> Option<(Vec<String>, String)> {
    let rest = ann.trim_start();
    let rest = rest
        .strip_prefix("__fn(")
        .or_else(|| rest.strip_prefix("__cls("))?;
    let bytes = rest.as_bytes();
    let mut depth: i32 = 1;
    let mut close = None;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close?;
    let params_str = &rest[..close];
    let ret = crate::type_ann_fnsig::ret_of_tail(&rest[close + 1..])?.to_string();
    let mut params: Vec<String> = Vec::new();
    if !params_str.is_empty() {
        let mut d: i32 = 0;
        let mut last = 0usize;
        for (i, &b) in params_str.as_bytes().iter().enumerate() {
            match b {
                b'(' => d += 1,
                b')' => d -= 1,
                b'|' if d == 0 => {
                    params.push(params_str[last..i].to_string());
                    last = i + 1;
                }
                _ => {}
            }
        }
        params.push(params_str[last..].to_string());
    }
    Some((params, ret))
}
