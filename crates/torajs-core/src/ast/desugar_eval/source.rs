//! Recognition and parsing for the eval desugar: which call sites are
//! eval calls, what text they pass, and how that text becomes
//! statements in the caller's arena. Split out of `desugar_eval.rs`
//! when the indirect form pushed the file past the 500-line cap.

use super::super::{Ast, BinOp, Expr, ExprId, Stmt};
use crate::{lexer, parser};

/// How the call site reaches `eval` — the distinction §19.2.1.1 hangs
/// the entire environment story on.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum CallForm {
    /// `eval("…")` — the callee is the bare identifier. Evaluates in
    /// the caller's scope, and inherits its strictness (§19.2.1.1
    /// step 8) — which is what
    /// [`super::walk::caller_strict_exprs`] answers per call site.
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
///
/// `super_ok` is the §19.2.1.1 steps 4-6 verdict: a DIRECT eval whose
/// call site sits in a class member body (through arrows) may contain
/// SuperProperty; everywhere else — indirect always, direct in global
/// or ordinary-function code — `super` in the text is an early error,
/// surfaced as this parse failing, which the callers turn into the
/// runtime SyntaxError step 12 wants. The parser's own position gate
/// (`super_prop_allowed`) does the refusing, so the whole source is
/// rejected before any of it could run (`(0, eval)('executed = true;
/// super.x;')` must leave `executed` untouched).
///
/// Is the text about to be parsed strict mode code? §19.2.1.1 step 2
/// asks it once and three parts of this function answer to it, so it
/// arrives as one verdict rather than one flag per consumer:
///
/// - the parse itself runs with `in_strict_fn` seeded from it, which
///   puts the text under every judge the parser already owns —
///   `yield` / §12.7.2 reserved words as identifiers, the Annex B
///   function-declaration positions (§B.3.2 / §B.3.4), duplicate
///   parameters (§15.1.2), `with` (§14.11.1);
/// - the Annex B legacy-octal spellings (§B.1.1 / §B.1.2), which the
///   lexer records rather than refuses, are read back below;
/// - `delete <bare name>` and the §15.2.1 assignment-target sites are
///   the §13.5.1.1 / §13.15.1 early errors under `Strict` and fold to
///   their §13.5.1.2 constants under `Sloppy`.
///
/// Who answers it: a DIRECT eval inherits the calling code's
/// strictness ([`super::walk::caller_strict_exprs`]); an INDIRECT one
/// never does (step 3 — its code is global sloppy code whatever the
/// caller was); a `Function(...)` body is strict only on its own
/// prologue (§20.2.1.1). Text that carries its OWN `"use strict"` is
/// strict either way, and the parser sees that prologue for itself.
///
/// A refusal neutralizes the freshly parsed segment before returning:
/// a failed parse leaves its expressions in the arena, and the goal
/// triages walk the WHOLE arena, orphans included.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum EvalStrictness {
    Strict,
    Sloppy,
}

/// The verdict for one call site, from its form and its arena slot:
/// a DIRECT eval inherits the calling code's strictness, an INDIRECT
/// one never does (§19.2.1.1 step 3 — its code is global sloppy code
/// whatever the caller was). `caller_strict` is
/// [`super::walk::caller_strict_exprs`]; a slot past its end is an
/// appended one and reads sloppy, per that function's doc.
pub(super) fn eval_strictness(
    form: CallForm,
    slot: usize,
    caller_strict: &[bool],
) -> EvalStrictness {
    if form == CallForm::Direct && caller_strict.get(slot).copied().unwrap_or(false) {
        EvalStrictness::Strict
    } else {
        EvalStrictness::Sloppy
    }
}

/// Why a parse attempt refused the text. `NoParse` is a lex / parse /
/// Script-goal failure — a real syntax error OR a shape tr's subset
/// does not cover, indistinguishable here. `StrictEarlyError` is a
/// strict-code early error resolved post-parse — `delete <bare name>`
/// (§13.5.1.1), an assignment / update targeting `eval` / `arguments`
/// (§13.15.1, §13.4 via AssignmentTargetType), or an Annex B
/// legacy-octal spelling (§B.1.1 / §B.1.2): a DEFINITE syntax error,
/// so callers that must separate "creation-time SyntaxError" from
/// "honest reject" (the `Function(...)` desugar) can throw on it
/// without probing.
pub(super) enum EvalRefusal {
    NoParse,
    StrictEarlyError,
}

/// Refuse the WHOLE freshly parsed segment, not just the offending
/// site. The dropped statements leave their expressions orphaned in
/// the arena, and arena-walking passes pick orphans up — the goal
/// triage meets a `Delete{Ident}`, the closure collector meets a
/// complete `ArrowFn` (a fn expression out of the refused text) and
/// lowers it as if the user had written it.
fn neutralize(ast: &mut Ast, exprs_before: usize, why: EvalRefusal) -> EvalRefusal {
    for e in ast.exprs[exprs_before..].iter_mut() {
        *e = Expr::Bool(false);
    }
    why
}

pub(super) fn parse_eval_source(
    src: &str,
    ast: &mut Ast,
    super_ok: bool,
    strict: EvalStrictness,
) -> Result<Vec<Stmt>, EvalRefusal> {
    let exprs_before = ast.exprs.len();
    let octal_before = ast.legacy_octal_positions.len();
    let is_strict = strict == EvalStrictness::Strict;
    let stmts = parse_once(src, ast, super_ok, is_strict)
        .and_then(reject_module_decls)
        .or_else(|| {
            // §12.9.1 rule 2 — automatic semicolon insertion at end of
            // input: when the token stream ends where the grammar still
            // wants a terminator, a semicolon is inserted. `eval("var
            // x")` is valid JavaScript by exactly this rule, and it is
            // how test262 writes it (a file always ends in a newline,
            // so tr's parser never meets the no-terminator end anywhere
            // else). Retrying with the semicolon appended IS that rule
            // — it cannot make an invalid source valid, because a
            // semicolon only ever terminates a final statement.
            let with_semi = format!("{src};");
            parse_once(&with_semi, ast, super_ok, is_strict).and_then(reject_module_decls)
        })
        .ok_or(EvalRefusal::NoParse)?;
    // Annex B §B.1.1 / §B.1.2 — the one family of this set the lexer
    // RECORDS instead of refusing, because a sloppy program evaluates
    // every one of them and only the goal (or a prologue span) says
    // otherwise. Its sites therefore have to be read back here rather
    // than falling out of the parse. The table is truncated on every
    // path, strict or not: the offsets index the eval text, so leaving
    // them would make the NEXT eval's read see this one's sites.
    // The statements now belong to a program whose `Ast::source` is a
    // different string — see `super::foreign_spans`.
    let mut stmts = stmts;
    super::foreign_spans::blank_fn_spans(&mut stmts);
    let octal_here = ast.legacy_octal_positions.len() > octal_before;
    ast.legacy_octal_positions.truncate(octal_before);
    if is_strict && octal_here {
        return Err(neutralize(ast, exprs_before, EvalRefusal::StrictEarlyError));
    }
    // §13.5.1.1 sites — `delete <bare name>` — carry their name for
    // the sloppy fold and must be neutralized on refusal (the goal
    // triage walks the WHOLE arena, orphans included).
    let del_sites: Vec<(usize, String)> = ast.exprs[exprs_before..]
        .iter()
        .enumerate()
        .filter_map(|(off, e)| match e {
            Expr::Delete { expr } => match ast.get_expr(*expr) {
                Expr::Ident(n) => Some((exprs_before + off, n.clone())),
                _ => None,
            },
            _ => None,
        })
        .collect();
    // Assignment / update sites targeting `eval` or `arguments` —
    // §15.2.1's AssignmentTargetType early errors. Pre-increment
    // desugars to the Assign shape at parse time, so the two variants
    // cover every spelling. In sloppy code the forms are LEGAL and
    // are left alone (their runtime semantics are a separate,
    // unresolved surface); orphaned copies bother no later pass — the
    // checker walks the statement tree, not the arena.
    let ea_sites: Vec<usize> = ast.exprs[exprs_before..]
        .iter()
        .enumerate()
        .filter_map(|(off, e)| {
            let target = match e {
                Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => target,
                _ => return None,
            };
            match ast.get_expr(*target) {
                Expr::Ident(n) if n == "eval" || n == "arguments" => Some(exprs_before + off),
                _ => None,
            }
        })
        .collect();
    if del_sites.is_empty() && ea_sites.is_empty() {
        return Ok(stmts);
    }
    let strict_goal = !ast.sloppy_script_goal;
    // Under the sloppy SCRIPT goal (`.cts`) every site is left to the
    // goal triage. The triage runs AFTER the `with` desugar for a
    // reason its own doc records — §14.11 resolves a with-body
    // reference through the scope object, so `eval("with(o){ delete
    // p }")` is a PROPERTY delete that an early fold here silently
    // broke (r443: the S12.10_A5 family folded `delete p1` to a
    // constant and deleted nothing).
    if !strict_goal {
        return Ok(stmts);
    }
    // Which sites sit in strict code? Under `Strict` every one does;
    // under `SloppyFold` only those inside a nested 'use strict'
    // function (or class code) — §11.2.1 arms the prologue's body
    // even when the outermost text is sloppy.
    let strict_hit = if is_strict {
        true
    } else {
        let strict_owned = super::walk::strict_owned_exprs(ast, &stmts);
        let in_strict = |i: &usize| strict_owned.get(*i).copied().unwrap_or(false);
        del_sites.iter().any(|(i, _)| in_strict(i)) || ea_sites.iter().any(in_strict)
    };
    if strict_hit {
        return Err(neutralize(ast, exprs_before, EvalRefusal::StrictEarlyError));
    }
    // A sloppy `Function(...)` body that ALSO contains a `with`
    // somewhere in the program cannot take the fold — a with-body
    // site is a property reference (§14.11), and the flag is
    // program-wide, so which body owns the `with` is not knowable
    // here. Honest reject, segment neutralized (orphans, see above).
    if !del_sites.is_empty() && ast.has_with_stmt {
        return Err(neutralize(ast, exprs_before, EvalRefusal::NoParse));
    }
    let mut declared = std::collections::HashSet::new();
    crate::ast::delete_bare_name::collect_declared_names(&ast.stmts, &mut declared);
    crate::ast::delete_bare_name::collect_declared_names(&stmts, &mut declared);
    for (i, n) in del_sites {
        ast.exprs[i] = Expr::Bool(crate::ast::delete_bare_name::sloppy_delete_answer(
            &n, &declared,
        ));
    }
    Ok(stmts)
}

/// §19.2.1.1 step 2 parses eval code with the **Script** goal symbol,
/// and `import` / `export` declarations exist only in the Module
/// grammar — `eval("export default null")` is a SyntaxError, raised at
/// evaluation time like any other parse failure. tr's parser accepts
/// them (it parses whole programs, which may be modules), so the
/// Script-goal restriction is applied here: a source containing one is
/// reported as failed, which routes the statement-position call into
/// the `throw new SyntaxError` rewrite. The drained statements are
/// dropped; their expressions stay in the arena unreferenced, costing
/// slots and nothing else.
fn reject_module_decls(stmts: Vec<Stmt>) -> Option<Vec<Stmt>> {
    if stmts
        .iter()
        .any(|s| matches!(s, Stmt::ImportDecl { .. } | Stmt::ExportDecl { .. }))
    {
        return None;
    }
    if has_orphan_jump(&stmts, false, false) {
        return None;
    }
    Some(stmts)
}

/// §16.1's early errors for a Script, applied to the shapes tr's
/// parser happily accepts in isolation: a `continue` / `break` with no
/// enclosing iteration (or switch, for `break`) and a `return`
/// outside any function body are SyntaxErrors — `eval("return;")` and
/// `eval("continue;")` must throw, not run. The walk enters blocks and
/// control flow, flips the flags at the constructs that legalize the
/// jump, and stops at a `FnDecl` body (a `return` is legal there, and
/// the parser already vets the body's own jumps as part of the
/// function). Labeled jumps are left alone — validating a label
/// target needs the label environment, and test262 spells these cases
/// with the bare forms.
fn has_orphan_jump(stmts: &[Stmt], in_loop: bool, in_switch: bool) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::Continue(None) => !in_loop,
        Stmt::Break(None) => !in_loop && !in_switch,
        Stmt::Return(_) => true,
        Stmt::Block(b) | Stmt::Multi(b) => has_orphan_jump(b, in_loop, in_switch),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            has_orphan_jump(std::slice::from_ref(then_branch), in_loop, in_switch)
                || else_branch
                    .as_deref()
                    .is_some_and(|e| has_orphan_jump(std::slice::from_ref(e), in_loop, in_switch))
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::ForOf { body, .. } => {
            has_orphan_jump(std::slice::from_ref(body), true, in_switch)
        }
        Stmt::For { init, body, .. } => {
            init.as_deref()
                .is_some_and(|i| has_orphan_jump(std::slice::from_ref(i), in_loop, in_switch))
                || has_orphan_jump(std::slice::from_ref(body), true, in_switch)
        }
        Stmt::Labeled { body, .. } => {
            has_orphan_jump(std::slice::from_ref(body), in_loop, in_switch)
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            has_orphan_jump(body, in_loop, in_switch)
                || has_orphan_jump(catch_body, in_loop, in_switch)
                || finally_body
                    .as_ref()
                    .is_some_and(|f| has_orphan_jump(f, in_loop, in_switch))
        }
        Stmt::Switch { cases, default, .. } => {
            cases
                .iter()
                .any(|c| has_orphan_jump(&c.body, in_loop, true))
                || default
                    .as_deref()
                    .is_some_and(|d| has_orphan_jump(d, in_loop, true))
        }
        _ => false,
    })
}

/// One tokenize + parse attempt into the shared arena.
///
/// The truncate on the error path is load-bearing: `parse_into`
/// assigns `*target = p.ast` BEFORE propagating its error, so a failed
/// parse leaves whatever it managed to build appended to `ast.stmts` —
/// statements belonging to nobody, which would otherwise be spliced
/// into the program as if the user had written them.
fn parse_once(src: &str, ast: &mut Ast, super_ok: bool, strict: bool) -> Option<Vec<Stmt>> {
    let before = ast.stmts.len();
    // Script goal — eval / dynamic-Function text is script code, so
    // the annexB §B.1.3 HTML-like comments are comments here.
    let tokens = lexer::tokenize_script(src).ok()?;
    match parser::parse_into_eval(src, &tokens, ast, super_ok, strict) {
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
    let msg_id = ast.add_expr(Expr::String(msg.into()));
    let exc = ast.add_expr(Expr::New {
        class_name: "SyntaxError".to_string(),
        args: vec![msg_id],
        type_args: Vec::new(),
    });
    Stmt::Throw(exc)
}
