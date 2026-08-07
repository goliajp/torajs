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
//! `Stmt::Block` the whole environment for `var`. Sealing stops at a
//! nested `FnDecl` body: that body is its own VariableEnvironment and
//! its `var`s belong to it, not to the eval.
//!
//! **A function declaration still escapes, and that is a defect this
//! pass inherits rather than introduces.** `eval("{ function f() {} }")`
//! leaves `f` visible afterwards under tr and does not under bun — but
//! so does a plain `{ function f() {} }` with no eval anywhere, which
//! is where the difference actually lives (§14.2 says a block-level
//! function declaration is block-scoped; Annex B B.3.3 hoists it only
//! in sloppy mode, and tr is not in sloppy mode). Until that is fixed,
//! `var` here follows the strict rule and `function` follows the
//! sloppy one, which is not a coherent mode. Twenty-six
//! `annexB/language/eval-code` cases currently report `pass` off the
//! back of it — they are asking for the sloppy behaviour and getting
//! it by accident, so they are counted as water under the
//! no-metric-inflation rule and will fall back out when the block
//! scoping is corrected.
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
//! - **Only the single-expression completion value.** A source that is
//!   exactly one ExpressionStatement collapses to that expression, which
//!   is exact and works in value position. The *general* completion
//!   value — `eval("if (true) { }")` is `undefined`, `eval("1; ;")` is
//!   `1` — has no counterpart in tr, so a multi-statement source in
//!   value position is left alone and keeps reporting the honest
//!   `unknown identifier` rather than silently evaluating to something
//!   wrong. 85% of the core eval-code cases discard the result
//!   entirely, so this boundary costs little.
//! - **Only literal source.** A runtime string needs the compiler in
//!   the artifact; that is a separate layer with its own cost to
//!   measure (artifact size is a headline property of tr).
//! - **Only when the program has not rebound `eval`.** If any binding
//!   named `eval` exists anywhere, the pass declines wholesale — a
//!   local `eval` is not this `eval`, and proving which one a given
//!   call site sees needs scope resolution this pass runs before.
//! - **A parse failure in statement position becomes a throw**, per
//!   §19.2.1.1 step 12 — raised when the eval is reached, so
//!   `if (false) { eval("((("); }` still runs to completion. In value
//!   position it is still left alone: JavaScript has no throw
//!   expression, so that shape needs a statement-level rewrite this
//!   pass does not do yet.

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
    // Single-expression sources first: those collapse to the
    // expression itself and are then no longer eval calls, so the
    // block-inlining walks below see only the multi-statement ones.
    rewrite_single_expr_evals(ast);
    let mut stmts = std::mem::take(&mut ast.stmts);
    rewrite_list(&mut stmts, ast);
    ast.stmts = stmts;
    rewrite_arrow_bodies(ast);
}

/// `eval("<one expression>")` becomes that expression, wherever it sits.
///
/// This is the one shape whose completion value needs no machinery: a
/// source consisting of a single ExpressionStatement completes with
/// that expression's value (§14.5.1), so replacing the call node with
/// the parsed expression is exact — and it works in value position,
/// which the block inlining cannot reach. `assert.sameValue(eval("1 +
/// 1"), 2)` is this shape, and so is most of what test262 writes when
/// it wants eval's result rather than its effects.
///
/// The general completion value — `eval("if (true) { }")` is
/// `undefined`, `eval("1; ;")` is `1` — stays out of reach and out of
/// scope here; those need a notion tr has no counterpart for. Sources
/// that are not exactly one expression statement fall through
/// untouched and are picked up by the block inlining if they are in
/// statement position.
///
/// The replacement writes over the Call node in place rather than
/// re-pointing the parent at a new id, so every existing reference to
/// this ExprId keeps working without a remap. The parsed expression's
/// own node stays in the arena unreferenced, which costs a slot and
/// nothing else.
fn rewrite_single_expr_evals(ast: &mut Ast) {
    let mut i = 0;
    while i < ast.exprs.len() {
        let Some(src) = literal_eval_source(super::ExprId(i as u32), ast) else {
            i += 1;
            continue;
        };
        if let Some(parsed) = parse_eval_source(&src, ast) {
            if let [Stmt::Expr(inner)] = parsed[..] {
                if let Some(e) = ast.exprs.get(inner.0 as usize).cloned() {
                    ast.exprs[i] = e;
                    // Do not advance: the expression just written in
                    // may itself be an `eval("…")` that came out of a
                    // nested literal, and it now occupies this slot.
                    continue;
                }
            }
        }
        i += 1;
    }
}

/// The statement walk above reaches a `FnDecl` body because that body
/// hangs off the statement tree. A function written in *expression*
/// position does not: `() => { … }`, `function () { … }` and a method
/// shorthand all park their statements inside an `Expr::ArrowFn` in the
/// expression arena, which no statement walk visits. Missing them is
/// not a corner: `closure __closure_N references unknown identifier
/// eval` accounts for ~390 of the eval-blocked cases on its own, and
/// `assert.throws(…, function () { eval("…") })` is how test262 spells
/// most of its eval assertions.
///
/// The loop re-reads `len()` each turn rather than snapshotting it,
/// because inlining appends to the arena — an arrow written inside an
/// eval'd literal lands past the original end and still has to be
/// visited. Each body is taken out before the rewrite so `ast` can be
/// handed to `parse_into`, then put back.
fn rewrite_arrow_bodies(ast: &mut Ast) {
    let mut i = 0;
    while i < ast.exprs.len() {
        let Some(Expr::ArrowFn { body, .. }) = ast.exprs.get_mut(i) else {
            i += 1;
            continue;
        };
        let mut taken = std::mem::take(body);
        rewrite_list(&mut taken, ast);
        if let Some(Expr::ArrowFn { body, .. }) = ast.exprs.get_mut(i) {
            *body = taken;
        }
        i += 1;
    }
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
    if in_list(&ast.stmts) {
        return true;
    }
    // Arrows live in the expression arena, so a parameter or body
    // binding named `eval` there is invisible to the statement walk.
    ast.exprs.iter().any(|e| match e {
        Expr::ArrowFn { params, body, .. } => {
            params.iter().any(|p| p.name == "eval") || in_list(body)
        }
        _ => false,
    })
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
            match parse_eval_source(&src, ast) {
                Some(mut inlined) => {
                    // An eval inside the inlined text is an eval like
                    // any other; the nesting is finite because each
                    // level is a literal written in the level above.
                    rewrite_list(&mut inlined, ast);
                    seal_var_scope(&mut inlined);
                    *s = Stmt::Block(inlined);
                    return;
                }
                None => {
                    // The text does not parse. §19.2.1.1 step 12 wants
                    // a SyntaxError at evaluation time — see
                    // `syntax_error_throw` on why this is a throw and
                    // not a compile error.
                    *s = syntax_error_throw(format!("eval: {}", first_line(&src)), ast);
                    return;
                }
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
///
/// It does NOT seal the function declaration itself: nothing here can,
/// because a block-level `function` escapes its block throughout tr,
/// with or without an eval. See the module doc — that is a separate
/// defect, and sealing it from this side would paper over the general
/// case while leaving it wrong everywhere else.
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
    // `parse_into` assigns `*target = p.ast` BEFORE propagating the
    // error, so a failed parse leaves whatever it managed to build
    // appended to `ast.stmts`. Those statements belong to nobody —
    // without this truncate they would be spliced into the program as
    // if the user had written them.
    let before = ast.stmts.len();
    let tokens = lexer::tokenize(src).ok()?;
    match parser::parse_into(src, &tokens, ast) {
        Ok(offset) => Some(ast.stmts.drain(offset..).collect()),
        Err(_) => {
            ast.stmts.truncate(before);
            None
        }
    }
}

/// The first line of the offending source, capped, for the error
/// message. A whole eval'd program in a diagnostic is noise.
fn first_line(src: &str) -> String {
    let line = src.lines().next().unwrap_or("").trim();
    if line.chars().count() > 60 {
        let cut: String = line.chars().take(60).collect();
        format!("{cut}…")
    } else {
        line.to_string()
    }
}

/// A `throw new SyntaxError(<message>)` statement.
///
/// §19.2.1.1 step 12 wants the SyntaxError raised **when the eval is
/// evaluated**, not when the program is compiled — `if (false) {
/// eval("((("); }` runs to completion under bun. Replacing the call
/// with a throw keeps that: unreachable code raises nothing, and a
/// reached one raises at the right moment with the right error type.
fn syntax_error_throw(msg: String, ast: &mut Ast) -> Stmt {
    let msg_id = ast.add_expr(Expr::String(msg));
    let exc = ast.add_expr(Expr::New {
        class_name: "SyntaxError".to_string(),
        args: vec![msg_id],
        type_args: Vec::new(),
    });
    Stmt::Throw(exc)
}
