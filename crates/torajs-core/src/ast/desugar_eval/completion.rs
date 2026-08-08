//! The completion value of a multi-statement DIRECT source, placed
//! exactly — the general case the single-expression collapse in the
//! parent module cannot reach. §14.5.1: when the final statement of
//! the source is an ExpressionStatement, its value IS the source's
//! completion, overriding whatever the earlier statements completed
//! with. The call becomes an IIFE at the call site:
//!
//!   eval("f(); 2;")   →   (() => { f(); return 2; })()
//!
//! The arrow is not an approximation of the direct-eval environment;
//! it IS that environment. An arrow transmits `this`, `arguments` and
//! `super` from the enclosing context — exactly what a direct eval
//! sees — and its body is its own VariableEnvironment, so the source's
//! `var` and function declarations die with the eval, which is the
//! strict-mode rule tr is under. A trailing declaration would leave
//! the completion on an *earlier* statement (declarations complete
//! empty), so only the trailing-expression shape rewrites; the rest
//! keep the honest reject.

use super::super::{Ast, Expr, ExprId, Stmt};
use super::rewrite_list;
use super::source::{CallForm, literal_eval_call, parse_eval_source};

/// This pass runs LAST in the desugar deliberately: by then the
/// statement walks have inlined every statement-position direct eval
/// as a sealed block, so any literal direct call still standing is in
/// value position — no parent-tracking is needed to know that, and
/// the established sealed-block shape for statement evals stays
/// untouched.
pub(super) fn rewrite_completion_value_evals(ast: &mut Ast) {
    let mut i = 0;
    while i < ast.exprs.len() {
        let eid = ExprId(i as u32);
        let Some((src, CallForm::Direct)) = literal_eval_call(eid, ast) else {
            i += 1;
            continue;
        };
        if let Some(mut body) = parse_eval_source(&src, ast) {
            if body.len() >= 2 && matches!(body.last(), Some(Stmt::Expr(_))) {
                let Some(Stmt::Expr(tail)) = body.pop() else {
                    unreachable!()
                };
                // A nested statement-position eval inside the source is
                // an eval like any other; the statement walk no longer
                // runs, so give the new body its own pass.
                rewrite_list(&mut body, ast, true);
                body.push(Stmt::Return(Some(tail)));
                let arrow = ast.add_expr(Expr::ArrowFn {
                    params: Vec::new(),
                    return_type: None,
                    body,
                });
                ast.exprs[i] = Expr::Call {
                    callee: arrow,
                    args: Vec::new(),
                };
            }
        }
        i += 1;
    }
}
