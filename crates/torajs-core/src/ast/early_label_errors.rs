//! §14.8.1 / §14.9.1 `break` / `continue` early errors.
//!
//! Four Syntax Errors, one rule each, all decided statically from the
//! enclosing label set:
//!
//! - `break L` where no enclosing LabelledStatement carries `L`;
//! - `continue L` where none carries it, *or* where the one that does
//!   does not label an **IterationStatement** — a labelled `switch` is
//!   a legal `break` target and an illegal `continue` one, and so is a
//!   labelled block;
//! - bare `break` outside any IterationStatement or SwitchStatement;
//! - bare `continue` outside any IterationStatement.
//!
//! ssa_lower already decided all of this, correctly, and said so —
//! `resolve_label`'s comment reads "a valid program never reaches
//! this" and `lower_continue`'s reads "a labeled non-loop target is a
//! syntax error upstream". Upstream was never written, so the verdict
//! came out of the LOWERING as `not yet supported: ssa-lower: …`,
//! which says tr has not implemented something when what actually
//! happened is that tr diagnosed a SyntaxError in the program. A
//! negative test asking for a parse-phase SyntaxError reads that as
//! the wrong phase; four test262 `for` cases were only counted right
//! because a different, stricter rejection happened to fire first
//! (the `__`-prefix undeclared-name reject their `__str=""` used to
//! hit, rotation 573). And ssa_lower's `stmt_is_loop_like` answers
//! BOTH questions with one predicate, so `sw: switch (1) { case 1:
//! continue sw; }` — a Syntax Error — ran to completion.
//!
//! Function boundaries reset the label set (§14.9.1: "not crossing
//! function or static initialization block boundaries"), which is what
//! lets every function body be visited with a fresh, empty stack —
//! and that in turn is why this needs no expression traversal at all.
//! Statement nesting is walked; every `Expr::ArrowFn` in the arena
//! (arrow, function expression, generator, method — they share the
//! node) is then visited from the arena directly, because where it
//! sits cannot change the answer.
//!
//! Runs on the RAW AST, beside `early_redecl_errors` and for the same
//! reason: the generator / async / for-of desugars rewrite loops, and
//! a rewritten loop is not the shape the spec asks about.

use super::{Ast, Expr, Stmt};

/// One entry per enclosing LabelledStatement: the label, and whether
/// the statement it labels is an IterationStatement (through any
/// stack of further labels — `a: b: for (…)` labels a loop twice).
type LabelFrame = (String, bool);

/// The enclosing breakable / continuable context, independent of
/// labels: bare `break` wants either, bare `continue` wants a loop.
#[derive(Clone, Copy, Default)]
struct Enclosing {
    loop_: bool,
    switch: bool,
}

pub fn early_label_errors(ast: &Ast) -> Option<String> {
    let mut err = None;
    walk_block(&ast.stmts, &mut Vec::new(), Enclosing::default(), &mut err);
    // Every function body, wherever it sits: the label set resets at
    // the boundary, so the context it is reached through is irrelevant.
    for e in &ast.exprs {
        if let Expr::ArrowFn { body, .. } = e {
            walk_block(body, &mut Vec::new(), Enclosing::default(), &mut err);
        }
    }
    err
}

fn is_iteration(s: &Stmt) -> bool {
    match s {
        Stmt::For { .. }
        | Stmt::While { .. }
        | Stmt::DoWhile { .. }
        | Stmt::ForOf { .. }
        | Stmt::ForOfSplitIter { .. } => true,
        Stmt::Labeled { body, .. } => is_iteration(body),
        _ => false,
    }
}

fn walk_block(
    stmts: &[Stmt],
    labels: &mut Vec<LabelFrame>,
    enc: Enclosing,
    err: &mut Option<String>,
) {
    for s in stmts {
        walk(s, labels, enc, err);
    }
}

fn in_loop(enc: Enclosing) -> Enclosing {
    Enclosing {
        loop_: true,
        switch: enc.switch,
    }
}

fn walk(s: &Stmt, labels: &mut Vec<LabelFrame>, enc: Enclosing, err: &mut Option<String>) {
    if err.is_some() {
        return;
    }
    match s {
        Stmt::Break(None) => {
            if !enc.loop_ && !enc.switch {
                *err = Some("`break` must be inside a loop or switch (ES 14.9.1)".to_string());
            }
        }
        Stmt::Continue(None) => {
            if !enc.loop_ {
                *err = Some("`continue` must be inside a loop (ES 14.8.1)".to_string());
            }
        }
        Stmt::Break(Some(l)) => {
            if !labels.iter().any(|(n, _)| n == l) {
                *err = Some(format!("undefined label `{l}` in `break` (ES 14.9.1)"));
            }
        }
        Stmt::Continue(Some(l)) => {
            match labels.iter().rev().find(|(n, _)| n == l) {
                None => {
                    *err = Some(format!("undefined label `{l}` in `continue` (ES 14.8.1)"));
                }
                // The label exists but labels a switch or a block:
                // `continue` requires an IterationStatement.
                Some((_, false)) => {
                    *err = Some(format!(
                        "label `{l}` does not label a loop, so `continue {l}` is not allowed (ES 14.8.1)"
                    ));
                }
                Some((_, true)) => {}
            }
        }
        Stmt::Labeled { label, body } => {
            labels.push((label.clone(), is_iteration(body)));
            walk(body, labels, enc, err);
            labels.pop();
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            walk(body, labels, in_loop(enc), err)
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                walk(i, labels, enc, err);
            }
            walk(body, labels, in_loop(enc), err);
        }
        Stmt::ForOf { body, .. } => walk(body, labels, in_loop(enc), err),
        Stmt::ForOfSplitIter { body, .. } => walk(body, labels, in_loop(enc), err),
        Stmt::Switch { cases, default, .. } => {
            let inner = Enclosing {
                loop_: enc.loop_,
                switch: true,
            };
            for c in cases {
                walk_block(&c.body, labels, inner, err);
            }
            if let Some(d) = default {
                walk_block(d, labels, inner, err);
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            walk(then_branch, labels, enc, err);
            if let Some(eb) = else_branch {
                walk(eb, labels, enc, err);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            walk_block(body, labels, enc, err);
            walk_block(catch_body, labels, enc, err);
            if let Some(fb) = finally_body {
                walk_block(fb, labels, enc, err);
            }
        }
        Stmt::Block(inner) | Stmt::Multi(inner) => walk_block(inner, labels, enc, err),
        // Function boundary — §14.9.1 does not cross it, so the body
        // starts from nothing.
        Stmt::FnDecl { body, .. } => walk_block(body, &mut Vec::new(), Enclosing::default(), err),
        Stmt::ClassDecl {
            methods,
            static_methods,
            ..
        } => {
            for m in methods.iter().chain(static_methods) {
                walk_block(&m.body, &mut Vec::new(), Enclosing::default(), err);
            }
        }
        Stmt::ExportDecl { inner, .. } => {
            if let Some(i) = inner.as_deref() {
                walk(i, labels, enc, err);
            }
        }
        _ => {}
    }
}
