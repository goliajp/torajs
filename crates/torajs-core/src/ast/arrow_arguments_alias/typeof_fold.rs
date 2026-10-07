//! `typeof arguments` in a non-arrow function's own scope is always
//! "object": §10.2.11 step 22 creates the arguments object for every
//! such function whose body or parameters name it, and §13.5.3 answers
//! "object" for an ordinary object. Folding it here keeps the read out
//! of the arguments pass entirely, so a body whose only other touch is
//! `arguments.length` stays on the length-only tiers instead of falling
//! to one that wants the argument values, or to none at all, where the
//! raw identifier reaches the undeclared-name typeof and answers
//! "undefined".
//!
//! The fold stands aside when the scope can rebind the name: a local
//! declaration of `arguments` (a `let`, a `var`, a block function, a
//! catch parameter) or a bare `arguments = v` / `arguments++` in sloppy
//! code. A parameter named `arguments` never reaches here (that scope
//! walks in `Mode::Shadowed`).

use super::super::ns_alias::walk_stmts;
use super::{Ast, Expr, ExprId, Hits, Mode, Stmt};

/// Record `eid` when it is a `typeof arguments` of the scope being
/// walked itself (not of an arrow inside it), for [`fold_own_typeofs`].
pub(super) fn take_own_typeof(ast: &Ast, eid: ExprId, mode: Mode, hits: &mut Hits) -> bool {
    let own = mode == Mode::Outer && is_typeof_arguments(ast, eid);
    if own {
        hits.typeofs.push(eid);
    }
    own
}

/// `typeof arguments`, through any `as` casts on the operand.
fn is_typeof_arguments(ast: &Ast, eid: ExprId) -> bool {
    let Expr::TypeOf { expr } = ast.get_expr(eid) else {
        return false;
    };
    let mut cur = *expr;
    while let Expr::As { expr, .. } = ast.get_expr(cur) {
        cur = *expr;
    }
    matches!(ast.get_expr(cur), Expr::Ident(n) if n == "arguments")
}

/// Rewrite the scope's own `typeof arguments` nodes to "object" unless
/// the scope can rebind the name.
pub(super) fn fold_own_typeofs(ast: &mut Ast, body: &[Stmt], hits: &Hits) {
    if hits.typeofs.is_empty() || !hits.rebinds.is_empty() || scope_may_rebind(ast, body) {
        return;
    }
    for &eid in &hits.typeofs {
        ast.exprs[eid.0 as usize] = Expr::String("object".into());
    }
}

fn scope_may_rebind(ast: &Ast, body: &[Stmt]) -> bool {
    let mut names = std::collections::HashSet::new();
    crate::ast_collect_bindings::collect_local_binding_names(body, &mut names);
    if names.contains("arguments") {
        return true;
    }
    let mut fn_named = false;
    walk_stmts(body, &mut |s| {
        fn_named |= matches!(s, Stmt::FnDecl { name, .. } if name == "arguments");
    });
    fn_named || super::super::arguments_object_walkers::body_has_bare_arguments_assign(ast, body)
}
