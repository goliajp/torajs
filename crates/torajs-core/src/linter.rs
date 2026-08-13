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

mod can_throw;
pub(crate) mod refs;

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, ExprId, Param, Stmt};
use crate::check::{Diagnostic, Severity};
use crate::lexer::{self, Span};
use crate::parser;

use can_throw::block_can_throw;
use refs::count_refs_stmt;

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
    let ast = parser::parse(source, &tokens).map_err(LintError::Parse)?;
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
            Stmt::LetDecl { name, init, .. } | Stmt::UsingDecl { name, init, .. } => {
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
                Stmt::Return(_) | Stmt::Throw(_) | Stmt::Break(_) | Stmt::Continue(_)
            ) {
                diverged = true;
            }
        }
    }
}

fn scope_from_params(params: &[Param]) -> HashSet<String> {
    params.iter().map(|p| p.name.clone()).collect()
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
