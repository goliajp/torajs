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
use super::source::{const_string, parse_eval_source};

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
        let body = &texts[texts.len() - 1];
        let full = format!("function {name}({params}\n) {{\n{body}\n}}");
        let arena_before = ast.exprs.len();
        if let Some(mut parsed) = parse_eval_source(&full, ast) {
            let is_the_decl = matches!(
                parsed.as_slice(),
                [Stmt::FnDecl { name: n, .. }] if *n == name
            );
            // A body that touches `this` cannot be synthesized
            // honestly: a dynamic function's sloppy `this` is the
            // global OBJECT (`Function("return this")()` hands it
            // back), and tr has no such object — its top level is its
            // global. The parse appended the body's expressions after
            // `arena_before`, so scanning that tail sees every `this`
            // at any depth. Harness `fnGlobalObject.js` is exactly
            // this shape; it keeps the loud reject.
            let touches_this = ast.exprs[arena_before..]
                .iter()
                .any(|e| matches!(e, Expr::This));
            if is_the_decl && !touches_this {
                ast.stmts.push(parsed.pop().unwrap());
                ast.exprs[i] = Expr::Ident(name);
                synth += 1;
            }
        }
        i += 1;
    }
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
