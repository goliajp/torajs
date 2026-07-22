//! `stmt_diverges` pulled out of [`crate::check`] as chunk-317
//! of the check.rs god-file decomp.
//!
//! Pure read-only AST walker — given a `Stmt`, returns `true`
//! iff control flow cannot fall through past it (the next
//! statement is unreachable in source order). Used by
//! `check_stmt_if` to gate moved-binding re-narrowing on
//! divergent branches.
//!
//! Recursive over `Stmt::Block` / `Stmt::Multi` (checks the
//! last child) and `Stmt::If` (both arms must diverge). Loops
//! (`while` / `for` / `do-while`) and `Stmt::Switch` / `Try`
//! are conservatively non-diverging — see the body comment.

pub(crate) fn stmt_diverges(s: &crate::ast::Stmt) -> bool {
    use crate::ast::Stmt;
    match s {
        Stmt::Return(_) | Stmt::Throw(_) | Stmt::Break(_) | Stmt::Continue(_) => true,
        Stmt::Block(stmts) | Stmt::Multi(stmts) => stmts.last().is_some_and(stmt_diverges),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            // If both branches diverge, the if as a whole diverges.
            stmt_diverges(then_branch) && else_branch.as_deref().is_some_and(stmt_diverges)
        }
        // While/For/DoWhile/Switch/Try/etc. could diverge in principle
        // (e.g. `while(true) { return ... }`) but we conservatively say
        // they don't — avoids false negatives on potentially-finite
        // loops. Worst case is we keep moves that should have been
        // discarded; the trailing post-loop code stays safe.
        _ => false,
    }
}
