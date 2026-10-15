//! An unclaimed function expression minted inside a body that DOES
//! bind `__this`.
//!
//! There the `__this` capture resolves, the checker accepts, and the
//! function body reads the ENCLOSING receiver where §10.2.1.2 binds
//! its own. The answer is still right wherever the receiver a call
//! site hands the value is the enclosing one anyway; this scan refuses
//! only the sites where it provably is not:
//!
//! * `new f()` — §10.2.2 always supplies a fresh object.
//! * `f()` — the call supplies `undefined` (§13.3.6.2 step 2.b), which
//!   is the enclosing receiver exactly when the enclosing function was
//!   itself called plainly. A plain function or a promoted function
//!   expression may well have been, so the call is left alone there;
//!   a class member, a constructor and an object-literal method are
//!   refused, because their receiver is the object they were reached
//!   through. A lifted arrow answers for the function that minted it.
//! * `f.call(t)` / `f.apply(t, …)` — `t` is the receiver. The enclosing
//!   `this` (`__this` after the class desugar) is right everywhere; an
//!   absent or `undefined` thisArg is the plain call above; anything
//!   else is refused.
//!
//! `f` is the function expression inline, or a binding whose only
//! value is one: declared once in the body, never the target of an
//! assignment anywhere, and initialized with the function expression
//! or with another such binding; a lifted arrow sees the bindings it
//! captures from the body that minted it. Two declarations of the name (sibling
//! blocks, `var` twice) or an assignment drop the name — the scan
//! under-fires there and never refuses a program whose capture nothing
//! observes.

use std::collections::HashMap;

use crate::ast::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use crate::ast::fnexpr_this_default::unclaimed_fnexpr;
use crate::ast::fnexpr_this_names::peel_as;
use crate::ast::{Ast, Expr, ExprId, Param, Stmt};

mod held;

use held::{assigned_names, held_of};

/// A binding's minting-body receiver kind, and whether the body
/// reading the binding sees that same `__this` (it reached the body
/// through arrows only).
type Held = (Outer, bool);

/// What the enclosing body's `__this` holds when it was entered.
#[derive(Clone, Copy, PartialEq)]
enum Outer {
    /// The object the member was reached through.
    Object,
    /// Whatever the caller passed — `undefined` under a plain call.
    Passed,
}

/// `true` = some site hands an unclaimed function expression a
/// receiver it would ignore. The receiver the body reads is the one
/// of the function that MINTED the expression, so that is where the
/// verdict looks, whichever body the call site sits in.
pub(super) fn misrouted_receiver(ast: &Ast) -> bool {
    // most programs mint no unclaimed function expression at all
    if !ast
        .fn_expr_exprs
        .iter()
        .any(|&e| unclaimed_fnexpr(ast, e).is_some())
    {
        return false;
    }
    let fns: HashMap<&str, (&[Param], &[Stmt])> = ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl {
                name, params, body, ..
            } => Some((name.as_str(), (params.as_slice(), body.as_slice()))),
            _ => None,
        })
        .collect();
    let mints = mint_sites(ast, &fns);
    let assigned = assigned_names(ast);
    for (&name, &(_, body)) in &fns {
        let outer = outer_of(ast, &fns, &mints, name, 0);
        let held = held_of(ast, &fns, &mints, &assigned, name, 0);
        if outer.is_none() && held.is_empty() {
            continue;
        }
        let mut roots: Vec<ExprId> = Vec::new();
        collect_roots(body, &mut roots);
        while let Some(e) = roots.pop() {
            if misroutes(ast, e, &held, outer) {
                return true;
            }
            roots.extend(expr_children(ast, e));
        }
    }
    false
}

/// For every lifted function: the closure node that mints it and the
/// function whose body holds that node.
fn mint_sites<'a>(
    ast: &'a Ast,
    fns: &HashMap<&'a str, (&'a [Param], &'a [Stmt])>,
) -> HashMap<&'a str, (ExprId, &'a str)> {
    let mut out = HashMap::new();
    for (&owner, &(_, body)) in fns {
        let mut roots: Vec<ExprId> = Vec::new();
        collect_roots(body, &mut roots);
        while let Some(e) = roots.pop() {
            if let Expr::Closure { fn_name, .. } = ast.get_expr(e) {
                out.insert(fn_name.as_str(), (e, owner));
            }
            roots.extend(expr_children(ast, e));
        }
    }
    out
}

/// `None` = the body binds no `__this` of its own or through its
/// capture list.
fn outer_of(
    ast: &Ast,
    fns: &HashMap<&str, (&[Param], &[Stmt])>,
    mints: &HashMap<&str, (ExprId, &str)>,
    name: &str,
    depth: usize,
) -> Option<Outer> {
    let &(params, body) = fns.get(name)?;
    if body.iter().any(super::stmt_declares_this) {
        // the default-binding and sloppy-prologue mints: a receiver
        // the caller passed, or its plain-call stand-in
        return Some(Outer::Passed);
    }
    if params.iter().any(|p| p.name == "__this") {
        let fnexpr = mints.get(name).is_some_and(|&(e, _)| {
            ast.fn_expr_exprs.contains(&e) && !ast.objlit_method_exprs.contains(&e)
        });
        return Some(if ast.this_param_fns.contains(name) || fnexpr {
            Outer::Passed
        } else {
            Outer::Object
        });
    }
    let env_this = params.first().is_some_and(|p| {
        p.name == "__env" && p.type_ann.as_deref().is_some_and(super::env_ann_has_this)
    });
    if !env_this || depth > 64 {
        return None;
    }
    let &(_, minter) = mints.get(name)?;
    outer_of(ast, fns, mints, minter, depth + 1)
}

fn collect_roots(stmts: &[Stmt], out: &mut Vec<ExprId>) {
    for s in stmts {
        if matches!(s, Stmt::FnDecl { .. }) {
            continue;
        }
        out.extend(stmt_exprs(s));
        for c in stmt_children_ref(s) {
            collect_roots(std::slice::from_ref(c), out);
        }
    }
}

/// `e` hands an unclaimed function expression — inline, or through a
/// binding in `held` — a receiver other than the one its body reads.
fn misroutes(ast: &Ast, e: ExprId, held: &HashMap<&str, Held>, outer: Option<Outer>) -> bool {
    let target = |callee: ExprId| {
        let c = peel_as(&ast.exprs, callee);
        match ast.get_expr(c) {
            Expr::Ident(n) => held.get(n.as_str()).copied(),
            _ => outer
                .filter(|_| unclaimed_fnexpr(ast, c).is_some())
                .map(|k| (k, true)),
        }
    };
    match ast.get_expr(e) {
        Expr::NewDynamic { callee, .. } => target(*callee).is_some(),
        Expr::Call { callee, args } => match ast.get_expr(*callee) {
            Expr::Member { obj, name } if name == "call" || name == "apply" => {
                let this_arg = args.first().map(|&a| ast.get_expr(peel_as(&ast.exprs, a)));
                let Some((kind, same_this)) = target(*obj) else {
                    return false;
                };
                match this_arg {
                    // the enclosing `this`: the one the body reads,
                    // unless a function with its own receiver sits
                    // between the mint and the call
                    Some(Expr::Ident(n)) if n == "__this" => !same_this,
                    None => kind == Outer::Object,
                    Some(Expr::Ident(n)) if n == "undefined" => kind == Outer::Object,
                    Some(_) => true,
                }
            }
            _ => target(*callee).is_some_and(|(k, _)| k == Outer::Object),
        },
        _ => false,
    }
}
