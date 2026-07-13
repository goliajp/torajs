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
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClsShape {
    /// Not statically a closure — keep the `__fn(` status quo.
    NotClosure,
    /// A closure value (env-block ptr) — flip to `__cls(`.
    Closure,
    /// A closure carrying the synthetic `__torajs_real_argc` first
    /// param (RFC 20260708-closure-argc-abi) — flip to `__clsargc(`
    /// so the mono body's param-call arm prepends the runtime argc.
    ClosureArgc,
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
/// binding init is one; argc-carrying closures (synthetic
/// `__torajs_real_argc` after `__env`) report `ClosureArgc`.
///
/// `param_env` maps an enclosing specialization's param names to
/// their substituted annotations — an Ident arg naming a `__cls(`/
/// `__clsargc(` param carries that shape through generic-to-generic
/// calls (same env as `num_width::infer_arg_width`; without it the
/// mono body's call arm would jump an env-block ptr as a bare fn
/// ptr, SIGBUS).
fn arg_cls_shape(ast: &Ast, eid: ExprId, param_env: &HashMap<String, String>) -> ClsShape {
    if let Expr::Ident(n) = ast.get_expr(eid)
        && let Some(ann) = param_env.get(n.as_str())
    {
        if ann.starts_with("__clsargc(") {
            return ClsShape::ClosureArgc;
        }
        if ann.starts_with("__cls(") {
            return ClsShape::Closure;
        }
    }
    let fn_name = match ast.get_expr(eid) {
        Expr::Closure { fn_name, .. } => Some(fn_name.clone()),
        Expr::Ident(n) => ast.stmts.iter().find_map(|s| match s {
            Stmt::LetDecl { name, init, .. } if name == n => match ast.get_expr(*init) {
                Expr::Closure { fn_name, .. } => Some(fn_name.clone()),
                _ => None,
            },
            _ => None,
        }),
        _ => None,
    };
    let Some(fn_name) = fn_name else {
        return ClsShape::NotClosure;
    };
    let has_argc = ast.stmts.iter().any(|s| {
        matches!(s, Stmt::FnDecl { name, params, .. }
            if *name == fn_name
                && params.first().is_some_and(|p| p.name == "__env")
                && params.get(1).is_some_and(|p| p.name == "__torajs_real_argc"))
    });
    if has_argc {
        ClsShape::ClosureArgc
    } else {
        ClsShape::Closure
    }
}
