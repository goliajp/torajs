//! Recognition and parsing for the eval desugar: which call sites are
//! eval calls, what text they pass, and how that text becomes
//! statements in the caller's arena. Split out of `desugar_eval.rs`
//! when the indirect form pushed the file past the 500-line cap.

use super::super::{Ast, Expr, ExprId, Stmt};
use crate::{lexer, parser};

/// How the call site reaches `eval` — the distinction §19.2.1.1 hangs
/// the entire environment story on.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum CallForm {
    /// `eval("…")` — the callee is the bare identifier. Evaluates in
    /// the caller's scope; strict (tr's mode), so nothing leaks.
    Direct,
    /// `(0, eval)("…")` — the callee is a comma expression whose value
    /// is `eval`. Evaluates in the GLOBAL scope: it cannot see the
    /// caller's locals, and its `var`s (sloppy eval code — indirect
    /// eval does not inherit the caller's strictness) land on the
    /// global.
    Indirect,
}

/// `eval("…")` or `(0, eval)("…")` — a call whose callee reaches
/// `eval` and whose single argument is a string literal. Answers the
/// source text and the call form.
///
/// The indirect spelling accepted here is exactly the comma shape with
/// a LITERAL left operand — `(0, eval)`, `(1, eval)` — which is how
/// test262 writes it (263 of the 311 indirect-shaped blocked cases).
/// A non-literal left operand would have to be evaluated for effect
/// before the call, so it is left alone. Alias forms (`var e = eval;
/// e("…")`), `eval.call`, and `globalThis.eval` need value tracking or
/// a runtime eval object and stay out of scope.
pub(super) fn literal_eval_call(eid: ExprId, ast: &Ast) -> Option<(String, CallForm)> {
    let Expr::Call { callee, args } = ast.exprs.get(eid.0 as usize)? else {
        return None;
    };
    if args.len() != 1 {
        return None;
    }
    let form = callee_eval_form(*callee, ast)?;
    match ast.exprs.get(args[0].0 as usize)? {
        Expr::String(s) => Some((s.clone(), form)),
        _ => None,
    }
}

/// `eval(7)` / `(0, eval)(false)` — a call reaching `eval` whose single
/// argument is a NON-string literal. §19.2.1.1 step 2: if the argument
/// is not a String, eval returns it unchanged, so the call collapses to
/// the argument itself. Only literals qualify — a variable might hold a
/// string at runtime, and the compiler cannot know.
pub(super) fn nonstring_literal_eval_arg(eid: ExprId, ast: &Ast) -> Option<Expr> {
    let Expr::Call { callee, args } = ast.exprs.get(eid.0 as usize)? else {
        return None;
    };
    if args.len() != 1 {
        return None;
    }
    callee_eval_form(*callee, ast)?;
    match ast.exprs.get(args[0].0 as usize)? {
        e @ (Expr::Number(_) | Expr::Bool(_)) => Some(e.clone()),
        _ => None,
    }
}

fn callee_eval_form(callee: ExprId, ast: &Ast) -> Option<CallForm> {
    match ast.exprs.get(callee.0 as usize)? {
        Expr::Ident(n) if n == "eval" => Some(CallForm::Direct),
        Expr::Sequence { left, right } => {
            let is_lit = matches!(
                ast.exprs.get(left.0 as usize)?,
                Expr::Number(_) | Expr::String(_) | Expr::Bool(_) | Expr::Null
            );
            match ast.exprs.get(right.0 as usize)? {
                Expr::Ident(n) if is_lit && n == "eval" => Some(CallForm::Indirect),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Does the parsed source open with a `"use strict"` directive? Per
/// §19.2.1.1 steps 3-5 the eval code's strictness is its OWN — an
/// indirect eval is sloppy unless its source says otherwise, and this
/// prologue is how it says otherwise: with it, the eval gets its own
/// VariableEnvironment and nothing it declares escapes, exactly the
/// direct-strict treatment. The directive parses as an ordinary
/// expression statement holding the string, which is also why it is
/// the source's completion value when nothing after it produces one.
pub(super) fn has_use_strict_prologue(parsed: &[Stmt], ast: &Ast) -> bool {
    matches!(parsed.first(), Some(Stmt::Expr(e))
        if matches!(ast.exprs.get(e.0 as usize), Some(Expr::String(s)) if s == "use strict"))
}

/// Parse eval's source text into the caller's arena. `parse_into`
/// appends to `ast.stmts` and shares the expression arena, so the
/// statements come back already numbered for the program they are being
/// spliced into — the same mechanism `modules::resolve_imports` uses to
/// merge an imported file. `None` on a lex or parse failure, which
/// leaves the call site untouched.
pub(super) fn parse_eval_source(src: &str, ast: &mut Ast) -> Option<Vec<Stmt>> {
    if let Some(stmts) = parse_once(src, ast) {
        return Some(stmts);
    }
    // §12.9.1 rule 2 — automatic semicolon insertion at end of input:
    // when the token stream ends where the grammar still wants a
    // terminator, a semicolon is inserted. `eval("var x")` is valid
    // JavaScript by exactly this rule, and it is how test262 writes it
    // (a file always ends in a newline, so tr's parser never meets the
    // no-terminator end anywhere else). Retrying with the semicolon
    // appended IS that rule — it cannot make an invalid source valid,
    // because a semicolon only ever terminates a final statement.
    let with_semi = format!("{src};");
    parse_once(&with_semi, ast)
}

/// One tokenize + parse attempt into the shared arena.
///
/// The truncate on the error path is load-bearing: `parse_into`
/// assigns `*target = p.ast` BEFORE propagating its error, so a failed
/// parse leaves whatever it managed to build appended to `ast.stmts` —
/// statements belonging to nobody, which would otherwise be spliced
/// into the program as if the user had written them.
fn parse_once(src: &str, ast: &mut Ast) -> Option<Vec<Stmt>> {
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
pub(super) fn first_line(src: &str) -> String {
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
pub(super) fn syntax_error_throw(msg: String, ast: &mut Ast) -> Stmt {
    let msg_id = ast.add_expr(Expr::String(msg));
    let exc = ast.add_expr(Expr::New {
        class_name: "SyntaxError".to_string(),
        args: vec![msg_id],
        type_args: Vec::new(),
    });
    Stmt::Throw(exc)
}
