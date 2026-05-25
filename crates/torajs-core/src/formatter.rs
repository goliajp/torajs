//! T-05 (v0.3.0) — `tr fmt` deterministic source reformatter.
//!
//! Re-emits valid TypeScript from the parsed AST in tr's canonical
//! style:
//!   - 2-space indent
//!   - single quotes for string literals
//!   - no trailing semicolons
//!   - no config knobs (prettier-shape opinionated; no .prettierrc)
//!
//! ## Comment preservation
//!
//! The lexer drops `//` and `/* */` comments before they reach the
//! AST, so an AST-walking formatter can't see them. To prevent silent
//! comment loss, `format()` runs a comment-detection pre-pass over
//! the raw source; if any line or block comment is found outside a
//! string / regex literal, it returns `FormatError::CommentsPresent`
//! and the CLI exits non-zero with a "comment preservation lands in
//! v0.4 (#N)" message. No `--force` flag is provided — silent
//! comment stripping is the kind of footgun this project's
//! `feedback_no_tech_debt` rule explicitly bans.
//!
//! ## Coverage
//!
//! Covers every Stmt and Expr variant that the conformance / examples
//! corpus uses (LetDecl, FnDecl, ClassDecl, If/While/For/Switch,
//! Return/Throw/Try, all Expr shapes including ObjectLit / ArrayLit
//! / ArrowFn / Closure / Ternary / OptChain / Nullish / PostIncr).
//! Compiler-synthesized shapes (`Stmt::Multi`, `Stmt::ForOfSplitIter`
//! when it predates a desugar pass) are formatted back into their
//! source-level equivalents. Anything genuinely unsupported panics
//! with a `not yet supported: fmt(<variant>)` message — same shape
//! as ssa_lower's panic-hook contract.

mod fmt_expr;
mod fmt_stmt;

use crate::ast::{Ast, Stmt};
use crate::lexer;
use crate::parser;

#[derive(Debug)]
pub enum FormatError {
    Lex(String),
    Parse(String),
    /// Source contains line or block comments. `format()` refuses to
    /// strip them; comment-aware reformatter is roadmap T-Nfollowup.
    CommentsPresent,
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatError::Lex(e) => write!(f, "lex error: {e}"),
            FormatError::Parse(e) => write!(f, "parse error: {e}"),
            FormatError::CommentsPresent => write!(
                f,
                "tr fmt: source contains comments — comment-preserving \
                 reformat is a v0.4 follow-up; refusing to strip them \
                 silently. (Run `tr fmt --strip-comments` once that \
                 flag lands, or pre-strip with another tool.)"
            ),
        }
    }
}

impl std::error::Error for FormatError {}

pub fn format(source: &str) -> Result<String, FormatError> {
    if has_comments_in_code(source) {
        return Err(FormatError::CommentsPresent);
    }
    let tokens = lexer::tokenize(source).map_err(FormatError::Lex)?;
    let ast = parser::parse(&tokens).map_err(FormatError::Parse)?;
    let mut f = Formatter::new(&ast);
    for stmt in &ast.stmts {
        f.fmt_top_stmt(stmt);
    }
    Ok(f.into_output())
}

/// Detect whether source contains TS comments (`//` line or `/* */`
/// block) outside of string / template / regex literals. Mirrors the
/// lexer's tokenization rules well enough to avoid false positives on
/// `"http://..."` / `` `${'/'}` `` / `/[/]/g`. Worst-case false
/// positive flags fmt-able sources as comment-bearing — that's the
/// safe direction.
fn has_comments_in_code(src: &str) -> bool {
    let bytes = src.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    while i < n {
        let c = bytes[i];
        match c {
            b'"' | b'\'' => {
                // Plain string literal.
                let q = c;
                i += 1;
                while i < n && bytes[i] != q {
                    if bytes[i] == b'\\' && i + 1 < n {
                        i += 2;
                        continue;
                    }
                    i += 1;
                }
                i = (i + 1).min(n);
            }
            b'`' => {
                // Template literal. Skip until matching backtick;
                // handle `${ ... }` interpolations by skipping
                // balanced braces (which themselves can contain any
                // shape including comments — we play it safe by
                // counting braces).
                i += 1;
                let mut brace = 0i32;
                while i < n {
                    let b = bytes[i];
                    if b == b'\\' && i + 1 < n {
                        i += 2;
                        continue;
                    }
                    if brace == 0 && b == b'`' {
                        i += 1;
                        break;
                    }
                    if b == b'$' && i + 1 < n && bytes[i + 1] == b'{' {
                        brace += 1;
                        i += 2;
                        continue;
                    }
                    if brace > 0 && b == b'{' {
                        brace += 1;
                    } else if brace > 0 && b == b'}' {
                        brace -= 1;
                    }
                    i += 1;
                }
            }
            b'/' if i + 1 < n => {
                let next = bytes[i + 1];
                if next == b'/' || next == b'*' {
                    return true;
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    false
}

struct Formatter<'a> {
    ast: &'a Ast,
    out: String,
    indent: usize,
}

impl<'a> Formatter<'a> {
    fn new(ast: &'a Ast) -> Self {
        Self {
            ast,
            out: String::with_capacity(1024),
            indent: 0,
        }
    }

    fn into_output(self) -> String {
        self.out
    }

    fn write(&mut self, s: &str) {
        self.out.push_str(s);
    }

    fn write_indent(&mut self) {
        for _ in 0..self.indent {
            self.out.push_str("  ");
        }
    }

    fn newline(&mut self) {
        self.out.push('\n');
    }

    fn fmt_top_stmt(&mut self, s: &Stmt) {
        self.fmt_stmt(s);
        self.newline();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(src: &str) -> String {
        format(src).expect("fmt")
    }

    #[test]
    fn simple_let() {
        let out = fmt("let x:i64=1\n");
        assert_eq!(out, "let x: i64 = 1\n");
    }

    #[test]
    fn fn_decl_and_call() {
        let src = "function add(a:i64,b:i64):i64{return a+b}\nadd(1,2)\n";
        let out = fmt(src);
        assert!(out.contains("function add(a: i64, b: i64): i64"));
        assert!(out.contains("return a + b"));
        assert!(out.contains("add(1, 2)"));
    }

    #[test]
    fn idempotent_round_trip() {
        let src = "let x:i64=1\nfunction id<T>(v:T):T{return v}\nid<i64>(x)\n";
        let one = fmt(src);
        let two = fmt(&one);
        assert_eq!(one, two, "fmt is not idempotent");
    }

    #[test]
    fn refuses_to_strip_comments() {
        let src = "// hello\nlet x: i64 = 1\n";
        assert!(matches!(format(src), Err(FormatError::CommentsPresent)));
    }

    #[test]
    fn block_comment_detected() {
        let src = "/* hello */\nlet x: i64 = 1\n";
        assert!(matches!(format(src), Err(FormatError::CommentsPresent)));
    }

    #[test]
    fn url_in_string_is_not_a_comment() {
        let src = "let url: string = 'https://example.com/path'\n";
        // Should NOT detect the `//` inside the string literal.
        let out = fmt(src);
        assert!(out.contains("https://example.com/path"));
    }

    #[test]
    fn single_quotes_emitted() {
        let src = "let s: string = \"hello\"\n";
        let out = fmt(src);
        assert!(out.contains("'hello'"));
    }
}
