//! The binding → annotation map the return sniff reads.

use super::{AstExprsView, binds_to_params, infer_expr_ann_with};
use crate::ast::{Param, Stmt};

/// Walk `body` and for each `let x = INIT;` whose `INIT` we can
/// statically annotate, register `x → ann` in `binds`. Inner
/// FnDecls / nested classes etc. are skipped — we only care about
/// the immediate fn's binding scope. Idempotent over re-runs since
/// later let-decls in the same scope shadow earlier ones.
pub(super) fn collect_let_binding_anns(
    exprs: AstExprsView,
    body: &[Stmt],
    binds: &mut std::collections::HashMap<String, String>,
    fn_sigs: &std::collections::HashMap<String, String>,
) {
    for s in body {
        collect_let_binding_anns_stmt(exprs, s, binds, fn_sigs);
    }
}

fn collect_let_binding_anns_stmt(
    exprs: AstExprsView,
    s: &Stmt,
    binds: &mut std::collections::HashMap<String, String>,
    fn_sigs: &std::collections::HashMap<String, String>,
) {
    match s {
        Stmt::LetDecl {
            name,
            type_ann,
            init,
            ..
        } => {
            // Explicit annotation wins.
            if let Some(ann) = type_ann {
                binds.insert(name.clone(), ann.clone());
                return;
            }
            // Else infer from init shape — using the binds map we've
            // built up to here, so `let x = 3; let y = x + 1;` chains
            // correctly.
            let bs: Vec<Param> = binds_to_params(binds);
            if let Some(ann) = infer_expr_ann_with(exprs, *init, &bs, binds, fn_sigs) {
                binds.insert(name.clone(), ann);
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_let_binding_anns_stmt(exprs, then_branch, binds, fn_sigs);
            if let Some(eb) = else_branch {
                collect_let_binding_anns_stmt(exprs, eb, binds, fn_sigs);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            collect_let_binding_anns_stmt(exprs, body, binds, fn_sigs);
        }
        Stmt::Labeled { body, .. } => {
            collect_let_binding_anns_stmt(exprs, body, binds, fn_sigs);
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                collect_let_binding_anns_stmt(exprs, i, binds, fn_sigs);
            }
            collect_let_binding_anns_stmt(exprs, body, binds, fn_sigs);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            collect_let_binding_anns(exprs, stmts, binds, fn_sigs);
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            collect_let_binding_anns(exprs, body, binds, fn_sigs);
            collect_let_binding_anns(exprs, catch_body, binds, fn_sigs);
            if let Some(fb) = finally_body {
                collect_let_binding_anns(exprs, fb, binds, fn_sigs);
            }
        }
        _ => {}
    }
}
