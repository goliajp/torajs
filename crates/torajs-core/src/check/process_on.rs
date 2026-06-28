//! `process.on(eventName, cb)` typecheck.
//!
//! P10.5-A4 — narrow-MVP scope:
//!   - `eventName` must be the literal string `"unhandledRejection"`
//!     (matches the lower-side gate at
//!     `ssa_lower_process_on::try_lower`).
//!   - `cb` must be a callable (named fn → `Type::Function`,
//!     captured lambda → `Type::Closure`) accepting one `any`-typed
//!     reason argument and returning `void` / `undefined`.
//!
//! The runtime registers the cb in
//! `torajs_promise::unhandled::UNHANDLED_CB`; the pending-list
//! sweep at `main` exit (see `__torajs_main_exit_code`) dispatches
//! the cb instead of the default `error: <reason>` reporter for
//! every rejected promise whose `has_handler` is still 0.
//!
//! Return type is `void` per the spec's `EventEmitter#on`
//! convention (`process` is the only `EventEmitter` we model
//! today; method-chaining `process.on(...).on(...)` isn't part of
//! this narrow MVP).

use super::{Checker, Type};
use crate::ast::{Ast, Expr, ExprId};

impl Checker {
    /// Returns `None` if `callee` is not `process.on`; otherwise
    /// returns the typecheck verdict.
    pub(crate) fn check_process_on(
        &mut self,
        ast: &Ast,
        callee: ExprId,
        args: &[ExprId],
    ) -> Option<Result<Type, String>> {
        let Expr::Member {
            obj: ns_id,
            name: m_name,
        } = ast.get_expr(callee)
        else {
            return None;
        };
        if m_name != "on" {
            return None;
        }
        let Expr::Ident(ns) = ast.get_expr(*ns_id) else {
            return None;
        };
        if ns != "process" {
            return None;
        }
        if args.len() != 2 {
            return Some(Err(format!(
                "process.on expects 2 args (event, cb), got {}",
                args.len()
            )));
        }
        let event_ok = matches!(
            ast.get_expr(args[0]),
            Expr::String(s) if s == "unhandledRejection"
        );
        if !event_ok {
            return Some(Err("process.on: event must be the literal \
                 \"unhandledRejection\" in v0.5 MVP (P10.5-A4)"
                .to_string()));
        }
        let cb_ty = match self.type_of(ast, args[1]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        // check::Type::Function covers both named fn declarations and
        // captured lambdas; the SSA-side `ssa_lower_process_on::
        // try_lower` discriminates FnSig vs Closure at lower time.
        match &cb_ty {
            Type::Function(_, _) => {}
            other => {
                return Some(Err(format!(
                    "process.on: cb must be a callable (fn or closure), got {other:?}"
                )));
            }
        }
        Some(Ok(Type::Void))
    }
}
