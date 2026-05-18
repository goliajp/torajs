//! T-06 (v0.3.0) — `tr lint` MVP. Five starting rules:
//!
//!   - `unused-let`            — top-level `let X` / `const X` declared
//!                                but never referenced anywhere
//!   - `dead-code-after-return` — statements following `return` /
//!                                `throw` / `break` / `continue` in
//!                                the same block
//!   - `unreachable-catch`     — `catch` block on a `try` whose body
//!                                contains zero `throw` (and zero
//!                                `Call` to anything possibly-throwing
//!                                — for MVP we under-approximate to
//!                                "no throw and no Call at all" to
//!                                avoid false positives on standard
//!                                library calls that may throw)
//!   - `shadowed-let`          — inner-scope `let` / `const` with the
//!                                same name as one declared in an
//!                                enclosing scope
//!   - `unused-import`         — named import that's never referenced
//!
//! Reuses the lex+parse+AST pipeline; emits warnings via the T-04
//! `Diagnostic{ span, severity, message }` substrate. Output flows
//! through both `tr lint` (CLI) and `tr lsp` (LSP `WARNING` severity).

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use crate::check::{Diagnostic, Severity};
use crate::lexer::{self, Span};
use crate::parser;

#[derive(Debug)]
pub enum LintError {
    Lex(String),
    Parse(String),
}

impl std::fmt::Display for LintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LintError::Lex(e) => write!(f, "lex error: {e}"),
            LintError::Parse(e) => write!(f, "parse error: {e}"),
        }
    }
}

impl std::error::Error for LintError {}

pub fn lint(source: &str) -> Result<Vec<Diagnostic>, LintError> {
    let tokens = lexer::tokenize(source).map_err(LintError::Lex)?;
    let ast = parser::parse(&tokens).map_err(LintError::Parse)?;
    let mut diags = Vec::new();
    let mut linter = Linter {
        ast: &ast,
        diags: &mut diags,
    };
    linter.run();
    Ok(diags)
}

fn warn(message: String) -> Diagnostic {
    Diagnostic {
        span: Span { start: 0, end: 0 },
        severity: Severity::Warning,
        message,
    }
}

fn warn_at(ast: &Ast, eid: ExprId, message: String) -> Diagnostic {
    let span = ast
        .expr_spans
        .get(eid.0 as usize)
        .copied()
        .unwrap_or(Span { start: 0, end: 0 });
    Diagnostic {
        span,
        severity: Severity::Warning,
        message,
    }
}

struct Linter<'a> {
    ast: &'a Ast,
    diags: &'a mut Vec<Diagnostic>,
}

impl<'a> Linter<'a> {
    fn run(&mut self) {
        // Build a global identifier-reference set used by unused-let
        // and unused-import. Single pass over every expression in the
        // AST counts ident reads.
        let mut refs: HashMap<String, usize> = HashMap::new();
        for stmt in &self.ast.stmts {
            count_refs_stmt(self.ast, stmt, &mut refs);
        }
        let ref_set: HashSet<String> = refs.into_keys().collect();

        // Per-stmt walks: unused-let / unused-import / shadowed-let /
        // dead-code-after-return / unreachable-catch all share a
        // recursive scope walker.
        let mut scope_stack: Vec<HashSet<String>> = vec![HashSet::new()];
        for stmt in &self.ast.stmts {
            self.lint_stmt(stmt, &ref_set, &mut scope_stack);
        }
    }

    fn lint_stmt(
        &mut self,
        stmt: &Stmt,
        refs: &HashSet<String>,
        scopes: &mut Vec<HashSet<String>>,
    ) {
        match stmt {
            Stmt::LetDecl { name, init, .. } => {
                // shadowed-let — does any enclosing scope declare the
                // same name?
                if scopes
                    .iter()
                    .take(scopes.len().saturating_sub(1))
                    .any(|s| s.contains(name))
                {
                    self.diags.push(warn(format!(
                        "shadowed-let: `{name}` shadows an outer binding"
                    )));
                }
                // Record in current scope.
                if let Some(top) = scopes.last_mut() {
                    top.insert(name.clone());
                }
                // unused-let — only check at the top scope (function-
                // scoped lets are common scratch slots; chasing every
                // inner unused let is more noise than signal in MVP).
                if scopes.len() == 1 && !refs.contains(name) {
                    self.diags.push(warn_at(
                        self.ast,
                        *init,
                        format!("unused-let: `{name}` is declared but never read"),
                    ));
                }
            }
            Stmt::ImportDecl {
                default,
                namespace,
                named,
                ..
            } => {
                let mut check = |name: &str| {
                    if !refs.contains(name) {
                        self.diags.push(warn(format!(
                            "unused-import: `{name}` is imported but never used"
                        )));
                    }
                };
                if let Some(d) = default {
                    check(d);
                }
                if let Some(ns) = namespace {
                    check(ns);
                }
                for (n, alias) in named {
                    let bound = alias.as_deref().unwrap_or(n);
                    check(bound);
                }
            }
            Stmt::Block(stmts) => {
                scopes.push(HashSet::new());
                self.lint_block(stmts, refs, scopes);
                scopes.pop();
            }
            Stmt::Multi(stmts) => {
                // No fresh scope — Multi shares the surrounding frame.
                for s in stmts {
                    self.lint_stmt(s, refs, scopes);
                }
            }
            Stmt::FnDecl { params, body, .. } => {
                scopes.push(scope_from_params(params));
                self.lint_block(body, refs, scopes);
                scopes.pop();
            }
            Stmt::ClassDecl {
                ctor,
                methods,
                static_methods,
                ..
            } => {
                if let Some(c) = ctor {
                    scopes.push(scope_from_params(&c.params));
                    self.lint_block(&c.body, refs, scopes);
                    scopes.pop();
                }
                for m in methods.iter().chain(static_methods.iter()) {
                    scopes.push(scope_from_params(&m.params));
                    self.lint_block(&m.body, refs, scopes);
                    scopes.pop();
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.lint_stmt(then_branch, refs, scopes);
                if let Some(eb) = else_branch {
                    self.lint_stmt(eb, refs, scopes);
                }
            }
            Stmt::While { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::For { body, .. }
            | Stmt::ForOfSplitIter { body, .. }
            | Stmt::ForOf { body, .. } => {
                self.lint_stmt(body, refs, scopes);
            }
            Stmt::Switch { cases, default, .. } => {
                for c in cases {
                    self.lint_block(&c.body, refs, scopes);
                }
                if let Some(d) = default {
                    self.lint_block(d, refs, scopes);
                }
            }
            Stmt::Try {
                body,
                had_catch,
                catch_body,
                finally_body,
                ..
            } => {
                // unreachable-catch — body has no throw and no Call
                // (Call is the conservative under-approximation: most
                // user-defined fns can throw, so we only flag bodies
                // with zero call sites + zero throw).
                if *had_catch && !block_can_throw(self.ast, body) {
                    self.diags.push(warn(
                        "unreachable-catch: try body contains no throw or call".into(),
                    ));
                }
                self.lint_block(body, refs, scopes);
                if *had_catch {
                    self.lint_block(catch_body, refs, scopes);
                }
                if let Some(fb) = finally_body {
                    self.lint_block(fb, refs, scopes);
                }
            }
            Stmt::ExportDecl { inner, .. } => {
                if let Some(s) = inner {
                    self.lint_stmt(s, refs, scopes);
                }
            }
            _ => {}
        }
    }

    /// Walk a sequence of statements, applying per-stmt lints AND
    /// detecting dead-code-after-{return,throw,break,continue}.
    fn lint_block(
        &mut self,
        stmts: &[Stmt],
        refs: &HashSet<String>,
        scopes: &mut Vec<HashSet<String>>,
    ) {
        let mut diverged = false;
        for s in stmts {
            if diverged {
                self.diags.push(warn(
                    "dead-code-after-return: statement is unreachable".into(),
                ));
                // Don't double-report all subsequent stmts; just one.
                diverged = false;
            }
            self.lint_stmt(s, refs, scopes);
            if matches!(
                s,
                Stmt::Return(_) | Stmt::Throw(_) | Stmt::Break | Stmt::Continue
            ) {
                diverged = true;
            }
        }
    }
}

fn scope_from_params(params: &[Param]) -> HashSet<String> {
    params.iter().map(|p| p.name.clone()).collect()
}

/// Conservative "can this block throw?" — returns true if it contains
/// any `throw`, any `Call` (since user-defined fns may throw), any
/// nested try, or any other shape we can't statically prove safe.
/// False means definitively no throw possible — the catch block is
/// then unreachable.
fn block_can_throw(ast: &Ast, stmts: &[Stmt]) -> bool {
    for s in stmts {
        if stmt_can_throw(ast, s) {
            return true;
        }
    }
    false
}

fn stmt_can_throw(ast: &Ast, s: &Stmt) -> bool {
    match s {
        Stmt::Throw(_) => true,
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            had_catch,
            ..
        } => {
            // A nested try with its own catch absorbs its body's
            // throws — but the catch / finally bodies themselves may
            // still throw upward.
            if !had_catch && block_can_throw(ast, body) {
                return true;
            }
            block_can_throw(ast, catch_body)
                || finally_body
                    .as_ref()
                    .map(|fb| block_can_throw(ast, fb))
                    .unwrap_or(false)
        }
        Stmt::Expr(eid) => expr_can_throw(ast, *eid),
        Stmt::Return(Some(eid)) => expr_can_throw(ast, *eid),
        Stmt::LetDecl { init, .. } => expr_can_throw(ast, *init),
        Stmt::Yield(eid) => expr_can_throw(ast, *eid),
        Stmt::YieldInto { value, .. } => expr_can_throw(ast, *value),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_can_throw(ast, *cond)
                || stmt_can_throw(ast, then_branch)
                || else_branch
                    .as_ref()
                    .map(|e| stmt_can_throw(ast, e))
                    .unwrap_or(false)
        }
        Stmt::While { cond, body } | Stmt::DoWhile { cond, body } => {
            expr_can_throw(ast, *cond) || stmt_can_throw(ast, body)
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            init.as_ref()
                .map(|s| stmt_can_throw(ast, s))
                .unwrap_or(false)
                || cond.map(|c| expr_can_throw(ast, c)).unwrap_or(false)
                || step.map(|c| expr_can_throw(ast, c)).unwrap_or(false)
                || stmt_can_throw(ast, body)
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => expr_can_throw(ast, *parent) || expr_can_throw(ast, *sep) || stmt_can_throw(ast, body),
        Stmt::ForOf {
            elem_expr, body, ..
        } => expr_can_throw(ast, *elem_expr) || stmt_can_throw(ast, body),
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            if expr_can_throw(ast, *scrutinee) {
                return true;
            }
            for c in cases {
                if expr_can_throw(ast, c.value) || block_can_throw(ast, &c.body) {
                    return true;
                }
            }
            default
                .as_ref()
                .map(|d| block_can_throw(ast, d))
                .unwrap_or(false)
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => block_can_throw(ast, stmts),
        Stmt::ExportDecl { inner, .. } => inner
            .as_ref()
            .map(|s| stmt_can_throw(ast, s))
            .unwrap_or(false),
        Stmt::Return(None)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::FnDecl { .. }
        | Stmt::TypeDecl { .. }
        | Stmt::ClassDecl { .. }
        | Stmt::ImportDecl { .. } => false,
    }
}

fn expr_can_throw(ast: &Ast, eid: ExprId) -> bool {
    match ast.get_expr(eid) {
        Expr::Call { .. } | Expr::New { .. } | Expr::Super { .. } => true,
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => expr_can_throw(ast, *obj),
        Expr::Index { obj, index } => expr_can_throw(ast, *obj) || expr_can_throw(ast, *index),
        Expr::BinOp { left, right, .. } => {
            expr_can_throw(ast, *left) || expr_can_throw(ast, *right)
        }
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::PostIncr { target: expr, .. }
        | Expr::As { expr, .. } => expr_can_throw(ast, *expr),
        Expr::Sequence { left, right } => expr_can_throw(ast, *left) || expr_can_throw(ast, *right),
        Expr::Assign { target, value } => {
            expr_can_throw(ast, *target) || expr_can_throw(ast, *value)
        }
        Expr::Array(items) => items.iter().any(|e| expr_can_throw(ast, *e)),
        Expr::ObjectLit { fields } => fields.iter().any(|(_, e)| expr_can_throw(ast, *e)),
        Expr::Spread { expr } => expr_can_throw(ast, *expr),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_can_throw(ast, *cond)
                || expr_can_throw(ast, *then_branch)
                || expr_can_throw(ast, *else_branch)
        }
        Expr::Nullish { lhs, rhs } => expr_can_throw(ast, *lhs) || expr_can_throw(ast, *rhs),
        Expr::ArrowFn { .. } | Expr::Closure { .. } => false, // declaration only
        Expr::Ident(_)
        | Expr::String(_)
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Uninit
        | Expr::Regex { .. }
        | Expr::This
        | Expr::NewTarget
        | Expr::InstanceOf { .. } => false,
    }
}

fn count_refs_stmt(ast: &Ast, s: &Stmt, refs: &mut HashMap<String, usize>) {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => count_refs_expr(ast, *eid, refs),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                count_refs_expr(ast, *e, refs);
            }
        }
        Stmt::LetDecl { init, .. } => count_refs_expr(ast, *init, refs),
        Stmt::YieldInto { value, .. } => count_refs_expr(ast, *value, refs),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            count_refs_expr(ast, *cond, refs);
            count_refs_stmt(ast, then_branch, refs);
            if let Some(eb) = else_branch {
                count_refs_stmt(ast, eb, refs);
            }
        }
        Stmt::While { cond, body } | Stmt::DoWhile { cond, body } => {
            count_refs_expr(ast, *cond, refs);
            count_refs_stmt(ast, body, refs);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                count_refs_stmt(ast, i, refs);
            }
            if let Some(c) = cond {
                count_refs_expr(ast, *c, refs);
            }
            if let Some(st) = step {
                count_refs_expr(ast, *st, refs);
            }
            count_refs_stmt(ast, body, refs);
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            count_refs_expr(ast, *parent, refs);
            count_refs_expr(ast, *sep, refs);
            count_refs_stmt(ast, body, refs);
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => {
            count_refs_expr(ast, *elem_expr, refs);
            count_refs_stmt(ast, body, refs);
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            count_refs_expr(ast, *scrutinee, refs);
            for c in cases {
                count_refs_expr(ast, c.value, refs);
                for s in &c.body {
                    count_refs_stmt(ast, s, refs);
                }
            }
            if let Some(d) = default {
                for s in d {
                    count_refs_stmt(ast, s, refs);
                }
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for s in body {
                count_refs_stmt(ast, s, refs);
            }
            for s in catch_body {
                count_refs_stmt(ast, s, refs);
            }
            if let Some(fb) = finally_body {
                for s in fb {
                    count_refs_stmt(ast, s, refs);
                }
            }
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                count_refs_stmt(ast, s, refs);
            }
        }
        Stmt::FnDecl { body, params, .. } => {
            for p in params {
                if let Some(d) = p.default {
                    count_refs_expr(ast, d, refs);
                }
            }
            for s in body {
                count_refs_stmt(ast, s, refs);
            }
        }
        Stmt::ClassDecl {
            ctor,
            methods,
            static_methods,
            static_fields,
            ..
        } => {
            if let Some(c) = ctor {
                for s in &c.body {
                    count_refs_stmt(ast, s, refs);
                }
            }
            for m in methods.iter().chain(static_methods.iter()) {
                for s in &m.body {
                    count_refs_stmt(ast, s, refs);
                }
            }
            for sf in static_fields {
                count_refs_expr(ast, sf.init, refs);
            }
        }
        Stmt::ExportDecl {
            inner,
            default_expr,
            ..
        } => {
            if let Some(s) = inner {
                count_refs_stmt(ast, s, refs);
            }
            if let Some(e) = default_expr {
                count_refs_expr(ast, *e, refs);
            }
        }
        Stmt::TypeDecl { .. } | Stmt::ImportDecl { .. } | Stmt::Break | Stmt::Continue => {}
    }
}

fn count_refs_expr(ast: &Ast, eid: ExprId, refs: &mut HashMap<String, usize>) {
    match ast.get_expr(eid) {
        Expr::Ident(name) => {
            *refs.entry(name.clone()).or_insert(0) += 1;
        }
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => count_refs_expr(ast, *obj, refs),
        Expr::Index { obj, index } => {
            count_refs_expr(ast, *obj, refs);
            count_refs_expr(ast, *index, refs);
        }
        Expr::Call { callee, args } => {
            count_refs_expr(ast, *callee, refs);
            for a in args {
                count_refs_expr(ast, *a, refs);
            }
        }
        Expr::Assign { target, value } => {
            count_refs_expr(ast, *target, refs);
            count_refs_expr(ast, *value, refs);
        }
        Expr::Array(items) => {
            for e in items {
                count_refs_expr(ast, *e, refs);
            }
        }
        Expr::Spread { expr } => count_refs_expr(ast, *expr, refs),
        Expr::ObjectLit { fields } => {
            for (_, v) in fields {
                count_refs_expr(ast, *v, refs);
            }
        }
        Expr::ArrowFn { body, params, .. } => {
            for p in params {
                if let Some(d) = p.default {
                    count_refs_expr(ast, d, refs);
                }
            }
            for s in body {
                count_refs_stmt(ast, s, refs);
            }
        }
        Expr::Closure { fn_name, captures } => {
            *refs.entry(fn_name.clone()).or_insert(0) += 1;
            for c in captures {
                *refs.entry(c.clone()).or_insert(0) += 1;
            }
        }
        Expr::New { args, .. } => {
            for a in args {
                count_refs_expr(ast, *a, refs);
            }
        }
        Expr::Super { args } => {
            for a in args {
                count_refs_expr(ast, *a, refs);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            count_refs_expr(ast, *cond, refs);
            count_refs_expr(ast, *then_branch, refs);
            count_refs_expr(ast, *else_branch, refs);
        }
        Expr::TypeOf { expr } | Expr::PostIncr { target: expr, .. } => {
            count_refs_expr(ast, *expr, refs)
        }
        Expr::InstanceOf { expr, .. } => count_refs_expr(ast, *expr, refs),
        Expr::As { expr, .. } => count_refs_expr(ast, *expr, refs),
        Expr::Sequence { left, right } => {
            count_refs_expr(ast, *left, refs);
            count_refs_expr(ast, *right, refs);
        }
        Expr::Nullish { lhs, rhs } => {
            count_refs_expr(ast, *lhs, refs);
            count_refs_expr(ast, *rhs, refs);
        }
        Expr::Unary { expr, .. } => count_refs_expr(ast, *expr, refs),
        Expr::BinOp { left, right, .. } => {
            count_refs_expr(ast, *left, refs);
            count_refs_expr(ast, *right, refs);
        }
        Expr::String(_)
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Uninit
        | Expr::Regex { .. }
        | Expr::This
        | Expr::NewTarget => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lints(src: &str) -> Vec<Diagnostic> {
        lint(src).expect("lint")
    }

    fn has_message(diags: &[Diagnostic], substr: &str) -> bool {
        diags.iter().any(|d| d.message.contains(substr))
    }

    #[test]
    fn unused_let_flagged() {
        let src = "let x: i64 = 1\nconsole.log(2)\n";
        let diags = lints(src);
        assert!(has_message(&diags, "unused-let: `x`"));
    }

    #[test]
    fn used_let_clean() {
        let src = "let x: i64 = 1\nconsole.log(x)\n";
        let diags = lints(src);
        assert!(!has_message(&diags, "unused-let"));
    }

    #[test]
    fn dead_code_after_return() {
        let src = "function f(): i64 { return 1\nlet x: i64 = 2\nreturn x }\n";
        let diags = lints(src);
        assert!(has_message(&diags, "dead-code-after-return"));
    }

    #[test]
    fn shadowed_let() {
        let src = "let x: i64 = 1\nfunction f(): i64 { let x: i64 = 2\nreturn x }\nf()\n";
        let diags = lints(src);
        assert!(has_message(&diags, "shadowed-let: `x`"));
    }

    #[test]
    fn unreachable_catch_pure_body() {
        let src = "try { let n: i64 = 1 + 2 } catch (e) { console.log(e) }\n";
        let diags = lints(src);
        assert!(has_message(&diags, "unreachable-catch"));
    }

    #[test]
    fn reachable_catch_with_call_clean() {
        let src = "try { console.log('hi') } catch (e) { console.log(e) }\n";
        let diags = lints(src);
        assert!(!has_message(&diags, "unreachable-catch"));
    }

    #[test]
    fn unused_import_flagged() {
        // bare-source — we only lex / parse / lint; unresolved imports
        // don't fail because lint doesn't do module resolution.
        let src = "import { foo, bar } from './x'\nconsole.log(foo)\n";
        let diags = lints(src);
        assert!(has_message(&diags, "unused-import: `bar`"));
        assert!(!has_message(&diags, "unused-import: `foo`"));
    }
}
