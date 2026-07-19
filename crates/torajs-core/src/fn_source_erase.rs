//! Type-erased fn source text (RFC 20260719-fn-tostring-source B2).
//!
//! `Function.prototype.toString` under bun answers the TRANSPILED
//! JS source — the user's text with every type annotation spliced
//! out and original whitespace kept verbatim (probe 2026-07-19).
//! The parser is the single authority on what is type-side syntax:
//! `parse_type_ann`'s recording wrapper and `parse_fn_type_params`
//! push each outermost annotation's byte range into
//! `Ast.type_ann_spans` as they consume it. This module is the
//! splice: given a recorded fn span, drop every annotation range
//! inside it, extending each one backward over the `:` / `?:` that
//! introduced it or the `as` / `satisfies` keyword it followed.
//!
//! Recorded boundaries (RFC): bare-arrow param parenthesisation
//! (`x => …` — bun's transpiler form not probed yet) and transpiler
//! rewrites beyond pure erasure (enum inlining, namespace expansion)
//! are B4/B6 concerns, not splices.

use crate::lexer::Span;

/// Splice every recorded annotation range inside `fn_span` out of
/// `src`, answering the type-erased source slice. `ann_spans` is the
/// whole program's table (source order); ranges outside `fn_span`
/// are ignored.
// dead_code: consumer lands with B3 (fn_name_table src bake) --
// this module ships first so the parser-recording half and the
// splice contract are locked by unit tests before the ABI change.
#[allow(dead_code)]
pub(crate) fn erase_types(src: &str, ann_spans: &[Span], fn_span: Span) -> String {
    let bytes = src.as_bytes();
    let (lo, hi) = (fn_span.start as usize, fn_span.end as usize);
    // Extend each in-range annotation backward over its introducer,
    // then merge overlaps (an `as`-form records both the combined
    // and the inner range on re-parse shapes).
    let mut cuts: Vec<(usize, usize)> = ann_spans
        .iter()
        .filter(|s| (s.start as usize) >= lo && (s.end as usize) <= hi)
        .map(|s| (extend_over_introducer(bytes, s.start as usize, lo), s.end as usize))
        .collect();
    cuts.sort_unstable();
    let mut out = String::with_capacity(hi - lo);
    let mut cursor = lo;
    for (start, end) in cuts {
        if start < cursor {
            // overlap with the previous cut — keep the wider reach
            cursor = cursor.max(end);
            continue;
        }
        out.push_str(&src[cursor..start]);
        cursor = end;
    }
    out.push_str(&src[cursor..hi]);
    out
}

/// Walk backward from an annotation's first byte to include the
/// syntax that introduced it: `: ` (optionally preceded by the `?`
/// optional-param marker) or the ` as ` / ` satisfies ` operator,
/// whitespace runs included. Anything else (a fn type-param list's
/// own `<`) starts the cut as recorded.
#[allow(dead_code)]
fn extend_over_introducer(bytes: &[u8], ann_start: usize, floor: usize) -> usize {
    let skip_ws = |mut i: usize| {
        while i > floor && bytes[i - 1].is_ascii_whitespace() {
            i -= 1;
        }
        i
    };
    let i = skip_ws(ann_start);
    if i > floor && bytes[i - 1] == b':' {
        let j = skip_ws(i - 1);
        if j > floor && bytes[j - 1] == b'?' {
            return j - 1;
        }
        return i - 1;
    }
    for kw in [b"as".as_slice(), b"satisfies".as_slice()] {
        if i >= floor + kw.len() && &bytes[i - kw.len()..i] == kw {
            // require a word boundary before the keyword
            let k = i - kw.len();
            if k == floor || !bytes[k - 1].is_ascii_alphanumeric() {
                return skip_ws(k);
            }
        }
    }
    ann_start
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Expr, Stmt};
    use crate::lexer::tokenize;
    use crate::parser::parse;

    /// Parse `src`, take the first fn-like span (FnDecl stmt or
    /// ArrowFn expr), and erase — the exact pipeline the table bake
    /// will run.
    fn erased_first_fn(src: &str) -> String {
        let tokens = tokenize(src).expect("tokenize");
        let ast = parse(src, &tokens).expect("parse");
        let span = ast
            .stmts
            .iter()
            .find_map(|s| match s {
                Stmt::FnDecl { span, .. } if span.start != 0 || span.end != 0 => Some(*span),
                _ => None,
            })
            .or_else(|| {
                ast.exprs.iter().enumerate().find_map(|(i, e)| {
                    matches!(e, Expr::ArrowFn { .. }).then(|| ast.expr_spans[i])
                })
            })
            .expect("a spanned fn-like node");
        erase_types(src, &ast.type_ann_spans, span)
    }

    #[test]
    fn fn_decl_params_and_return_erase() {
        // bun ground truth from the 2026-07-19 probe.
        let src = "function f(a: number, b: number): number {\n  return a + b;\n}\n";
        assert_eq!(erased_first_fn(src), "function f(a, b) {\n  return a + b;\n}");
    }

    #[test]
    fn paren_arrow_param_ann_erases() {
        let src = "const g = (x: number) => x * 2;\n";
        assert_eq!(erased_first_fn(src), "(x) => x * 2");
    }

    #[test]
    fn generic_type_params_erase() {
        let src = "function id<T>(x: T): T {\n  return x;\n}\n";
        assert_eq!(erased_first_fn(src), "function id(x) {\n  return x;\n}");
    }

    #[test]
    fn optional_param_marker_erases_with_its_ann() {
        let src = "function g(a?: number) {\n  return a;\n}\n";
        assert_eq!(erased_first_fn(src), "function g(a) {\n  return a;\n}");
    }

    #[test]
    fn as_cast_erases_with_keyword() {
        let src = "function h() {\n  return 1 as any;\n}\n";
        assert_eq!(erased_first_fn(src), "function h() {\n  return 1;\n}");
    }

    #[test]
    fn fn_typed_param_erases_whole_ann() {
        let src = "function k(cb: (x: number) => void) {\n  cb(1);\n}\n";
        assert_eq!(erased_first_fn(src), "function k(cb) {\n  cb(1);\n}");
    }

    #[test]
    fn body_of_nested_fn_erases_too() {
        let src = "function outer() {\n  const inner = (y: number) => y;\n  return inner(1);\n}\n";
        assert_eq!(
            erased_first_fn(src),
            "function outer() {\n  const inner = (y) => y;\n  return inner(1);\n}"
        );
    }

    #[test]
    fn untyped_source_is_identity() {
        let src = "function plain(a, b) {\n  return a + b;\n}\n";
        assert_eq!(erased_first_fn(src), "function plain(a, b) {\n  return a + b;\n}");
    }
}
