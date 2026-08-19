//! §20.2.1.1 CreateDynamicFunction, for the compile-time-known case —
//! `Function("a", "return a + 1")` with every argument a constant
//! string. The spec itself resolves the call by TEXT ASSEMBLY: step 16
//! builds the source `function anonymous(P\n) {\nbodyText\n}` and
//! parses the whole thing, newlines and all (the newline after `P`
//! is what stops a `//` comment in the parameter text from eating the
//! body). This pass performs exactly that assembly at compile time and
//! lands the result as a top-level function declaration, which is also
//! the spec's scope answer: the created function's [[Environment]] is
//! the GLOBAL environment (step 20.b) — it does not capture the call
//! site — and tr's top level is its global under the script framing
//! the eval family already rides on.
//!
//! The call site is then just a reference to that function, in call or
//! `new` position alike (§20.2.1.1 serves both through the same
//! algorithm).
//!
//! Honest-reject boundaries, same posture as the eval passes:
//! - any non-constant argument (a runtime string needs the compiler in
//!   the artifact — the B layer);
//! - a program that binds `Function` anywhere;
//! - assembled text that does not parse, or parses to anything other
//!   than exactly the one declaration written — the latter is how a
//!   parameter text that escapes its slot (`"a) { evil(); } ("`)
//!   is caught: the assembly grows a second statement and the shape
//!   check refuses it. Those cases keep today's loud reject rather
//!   than getting a wrong function.
//!
//! Recorded divergences, deliberate: the synthesized function's
//! `.name` is not `"anonymous"` and sloppy-mode bodies run under tr's
//! strict substrate — the same single-mode boundary the eval family
//! documents in the parent module.

use super::super::{Ast, Expr, ExprId, Stmt};
use super::scope::binds_name;
use super::source::{
    DeleteSites, EvalRefusal, const_string, parse_eval_source, syntax_error_throw,
};
use crate::lexer::{self, Token};

/// Rewrite every argument-bearing `Function(...)` / `new Function(...)`
/// whose arguments are all constant strings. The zero-argument shape is
/// not handled here — it carries no dynamic text and already desugars
/// to `() => {}` in `ast_desugar_builtin_new::fn_ctor`.
pub(super) fn rewrite_function_ctors(ast: &mut Ast) {
    if binds_name(ast, "Function") {
        return;
    }
    let mut synth = 0usize;
    let mut i = 0;
    while i < ast.exprs.len() {
        let Some(texts) = function_ctor_args(ExprId(i as u32), ast) else {
            i += 1;
            continue;
        };
        let name = format!("__dynfn_{synth}");
        let params = texts[..texts.len() - 1].join(",");
        let body = texts[texts.len() - 1].clone();
        let strict_body = body_prologue_strict(&body);
        // §15.2.1 — a bare `with` in a strict body is a SyntaxError
        // whether tr's parser happens to read it (as a call to an
        // unknown `with` ident) or not, so the check runs BEFORE the
        // parse attempt and applies to both outcomes.
        if strict_body && body_lexes_bare_with(&body) {
            let throw = syntax_error_throw("dynamic function: `with` in strict mode".into(), ast);
            wrap_throw_iife(i, throw, ast);
            i += 1;
            continue;
        }
        // Same before-the-parse posture, and for the same reason: the
        // parser refuses a strict-reserved BINDING outright, so a body
        // whose prologue is strict never reaches `strict_early_error`
        // with `function anonymous(eval)` — the assembly fails to parse
        // and lands in the honest-reject arm, which answers the runtime
        // "not a constructor" rather than the creation-time SyntaxError
        // §20.2.1.1 step 22 asks for.
        if strict_body && let Some(msg) = strict_param_name_error(&params) {
            let throw = syntax_error_throw(msg, ast);
            wrap_throw_iife(i, throw, ast);
            i += 1;
            continue;
        }
        // §20.2.1.1 parses the assembled text NON-strict, where a
        // duplicate parameter name is LEGAL (§15.2.1 refuses it only
        // under a strict prologue — `strict_early_error` still sees
        // the duplicate post-parse, since the parse admits it). The
        // parse admitting it was the problem: two same-named slots
        // both resolved to the LAST binding, so the materialized
        // `arguments` snapshot read `[a, a]` and answered the second
        // argument twice (`Function('a','a','return arguments[0];')`
        // gave 8 for (7, 8); bun: 7). For the plain
        // comma-list-of-idents shape the duplicate resolves BEFORE
        // assembly: every occurrence but the LAST renames to a fresh
        // synthetic — §8.6.1 initializes duplicates so the NAME
        // resolves to the last one's argument (reads unchanged), the
        // earlier slots keep their positions (`.length` and the
        // `arguments` snapshot are positional, so both come out
        // right), and §10.4.4's simple-uniqueness test makes the
        // duplicate case UNMAPPED anyway — a snapshot with no
        // write-through is exactly its semantics. Out-of-shape
        // parameter text (defaults, patterns, comments) is left
        // alone.
        let params = if strict_body {
            params
        } else {
            dedupe_sloppy_params(&params)
        };
        let full = format!("function {name}({params}\n) {{\n{body}\n}}");
        let arena_before = ast.exprs.len();
        // super_ok = false — §20.2.1.1 parses the body as an ordinary
        // FunctionBody, where `super` is an early SyntaxError in every
        // call context; the parse failure lands in the throw arm below,
        // which is exactly the creation-time SyntaxError the spec wants.
        let delete_sites = if strict_body {
            DeleteSites::Strict
        } else {
            DeleteSites::SloppyFold
        };
        match parse_eval_source(&full, ast, false, delete_sites) {
            Ok(mut parsed) => {
                let is_the_decl = matches!(
                    parsed.as_slice(),
                    [Stmt::FnDecl { name: n, .. }] if *n == name
                );
                // §20.2.1.1 steps 17/22 — a body whose directive
                // prologue opens with 'use strict' subjects the
                // synthesized function to the §15.2.1 strict early
                // errors AT CREATION TIME: the call site becomes a
                // throw-IIFE (same carrier as the parse-failure arm
                // below). Gated on the prologue: the same shapes are
                // LEGAL in a sloppy body.
                let early = if is_the_decl && strict_body {
                    strict_early_error(&parsed, ast, arena_before)
                } else {
                    None
                };
                if let Some(msg) = early {
                    let throw = syntax_error_throw(msg, ast);
                    wrap_throw_iife(i, throw, ast);
                    i += 1;
                    continue;
                }
                // A body that touches `this`: a dynamic function is
                // sloppy (absent a 'use strict' prologue), and a
                // sloppy function called with an undefined thisArg
                // binds `this` to the GLOBAL OBJECT (§10.2.1.2
                // OrdinaryCallBindThis step 5.a.ii) — which tr mints
                // (the G2 globalThis singleton), so the read rewrites
                // to the `globalThis` ident and answers the same
                // object. Harness `fnGlobalObject.js`'s
                // `Function("return this;")()` is exactly this shape.
                //
                // The rewrite is only sound while every `this` in the
                // tail belongs to the synthesized function itself: a
                // nested `function` (declaration or expression) owns
                // its own `this`, and the arena scan is flat — it
                // cannot tell whose `this` it is looking at. The
                // nesting judgement is a token scan of the body text
                // (the `body_lexes_bare_with` precedent): any
                // `function` keyword keeps the loud reject — a true
                // arrow (`=>`) pierces `this` to the synthesized
                // function per §8.3.4, so arrows do not disqualify.
                // A strict-prologue body keeps the reject too (strict
                // `this` is undefined, a different rewrite — recorded
                // follow-up, no test262 demand measured yet).
                let touches_this = ast.exprs[arena_before..]
                    .iter()
                    .any(|e| matches!(e, Expr::This));
                let this_rewritable = touches_this && !strict_body && !body_lexes_fn_keyword(&body);
                if this_rewritable {
                    for e in ast.exprs[arena_before..].iter_mut() {
                        if matches!(e, Expr::This) {
                            *e = Expr::Ident("globalThis".into());
                        }
                    }
                }
                if is_the_decl && (!touches_this || this_rewritable) {
                    let mut decl = parsed.pop().unwrap();
                    // The parse assigned spans relative to the
                    // ASSEMBLED text, but the fn-source registry
                    // slices the MAIN source by span — a stale
                    // offset lands in unrelated bytes (silent-wrong
                    // toString, or a panic when it splits a
                    // multi-byte char). Synthesized decls carry the
                    // (0,0) sentinel like every other synth site.
                    if let Stmt::FnDecl { span, .. } = &mut decl {
                        *span = crate::lexer::Span { start: 0, end: 0 };
                    }
                    ast.stmts.push(decl);
                    ast.exprs[i] = Expr::Ident(name);
                    synth += 1;
                }
            }
            // A definite §13.5.1.1 early error resolved post-parse —
            // in the body's own strict code, or a nested 'use strict'
            // function inside a sloppy body. Creation-time SyntaxError
            // either way (§20.2.1.1 step 22).
            Err(EvalRefusal::StrictDelete) => {
                let throw = syntax_error_throw(
                    "dynamic function: `delete` on a bare name in strict code".into(),
                    ast,
                );
                wrap_throw_iife(i, throw, ast);
            }
            Err(EvalRefusal::NoParse) => {
                // A strict body that fails to parse has two possible
                // causes, and they want opposite answers: a §15.2.1
                // early error the parser itself refuses (assignment to
                // `eval`, a reserved binding) is the creation-time
                // SyntaxError the spec asks for, while a shape tr's
                // subset does not cover yet is the honest reject this
                // arm otherwise keeps. Asking which is which needs no
                // message sniffing — it is the same question §15.2.1
                // asks: does this text parse when it is NOT strict?
                if strict_body && parses_without_the_prologue(&name, &params, &body, ast) {
                    let throw = syntax_error_throw(
                        "dynamic function: strict-mode early error in body".into(),
                        ast,
                    );
                    wrap_throw_iife(i, throw, ast);
                }
            }
        }
        i += 1;
    }
}

/// Re-assemble with an empty statement ahead of the body, which closes
/// the directive prologue before it opens: the `"use strict"` that
/// follows is then an ordinary string expression and arms nothing. A
/// text that parses this way and not the other failed on strictness
/// alone.
fn parses_without_the_prologue(name: &str, params: &str, body: &str, ast: &mut Ast) -> bool {
    let probe = format!("function {name}({params}\n) {{\n;\n{body}\n}}");
    matches!(
        parse_eval_source(&probe, ast, false, DeleteSites::SloppyFold),
        Ok(_) | Err(EvalRefusal::StrictDelete)
    )
}

/// Whether the body text's directive prologue opens with a
/// 'use strict' directive (either quote form). First-statement-only is
/// the §11.2.1 shape every test262 case uses; a directive buried
/// after other directives is a recorded non-detection (stays sloppy
/// here → no early error → the loud paths keep it).
fn body_prologue_strict(body: &str) -> bool {
    let t = body.trim_start();
    t.starts_with("'use strict'") || t.starts_with("\"use strict\"")
}

/// §15.2.1 strict early errors detectable on the synthesized decl:
/// duplicate parameter names, `eval` / `arguments` / a future
/// reserved word as a parameter name, and assignment (or ++/--) to
/// `eval` / `arguments` anywhere in the body — the parse appended
/// every body expression after `arena_before`, so the arena tail scan
/// sees all depths (same posture as the `this` scan).
/// The names §15.2.1 refuses as a parameter of a strict function —
/// `eval` / `arguments` (§13.1.1) plus the §12.7.2 future reserved
/// words.
const FUTURE_RESERVED: &[&str] = &[
    "eval",
    "arguments",
    "implements",
    "interface",
    "let",
    "package",
    "private",
    "protected",
    "public",
    "static",
    "yield",
];

/// The same rule read off the parameter TEXT, for the names the parser
/// refuses before a declaration can be handed back. Splitting on commas
/// is the spec's own assembly (step 16 joins the argument strings with
/// commas), and only a segment that is EXACTLY the bare name counts, so
/// the legal `function f(a = eval)` stays untouched.
fn strict_param_name_error(params: &str) -> Option<String> {
    params
        .split(',')
        .map(str::trim)
        .find(|seg| FUTURE_RESERVED.contains(seg))
        .map(|name| format!("dynamic function: `{name}` as a parameter name in strict mode"))
}

/// The sloppy half of the duplicate-parameter answer (call-site doc at
/// the assembly step): rename every occurrence but the last to a fresh
/// synthetic, only for a plain comma list of simple identifiers.
fn dedupe_sloppy_params(params: &str) -> String {
    let segs: Vec<&str> = params.split(',').map(str::trim).collect();
    if segs.len() < 2 || !segs.iter().all(|s| is_simple_ident(s)) {
        return params.to_string();
    }
    let mut out: Vec<String> = Vec::with_capacity(segs.len());
    for (i, seg) in segs.iter().enumerate() {
        if segs[i + 1..].iter().any(|l| l == seg) {
            out.push(format!("__dupparam_{i}"));
        } else {
            out.push((*seg).to_string());
        }
    }
    out.join(",")
}

fn is_simple_ident(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

fn strict_early_error(parsed: &[Stmt], ast: &Ast, arena_before: usize) -> Option<String> {
    let [Stmt::FnDecl { params, .. }] = parsed else {
        return None;
    };
    let mut seen: Vec<&str> = Vec::new();
    for p in params {
        let name = &p.name;
        if seen.contains(&name.as_str()) {
            return Some(format!("dynamic function: duplicate parameter `{name}`"));
        }
        if FUTURE_RESERVED.contains(&name.as_str()) {
            return Some(format!(
                "dynamic function: `{name}` as a parameter name in strict mode"
            ));
        }
        seen.push(name);
    }
    for e in &ast.exprs[arena_before..] {
        let target = match e {
            Expr::Assign { target, .. } => target,
            Expr::PostIncr { target, .. } => target,
            _ => continue,
        };
        if let Expr::Ident(n) = ast.get_expr(*target)
            && (n == "eval" || n == "arguments")
        {
            return Some(format!(
                "dynamic function: assignment to `{n}` in strict mode"
            ));
        }
    }
    None
}

/// Whether the body text lexes a BARE `with` identifier token — one
/// that is neither a member name (`x.with`) nor an object key
/// (`{ with: 1 }`), the two positions where strict mode still allows
/// the word. A "with" inside a string or comment never lexes as an
/// Ident, so no textual false positives.
/// Whether the body text contains a `function` keyword — the
/// this-rewrite disqualifier (a nested function owns its own `this`).
/// Token-level like the `with` scan below, so a "function" inside a
/// string literal does not disqualify; an unlexable body answers
/// false and the parse-failure arm keeps it loud anyway.
fn body_lexes_fn_keyword(body: &str) -> bool {
    let Ok(ts) = lexer::tokenize(body) else {
        return false;
    };
    ts.iter().any(|s| matches!(&s.token, Token::Function))
}

fn body_lexes_bare_with(body: &str) -> bool {
    let Ok(ts) = lexer::tokenize(body) else {
        return false;
    };
    ts.iter().enumerate().any(|(idx, s)| {
        matches!(&s.token, Token::Ident(n) if n == "with")
            && !matches!(idx.checked_sub(1).map(|p| &ts[p].token), Some(Token::Dot))
            && !matches!(ts.get(idx + 1).map(|n| &n.token), Some(Token::Colon))
    })
}

/// Replace the call site with `(() => { throw ... })()`.
fn wrap_throw_iife(i: usize, throw: Stmt, ast: &mut Ast) {
    let arrow = ast.add_expr(Expr::ArrowFn {
        params: Vec::new(),
        return_type: None,
        body: vec![throw],
    });
    ast.exprs[i] = Expr::Call {
        callee: arrow,
        args: Vec::new(),
    };
}

/// The argument texts of a `Function(...)` or `new Function(...)`
/// call whose arguments are ALL compile-time strings — the last is the
/// body text, the rest are parameter texts. `None` for anything else,
/// including the zero-argument shape (owned elsewhere, see above).
fn function_ctor_args(eid: ExprId, ast: &Ast) -> Option<Vec<String>> {
    let args: &[ExprId] = match ast.exprs.get(eid.0 as usize)? {
        Expr::Call { callee, args } if !args.is_empty() => {
            match ast.exprs.get(callee.0 as usize)? {
                Expr::Ident(n) if n == "Function" => args,
                // §20.2.3.3 → §20.2.1.1 — `Function.call(thisArg,
                // ...texts)`: CreateDynamicFunction never reads the
                // this argument (t262 S15.3_A3 asserts exactly that),
                // so the shape is the direct call with the first
                // argument peeled. Peeling drops the thisArg's
                // EVALUATION, so only a side-effect-free thisArg
                // qualifies — anything else keeps the loud reject
                // (the prototype-methods pass records the same rule:
                // "dropping surplus args would drop their
                // evaluation"). The no-text form (`Function.call(x)`)
                // stays rejected too — the zero-arg sibling pass owns
                // the empty-body semantics and does not know this
                // spelling (recorded follow-up).
                Expr::Member { obj, name } if name == "call" => {
                    let is_fn_ctor = matches!(
                        ast.exprs.get(obj.0 as usize)?,
                        Expr::Ident(n) if n == "Function"
                    );
                    if !is_fn_ctor || args.len() < 2 || !expr_is_pure(args[0], ast) {
                        return None;
                    }
                    &args[1..]
                }
                _ => return None,
            }
        }
        Expr::New {
            class_name, args, ..
        } if class_name == "Function" && !args.is_empty() => args,
        _ => return None,
    };
    args.iter().map(|a| const_text(*a, ast)).collect()
}

/// §20.2.1.1 step 8 — every argument passes through ToString before
/// the assembly, so a non-string LITERAL folds to its spec string at
/// compile time (`new Function(undefined)` is a function whose body
/// text is `undefined`). Only shapes with an exact static answer
/// qualify: integral doubles below 2^53 print without an exponent
/// (§6.1.6.1.20), booleans / `null` / `undefined` print their names,
/// and everything else — fractional or huge numbers, runtime values —
/// keeps the loud reject rather than risking a formatting divergence.
fn const_text(eid: ExprId, ast: &Ast) -> Option<String> {
    if let Some(s) = const_string(eid, ast) {
        return Some(s);
    }
    match ast.exprs.get(eid.0 as usize)? {
        Expr::Number(n) if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 => {
            Some(format!("{}", *n as i64))
        }
        Expr::Bool(b) => Some(b.to_string()),
        Expr::Null => Some("null".into()),
        Expr::Ident(n) if n == "undefined" => Some("undefined".into()),
        _ => None,
    }
}

/// Side-effect-free thisArg shapes — the only ones the
/// `Function.call` peel may silently drop.
fn expr_is_pure(eid: ExprId, ast: &Ast) -> bool {
    matches!(
        ast.exprs.get(eid.0 as usize),
        Some(
            Expr::Ident(_)
                | Expr::This
                | Expr::String(_)
                | Expr::Number(_)
                | Expr::Bool(_)
                | Expr::Null
        )
    )
}
