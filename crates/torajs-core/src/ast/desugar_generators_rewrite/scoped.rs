//! The lifted-local rewrite inside a function a generator body makes:
//! a closure or a class member. Unlike the generator body itself,
//! whose own declarations are renamed apart before the lift, such a
//! function keeps the names it declares, so which reference reads the
//! generator's local is a question of scope, answered by the
//! free-variable walk (`free_vars::scoped_refs`).

use super::{GEN_SELF_ALIAS, ParamMap, RefBase};
use crate::ast::free_vars::{ScopedRef, scoped_refs};
use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use std::collections::HashSet;

/// The closure `eid` (an arrow, a function expression or an
/// object-literal method) with parameters `params` and body `body`.
/// The parameter defaults are not walked: by the closure lift each is
/// either in the body or pasted into the call sites.
pub(super) fn rewrite_fn_expr(
    ast: &mut Ast,
    eid: ExprId,
    params: &[Param],
    body: &[Stmt],
    pset: &ParamMap,
    visited: &mut HashSet<u32>,
) {
    let mut frame: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
    // §15.5.5: a named function expression binds its own name
    frame.extend(ast.fn_expr_self_names.get(&eid).cloned());
    if ast.fn_expr_exprs.contains(&eid) || ast.objlit_method_exprs.contains(&eid) {
        frame.push("__this".into());
    }
    let refs = scoped_refs(ast, &frame, body, &names(pset));
    rewrite_refs(ast, &refs, pset, visited);
}

/// The class declaration `s`: its heritage and computed keys read the
/// generator's locals in the scope around the class (§15.7.14), its
/// members and static initializers from inside functions whose `this`
/// is the instance or the class. The class's own name is its binding
/// throughout, not the generator field a generator-local class value
/// is also kept in.
///
/// A heritage that is a bare name stays a name: the class lanes
/// resolve the parent class by its binding name
/// (`Ast::parent_ident_name`), and a generator-local class is still
/// declared under that name.
pub(super) fn rewrite_class(ast: &mut Ast, s: &Stmt, pset: &ParamMap, visited: &mut HashSet<u32>) {
    let Stmt::ClassDecl { parent, .. } = s else {
        return;
    };
    let bare_parent = parent.filter(|p| matches!(ast.get_expr(*p), Expr::Ident(_)));
    let mut refs = scoped_refs(ast, &[], std::slice::from_ref(s), &names(pset));
    refs.retain(|r| Some(r.site) != bare_parent);
    rewrite_refs(ast, &refs, pset, visited);
}

fn names(pset: &ParamMap) -> Vec<String> {
    pset.fields.keys().cloned().collect()
}

/// Point each reference at its field. A reference inside a function
/// that binds its own `this` reaches the generator through the
/// `__gself` alias.
fn rewrite_refs(ast: &mut Ast, refs: &[ScopedRef], pset: &ParamMap, visited: &mut HashSet<u32>) {
    for r in refs {
        if !visited.insert(r.site.0) {
            continue;
        }
        let base = match &pset.base {
            RefBase::This if r.own_this => RefBase::SelfAlias,
            b => b.clone(),
        };
        let obj = match base {
            RefBase::This => ast.add_expr(Expr::This),
            RefBase::SelfAlias => ast.add_expr(Expr::Ident(GEN_SELF_ALIAS.into())),
            RefBase::Frame(handle) => ast.add_expr(Expr::Ident(handle)),
        };
        let read = Expr::Member {
            obj,
            name: pset.fields[&r.name].clone(),
        };
        match ast.exprs[r.site.0 as usize].clone() {
            // `new C()` on a lifted local constructs through the value
            // channel, as in the generator body
            Expr::New { args, .. } => {
                let callee = ast.add_expr(read);
                ast.exprs[r.site.0 as usize] = Expr::NewDynamic { callee, args };
            }
            _ => ast.exprs[r.site.0 as usize] = read,
        }
    }
}
