//! Can a function body complete at all — the `never` inference gate.
//!
//! tsc types an arrow / function expression whose end point is
//! unreachable and that has no `return` as `() => never`. That is a
//! stricter question than [`super::body_always_terminates`], which
//! counts `return` as terminating: a body that may `return;` produces
//! `undefined` (§10.2.1.4), and typing it `never` would let a caller
//! read the 0 sentinel as a value.
//!
//! One-sided like its sibling: `true` means every path throws, and
//! anything not proven answers `false` (the callback then keeps its
//! `Void` return, which is a compile reject, never a wrong value).
//! Only `if` / blocks / `try` are descended; loops, `switch` and
//! labeled statements answer `false`. That also keeps `break` /
//! `continue` out of the picture: in the descended region they have
//! no target, so no path can jump past a `throw`.

use super::Stmt;

/// What every path through a statement does, as far as the walk can
/// tell without reading conditions.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Completion {
    /// every path throws
    Throws,
    /// some path provably reaches the end
    Falls,
    /// a loop, `switch`, label, jump or `return` is in the way
    Unknown,
}

/// True when no path through `body` completes normally or returns:
/// every path ends in a `throw`, the body holds no `return` anywhere
/// (a `return` inside a skipped loop still escapes), and no `yield`.
pub(crate) fn body_never_completes(body: &[Stmt]) -> bool {
    list_completion(body) == Completion::Throws && !body.iter().any(stmt_returns_or_yields)
}

/// How `body` ends; `Falls` is the TS2534 case for a `: never`
/// annotation (a reachable end point).
pub(crate) fn list_completion(body: &[Stmt]) -> Completion {
    let mut out = Completion::Falls;
    for s in body {
        match stmt_completion(s) {
            // what follows a throw is unreachable
            Completion::Throws => return Completion::Throws,
            Completion::Unknown => out = Completion::Unknown,
            Completion::Falls => {}
        }
    }
    out
}

/// Two alternatives either of which may run: the end is reachable if
/// either reaches it.
fn either(a: Completion, b: Completion) -> Completion {
    use Completion::*;
    match (a, b) {
        (Falls, _) | (_, Falls) => Falls,
        (Throws, Throws) => Throws,
        _ => Unknown,
    }
}

fn stmt_completion(s: &Stmt) -> Completion {
    use Completion::*;
    match s {
        Stmt::Throw(_) => Throws,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => either(
            stmt_completion(then_branch),
            else_branch.as_deref().map_or(Falls, stmt_completion),
        ),
        Stmt::Block(stmts) | Stmt::Multi(stmts) => list_completion(stmts),
        // A `finally` that throws decides alone; one that completes
        // normally re-raises the pending completion (§14.15.3), so
        // the protected body (and the handler, when present) decide.
        Stmt::Try {
            body,
            had_catch,
            catch_body,
            finally_body,
            ..
        } => {
            let fin = finally_body.as_deref().map_or(Falls, list_completion);
            if fin != Falls {
                return fin;
            }
            let b = list_completion(body);
            if *had_catch {
                either(b, list_completion(catch_body))
            } else {
                b
            }
        }
        Stmt::While { .. }
        | Stmt::DoWhile { .. }
        | Stmt::For { .. }
        | Stmt::ForOf { .. }
        | Stmt::ForOfSplitIter { .. }
        | Stmt::Switch { .. }
        | Stmt::Labeled { .. }
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::Return(_)
        | Stmt::Yield(_)
        | Stmt::YieldInto { .. } => Unknown,
        _ => Falls,
    }
}

fn stmt_returns_or_yields(s: &Stmt) -> bool {
    let list = |b: &[Stmt]| b.iter().any(stmt_returns_or_yields);
    match s {
        Stmt::Return(_) | Stmt::Yield(_) | Stmt::YieldInto { .. } => true,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            stmt_returns_or_yields(then_branch)
                || else_branch.as_deref().is_some_and(stmt_returns_or_yields)
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::Labeled { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::ForOfSplitIter { body, .. } => stmt_returns_or_yields(body),
        Stmt::For { init, body, .. } => {
            init.as_deref().is_some_and(stmt_returns_or_yields) || stmt_returns_or_yields(body)
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => list(stmts),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => list(body) || list(catch_body) || finally_body.as_deref().is_some_and(list),
        Stmt::Switch { cases, default, .. } => {
            cases.iter().any(|c| list(&c.body)) || default.as_deref().is_some_and(list)
        }
        // nested functions and classes own their returns
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::ExprId;

    fn throw() -> Stmt {
        Stmt::Throw(ExprId(0))
    }

    fn if_else(t: Stmt, e: Option<Stmt>) -> Stmt {
        Stmt::If {
            cond: ExprId(0),
            then_branch: Box::new(t),
            else_branch: e.map(Box::new),
        }
    }

    fn try_(body: Vec<Stmt>, catch: Option<Vec<Stmt>>, fin: Option<Vec<Stmt>>) -> Stmt {
        Stmt::Try {
            body,
            had_catch: catch.is_some(),
            catch_param: None,
            catch_type: None,
            catch_body: catch.unwrap_or_default(),
            finally_body: fin,
        }
    }

    #[test]
    fn plain_throw_is_never() {
        assert!(body_never_completes(&[Stmt::Expr(ExprId(0)), throw()]));
    }

    #[test]
    fn empty_body_completes() {
        assert!(!body_never_completes(&[]));
    }

    #[test]
    fn any_return_disqualifies() {
        assert!(!body_never_completes(&[Stmt::Return(None), throw()]));
        let looped = Stmt::While {
            cond: ExprId(0),
            body: Box::new(Stmt::Return(Some(ExprId(0)))),
        };
        assert!(!body_never_completes(&[looped, throw()]));
    }

    #[test]
    fn if_needs_both_arms() {
        assert!(!body_never_completes(&[if_else(throw(), None)]));
        assert!(body_never_completes(&[if_else(throw(), Some(throw()))]));
    }

    #[test]
    fn try_catch_that_swallows_completes() {
        assert!(!body_never_completes(&[try_(
            vec![throw()],
            Some(vec![]),
            None
        )]));
        assert!(body_never_completes(&[try_(
            vec![throw()],
            Some(vec![throw()]),
            None
        )]));
        assert!(body_never_completes(&[try_(
            vec![throw()],
            None,
            Some(vec![])
        )]));
        assert!(body_never_completes(&[try_(
            vec![],
            Some(vec![]),
            Some(vec![throw()])
        )]));
    }

    #[test]
    fn loops_and_switch_are_not_analysed() {
        let lp = Stmt::While {
            cond: ExprId(0),
            body: Box::new(throw()),
        };
        assert!(!body_never_completes(&[lp]));
        let sw = Stmt::Switch {
            scrutinee: ExprId(0),
            cases: vec![],
            default: Some(vec![throw()]),
        };
        assert!(!body_never_completes(&[sw]));
    }

    #[test]
    fn completion_separates_falls_from_unknown() {
        assert!(list_completion(&[Stmt::Expr(ExprId(0))]) == Completion::Falls);
        assert!(list_completion(&[if_else(throw(), None)]) == Completion::Falls);
        let lp = Stmt::While {
            cond: ExprId(0),
            body: Box::new(Stmt::Block(vec![])),
        };
        assert!(list_completion(&[lp]) == Completion::Unknown);
    }
}
