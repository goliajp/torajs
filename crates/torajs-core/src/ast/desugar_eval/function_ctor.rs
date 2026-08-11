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
use super::source::{const_string, parse_eval_source, syntax_error_throw};
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
        let full = format!("function {name}({params}\n) {{\n{body}\n}}");
        let arena_before = ast.exprs.len();
        // super_ok = false — §20.2.1.1 parses the body as an ordinary
        // FunctionBody, where `super` is an early SyntaxError in every
        // call context; the parse failure lands in the throw arm below,
        // which is exactly the creation-time SyntaxError the spec wants.
        match parse_eval_source(&full, ast, false) {
            Some(mut parsed) => {
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
            None => {}
        }
        i += 1;
    }
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
fn strict_early_error(parsed: &[Stmt], ast: &Ast, arena_before: usize) -> Option<String> {
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
    let args = match ast.exprs.get(eid.0 as usize)? {
        Expr::Call { callee, args } if !args.is_empty() => {
            match ast.exprs.get(callee.0 as usize)? {
                Expr::Ident(n) if n == "Function" => args,
                _ => return None,
            }
        }
        Expr::New {
            class_name, args, ..
        } if class_name == "Function" && !args.is_empty() => args,
        _ => return None,
    };
    args.iter().map(|a| const_string(*a, ast)).collect()
}
