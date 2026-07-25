//! Does a statement list leave a path running off its end?
//!
//! ES §10.2.1.4 [[Call]] step 11 — a function body that completes
//! normally answers `undefined`. That is *the* return value for every
//! path a `return` does not cover, so a lowering that wants to know
//! "must I materialise an `undefined` at the tail" asks here.
//!
//! The answer is deliberately one-sided: `true` means **every** path
//! out of this list is a `return` or a `throw`, and being unsure
//! answers `false`. A false `false` costs a wider return slot; a
//! false `true` would leave the tail unwritten, so the imprecision
//! all points the safe way. Nothing here looks at reachability of
//! conditions — `if (true) return 1` is not treated as terminating,
//! matching what TS's own "all code paths return a value" check does.

use super::{Stmt, SwitchCase};

/// True when control cannot reach the end of `body` — every path
/// ends in `return` or `throw`. See the module doc on the one-sided
/// reading.
pub(crate) fn body_always_terminates(body: &[Stmt]) -> bool {
    body.iter().any(stmt_always_terminates)
}

fn stmt_always_terminates(s: &Stmt) -> bool {
    match s {
        Stmt::Return(_) | Stmt::Throw(_) => true,
        // Both arms have to terminate, and a missing `else` is a path
        // straight through.
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            stmt_always_terminates(then_branch)
                && else_branch.as_deref().is_some_and(stmt_always_terminates)
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => body_always_terminates(stmts),
        // A `finally` that terminates swallows every path through the
        // other two. Otherwise: with a `catch`, both the protected
        // body and the handler have to terminate, because either can
        // be the one that falls out of the statement. WITHOUT a
        // `catch` the body alone decides — an exception raised there
        // propagates out of the *function*, which is not the
        // fall-through path this asks about.
        Stmt::Try {
            body,
            had_catch,
            catch_body,
            finally_body,
            ..
        } => {
            if finally_body.as_deref().is_some_and(body_always_terminates) {
                return true;
            }
            if !*had_catch {
                return body_always_terminates(body);
            }
            body_always_terminates(body) && body_always_terminates(catch_body)
        }
        // Without a `default` the scrutinee can miss every case. With
        // one, every clause has to terminate — and an empty clause
        // body falls through to the next, which `body_always_terminates`
        // reports as non-terminating, so the conservative answer comes
        // out on its own.
        Stmt::Switch { cases, default, .. } => {
            let Some(default_body) = default else {
                return false;
            };
            body_always_terminates(default_body)
                && cases
                    .iter()
                    .all(|c: &SwitchCase| body_always_terminates(&c.body))
        }
        // Loops are not analysed: `while (true)` without a `break`
        // does terminate the path, but proving it needs a `break`
        // walk that has to respect labels and nested loops. Answering
        // `false` costs a slot width, never correctness.
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::ExprId;

    fn eid() -> ExprId {
        ExprId(0)
    }

    fn ret() -> Stmt {
        Stmt::Return(Some(eid()))
    }

    fn expr() -> Stmt {
        Stmt::Expr(eid())
    }

    fn if_stmt(then_branch: Stmt, else_branch: Option<Stmt>) -> Stmt {
        Stmt::If {
            cond: eid(),
            then_branch: Box::new(then_branch),
            else_branch: else_branch.map(Box::new),
        }
    }

    #[test]
    fn plain_return_terminates() {
        assert!(body_always_terminates(&[ret()]));
        assert!(body_always_terminates(&[expr(), ret()]));
        assert!(!body_always_terminates(&[expr()]));
        assert!(!body_always_terminates(&[]));
    }

    #[test]
    fn if_needs_both_arms() {
        assert!(!body_always_terminates(&[if_stmt(ret(), None)]));
        assert!(body_always_terminates(&[if_stmt(ret(), Some(ret()))]));
        assert!(!body_always_terminates(&[if_stmt(ret(), Some(expr()))]));
    }

    #[test]
    fn nested_blocks_see_through() {
        assert!(body_always_terminates(&[Stmt::Block(vec![ret()])]));
        assert!(body_always_terminates(&[if_stmt(
            Stmt::Block(vec![expr(), ret()]),
            Some(Stmt::Multi(vec![ret()])),
        )]));
    }

    #[test]
    fn switch_needs_a_default_and_every_clause() {
        let case = |body: Vec<Stmt>| SwitchCase { value: eid(), body };
        assert!(!body_always_terminates(&[Stmt::Switch {
            scrutinee: eid(),
            cases: vec![case(vec![ret()])],
            default: None,
        }]));
        assert!(body_always_terminates(&[Stmt::Switch {
            scrutinee: eid(),
            cases: vec![case(vec![ret()])],
            default: Some(vec![ret()]),
        }]));
        // an empty clause falls through to the next one
        assert!(!body_always_terminates(&[Stmt::Switch {
            scrutinee: eid(),
            cases: vec![case(vec![])],
            default: Some(vec![ret()]),
        }]));
    }

    #[test]
    fn try_paths() {
        let try_stmt = |body: Vec<Stmt>,
                        had_catch: bool,
                        catch_body: Vec<Stmt>,
                        finally_body: Option<Vec<Stmt>>| Stmt::Try {
            body,
            had_catch,
            catch_param: None,
            catch_type: None,
            catch_body,
            finally_body,
        };
        assert!(body_always_terminates(&[try_stmt(
            vec![ret()],
            true,
            vec![ret()],
            None
        )]));
        assert!(!body_always_terminates(&[try_stmt(
            vec![ret()],
            true,
            vec![expr()],
            None
        )]));
        // no catch — an exception leaves the whole function, so the
        // body alone decides
        assert!(body_always_terminates(&[try_stmt(
            vec![ret()],
            false,
            vec![],
            None
        )]));
        assert!(!body_always_terminates(&[try_stmt(
            vec![expr()],
            false,
            vec![],
            None
        )]));
        // a terminating finally covers everything
        assert!(body_always_terminates(&[try_stmt(
            vec![expr()],
            false,
            vec![],
            Some(vec![ret()])
        )]));
    }

    #[test]
    fn loops_answer_conservatively() {
        assert!(!body_always_terminates(&[Stmt::While {
            cond: eid(),
            body: Box::new(ret()),
        }]));
    }
}
