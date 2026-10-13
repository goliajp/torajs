//! The arguments a closure is called with, when every call of it is in
//! sight: one an immutable binding holds and only calls
//! (`const g = (f: F) => …; g(mkA(2))`), or an object literal's method
//! (`const obj = { m(f: F) { … } }; obj.m(mkA(2))`). A parameter census
//! slot of the closure then answers what the values stored there
//! return, as one of a declared function does.

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId, Stmt};

/// The values the calls of `f` pass for its user parameter `i`.
/// `Some(empty)` when `f` is a declaration, called by its name; `None`
/// when one of its calls may be out of sight, as a call of a closure
/// is unless [`binding_args`] or [`method_args`] finds them all.
pub(super) fn closure_args(ast: &Ast, f: &str, i: usize) -> Option<Vec<ExprId>> {
    let Some(site) = (ast.exprs.iter().enumerate())
        .find(|(_, e)| matches!(e, Expr::Closure { fn_name, .. } if fn_name == f))
        .map(|(j, _)| ExprId(j as u32))
    else {
        return Some(Vec::new());
    };
    let decls = literal_decls(ast);
    let lit_of = |e: ExprId| match ast.get_expr(e) {
        Expr::Ident(n) => match decls.get(n).map(Vec::as_slice) {
            Some([Some(l)]) => Some(*l),
            _ => None,
        },
        _ => None,
    };
    binding_args(ast, site, i, &lit_of).or_else(|| method_args(ast, site, i, &lit_of))
}

/// The values the default-parameter carrier `n` of function `f` may
/// hold (`__dflt_h`, the `any` slot a parameter with a default is
/// renamed to) by what the program shows: the defaults stored to it,
/// and the arguments of the calls of `f` in sight (by its name, or of a
/// closure whose calls are all in sight). A call out of sight hands it
/// what the parameter's annotation spells, as it does a parameter with
/// no default. An explicit `undefined` argument takes the default.
pub(crate) fn carrier_values(ast: &Ast, f: &str, n: &str) -> Option<Vec<ExprId>> {
    if !n.starts_with("__dflt_") {
        return None;
    }
    let params = ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl { name, params, .. } if name == f => Some(params),
        _ => None,
    })?;
    let i = (params.iter())
        .filter(|p| !p.name.starts_with("__") || p.name.starts_with("__dflt_"))
        .position(|p| p.name == n)?;
    let named = |e: ExprId, x: &str| matches!(ast.get_expr(e), Expr::Ident(m) if m == x);
    let mut vs = closure_args(ast, f, i).unwrap_or_default();
    for e in &ast.exprs {
        match e {
            Expr::Call { callee, args } if named(*callee, f) => vs.extend(args.get(i).copied()),
            Expr::Assign { target, value } if named(*target, n) => vs.push(*value),
            _ => {}
        }
    }
    vs.retain(|v| !named(*v, "undefined"));
    Some(vs)
}

/// Name → the value each declaration of it binds, when it is an
/// immutable binding of an object literal or a closure: a name declared
/// once that way always reads that one.
fn literal_decls(ast: &Ast) -> HashMap<String, Vec<Option<ExprId>>> {
    let mut decls: HashMap<String, Vec<Option<ExprId>>> = HashMap::new();
    crate::ast::walk_stmts(&ast.stmts, &mut |s| match s {
        Stmt::LetDecl {
            name,
            init,
            mutable,
            ..
        } => {
            let lit = !*mutable
                && matches!(
                    ast.get_expr(*init),
                    Expr::ObjectLit { .. } | Expr::Closure { .. }
                );
            let entry = decls.entry(name.clone()).or_default();
            entry.push(lit.then_some(*init));
        }
        Stmt::FnDecl { name, params, .. } => {
            decls.entry(name.clone()).or_default().push(None);
            for p in params {
                decls.entry(p.name.clone()).or_default().push(None);
            }
        }
        _ => {}
    });
    decls
}

/// The arguments of the calls of the closure `site` an immutable
/// binding holds, when every read of that binding is a call of it.
fn binding_args(
    ast: &Ast,
    site: ExprId,
    i: usize,
    lit_of: &dyn Fn(ExprId) -> Option<ExprId>,
) -> Option<Vec<ExprId>> {
    let mut callees: HashSet<ExprId> = HashSet::new();
    let mut out = Vec::new();
    for e in &ast.exprs {
        if let Expr::Call { callee, args } = e
            && lit_of(*callee) == Some(site)
        {
            callees.insert(*callee);
            out.extend(args.get(i).copied());
        }
    }
    let mut reads = (0..ast.exprs.len() as u32)
        .map(ExprId)
        .filter(|e| lit_of(*e) == Some(site))
        .peekable();
    let held = reads.peek().is_some();
    let escapes = reads.any(|e| !callees.contains(&e));
    (held && !escapes).then_some(out)
}

/// The arguments of the calls of method `site` of an object literal that
/// initializes an immutable binding `b`, when every read of `b` is a
/// member read, and every member read of the method's key is a call
/// through `b` or a read through the binding of another literal.
fn method_args(
    ast: &Ast,
    site: ExprId,
    i: usize,
    lit_of: &dyn Fn(ExprId) -> Option<ExprId>,
) -> Option<Vec<ExprId>> {
    let (lit, key) = ast.exprs.iter().enumerate().find_map(|(j, e)| match e {
        Expr::ObjectLit { fields } => (fields.iter())
            .find(|(_, v)| *v == site)
            .map(|(k, _)| (ExprId(j as u32), k.lossy().into_owned())),
        _ => None,
    })?;
    let mut called: HashSet<ExprId> = HashSet::new();
    let mut out = Vec::new();
    for e in &ast.exprs {
        if let Expr::Call { callee, args } = e
            && let Expr::Member { obj, name } = ast.get_expr(*callee)
            && *name == key
            && lit_of(*obj) == Some(lit)
        {
            called.insert(*callee);
            out.extend(args.get(i).copied());
        }
    }
    let mut receivers: HashSet<ExprId> = HashSet::new();
    for (j, e) in ast.exprs.iter().enumerate() {
        if let Expr::Member { obj, name } | Expr::OptChain { obj, name } = e {
            receivers.insert(*obj);
            let fits = match lit_of(*obj) {
                Some(l) => l != lit || called.contains(&ExprId(j as u32)),
                None => false,
            };
            if *name == key && !fits {
                return None;
            }
        }
    }
    let escapes = (0..ast.exprs.len() as u32)
        .map(ExprId)
        .any(|e| lit_of(e) == Some(lit) && !receivers.contains(&e));
    (!escapes).then_some(out)
}
