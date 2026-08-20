//! RFC 20260808 escape-store profile — the argv-face fns whose
//! binding is stored into a boxed-face position (the fnexpr-this B2
//! store roots). Split from `arguments_object_collect.rs` under the
//! 500-line file rule. The main pass evicts these fns from the
//! STATIC face (the store implies a runtime call the AST cannot
//! see); the kill walk in the collect sibling shares
//! [`boxed_face_store_target`] so a store into these positions does
//! not kill the binding chain.

use super::{Ast, Expr, ExprId, Stmt};

/// RFC 20260808 escape-store profile — of the argv-face fns, the
/// ones whose binding is stored into a boxed-face position. The
/// caller evicts these from the STATIC face: a store implies a
/// runtime call the AST cannot see, so the static fold's
/// direct-site argc (usually zero — a store-only escape has no
/// direct sites) would materialize an empty `arguments` where the
/// boxed dual entry delivers the true one.
pub(super) fn collect_escape_stored(
    ast: &Ast,
    argv_fns: &std::collections::HashSet<String>,
    argv_locals: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    let props_recvs =
        super::fnexpr_this_recvs::collect_props_receiver_binding_names(&ast.stmts, &ast.exprs);
    let expando_recvs = super::fnexpr_this_expando::ExpandoRecvs::scan(&ast.stmts, &ast.exprs);
    let mut stored: std::collections::HashSet<String> = std::collections::HashSet::new();
    for e in &ast.exprs {
        if let Expr::Assign { target, value } = e
            && let Expr::Ident(b) = ast.get_expr(*value)
            && argv_locals.contains(b)
            && boxed_face_store_target(ast, *target, &props_recvs, &expando_recvs)
        {
            // Resolve the binding back to its fn through the direct
            // LetDecl seed (the chain walk's own seeding shape).
            for s in &ast.stmts {
                if let Stmt::LetDecl { name, init, .. } = s
                    && name == b
                    && let Expr::Closure { fn_name, .. } = ast.get_expr(*init)
                    && argv_fns.contains(fn_name)
                {
                    stored.insert(fn_name.clone());
                }
            }
        }
    }
    stored
}

/// Rotation 345 — the ARGUMENT-POSITION variant of the escape-store
/// profile: of the argv-face fns, the ones whose binding stands as
/// an argument to an explicit-`any` / proven-generic param or to a
/// construct-channel builtin (`Array.from.call(C, …)` family). Both
/// channels reach the fn's calls through the boxed dual entry with
/// REAL argc/argv, which the static fold cannot see — the caller
/// evicts these from the STATIC face like the store profile above.
pub(super) fn collect_escape_arg_positions(
    ast: &Ast,
    argv_fns: &std::collections::HashSet<String>,
    argv_locals: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    let mut arg_sites = super::fnexpr_this_args::any_param_arg_idents(&ast.stmts, &ast.exprs);
    arg_sites.extend(super::fnexpr_this_args::construct_channel_arg_idents(
        &ast.exprs,
    ));
    let mut out: std::collections::HashSet<String> = std::collections::HashSet::new();
    for b in argv_locals {
        let escapes = ast.exprs.iter().enumerate().any(|(i, e)| {
            matches!(e, Expr::Ident(n) if n == b) && arg_sites.contains(&ExprId(i as u32))
        });
        if !escapes {
            continue;
        }
        for s in &ast.stmts {
            if let Stmt::LetDecl { name, init, .. } = s
                && name == b
                && let Expr::Closure { fn_name, .. } = ast.get_expr(*init)
                && argv_fns.contains(fn_name)
            {
                out.insert(fn_name.clone());
            }
        }
    }
    out
}

/// The store-position admit the fnexpr-this store arm defined (B2,
/// rotation 337; species key 2 merged the receiver predicate):
/// `<props>.k` / `<props>[k]` / `X.prototype.k` /
/// `<props>.constructor[k]` — every consumption path from these
/// slots enters the boxed dual entry.
pub(super) fn boxed_face_store_target(
    ast: &Ast,
    target: ExprId,
    props_recvs: &std::collections::HashSet<String>,
    expando_recvs: &super::fnexpr_this_expando::ExpandoRecvs,
) -> bool {
    // §27.2.4 static-slot patch (rotation 449) — `Promise.resolve =
    // function () { …arguments… }` / `.reject`: the store lands in
    // the interned ctor cell's expando dict, and BOTH read-back
    // channels (the any method dispatch and the combinators'
    // patch-consult `invoke_with_this`) enter the closure cell's
    // boxed dual entry — real argc/argv, exactly this profile's
    // consumption claim. Only the two CONSULTED names admit (the
    // checker's write-face bar), and only while nothing shadows the
    // builtin name — the fnexpr-this store arm's own gate.
    if let Expr::Member { obj, name } = ast.get_expr(target)
        && matches!(
            ast.get_expr(super::fnexpr_this_names::peel_as(&ast.exprs, *obj)),
            Expr::Ident(n) if n == "Promise"
        )
        && matches!(name.as_str(), "resolve" | "reject")
        && !super::fnexpr_this_names::name_shadowed_elsewhere(&ast.stmts, "Promise")
    {
        return true;
    }
    // Rotation 460 — the expando store the fnexpr-this arm admits
    // alongside these: a key the receiver's object literal never
    // declared has no typed slot to land in, so the read comes back a
    // NaN box and the call enters the boxed dual entry with real
    // argc/argv. The two admit sets have to move together — this
    // file's doc says so, and the first cut that widened only the
    // fnexpr side turned `o.f = function () { saved = arguments }`
    // into `unknown identifier `arguments``: the kill walk saw a
    // store it did not recognize and killed the argv face.
    if expando_recvs.admits(&ast.exprs, target) {
        return true;
    }
    let store_recv = match ast.get_expr(target) {
        Expr::Member { obj, .. } => Some(*obj),
        Expr::Index { obj, .. } => Some(*obj),
        _ => None,
    };
    store_recv.is_some_and(|obj| match ast.get_expr(obj) {
        Expr::Ident(n) => props_recvs.contains(n),
        Expr::Member {
            obj: pobj,
            name: pname,
        } => {
            (pname == "prototype"
                && matches!(ast.get_expr(*pobj), Expr::Ident(_) | Expr::Closure { .. }))
                || (pname == "constructor"
                    && matches!(ast.get_expr(*pobj), Expr::Ident(n)
                        if props_recvs.contains(n)))
        }
        _ => false,
    })
}
