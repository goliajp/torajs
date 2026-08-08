//! Scope-side helpers for the eval desugar: sealing a strict direct
//! eval's `var`s, and the wholesale decline when the program rebinds
//! `eval`. Split out of `desugar_eval.rs` when the indirect form pushed
//! the file past the 500-line cap.

use super::super::{Ast, Expr, Stmt};

/// Re-tag every `var` belonging to eval's own variable scope as
/// block-scoped, so the `Stmt::Block` the statements land in becomes
/// their whole environment (§19.2.1.1 strict branch — see module doc).
/// DIRECT eval only: an indirect eval's code is sloppy global code, its
/// `var`s belong on the global and must NOT be sealed.
///
/// The walk descends through blocks and control flow, which share the
/// eval's VariableEnvironment, and stops at a nested `FnDecl` body,
/// which has its own.
///
/// It does NOT seal the function declaration itself: nothing here can,
/// because a block-level `function` escapes its block throughout tr,
/// with or without an eval. See the module doc — that is a separate
/// defect, and sealing it from this side would paper over the general
/// case while leaving it wrong everywhere else.
pub(super) fn seal_var_scope(stmts: &mut [Stmt]) {
    for s in stmts.iter_mut() {
        match s {
            Stmt::LetDecl { is_var, .. } => *is_var = false,
            // A nested function's `var`s are its own — do not descend.
            Stmt::FnDecl { .. } => {}
            Stmt::Block(b) | Stmt::Multi(b) => seal_var_scope(b),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                seal_var_scope(std::slice::from_mut(then_branch));
                if let Some(e) = else_branch.as_deref_mut() {
                    seal_var_scope(std::slice::from_mut(e));
                }
            }
            Stmt::While { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::Labeled { body, .. }
            | Stmt::ForOf { body, .. } => seal_var_scope(std::slice::from_mut(body)),
            Stmt::For { init, body, .. } => {
                if let Some(i) = init.as_deref_mut() {
                    seal_var_scope(std::slice::from_mut(i));
                }
                seal_var_scope(std::slice::from_mut(body));
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                seal_var_scope(body);
                seal_var_scope(catch_body);
                if let Some(f) = finally_body.as_mut() {
                    seal_var_scope(f);
                }
            }
            Stmt::Switch { cases, default, .. } => {
                for c in cases.iter_mut() {
                    seal_var_scope(&mut c.body);
                }
                if let Some(d) = default.as_mut() {
                    seal_var_scope(d);
                }
            }
            _ => {}
        }
    }
}

/// Does any binding in the program shadow the global `eval`? A `var` /
/// `let` / parameter / function named `eval` means a call site may be
/// reaching something else entirely, and this pass runs before the
/// scope resolution that could tell which. Declining wholesale is the
/// answer that cannot be wrong.
pub(super) fn binds_eval(ast: &Ast) -> bool {
    binds_name(ast, "eval")
}

/// The same wholesale-decline question for any global this family
/// resolves at compile time — `Function` for the dynamic-function
/// desugar uses it too.
pub(super) fn binds_name(ast: &Ast, name: &str) -> bool {
    fn in_list(stmts: &[Stmt], name: &str) -> bool {
        stmts.iter().any(|s| in_stmt(s, name))
    }
    fn in_stmt(s: &Stmt, want: &str) -> bool {
        match s {
            Stmt::LetDecl { name, .. } => name == want,
            Stmt::FnDecl {
                name, params, body, ..
            } => name == want || params.iter().any(|p| p.name == want) || in_list(body, want),
            Stmt::ClassDecl { name, .. } => name == want,
            Stmt::Try {
                catch_param,
                body,
                catch_body,
                finally_body,
                ..
            } => {
                catch_param.as_deref() == Some(want)
                    || in_list(body, want)
                    || in_list(catch_body, want)
                    || finally_body.as_deref().is_some_and(|f| in_list(f, want))
            }
            Stmt::Block(b) | Stmt::Multi(b) => in_list(b, want),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                in_stmt(then_branch, want)
                    || else_branch.as_deref().is_some_and(|e| in_stmt(e, want))
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => in_stmt(body, want),
            Stmt::Labeled { body, .. } => in_stmt(body, want),
            Stmt::For { init, body, .. } => {
                init.as_deref().is_some_and(|i| in_stmt(i, want)) || in_stmt(body, want)
            }
            Stmt::ForOf { var_name, body, .. } => var_name == want || in_stmt(body, want),
            Stmt::Switch { cases, default, .. } => {
                cases.iter().any(|c| in_list(&c.body, want))
                    || default.as_deref().is_some_and(|d| in_list(d, want))
            }
            _ => false,
        }
    }
    if in_list(&ast.stmts, name) {
        return true;
    }
    // Arrows live in the expression arena, so a parameter or body
    // binding there is invisible to the statement walk.
    ast.exprs.iter().any(|e| match e {
        Expr::ArrowFn { params, body, .. } => {
            params.iter().any(|p| p.name == name) || in_list(body, name)
        }
        _ => false,
    })
}
