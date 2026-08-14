//! Does a fn body return a VALUE anywhere — a control-flow question,
//! split from its former host `implicit_generics_infer.rs` at the
//! file-size cap. Everything left there sniffs a TYPE; this pair asks
//! only whether an inferred return type is wanted at all, and it is
//! the one thing in that module the callers outside the
//! implicit-generics desugar reach for (the promise thunk lowering,
//! the generator desugars).

use super::Stmt;

pub(crate) fn body_has_value_return(body: &[Stmt]) -> bool {
    for s in body {
        if stmt_has_value_return(s) {
            return true;
        }
    }
    false
}

fn stmt_has_value_return(s: &Stmt) -> bool {
    match s {
        Stmt::Return(Some(_)) => true,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            stmt_has_value_return(then_branch)
                || else_branch.as_deref().is_some_and(stmt_has_value_return)
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => stmt_has_value_return(body),
        Stmt::Labeled { body, .. } => stmt_has_value_return(body),
        Stmt::For { init, body, .. } => {
            init.as_deref().is_some_and(stmt_has_value_return) || stmt_has_value_return(body)
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => body_has_value_return(stmts),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body_has_value_return(body)
                || body_has_value_return(catch_body)
                || finally_body.as_deref().is_some_and(body_has_value_return)
        }
        // Nested FnDecl returns are scoped to the inner fn — skip.
        Stmt::FnDecl { .. } => false,
        _ => false,
    }
}
