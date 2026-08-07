//! Direct `eval` of a string literal, inlined at the call site.
//!
//! ## Why this is a desugar and not a runtime
//!
//! The reflex is that `eval` means shipping an interpreter, and that an
//! AOT compiler therefore cannot have it. Measurement says otherwise:
//! across the 1473 test262 cases tr rejects on the `eval` identifier,
//! **79.5% pass a string or template literal** — the program eval is
//! asked to run is already sitting in the source being compiled, it
//! merely happens to be written between quotes. For those, parsing the
//! text at compile time and lowering it at the call site is not a trick;
//! it is the same path every other statement in the file takes.
//! (Decomposition: `.claude/rfcs/20260807-eval/decomposition.md`.)
//!
//! ## The environment split, and why tr sits on the strict side of it
//!
//! §19.2.1.1 PerformEval splits the environment in two, and which half
//! `var` lands in depends on strictness:
//!
//! - **sloppy** direct eval shares the caller's *VariableEnvironment*,
//!   so `var` and function declarations leak into the enclosing
//!   function and outlive the eval;
//! - **strict** direct eval gets its own VariableEnvironment too, so
//!   nothing it declares escapes.
//!
//! tr compiles a strict TS subset, and bun — the reference — treats a
//! `.ts` file as an ES module, which is strict. Measured, not assumed:
//! `function f() { eval("var a = 1"); console.log(a) }` raises
//! `ReferenceError: a is not defined` under bun. An earlier draft of
//! this pass inlined a bare `Stmt::Block`, which let `var` through via
//! `desugar_var_hoist` and printed `1` — correct for sloppy mode,
//! wrong for the mode tr is in.
//!
//! So the inlined statements are *sealed*: every `var` in eval's own
//! variable scope is re-tagged block-scoped, which makes the enclosing
//! `Stmt::Block` the whole environment — both halves of the split at
//! once, which is what strict eval asks for. Sealing stops at a nested
//! `FnDecl` body: that body is its own VariableEnvironment and its
//! `var`s belong to it, not to the eval.
//!
//! This is why the sloppy-only corpus is out of reach here rather than
//! merely unimplemented: 697 of the 1473 eval-blocked cases (47.3%)
//! carry `flags: [noStrict]`, and 404 of the 405 in
//! `annexB/language/eval-code` do — that directory tests the Annex B
//! web-compat extension whose own description says it is "not honored
//! in strict mode". A strict runtime cannot satisfy them by
//! implementing more; they need a second language mode.
//!
//! ## What this pass deliberately does NOT do
//!
//! - **Only statement position.** `eval(…)` whose result is consumed
//!   needs the *completion value* — "the value of the last statement
//!   that produced one" — which tr has no counterpart for. 85% of the
//!   core eval-code cases discard the result, so the scoping semantics
//!   can land first and completion value follows as its own piece of
//!   work. An `eval` in value position is left untouched and keeps
//!   reporting the honest `unknown identifier` rather than silently
//!   evaluating to something wrong.
//! - **Only literal source.** A runtime string needs the compiler in
//!   the artifact; that is a separate layer with its own cost to
//!   measure (artifact size is a headline property of tr).
//! - **Only when the program has not rebound `eval`.** If any binding
//!   named `eval` exists anywhere, the pass declines wholesale — a
//!   local `eval` is not this `eval`, and proving which one a given
//!   call site sees needs scope resolution this pass runs before.
//! - **Parse failure leaves the call alone.** §19.2.1.1 step 12 wants a
//!   SyntaxError thrown at runtime; until that exists, the call keeps
//!   its current diagnostic instead of being quietly dropped.

use super::{Ast, Expr, Stmt};
use crate::{lexer, parser};

/// Inline every statement-position `eval("…")` whose source text is a
/// literal. See the module doc for the boundaries.
pub fn desugar_eval(ast: &mut Ast) {
    if binds_eval(ast) {
        return;
    }
    // Take the statement list so the recursive rewrite can hand `ast`
    // to `parse_into`, which appends to `ast.stmts` and needs it to
    // itself. Every parse drains what it appended, so the arena is back
    // to empty before the next one.
    let mut stmts = std::mem::take(&mut ast.stmts);
    rewrite_list(&mut stmts, ast);
    ast.stmts = stmts;
}

/// Does any binding in the program shadow the global `eval`? A `var` /
/// `let` / parameter / function named `eval` means a call site may be
/// reaching something else entirely, and this pass runs before the
/// scope resolution that could tell which. Declining wholesale is the
/// answer that cannot be wrong.
fn binds_eval(ast: &Ast) -> bool {
    fn in_list(stmts: &[Stmt]) -> bool {
        stmts.iter().any(in_stmt)
    }
    fn in_stmt(s: &Stmt) -> bool {
        match s {
            Stmt::LetDecl { name, .. } => name == "eval",
            Stmt::FnDecl {
                name, params, body, ..
            } => name == "eval" || params.iter().any(|p| p.name == "eval") || in_list(body),
            Stmt::ClassDecl { name, .. } => name == "eval",
            Stmt::Try {
                catch_param,
                body,
                catch_body,
                finally_body,
                ..
            } => {
                catch_param.as_deref() == Some("eval")
                    || in_list(body)
                    || in_list(catch_body)
                    || finally_body.as_deref().is_some_and(in_list)
            }
            Stmt::Block(b) | Stmt::Multi(b) => in_list(b),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => in_stmt(then_branch) || else_branch.as_deref().is_some_and(in_stmt),
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => in_stmt(body),
            Stmt::Labeled { body, .. } => in_stmt(body),
            Stmt::For { init, body, .. } => init.as_deref().is_some_and(in_stmt) || in_stmt(body),
            Stmt::ForOf { var_name, body, .. } => var_name == "eval" || in_stmt(body),
            Stmt::Switch { cases, default, .. } => {
                cases.iter().any(|c| in_list(&c.body)) || default.as_deref().is_some_and(in_list)
            }
            _ => false,
        }
    }
    in_list(&ast.stmts)
}

fn rewrite_list(stmts: &mut Vec<Stmt>, ast: &mut Ast) {
    for s in stmts.iter_mut() {
        rewrite_stmt(s, ast);
    }
}

fn rewrite_stmt(s: &mut Stmt, ast: &mut Ast) {
    // The rewrite itself: a statement that is nothing but a call to
    // `eval` with one literal argument becomes the block that literal
    // parses to.
    if let Stmt::Expr(eid) = s {
        if let Some(src) = literal_eval_source(*eid, ast) {
            if let Some(mut inlined) = parse_eval_source(&src, ast) {
                // An eval inside the inlined text is an eval like any
                // other; the nesting is finite because each level is a
                // literal written in the level above.
                rewrite_list(&mut inlined, ast);
                seal_var_scope(&mut inlined);
                *s = Stmt::Block(inlined);
                return;
            }
        }
    }
    match s {
        Stmt::Block(b) | Stmt::Multi(b) => rewrite_list(b, ast),
        Stmt::FnDecl { body, .. } => rewrite_list(body, ast),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_stmt(then_branch, ast);
            if let Some(e) = else_branch.as_deref_mut() {
                rewrite_stmt(e, ast);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => rewrite_stmt(body, ast),
        Stmt::Labeled { body, .. } => rewrite_stmt(body, ast),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init.as_deref_mut() {
                rewrite_stmt(i, ast);
            }
            rewrite_stmt(body, ast);
        }
        Stmt::ForOf { body, .. } => rewrite_stmt(body, ast),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            rewrite_list(body, ast);
            rewrite_list(catch_body, ast);
            if let Some(f) = finally_body.as_mut() {
                rewrite_list(f, ast);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases.iter_mut() {
                rewrite_list(&mut c.body, ast);
            }
            if let Some(d) = default.as_mut() {
                rewrite_list(d, ast);
            }
        }
        _ => {}
    }
}

/// Re-tag every `var` belonging to eval's own variable scope as
/// block-scoped, so the `Stmt::Block` the statements land in becomes
/// their whole environment (§19.2.1.1 strict branch — see module doc).
///
/// The walk descends through blocks and control flow, which share the
/// eval's VariableEnvironment, and stops at a nested `FnDecl` body,
/// which has its own.
fn seal_var_scope(stmts: &mut [Stmt]) {
    for s in stmts.iter_mut() {
        match s {
            Stmt::LetDecl { is_var, .. } => *is_var = false,
            // A nested function's `var`s are its own — do not descend.
            Stmt::FnDecl { .. } => {}
            Stmt::Block(b) | Stmt::Multi(b) => seal_var_scope(b),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                seal_var_scope(std::slice::from_mut(then_branch));
                if let Some(e) = else_branch.as_deref_mut() {
                    seal_var_scope(std::slice::from_mut(e));
                }
            }
            Stmt::While { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::Labeled { body, .. }
            | Stmt::ForOf { body, .. } => seal_var_scope(std::slice::from_mut(body)),
            Stmt::For { init, body, .. } => {
                if let Some(i) = init.as_deref_mut() {
                    seal_var_scope(std::slice::from_mut(i));
                }
                seal_var_scope(std::slice::from_mut(body));
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                seal_var_scope(body);
                seal_var_scope(catch_body);
                if let Some(f) = finally_body.as_mut() {
                    seal_var_scope(f);
                }
            }
            Stmt::Switch { cases, default, .. } => {
                for c in cases.iter_mut() {
                    seal_var_scope(&mut c.body);
                }
                if let Some(d) = default.as_mut() {
                    seal_var_scope(d);
                }
            }
            _ => {}
        }
    }
}

/// `eval("…")` — a call whose callee is the bare identifier `eval` and
/// whose single argument is a string literal. Answers the source text.
fn literal_eval_source(eid: super::ExprId, ast: &Ast) -> Option<String> {
    let Expr::Call { callee, args } = ast.exprs.get(eid.0 as usize)? else {
        return None;
    };
    if args.len() != 1 {
        return None;
    }
    match ast.exprs.get(callee.0 as usize)? {
        Expr::Ident(n) if n == "eval" => {}
        _ => return None,
    }
    match ast.exprs.get(args[0].0 as usize)? {
        Expr::String(s) => Some(s.clone()),
        _ => None,
    }
}

/// Parse eval's source text into the caller's arena. `parse_into`
/// appends to `ast.stmts` and shares the expression arena, so the
/// statements come back already numbered for the program they are being
/// spliced into — the same mechanism `modules::resolve_imports` uses to
/// merge an imported file. `None` on a lex or parse failure, which
/// leaves the call site untouched.
fn parse_eval_source(src: &str, ast: &mut Ast) -> Option<Vec<Stmt>> {
    let tokens = lexer::tokenize(src).ok()?;
    let offset = parser::parse_into(src, &tokens, ast).ok()?;
    Some(ast.stmts.drain(offset..).collect())
}
