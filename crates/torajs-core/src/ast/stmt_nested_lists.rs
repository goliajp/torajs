//! The shared nested-statement-list spine — every compound
//! statement's bodies (fn decls, blocks, try/if/loops/switch) handed
//! to a callback in syntactic order. A by-name collector built on it
//! re-finds a binding wherever the program put it (an if-branch, a
//! loop body, the Try that `desugar_async` wraps an async body in);
//! one that hand-rolls its recursion skips a compound form sooner or
//! later and silently mis-judges the binding inside it (rotation
//! 345's any-let lesson, hit again in rotation 346 when the
//! capturing-lane `const` for a nested FnDecl-this landed inside the
//! async Try and never reached the promote candidate set).

use super::Stmt;

/// Every nested statement list of `s`, in syntactic order.
pub(crate) fn for_each_nested_list<'a>(s: &'a Stmt, f: &mut dyn FnMut(&'a [Stmt])) {
    match s {
        Stmt::FnDecl { body, .. } => f(body),
        Stmt::Block(inner) | Stmt::Multi(inner) => f(inner),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            f(body);
            f(catch_body);
            if let Some(fin) = finally_body {
                f(fin);
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            f(std::slice::from_ref(then_branch));
            if let Some(e) = else_branch {
                f(std::slice::from_ref(e));
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
            f(std::slice::from_ref(body))
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                f(std::slice::from_ref(i));
            }
            f(std::slice::from_ref(body));
        }
        Stmt::ForOf { body, .. } | Stmt::ForOfSplitIter { body, .. } => {
            f(std::slice::from_ref(body))
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                f(&c.body);
            }
            if let Some(d) = default {
                f(d);
            }
        }
        _ => {}
    }
}
