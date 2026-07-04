//! AST predicate — does a fn body directly return an `Expr::Closure`
//! (either as a literal or via a local bound to a Closure expression)?
//!
//! Used by `effective_ret_ty` in `ssa_lower.rs` to upgrade a declared
//! `(y) => R` return type from `Type::FnSig(sig)` to
//! `Type::Closure(sig)` when the body actually constructs a capturing
//! arrow — without this upgrade the caller's slot is FnSig but the
//! runtime value is a Closure env pointer, and dispatching via the
//! FnSig path interprets the env pointer as a raw fn address → SIGBUS.
//! Detected pattern is the direct-return case; returning a closure
//! stored deeper than a single local binding is not yet handled.
//!
//! Extracted from `ssa_lower.rs` so that file stays on the known-debt
//! "only-shrinks" trajectory.

use std::collections::HashSet;

use crate::ast::{Ast, Expr, Stmt};

pub(crate) fn body_returns_closure(ast: &Ast, body: &[Stmt]) -> bool {
    let mut closure_locals: HashSet<String> = HashSet::new();
    // Chunk 522 — top-level closure bindings count too: a lifted
    // arrow returning a CAPTURED closure local (`const g = (x) =>
    // ...; const h = () => g;`) has no body-local `let g`, so the
    // body-only collection missed it and the ret slot stayed FnSig
    // for a Closure value (SIGBUS via the raw-fn dispatch). A
    // body-local non-closure shadow re-inserting the name is
    // harmless — the upgrade only fires when the parsed ret is
    // already fn-shaped, and a FnSig-valued return under a Closure
    // ret is forwarder-wrapped by the Return arm.
    for s in &ast.stmts {
        if let Stmt::LetDecl { name, init, .. } = s
            && matches!(ast.get_expr(*init), Expr::Closure { .. })
        {
            closure_locals.insert(name.clone());
        }
    }
    collect_closure_locals(ast, body, &mut closure_locals);
    body.iter()
        .any(|s| stmt_returns_closure(ast, s, &closure_locals))
}

fn collect_closure_locals(ast: &Ast, body: &[Stmt], out: &mut HashSet<String>) {
    for s in body {
        match s {
            Stmt::LetDecl { name, init, .. } => {
                if matches!(ast.get_expr(*init), Expr::Closure { .. }) {
                    out.insert(name.clone());
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_closure_locals(ast, std::slice::from_ref(then_branch), out);
                if let Some(eb) = else_branch {
                    collect_closure_locals(ast, std::slice::from_ref(eb), out);
                }
            }
            Stmt::While { body, .. } | Stmt::For { body, .. } => {
                collect_closure_locals(ast, std::slice::from_ref(body), out);
            }
            Stmt::DoWhile { body, .. } => {
                collect_closure_locals(ast, std::slice::from_ref(body), out);
            }
            Stmt::Block(stmts) | Stmt::Multi(stmts) => {
                collect_closure_locals(ast, stmts, out);
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                collect_closure_locals(ast, body, out);
                collect_closure_locals(ast, catch_body, out);
                if let Some(fb) = finally_body {
                    collect_closure_locals(ast, fb, out);
                }
            }
            Stmt::Switch { cases, default, .. } => {
                for c in cases {
                    collect_closure_locals(ast, &c.body, out);
                }
                if let Some(db) = default {
                    collect_closure_locals(ast, db, out);
                }
            }
            _ => {}
        }
    }
}

fn stmt_returns_closure(ast: &Ast, s: &Stmt, closure_locals: &HashSet<String>) -> bool {
    match s {
        Stmt::Return(Some(eid)) => match ast.get_expr(*eid) {
            Expr::Closure { .. } => true,
            Expr::Ident(name) => closure_locals.contains(name),
            _ => false,
        },
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            stmt_returns_closure(ast, then_branch, closure_locals)
                || else_branch
                    .as_ref()
                    .map(|e| stmt_returns_closure(ast, e, closure_locals))
                    .unwrap_or(false)
        }
        Stmt::While { body, .. } | Stmt::For { body, .. } => {
            stmt_returns_closure(ast, body, closure_locals)
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => stmts
            .iter()
            .any(|s| stmt_returns_closure(ast, s, closure_locals)),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter()
                .any(|s| stmt_returns_closure(ast, s, closure_locals))
                || catch_body
                    .iter()
                    .any(|s| stmt_returns_closure(ast, s, closure_locals))
                || finally_body
                    .as_ref()
                    .map(|fb| {
                        fb.iter()
                            .any(|s| stmt_returns_closure(ast, s, closure_locals))
                    })
                    .unwrap_or(false)
        }
        _ => false,
    }
}
