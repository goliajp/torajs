//! Typechecker diagnostics (chunk 428).
//!
//! Extracted verbatim from check.rs — the `Diagnostic` / `Severity`
//! value types and the `DiagPush` extension trait over
//! `Vec<Diagnostic>`. Re-exported from `crate::check` so every
//! consumer keeps the canonical `crate::check::Diagnostic` /
//! `crate::check::DiagPush` import path.

use super::*;

/// T-04 (v0.3.0) — typechecker diagnostic with source span + severity.
/// Replaces the previous `Vec<String>` error bucket. `span = (0, 0)`
/// is the sentinel for "no source location attached" — the LSP and
/// the CLI both render that as the file's first character. Per-site
/// span attachment lands incrementally as each push site gets an
/// ExprId in scope.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub span: Span,
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Diagnostic {
    pub fn error(message: String) -> Self {
        Self {
            span: Span { start: 0, end: 0 },
            severity: Severity::Error,
            message,
        }
    }

    pub fn warning(message: String) -> Self {
        Self {
            span: Span { start: 0, end: 0 },
            severity: Severity::Warning,
            message,
        }
    }

    /// Same as `error` but attaches the source span of the offending
    /// `ExprId`. The Ast's `expr_spans` table is consulted; if the
    /// ExprId predates a span (common for synthesized exprs from the
    /// desugar passes), the diagnostic falls back to `(0, 0)`.
    pub fn error_at(ast: &Ast, eid: ExprId, message: String) -> Self {
        let span = ast
            .expr_spans
            .get(eid.0 as usize)
            .copied()
            .unwrap_or(Span { start: 0, end: 0 });
        Self {
            span,
            severity: Severity::Error,
            message,
        }
    }
}

/// T-04 — extension trait so the 35 `errors.push(format!(...))` sites
/// stay one mechanical rename (`push` → `push_err`) rather than each
/// growing a `Diagnostic::error(...)` wrapper. Future per-site span
/// attachment swaps `push_err(msg)` for `push_err_at(eid, msg)`.
pub(crate) trait DiagPush {
    fn push_err(&mut self, msg: String);
    fn push_warn(&mut self, msg: String);
    /// Span-aware variant. Looks up the source span from `ast.expr_spans`.
    #[allow(dead_code)]
    fn push_err_at(&mut self, ast: &Ast, eid: ExprId, msg: String);
}

impl DiagPush for Vec<Diagnostic> {
    fn push_err(&mut self, msg: String) {
        self.push(Diagnostic::error(msg));
    }
    fn push_warn(&mut self, msg: String) {
        self.push(Diagnostic::warning(msg));
    }
    fn push_err_at(&mut self, ast: &Ast, eid: ExprId, msg: String) {
        self.push(Diagnostic::error_at(ast, eid, msg));
    }
}
