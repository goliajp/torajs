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

/// Does `s` hold a `break label` that leaves it? Labels do not cross a
/// function body.
pub(crate) fn breaks_to(s: &crate::ast::Stmt, label: &str) -> bool {
    use crate::ast::Stmt;
    match s {
        Stmt::Break(Some(l)) => l == label,
        Stmt::Block(stmts) | Stmt::Multi(stmts) => stmts.iter().any(|b| breaks_to(b, label)),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            breaks_to(then_branch, label)
                || else_branch.as_deref().is_some_and(|b| breaks_to(b, label))
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::ForOf { body, .. } => breaks_to(body, label),
        Stmt::For { init, body, .. } => {
            init.as_deref().is_some_and(|i| breaks_to(i, label)) || breaks_to(body, label)
        }
        // an inner statement under the same label shadows it
        Stmt::Labeled { label: l, body } => l != label && breaks_to(body, label),
        Stmt::Switch { cases, default, .. } => {
            cases
                .iter()
                .flat_map(|c| &c.body)
                .any(|b| breaks_to(b, label))
                || default.iter().flatten().any(|b| breaks_to(b, label))
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => body
            .iter()
            .chain(catch_body)
            .chain(finally_body.iter().flatten())
            .any(|b| breaks_to(b, label)),
        _ => false,
    }
}

/// Does the loop body `s` hold a `continue` that starts the loop's next
/// iteration? An unlabeled one inside a nested loop goes to that loop;
/// a labeled one counts unless its label names a statement inside `s`.
pub(crate) fn continues_out(s: &crate::ast::Stmt) -> bool {
    continues_from(s, false, &mut Vec::new())
}

fn continues_from<'a>(s: &'a crate::ast::Stmt, nested: bool, inner: &mut Vec<&'a str>) -> bool {
    use crate::ast::Stmt;
    match s {
        Stmt::Continue(None) => !nested,
        Stmt::Continue(Some(l)) => !inner.contains(&l.as_str()),
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            stmts.iter().any(|b| continues_from(b, nested, inner))
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            continues_from(then_branch, nested, inner)
                || else_branch
                    .as_deref()
                    .is_some_and(|b| continues_from(b, nested, inner))
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::ForOf { body, .. } => continues_from(body, true, inner),
        Stmt::For { init, body, .. } => {
            init.as_deref()
                .is_some_and(|i| continues_from(i, nested, inner))
                || continues_from(body, true, inner)
        }
        Stmt::Labeled { label, body } => {
            inner.push(label);
            let hit = continues_from(body, nested, inner);
            inner.pop();
            hit
        }
        Stmt::Switch { cases, default, .. } => cases
            .iter()
            .flat_map(|c| &c.body)
            .chain(default.iter().flatten())
            .any(|b| continues_from(b, nested, inner)),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => body
            .iter()
            .chain(catch_body)
            .chain(finally_body.iter().flatten())
            .any(|b| continues_from(b, nested, inner)),
        _ => false,
    }
}
