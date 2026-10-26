//! The closure half of the machine-body temporal dead zone
//! (`desugar_generators_tdz`, whose module doc carries the design): a
//! binding read from a function value written BEFORE its declaration
//! gets a lifted initialised-flag, and every such read or write goes
//! through it.

use std::collections::HashSet;

use super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::tdz_reads::collect_bound_names;
use super::{Ast, Expr, ExprId, Param, Stmt};

mod rewrite;
pub(super) use rewrite::rewrite_site;

/// Entry — settle the closure reads of every binding `body` declares.
pub(super) fn flag_closure_reads(ast: &mut Ast, body: &mut Vec<Stmt>) {
    let mut counter = 0usize;
    flag_list(ast, body, Vec::new(), &mut counter);
}

/// One occurrence of the binding's name inside a closure written
/// before its declaration.
pub(super) enum Site {
    Read(ExprId),
    Write {
        assign: ExprId,
        value: ExprId,
    },
    Incr {
        node: ExprId,
        is_inc: bool,
        is_prefix: bool,
    },
}

/// Settle every `let` / `const` of one statement list, then of the
/// lists nested in it. `Multi` shares the surrounding scope and is
/// spliced in first so the list's positions are its own. `extra` holds
/// the closure sites an ENCLOSING list already found for a name this
/// list declares — the `using` wrapper's case, see [`using_try_body`].
fn flag_list(
    ast: &mut Ast,
    stmts: &mut Vec<Stmt>,
    mut extra: Vec<(String, Vec<Site>)>,
    counter: &mut usize,
) {
    splice_multi(stmts);
    let mut flagged: Vec<(usize, String)> = Vec::new();
    for j in 0..stmts.len() {
        let Stmt::LetDecl {
            name,
            init,
            is_var: false,
            ..
        } = &stmts[j]
        else {
            continue;
        };
        if name.starts_with("__") && !name.starts_with("__awd_") {
            continue;
        }
        let name = name.clone();
        let init = *init;
        let mut sites: Vec<Site> = match extra.iter().position(|(n, _)| *n == name) {
            Some(k) => extra.swap_remove(k).1,
            None => Vec::new(),
        };
        for s in &stmts[..j] {
            collect_sites_stmt(ast, s, &name, false, true, &mut sites);
        }
        collect_sites_expr(ast, init, &name, false, true, &mut sites);
        if sites.is_empty() {
            continue;
        }
        *counter += 1;
        let flag = format!("__tdz{}_{name}", *counter);
        for site in sites {
            rewrite_site(ast, site, &name, &flag);
        }
        flagged.push((j, flag));
    }
    // Back to front, so earlier indices stay valid; the flag
    // declarations go to the head last.
    for (j, flag) in flagged.iter().rev() {
        let target = ast.add_expr(Expr::Ident(flag.clone()));
        let value = ast.add_expr(Expr::Bool(true));
        let set = ast.add_expr(Expr::Assign { target, value });
        stmts.insert(j + 1, Stmt::Expr(set));
    }
    for (_, flag) in flagged.iter().rev() {
        let init = ast.add_expr(Expr::Bool(false));
        stmts.insert(
            0,
            Stmt::LetDecl {
                mutable: true,
                name: flag.clone(),
                type_ann: Some("boolean".into()),
                init,
                is_var: false,
            },
        );
    }
    for j in 0..stmts.len() {
        if using_try_body(&stmts[j]).is_some() {
            // The wrapper's body continues THIS list: a closure written
            // before the wrapper and reading a binding declared inside
            // it is the same shape as one written before the binding.
            if let Stmt::Try { body, .. } = &mut stmts[j] {
                splice_multi(body);
            }
            let names: Vec<String> = using_try_body(&stmts[j])
                .into_iter()
                .flatten()
                .filter_map(|s| match s {
                    Stmt::LetDecl {
                        name,
                        is_var: false,
                        ..
                    } if !name.starts_with("__") => Some(name.clone()),
                    _ => None,
                })
                .collect();
            let carried: Vec<(String, Vec<Site>)> = names
                .into_iter()
                .map(|name| {
                    let mut sites = Vec::new();
                    for s in &stmts[..j] {
                        collect_sites_stmt(ast, s, &name, false, true, &mut sites);
                    }
                    (name, sites)
                })
                .filter(|(_, sites)| !sites.is_empty())
                .collect();
            if let Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } = &mut stmts[j]
            {
                flag_list(ast, body, carried, counter);
                flag_list(ast, catch_body, Vec::new(), counter);
                if let Some(f) = finally_body {
                    flag_list(ast, f, Vec::new(), counter);
                }
            }
            continue;
        }
        flag_nested(ast, &mut stmts[j], counter);
    }
}

/// The body of `desugar_using`'s wrapper — `try { <rest of the list> }
/// catch (__usc_N) { … } finally { … }` — which is not a scope of its
/// own: the parser saw one list, and the declarations inside the
/// wrapper were written after the statements before it.
fn using_try_body(s: &Stmt) -> Option<&Vec<Stmt>> {
    match s {
        Stmt::Try {
            body,
            catch_param: Some(p),
            ..
        } if p.starts_with("__usc_") => Some(body),
        _ => None,
    }
}

/// Every statement list nested in `s` that shares this machine's
/// lift — a nested function's body is its own scope and keeps its
/// own lowering's dead zone.
fn flag_nested(ast: &mut Ast, s: &mut Stmt, counter: &mut usize) {
    match s {
        Stmt::FnDecl { .. } => {}
        Stmt::Block(v) => flag_list(ast, v, Vec::new(), counter),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            flag_list(ast, body, Vec::new(), counter);
            flag_list(ast, catch_body, Vec::new(), counter);
            if let Some(f) = finally_body {
                flag_list(ast, f, Vec::new(), counter);
            }
        }
        other => {
            for c in super::desugar_with::walk::stmt_children(other) {
                flag_nested(ast, c, counter);
            }
        }
    }
}

fn splice_multi(stmts: &mut Vec<Stmt>) {
    if !stmts.iter().any(|s| matches!(s, Stmt::Multi(_))) {
        return;
    }
    let mut out: Vec<Stmt> = Vec::with_capacity(stmts.len());
    for s in stmts.drain(..) {
        match s {
            Stmt::Multi(mut inner) => {
                splice_multi(&mut inner);
                out.extend(inner);
            }
            other => out.push(other),
        }
    }
    *stmts = out;
}

fn params_bind(params: &[Param], name: &str) -> bool {
    params.iter().any(|p| p.name == name)
}

/// Does a closure rebind `name` — a parameter, or any declaration in
/// its body at any depth? Such a closure is not entered.
fn closure_shadows(params: &[Param], body: &[Stmt], name: &str) -> bool {
    if params_bind(params, name) {
        return true;
    }
    let mut bound: HashSet<&str> = HashSet::new();
    for s in body {
        collect_bound_names(s, &mut bound);
        if let Stmt::FnDecl { name: n, .. } = s
            && n == name
        {
            return true;
        }
    }
    bound.contains(name)
}

/// `outer_fn_decls` — whether a `function` declaration standing in the
/// scanned list itself (not inside a closure) is entered. The machine
/// pass enters it; `closure_tdz_flag` does not (see its module doc).
pub(super) fn collect_sites_stmt(
    ast: &Ast,
    s: &Stmt,
    name: &str,
    inside: bool,
    outer_fn_decls: bool,
    out: &mut Vec<Site>,
) {
    if let Stmt::FnDecl { params, body, .. } = s {
        if (!inside && !outer_fn_decls) || params_bind(params, name) {
            return;
        }
        collect_default_sites(ast, params, name, outer_fn_decls, out);
        if closure_shadows(params, body, name) {
            return;
        }
        for b in body {
            collect_sites_stmt(ast, b, name, true, outer_fn_decls, out);
        }
        return;
    }
    for root in stmt_exprs(s) {
        collect_sites_expr(ast, root, name, inside, outer_fn_decls, out);
    }
    for c in stmt_children_ref(s) {
        collect_sites_stmt(ast, c, name, inside, outer_fn_decls, out);
    }
}

/// A parameter default runs when the function is called, as its body
/// does (§10.2.11), so its reads are the function's own.
fn collect_default_sites(
    ast: &Ast,
    params: &[Param],
    name: &str,
    outer_fn_decls: bool,
    out: &mut Vec<Site>,
) {
    for d in params.iter().filter_map(|p| p.default) {
        collect_sites_expr(ast, d, name, true, outer_fn_decls, out);
    }
}

pub(super) fn collect_sites_expr(
    ast: &Ast,
    eid: ExprId,
    name: &str,
    inside: bool,
    outer_fn_decls: bool,
    out: &mut Vec<Site>,
) {
    match ast.get_expr(eid) {
        Expr::Ident(n) => {
            if inside && n == name {
                out.push(Site::Read(eid));
            }
        }
        Expr::ArrowFn { params, body, .. } => {
            if params_bind(params, name) {
                return;
            }
            // the defaults cannot see the body's declarations
            collect_default_sites(ast, params, name, outer_fn_decls, out);
            if closure_shadows(params, body, name) {
                return;
            }
            for b in body {
                collect_sites_stmt(ast, b, name, true, outer_fn_decls, out);
            }
        }
        Expr::Assign { target, value } => {
            if inside && matches!(ast.get_expr(*target), Expr::Ident(n) if n == name) {
                out.push(Site::Write {
                    assign: eid,
                    value: *value,
                });
            } else {
                collect_sites_expr(ast, *target, name, inside, outer_fn_decls, out);
            }
            collect_sites_expr(ast, *value, name, inside, outer_fn_decls, out);
        }
        Expr::PostIncr {
            target,
            is_inc,
            is_prefix,
        } => {
            if inside && matches!(ast.get_expr(*target), Expr::Ident(n) if n == name) {
                out.push(Site::Incr {
                    node: eid,
                    is_inc: *is_inc,
                    is_prefix: *is_prefix,
                });
            } else {
                collect_sites_expr(ast, *target, name, inside, outer_fn_decls, out);
            }
        }
        _ => {
            for c in expr_children(ast, eid) {
                collect_sites_expr(ast, c, name, inside, outer_fn_decls, out);
            }
        }
    }
}
