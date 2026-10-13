//! What IS an eval call, and what kind — the recognizers `desugar_eval`
//! consults before any text is parsed. Split from `source.rs`
//! (rotation 618) along the seam the two halves already had: these
//! answer questions about the CALL SITE, and what stays behind answers
//! what the call's TEXT becomes. Verbatim move.

use super::super::{Ast, BinOp, Expr, ExprId, Stmt};
use super::source::CallForm;

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
    Some((const_string(args[0], ast)?, form))
}

/// The compile-time string value of an expression, when it has one: a
/// string literal, or a `+` tree of such. test262's generated Annex B
/// cases build their source with literal concatenation —
/// `eval('switch (1) {' + '  case 1:' + …)` — and §13.15.3 makes
/// String + String exact concatenation, so folding it loses nothing.
/// Mixed operands (`'a' + 1`) are left alone: they coerce, and the
/// coercion rules are the runtime's business, not this pass's.
pub(super) fn const_string(eid: ExprId, ast: &Ast) -> Option<String> {
    match ast.exprs.get(eid.0 as usize)? {
        Expr::String(s) => Some(s.to_string_lossy_owned()),
        Expr::BinOp {
            op: BinOp::Add,
            left,
            right,
        } => {
            let mut s = const_string(*left, ast)?;
            s.push_str(&const_string(*right, ast)?);
            Some(s)
        }
        _ => None,
    }
}

/// `eval(7)` / `(0, eval)(false)` — a call reaching `eval` whose single
/// argument is a NON-string literal. §19.2.1.1 step 2: if the argument
/// is not a String, eval returns it unchanged, so the call collapses to
/// the argument itself. Only literals qualify — a variable might hold a
/// string at runtime, and the compiler cannot know. The zero-argument
/// call is the same step over the missing argument: `eval()` is
/// `eval(undefined)`, which returns `undefined`.
pub(super) fn nonstring_literal_eval_arg(eid: ExprId, ast: &Ast) -> Option<Expr> {
    let Expr::Call { callee, args } = ast.exprs.get(eid.0 as usize)? else {
        return None;
    };
    if args.is_empty() {
        callee_eval_form(*callee, ast)?;
        return Some(Expr::Ident("undefined".to_string()));
    }
    if args.len() != 1 {
        return None;
    }
    callee_eval_form(*callee, ast)?;
    match ast.exprs.get(args[0].0 as usize)? {
        e @ (Expr::Number(_) | Expr::Bool(_)) => Some(e.clone()),
        _ => None,
    }
}

pub(super) fn callee_eval_form(callee: ExprId, ast: &Ast) -> Option<CallForm> {
    match ast.exprs.get(callee.0 as usize)? {
        Expr::Ident(n) if n == "eval" => Some(CallForm::Direct),
        Expr::Ident(n) if n == "__t262_evalScript" => Some(CallForm::Script),
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
        if matches!(ast.exprs.get(e.0 as usize), Some(Expr::String(s)) if s == "use strict")
            && !ast.escaped_use_strict.contains(e))
}
