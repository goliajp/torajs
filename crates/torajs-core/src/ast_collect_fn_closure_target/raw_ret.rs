//! The returns that hand back a bare function value of static origin:
//! a top-level function, directly or through a const binding of it, or
//! one of the function's own bare-pointer fn-typed params. A return
//! through the closure lane has to hand back that function's fn object
//! instead, so the forwarders those objects run are minted here, before
//! the check, wherever the lane may be taken (a function or vtable slot
//! whose returns mix such values with others), and
//! `check_monomorph::any_ret` rewrites the returns once it is.

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};

/// What a return hands back when its value is a bare function of
/// static origin.
pub(crate) enum RawRet {
    /// A top-level function, named directly or through a const binding.
    Named(String),
    /// The function's own bare-pointer fn-typed param, by index.
    Param(usize),
}

/// A body's const bindings by name, not counting those of a function
/// nested in it.
pub(crate) fn const_lets(body: &[Stmt]) -> HashMap<&str, ExprId> {
    let mut out = HashMap::new();
    let mut stack: Vec<&Stmt> = body.iter().collect();
    while let Some(s) = stack.pop() {
        match s {
            Stmt::LetDecl {
                mutable: false,
                name,
                init,
                ..
            } => {
                out.insert(name.as_str(), *init);
            }
            Stmt::FnDecl { .. } => {}
            _ => crate::ast_closure_param_tag::push_child_stmts(s, &mut stack),
        }
    }
    out
}

/// The [`RawRet`] a return value `e` is, if any. `is_fn` answers
/// whether a name is a top-level function.
pub(crate) fn raw_ret(
    ast: &Ast,
    params: &[Param],
    lets: &HashMap<&str, ExprId>,
    e: ExprId,
    is_fn: &dyn Fn(&str) -> bool,
) -> Option<RawRet> {
    let mut e = e;
    while let Expr::As { expr, .. } = ast.get_expr(e) {
        e = *expr;
    }
    let Expr::Ident(n) = ast.get_expr(e) else {
        return None;
    };
    if let Some(i) = params.iter().position(|p| &p.name == n) {
        let bare = crate::ast_closure_param_tag::is_fnsig_ann(&params[i].type_ann);
        return bare.then_some(RawRet::Param(i));
    }
    if let Some(&init) = lets.get(n.as_str()) {
        return match ast.get_expr(init) {
            Expr::Ident(o)
                if is_fn(o)
                    && !lets.contains_key(o.as_str())
                    && !params.iter().any(|p| &p.name == o) =>
            {
                Some(RawRet::Named(o.clone()))
            }
            _ => None,
        };
    }
    is_fn(n).then(|| RawRet::Named(n.clone()))
}

/// The raw returns of one declaration, and how many returns it has.
pub(crate) fn decl_raw_rets(
    ast: &Ast,
    params: &[Param],
    body: &[Stmt],
    is_fn: &dyn Fn(&str) -> bool,
) -> (Vec<RawRet>, usize) {
    let rets = super::fn_returns(body);
    let lets = const_lets(body);
    let raw = rets
        .iter()
        .filter_map(|&r| raw_ret(ast, params, &lets, r, is_fn))
        .collect();
    (raw, rets.len())
}

/// The top-level functions handed, at some call site, to a bare fn
/// param that a function or slot row whose returns mix (see the
/// module doc) returns: once that return takes the closure lane, the
/// param takes the closure repr and each such argument converts to its
/// fn object at the call (`ssa_lower_call_fn_cell`), so its forwarder
/// has to exist.
pub(crate) fn param_arg_targets(
    ast: &Ast,
    fn_sigs: &HashMap<String, super::FnSig>,
    mixed_rows: &HashSet<String>,
) -> HashSet<String> {
    let is_fn = |n: &str| fn_sigs.contains_key(n);
    let mut idxs: HashMap<String, HashSet<usize>> = HashMap::new();
    for s in crate::ast::toplevel_stmts_flat(ast) {
        let Stmt::FnDecl {
            name,
            params,
            return_type,
            body,
            ..
        } = s
        else {
            continue;
        };
        if !return_type
            .as_deref()
            .is_some_and(crate::ast::is_fn_like_ann)
        {
            continue;
        }
        let (raw, n) = decl_raw_rets(ast, params, body, &is_fn);
        let ps: HashSet<usize> = raw
            .iter()
            .filter_map(|r| match r {
                RawRet::Param(i) => Some(*i),
                RawRet::Named(_) => None,
            })
            .collect();
        if !ps.is_empty() && (raw.len() < n || mixed_rows.contains(name)) {
            idxs.entry(name.clone()).or_default().extend(ps);
        }
    }
    // a call through the slot reaches every row: its stubs and rows
    // take the indices of any row
    let names: Vec<String> = crate::ast::toplevel_stmts_flat(ast)
        .into_iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    for (m, by_root) in crate::ast::vtable_slot::slot_groups_by_name(ast) {
        let mut all: HashSet<usize> = HashSet::new();
        for rows in by_root.values() {
            let union: HashSet<usize> = rows
                .iter()
                .filter_map(|r| idxs.get(r.as_str()))
                .flatten()
                .copied()
                .collect();
            for r in rows.iter().filter(|_| !union.is_empty()) {
                idxs.entry(r.clone())
                    .or_default()
                    .extend(union.iter().copied());
            }
            all.extend(union);
        }
        if all.is_empty() {
            continue;
        }
        for stub in crate::ast::vtable_slot::dispatch_stub_names(names.iter(), &m) {
            idxs.entry(stub).or_default().extend(all.iter().copied());
        }
    }
    let mut out = HashSet::new();
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Ident(c) = ast.get_expr(*callee) else {
            continue;
        };
        for &i in idxs.get(c).into_iter().flatten() {
            let Some(&arg) = args.get(i) else {
                continue;
            };
            for leaf in crate::ast_closure_param_tag_member::passthrough_leaves(ast, arg) {
                if let Expr::Ident(n) = ast.get_expr(leaf)
                    && is_fn(n)
                {
                    out.insert(n.clone());
                }
            }
        }
    }
    out
}
