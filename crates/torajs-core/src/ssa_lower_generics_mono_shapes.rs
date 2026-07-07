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
pub(crate) fn compute_typevar_closure_shapes(
    ast: &Ast,
    call_eid: ExprId,
    _callee_name: &str,
    type_args: &[check_mod::Type],
    generics: &HashMap<String, (Vec<String>, Vec<Param>, Option<String>, Vec<Stmt>)>,
) -> Vec<bool> {
    let arg_eids: Vec<ExprId> = match ast.get_expr(call_eid) {
        Expr::Call { args, .. } => args.clone(),
        _ => Vec::new(),
    };
    let Some((tp_names, gen_params, _, _)) = generics.get(_callee_name) else {
        return vec![false; type_args.len()];
    };
    tp_names
        .iter()
        .map(|tp_name| {
            for (pi, p) in gen_params.iter().enumerate() {
                let Some(ann) = &p.type_ann else { continue };
                if ann == tp_name
                    && let Some(arg_eid) = arg_eids.get(pi)
                    && arg_is_closure_shaped(ast, *arg_eid)
                {
                    return true;
                }
            }
            false
        })
        .collect()
}

/// A closure literal, or an ident whose top-level `let`/`const`
/// binding init is one.
fn arg_is_closure_shaped(ast: &Ast, eid: ExprId) -> bool {
    match ast.get_expr(eid) {
        Expr::Closure { .. } => true,
        Expr::Ident(n) => ast.stmts.iter().any(|s| {
            matches!(s, Stmt::LetDecl { name, init, .. }
                if name == n && matches!(ast.get_expr(*init), Expr::Closure { .. }))
        }),
        _ => false,
    }
}
