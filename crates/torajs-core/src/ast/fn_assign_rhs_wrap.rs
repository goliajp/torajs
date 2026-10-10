//! A declared function assigned into a binding that holds closure
//! cells.
//!
//! `let k = h; k = inc` — the binding's slot is a closure cell (the
//! escaping-alias wrap in [`super::fn_ident_binding_wrap`] turns the
//! init into one, since the assignment target is a non-callee use of
//! `k`), and so is `let m = (x) => x; m = h` (the init is a lifted
//! arrow). Storing `inc` there has to store the function's canonical
//! cell, so the right-hand side becomes the forwarder closure the
//! other value sites already answer, before the checker runs: the
//! width and closure-flow analyses then see the forwarder flow into
//! the binding exactly as they see an arrow flow into it. Converting
//! at the store in lowering instead leaves them blind, and a shell
//! whose number widths were settled elsewhere is called with the
//! binding's widths.
//!
//! The binding a name resolves to is a scope question this pass does
//! not answer; it answers a stronger one instead. A store wraps only
//! when every declaration of that name anywhere in the module holds a
//! cell, so whichever one the target resolves to takes the cell. A
//! name that also names a parameter, a loop variable or a `let` whose
//! slot no syntax tells (`let t = mk()`) is left as written, and so is
//! a function name some other declaration rebinds. Such a store keeps
//! the bare pointer, and only its functions' shells are minted: the
//! assignment lowering hands over the cell where the slot it resolves
//! is a cell of the same signature, and keeps direct dispatch where
//! the slot is a bare pointer.

use super::{Ast, Expr, ExprId, Stmt, is_fn_like_ann};
use crate::ast_closure_param_tag_member::passthrough_leaves;
use std::collections::{HashMap, HashSet};

/// The stores of declared functions into bindings: the function-name
/// leaves to wrap, as (leaf, function), and the functions whose shell
/// is minted without a wrap. A store qualifies when every value its
/// right-hand side can answer (through `?:`, `||`, `&&`, `??`, `,` and
/// `as`) is a closure or a function `is_fn` accepts; it wraps when the
/// target is a binding every declaration of which holds a cell.
/// `cell_inits` are the initializers the caller is about to wrap into
/// closures.
pub(super) fn closure_slot_store_leaves(
    ast: &Ast,
    is_fn: &dyn Fn(&str) -> bool,
    cell_inits: &HashSet<ExprId>,
) -> (Vec<(ExprId, String)>, HashSet<String>) {
    let decls = binding_decls(ast, cell_inits);
    let mut out = Vec::new();
    let mut shells_only = HashSet::new();
    for e in &ast.exprs {
        let Expr::Assign { target, value } = e else {
            continue;
        };
        let Expr::Ident(t) = ast.get_expr(*target) else {
            continue;
        };
        let mut fns = Vec::new();
        let all_values_fit = passthrough_leaves(ast, *value).into_iter().all(|leaf| {
            match ast.get_expr(leaf) {
                Expr::Closure { .. } => true,
                // the declaration of `g` is its only binding
                Expr::Ident(g)
                    if is_fn(g)
                        && super::fn_ident_binding_wrap::relayable_target(g)
                        && decls.get(g).is_some_and(|d| d.count == 1) =>
                {
                    fns.push((leaf, g.clone()));
                    true
                }
                _ => false,
            }
        });
        if !all_values_fit {
            continue;
        }
        if decls.get(t).is_some_and(|d| d.all_cells) {
            out.extend(fns);
        } else {
            shells_only.extend(fns.into_iter().map(|(_, g)| g));
        }
    }
    (out, shells_only)
}

struct Decls {
    count: usize,
    all_cells: bool,
}

/// Every name a statement binds, with how many declarations bind it
/// and whether each of them holds a closure cell.
fn binding_decls(ast: &Ast, cell_inits: &HashSet<ExprId>) -> HashMap<String, Decls> {
    let mut out: HashMap<String, Decls> = HashMap::new();
    let mut note = |name: &str, cell: bool| {
        let d = out.entry(name.to_string()).or_insert(Decls {
            count: 0,
            all_cells: true,
        });
        d.count += 1;
        d.all_cells &= cell;
    };
    let mut visit = |s: &Stmt| match s {
        Stmt::LetDecl {
            name,
            init,
            type_ann,
            mutable,
            is_var,
            ..
        } => {
            let ann = type_ann.as_deref();
            let cell = *is_var
                || ann == Some("any")
                // a variadic function type keeps the boxed-dual lane,
                // not the closure repr
                || (*mutable && ann.is_some_and(|a| is_fn_like_ann(a) && !a.contains("__rest(")))
                || (ann.is_none()
                    && (cell_inits.contains(init)
                        || matches!(ast.get_expr(*init), Expr::Closure { .. })));
            note(name, cell);
        }
        Stmt::FnDecl { name, params, .. } => {
            note(name, false);
            for p in params {
                note(&p.name, false);
            }
        }
        Stmt::ForOf { var_name, .. } | Stmt::ForOfSplitIter { var_name, .. } => {
            note(var_name, false)
        }
        Stmt::UsingDecl { name, .. } | Stmt::ClassDecl { name, .. } => note(name, false),
        Stmt::Try {
            catch_param: Some(p),
            ..
        } => note(p, false),
        Stmt::YieldInto { var, .. } => note(var, false),
        Stmt::ImportDecl {
            default,
            namespace,
            named,
            ..
        } => {
            for n in default.iter().chain(namespace) {
                note(n, false);
            }
            for (imported, local) in named {
                note(local.as_ref().unwrap_or(imported), false);
            }
        }
        _ => {}
    };
    super::ns_alias::walk_stmts(&ast.stmts, &mut visit);
    // the walk does not enter an export's declaration
    for s in &ast.stmts {
        if let Stmt::ExportDecl {
            inner: Some(inner), ..
        } = s
        {
            super::ns_alias::walk_stmts(std::slice::from_ref(inner.as_ref()), &mut visit);
        }
    }
    out
}
