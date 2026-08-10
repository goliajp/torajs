//! Closure-shape probe for the M3 monomorphizer (rotation 44) —
//! sibling of [`crate::ssa_lower_generics_monomorph`], split out to
//! keep that file under the 500-line limit.
//!
//! `check::Type::Function` carries no closure-vs-bare-fn
//! distinction, so `type_to_ann` always answers `__fn(`; these
//! helpers walk the generic call site's arg positions (the same
//! param-position walk as `num_width::compute_typevar_widths`) so a
//! closure arg's type-param instantiates as `__cls(` instead.

use std::collections::HashMap;

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use crate::check as check_mod;

/// Per type-param: does the call-site arg instantiating it hold a
/// CLOSURE value (env-block ptr) rather than a bare fn ptr? Mirrors
/// `compute_typevar_widths`' param-position walk. Precise on the two
/// common shapes — a closure literal and an ident whose top-level
/// binding init is a closure literal; anything else answers false
/// (the `__fn(` status quo — a mis-shape in either direction jumps
/// through the wrong ABI, so unknown shapes keep today's behavior
/// rather than guessing).
/// What kind of callable a type-param's instantiating arg holds.
/// (The `__clsargc(` third state retired in RFC
/// 20260810-indirect-argc-abi S3.6 — an argc-reading closure body
/// rides the S1 hidden argc every env-first call path feeds, so the
/// mono track no longer needs an argc-carrying spelling.)
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClsShape {
    /// Not statically a closure — keep the `__fn(` status quo.
    NotClosure,
    /// A closure value (env-block ptr) — flip to `__cls(`.
    Closure,
}

pub(crate) fn compute_typevar_closure_shapes(
    ast: &Ast,
    call_eid: ExprId,
    _callee_name: &str,
    type_args: &[check_mod::Type],
    generics: &HashMap<String, (Vec<String>, Vec<Param>, Option<String>, Vec<Stmt>)>,
    param_env: &HashMap<String, String>,
) -> Vec<ClsShape> {
    let arg_eids: Vec<ExprId> = match ast.get_expr(call_eid) {
        Expr::Call { args, .. } => args.clone(),
        _ => Vec::new(),
    };
    let Some((tp_names, gen_params, _, _)) = generics.get(_callee_name) else {
        return vec![ClsShape::NotClosure; type_args.len()];
    };
    tp_names
        .iter()
        .map(|tp_name| {
            for (pi, p) in gen_params.iter().enumerate() {
                let Some(ann) = &p.type_ann else { continue };
                if ann == tp_name
                    && let Some(arg_eid) = arg_eids.get(pi)
                {
                    let shape = arg_cls_shape(ast, *arg_eid, param_env);
                    if shape != ClsShape::NotClosure {
                        return shape;
                    }
                }
            }
            ClsShape::NotClosure
        })
        .collect()
}

/// A closure literal, or an ident whose top-level `let`/`const`
/// binding init is one.
///
/// `param_env` maps an enclosing specialization's param names to
/// their substituted annotations — an Ident arg naming a `__cls(`
/// param carries that shape through generic-to-generic calls (same
/// env as `num_width::infer_arg_width`; without it the mono body's
/// call arm would jump an env-block ptr as a bare fn ptr, SIGBUS).
fn arg_cls_shape(ast: &Ast, eid: ExprId, param_env: &HashMap<String, String>) -> ClsShape {
    if let Expr::Ident(n) = ast.get_expr(eid)
        && let Some(ann) = param_env.get(n.as_str())
        && ann.starts_with("__cls(")
    {
        return ClsShape::Closure;
    }
    let is_closure = match ast.get_expr(eid) {
        Expr::Closure { .. } => true,
        Expr::Ident(n) => ast.stmts.iter().any(|s| {
            matches!(s, Stmt::LetDecl { name, init, .. }
                if name == n && matches!(ast.get_expr(*init), Expr::Closure { .. }))
        }),
        _ => false,
    };
    if is_closure {
        ClsShape::Closure
    } else {
        ClsShape::NotClosure
    }
}
