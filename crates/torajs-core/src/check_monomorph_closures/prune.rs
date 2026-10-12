//! The capture names a closure cannot resolve, taken off its lists
//! once the monomorph worklist has run.

use crate::ast::{Ast, Expr, Stmt};
use crate::check::Checker;

/// RFC 20260730-undeclared-ident 刀 3 — prune nowhere-resolving
/// capture names (recorded by check_closure per construction site)
/// from the owned AST's capture lists, so the lowerer's env
/// materialization never sees them; the body's marked Ident read
/// raises the ReferenceError instead. The lifted FnDecl's
/// `__env(...)` ann is kept in step (the lowerer only gates on
/// empty/non-empty, but a stale name list misleads readers). Runs
/// after the mono worklist so specialization-clone closures (fresh
/// ExprIds marked during their body checks) are covered too.
pub(crate) fn prune_unresolved_captures(c: &mut Checker, owned_ast: &mut Ast) {
    for (ceid, gone) in std::mem::take(&mut c.unresolved_captures) {
        let Expr::Closure { fn_name, captures } = &mut owned_ast.exprs[ceid.0 as usize] else {
            continue;
        };
        captures.retain(|cap| !gone.contains(cap));
        let caps = captures.join("|");
        let fname = fn_name.clone();
        for s in owned_ast.stmts.iter_mut() {
            let Stmt::FnDecl { name, params, .. } = s else {
                continue;
            };
            if *name != fname {
                continue;
            }
            if let Some(env) = params.first_mut()
                && env.name == "__env"
            {
                env.type_ann = Some(format!("__env({caps})"));
            }
        }
    }
}
