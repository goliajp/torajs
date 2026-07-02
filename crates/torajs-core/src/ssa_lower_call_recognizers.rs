//! Call-shape recognizers for `LowerCtx<'a>` extracted from
//! `ssa_lower.rs` chunk 384 — Path A.3-batch5.
//!
//! Three peek-at-Expr predicates the caller-typed lowering fast-paths
//! use before committing to a specialized emit route:
//!
//! - `is_json_parse_call(eid)`      — `JSON.parse(text)` shape (M6.3).
//!   Drives caller-typed JSON parsing via `lower_json_parse` when the
//!   slot annotation resolves to a concrete target type.
//! - `is_bun_file_json_await(eid)`  — `await Bun.file(p).json()` shape
//!   (T-19.d v0.5.0), post-await desugar; returns the path arg's ExprId
//!   when the chain matches. Used to dispatch to the caller-driven JSON
//!   parser when the slot has a concrete T.
//! - `is_fromentries_call(eid)`     — `Object.fromEntries(entries)`
//!   shape (T-09.c v0.4.0). Routes to `lower_fromentries` from LetDecl
//!   when the slot annotation gives a concrete struct type. Widens per
//!   S309 (ES §20.1.2.7 silently ignores trailing args).
//!
//! Method bodies are byte-for-byte preserved from the source; the
//! sibling reaches LowerCtx fields via `impl<'a> super::LowerCtx<'a>`,
//! so call sites need zero edits.

use crate::ast::{Expr, ExprId};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// M6.3 — peek at an Expr to see whether it's the
    /// `JSON.parse(text)` call shape that drives caller-typed JSON
    /// parsing. Used by Stmt::LetDecl to switch the init-lowering to
    /// `lower_json_parse` when the slot's annotation gives us a
    /// concrete target type.
    pub(crate) fn is_json_parse_call(&self, eid: ExprId) -> bool {
        let Expr::Call { callee, args } = self.ast.get_expr(eid) else {
            return false;
        };
        if args.len() != 1 {
            return false;
        }
        let Expr::Member { obj, name } = self.ast.get_expr(*callee) else {
            return false;
        };
        if name != "parse" {
            return false;
        }
        matches!(self.ast.get_expr(*obj), Expr::Ident(s) if s == "JSON")
    }

    /// T-19.d (v0.5.0) — `await Bun.file(p).json()` shape detection.
    /// After the parser's `await e` → `e.value` desugar, the init
    /// is `Member{obj=<Bun.file(p).json() call>, name: "value"}`.
    /// Returns Some(path_arg_eid) when the chain matches; None
    /// otherwise. Used by the LetDecl arm to dispatch to the
    /// caller-driven JSON parser when the slot has a concrete T.
    pub(crate) fn is_bun_file_json_await(&self, eid: ExprId) -> Option<ExprId> {
        let Expr::Member {
            obj: outer_call,
            name,
        } = self.ast.get_expr(eid)
        else {
            return None;
        };
        if name != "value" {
            return None;
        }
        let Expr::Call {
            callee: json_callee,
            args: json_args,
        } = self.ast.get_expr(*outer_call)
        else {
            return None;
        };
        if !json_args.is_empty() {
            return None;
        }
        let Expr::Member {
            obj: file_call,
            name: jname,
        } = self.ast.get_expr(*json_callee)
        else {
            return None;
        };
        if jname != "json" {
            return None;
        }
        let Expr::Call {
            callee: file_callee,
            args: file_args,
        } = self.ast.get_expr(*file_call)
        else {
            return None;
        };
        if file_args.len() != 1 {
            return None;
        }
        let Expr::Member {
            obj: bun_id,
            name: fname,
        } = self.ast.get_expr(*file_callee)
        else {
            return None;
        };
        if fname != "file" {
            return None;
        }
        if !matches!(self.ast.get_expr(*bun_id), Expr::Ident(s) if s == "Bun") {
            return None;
        }
        Some(file_args[0])
    }

    /// T-09.c (v0.4.0) — `Object.fromEntries(entries)` call shape.
    /// Routes to `lower_fromentries` from ssa_lower's LetDecl arm
    /// when the slot annotation gives a concrete struct type.
    pub(crate) fn is_fromentries_call(&self, eid: ExprId) -> bool {
        let Expr::Call { callee, args } = self.ast.get_expr(eid) else {
            return false;
        };
        // S309 — ES §20.1.2.7 silently ignores trailing args; widen
        // gate to accept >= 1. LetDecl fast-path lowers args[0] then
        // drops args[1..] before consuming entries.
        if args.is_empty() {
            return false;
        }
        let Expr::Member { obj, name } = self.ast.get_expr(*callee) else {
            return false;
        };
        if name != "fromEntries" {
            return false;
        }
        matches!(self.ast.get_expr(*obj), Expr::Ident(s) if s == "Object")
    }
}
