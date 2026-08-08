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
    let any_recvs = super::fnexpr_this_recvs::collect_any_binding_names(&ast.stmts, &ast.exprs);
    let array_lenient_recvs =
        super::fnexpr_this_recvs::collect_array_binding_names_lenient(&ast.stmts, &ast.exprs);
    let mut stored: std::collections::HashSet<String> = std::collections::HashSet::new();
    for e in &ast.exprs {
        if let Expr::Assign { target, value } = e
            && let Expr::Ident(b) = ast.get_expr(*value)
            && argv_locals.contains(b)
            && boxed_face_store_target(ast, *target, &any_recvs, &array_lenient_recvs)
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

/// The store-position admit the fnexpr-this store arm defined (B2,
/// rotation 337): `<any>.k` / `<any>[k]` / `X.prototype.k` /
/// `<any|array>.constructor[k]` — every consumption path from these
/// slots enters the boxed dual entry.
pub(super) fn boxed_face_store_target(
    ast: &Ast,
    target: ExprId,
    any_recvs: &std::collections::HashSet<String>,
    array_lenient_recvs: &std::collections::HashSet<String>,
) -> bool {
    let store_recv = match ast.get_expr(target) {
        Expr::Member { obj, .. } => Some(*obj),
        Expr::Index { obj, .. } => Some(*obj),
        _ => None,
    };
    store_recv.is_some_and(|obj| match ast.get_expr(obj) {
        Expr::Ident(n) => any_recvs.contains(n),
        Expr::Member {
            obj: pobj,
            name: pname,
        } => {
            (pname == "prototype"
                && matches!(ast.get_expr(*pobj), Expr::Ident(_) | Expr::Closure { .. }))
                || (pname == "constructor"
                    && matches!(ast.get_expr(*pobj), Expr::Ident(n)
                        if any_recvs.contains(n) || array_lenient_recvs.contains(n)))
        }
        _ => false,
    })
}
