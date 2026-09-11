//! The class factory's view of the constructor's parameter list —
//! split from `desugar_classes_pass3` at the size cap. One question:
//! which of the constructor's defaults may the factory carry, and
//! which must stay the constructor's own to evaluate.

use super::desugar_with::walk;
use super::{Ast, Expr, ExprId, Param, Stmt};

/// The constructor's parameter list as the FACTORY carries it. A
/// default that reads `this` stays the constructor's to evaluate:
/// §10.2.2 [[Construct]] initializes the instance's fields BEFORE
/// FunctionDeclarationInstantiation runs the defaults, and the
/// constructor fn (`__cm_C__ctor(__this, …)`) is where `this` is a
/// bound parameter. Carried on the factory instead, the default was
/// materialized into the one function whose `this` is a `let`
/// declared further down, and the checker refused the second
/// `__this` (rotation 634: every `constructor(o = this.x)` was a
/// compile error). Such a slot becomes `= undefined` on the factory —
/// the call-site pad's spelling — so the constructor's own guard sees
/// the omitted argument and fills it. Every other default keeps
/// riding the factory unchanged.
pub(super) fn factory_params(ast: &mut Ast, params: &[Param]) -> Vec<Param> {
    let mut out = Vec::with_capacity(params.len());
    for p in params {
        let mut q = p.clone();
        if let Some(d) = p.default
            && expr_reads_this(ast, d)
        {
            q.default = Some(ast.add_expr(Expr::Ident("undefined".into())));
        }
        out.push(q);
    }
    out
}

/// Whether `this` is read anywhere under `eid` — as the node the
/// parser emits or as the `__this` ident pass 2 rewrites it to —
/// arrow bodies included (an arrow's `this` is the enclosing one).
fn expr_reads_this(ast: &Ast, eid: ExprId) -> bool {
    match ast.get_expr(eid) {
        Expr::This => return true,
        Expr::Ident(n) if n == "__this" => return true,
        Expr::ArrowFn { body, .. } => {
            let mut stack: Vec<&Stmt> = body.iter().collect();
            while let Some(s) = stack.pop() {
                if walk::stmt_exprs(s)
                    .into_iter()
                    .any(|e| expr_reads_this(ast, e))
                {
                    return true;
                }
                stack.extend(walk::stmt_children_ref(s));
            }
        }
        _ => {}
    }
    walk::expr_children(ast, eid)
        .into_iter()
        .any(|c| expr_reads_this(ast, c))
}
