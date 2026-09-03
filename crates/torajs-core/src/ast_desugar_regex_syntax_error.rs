//! RFC 20260724-regex-literal-syntax-error — chunk 4 pivot
//! (compile-time reject, populate side-table only; pipeline consumer).
//!
//! Walks the AST **once** looking for `Expr::Regex(pat, flags)`
//! literals whose pattern or flags string is malformed per ES
//! §22.2.3.1 (via `torajs_regex::parser::Parser::parse` +
//! `flags::parse_flags` + u/v conflict gate). Every hit is recorded
//! in `ast.regex_parse_errors[expr_id] = <msg>`. The pass does NOT
//! rewrite any statement or arena expr — chunks 1+2 (`2cd52053` +
//! `003f970c` + `03477421`) previously wrapped stmt-position
//! `Stmt::LetDecl` inits into `Multi([LetDecl, Throw])` and expr-
//! position uses into IIFE `(() : any => throw new SyntaxError(...))
//! ()`, but that produced runtime `error: uncaught SyntaxError:` on
//! stderr — a shape test262 negative parse-phase judge treats as
//! `negative-phase-mismatch` (Bug), not a compile-time reject, so
//! `passTotal` was unchanged even though behavior was clearly
//! better than silent-wrong.
//!
//! Pivot: the pipeline (`main.rs` / `cmd_build.rs` / `lsp.rs`) will
//! inspect `ast.regex_parse_errors` right after this pass runs and,
//! if any entries exist, emit `parse error: regex literal
//! /pat/flags: <detail>` to stderr and return `ExitCode::from(1)`.
//! That matches bun's compile-time SyntaxError shape and test262
//! `verdict::incompat_kind` recognises the `parse error:` prefix as
//! `Some("parse error")` → `is_recognized_compile_reject` → the
//! negative parse-phase case scores `PassNegative`.
//!
//! Users writing `try { const r = /bad/; ... } catch (e) { ... }`
//! now match bun's semantics: the whole source rejects at parse
//! time, the `catch` is never entered — chunks 1+2's runtime
//! `throw`-inside-`try` behavior was a spec divergence and is
//! removed by this pivot.

use crate::ast::{Ast, Expr, ExprId};

pub fn run(ast: &mut Ast) {
    let n = ast.exprs.len();
    for i in 0..n {
        let (pattern, flags) = match &ast.exprs[i] {
            Expr::Regex { pattern, flags } => (pattern.clone(), flags.clone()),
            _ => continue,
        };
        let Some(msg) = try_regex_parse_error(&pattern, &flags) else {
            continue;
        };
        ast.regex_parse_errors.insert(ExprId(i as u32), msg);
    }
}

/// Returns `Some(msg)` when `/pattern/flags` is malformed per ES
/// §22.2.3.1 (RegExpInitialize); `None` when well-formed.
fn try_regex_parse_error(pattern: &str, flags: &str) -> Option<String> {
    let f = match torajs_regex::flags::parse_flags(flags.as_bytes()) {
        Some(v) => v,
        None => {
            return Some(format!(
                "/{}/{}: Invalid flags supplied to RegExp constructor",
                pattern, flags
            ));
        }
    };
    use torajs_regex::parser::{RE_FLAG_U, RE_FLAG_V};
    if (f & RE_FLAG_U) != 0 && (f & RE_FLAG_V) != 0 {
        return Some(format!(
            "/{}/{}: Invalid flags supplied to RegExp constructor",
            pattern, flags
        ));
    }
    // The parser reads the pattern in the form the flags name
    // (§22.2.2.1): under u/v a sequence of code points, otherwise a
    // sequence of code units, where a supplementary character spells
    // as its surrogate pair. `regex/compile.rs` and the DFA bake both
    // hand it that form; handing it the raw UTF-8 here would check a
    // different pattern than the one that runs.
    let bytes = if torajs_regex::flags::unicode_mode(f) {
        pattern.as_bytes().to_vec()
    } else {
        torajs_regex::utf8::split_surrogate_pairs(pattern.as_bytes())
    };
    let mut p = torajs_regex::parser::Parser::new(&bytes, f);
    let Some(mut root) = p.parse() else {
        return Some(format!("/{}/{}", pattern, flags));
    };
    // Two §22.2.1.1 Early Errors are not decidable during the walk —
    // a `\k<name>` whose GroupSpecifier does not exist, and a decimal
    // escape past NcapturingParens under u/v — because neither the
    // name table nor the capture count is complete until the pattern
    // ends. `resolve_backrefs` is the pass that decides them, and it
    // ran only on the compile-to-Program paths, so those two forms
    // reached the runtime instead of the compile-time reject.
    if !torajs_regex::resolve::resolve_backrefs(&mut root, &p.names, p.n_captures, f) {
        return Some(format!("/{}/{}", pattern, flags));
    }
    None
}
