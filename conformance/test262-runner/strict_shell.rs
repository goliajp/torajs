//! `sm/non262-strict-shell.js` — ported by CALL-SITE EXPANSION, not by
//! a harness function.
//!
//! The stock include defines six helpers that all take the code under
//! test as a RUNTIME string (`completesNormally(code)` → `eval(code)`,
//! `parsesSuccessfully(code)` → `Function(code)`), and tr's eval
//! desugar resolves literal text only — a harness-side port would put
//! `eval(<parameter>)` in every case's program and fail them all at
//! compile time. But every call site in the corpus passes a string
//! LITERAL, so the runner expands each call into the eval / Function
//! literal shape the desugar does resolve:
//!
//!   testLenientAndStrict('delete x;', parsesSuccessfully,
//!                        parseRaisesException(SyntaxError))
//!     → ((() => { try { Function("'use strict'; delete x;");
//!                       return false; }
//!                 catch (e) { return (SyntaxError).prototype
//!                                     .isPrototypeOf(e); } })()
//!        && (() => { try { Function('delete x;'); return true; }
//!                    catch (e) { return false; } })())
//!
//! The strict predicate runs FIRST — the stock helper's own ordering,
//! kept because a lenient evaluation can mutate the global environment
//! the strict one then observes. Under bun the expansion is equivalent
//! to the stock helper: both are direct evals in module (strict)
//! context, so the lenient side sees module strictness either way —
//! the expansion changes where the eval sits, not what it inherits.
//!
//! The code literal is re-used RAW — the bytes between its quotes are
//! embedded verbatim in a fresh literal of the same quote kind, so no
//! escape sequence is ever decoded or re-encoded. The `'use strict'; `
//! prefix spells its quotes to match.
//!
//! Expansion is all-or-nothing per case: any reference to the six
//! helpers that does not match a known shape (a computed code string —
//! `raisesException(TypeError)(in_strict_with('x = 2;'))` — or a
//! helper used as a value) returns `None`, and the case stays in the
//! harness-includes bucket rather than running against a half-ported
//! surface.

/// Expand every strict-shell helper call in `src`. `Some(expanded)`
/// iff no reference to the six helpers survives; `None` keeps the
/// case attributably unported.
pub fn expand(src: &str) -> Option<String> {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        // String / template literals and comments pass through
        // verbatim — a helper NAME inside them is prose, not a call.
        if b == b'\'' || b == b'"' || b == b'`' {
            let end = skip_string(bytes, i);
            out.push_str(&src[i..end]);
            i = end;
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            let end = bytes[i..]
                .iter()
                .position(|&c| c == b'\n')
                .map_or(bytes.len(), |p| i + p);
            out.push_str(&src[i..end]);
            i = end;
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            let end = src[i + 2..]
                .find("*/")
                .map_or(bytes.len(), |p| i + 2 + p + 2);
            out.push_str(&src[i..end]);
            i = end;
            continue;
        }
        if ident_boundary(bytes, i) {
            if let Some((name, after)) = helper_name_at(src, i) {
                let expanded = expand_call(src, name, after)?;
                out.push_str(&expanded.0);
                i = expanded.1;
                continue;
            }
        }
        out.push(b as char);
        i += 1;
    }
    Some(out)
}

const HELPERS: &[&str] = &[
    "testLenientAndStrict",
    "completesNormally",
    "parsesSuccessfully",
    "raisesException",
    "parseRaisesException",
    "returns",
];

/// Is position `i` NOT preceded by an identifier character or `.`?
fn ident_boundary(bytes: &[u8], i: usize) -> bool {
    i == 0 || {
        let p = bytes[i - 1];
        !(p.is_ascii_alphanumeric() || p == b'_' || p == b'$' || p == b'.')
    }
}

/// The helper name starting at `i`, if any, with the index just past
/// it — longest match first so `parseRaisesException` is not read as
/// a prefix miss, and the next character must close the identifier.
fn helper_name_at(src: &str, i: usize) -> Option<(&'static str, usize)> {
    let rest = &src.as_bytes()[i..];
    let mut names: Vec<&'static str> = HELPERS.to_vec();
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    for name in names {
        if rest.starts_with(name.as_bytes()) {
            let after = i + name.len();
            let next = src.as_bytes().get(after).copied();
            let closes = !next.is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'$');
            if closes {
                return Some((name, after));
            }
        }
    }
    None
}

/// Expand one helper reference beginning at its name. Returns the
/// replacement text and the index just past the consumed call.
fn expand_call(src: &str, name: &'static str, after_name: usize) -> Option<(String, usize)> {
    let bytes = src.as_bytes();
    let open = skip_ws(bytes, after_name);
    // A helper used as a VALUE (no call parens) has no literal to
    // expand — the whole case stays unported.
    if bytes.get(open) != Some(&b'(') {
        return None;
    }
    let (args, close) = split_args(src, open)?;
    match name {
        "testLenientAndStrict" => {
            let [code, lenient, strict] = args.as_slice() else {
                return None;
            };
            let lit = CodeLit::parse(code)?;
            let strict_side = expand_pred(strict, &lit.strict_prefixed())?;
            let lenient_side = expand_pred(lenient, lit.raw)?;
            Some((format!("({strict_side} && {lenient_side})"), close))
        }
        "completesNormally" | "parsesSuccessfully" => {
            let [code] = args.as_slice() else { return None };
            let lit = CodeLit::parse(code)?;
            Some((pred_template(name, "", lit.raw)?, close))
        }
        // Curried helpers: the factory call must be APPLIED to a
        // literal right here — `raisesException(E)('code')`.
        "raisesException" | "parseRaisesException" | "returns" => {
            let [inner] = args.as_slice() else {
                return None;
            };
            let open2 = skip_ws(bytes, close);
            if bytes.get(open2) != Some(&b'(') {
                return None;
            }
            let (args2, close2) = split_args(src, open2)?;
            let [code] = args2.as_slice() else {
                return None;
            };
            let lit = CodeLit::parse(code)?;
            Some((pred_template(name, inner, lit.raw)?, close2))
        }
        _ => None,
    }
}

/// One predicate expression applied to a code literal — the arm bodies
/// of the stock helpers, with the literal inlined.
fn expand_pred(pred: &str, code_lit: &str) -> Option<String> {
    let p = pred.trim();
    if p == "completesNormally" || p == "parsesSuccessfully" {
        return pred_template(p, "", code_lit);
    }
    for factory in ["raisesException", "parseRaisesException", "returns"] {
        if let Some(inner) = p
            .strip_prefix(factory)
            .and_then(|r| r.trim_start().strip_prefix('('))
            .and_then(|r| r.trim_end().strip_suffix(')'))
        {
            return pred_template(factory, inner, code_lit);
        }
    }
    None
}

fn pred_template(name: &str, inner: &str, code_lit: &str) -> Option<String> {
    Some(match name {
        "completesNormally" => format!(
            "(() => {{ try {{ eval({code_lit}); return true; }} catch (e) {{ return false; }} }})()"
        ),
        "raisesException" => format!(
            "(() => {{ try {{ eval({code_lit}); return false; }} catch (e) {{ return e instanceof ({inner}); }} }})()"
        ),
        "parsesSuccessfully" => format!(
            "(() => {{ try {{ Function({code_lit}); return true; }} catch (e) {{ return false; }} }})()"
        ),
        "parseRaisesException" => format!(
            "(() => {{ try {{ Function({code_lit}); return false; }} catch (e) {{ return ({inner}).prototype.isPrototypeOf(e); }} }})()"
        ),
        "returns" => format!(
            "(() => {{ try {{ return eval({code_lit}) === ({inner}); }} catch (e) {{ return false; }} }})()"
        ),
        _ => return None,
    })
}

/// A code argument that is a plain string literal, held RAW: `raw` is
/// the literal exactly as written (quotes included), `body` the bytes
/// between the quotes with every escape sequence intact.
struct CodeLit<'a> {
    raw: &'a str,
    body: &'a str,
    quote: u8,
}

impl<'a> CodeLit<'a> {
    fn parse(arg: &'a str) -> Option<CodeLit<'a>> {
        let t = arg.trim();
        let bytes = t.as_bytes();
        let quote = *bytes.first()?;
        if quote != b'\'' && quote != b'"' {
            return None;
        }
        let end = skip_string(bytes, 0);
        // The literal must BE the argument — `'a' + f()` is a runtime
        // string, not a literal.
        if end != t.len() || bytes.get(end - 1) != Some(&quote) || end < 2 {
            return None;
        }
        Some(CodeLit {
            raw: t,
            body: &t[1..end - 1],
            quote,
        })
    }

    /// The same literal with the `'use strict'; ` directive prefixed,
    /// quotes spelled to match the literal's own kind.
    fn strict_prefixed(&self) -> String {
        let body = self.body;
        if self.quote == b'\'' {
            format!("'\\'use strict\\'; {body}'")
        } else {
            format!("\"'use strict'; {body}\"")
        }
    }
}

/// Split a parenthesized argument list starting at the `(` at `open`.
/// Returns the top-level comma-separated argument texts and the index
/// just past the closing `)`.
fn split_args(src: &str, open: usize) -> Option<(Vec<&str>, usize)> {
    let bytes = src.as_bytes();
    let mut args = Vec::new();
    let mut depth = 1usize;
    let mut start = open + 1;
    let mut i = open + 1;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\'' || b == b'"' || b == b'`' {
            i = skip_string(bytes, i);
            continue;
        }
        match b {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    args.push(&src[start..i]);
                    return Some((args, i + 1));
                }
            }
            b',' if depth == 1 => {
                args.push(&src[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Index just past a string / template literal starting at `i`.
fn skip_string(bytes: &[u8], i: usize) -> usize {
    let quote = bytes[i];
    let mut j = i + 1;
    while j < bytes.len() {
        if bytes[j] == b'\\' {
            j += 2;
            continue;
        }
        if bytes[j] == quote {
            return j + 1;
        }
        j += 1;
    }
    bytes.len()
}

fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::expand;

    #[test]
    fn tls_parse_pair_expands_strict_first() {
        let src = "assert.sameValue(testLenientAndStrict('delete x;',\n  parsesSuccessfully,\n  parseRaisesException(SyntaxError)),\n true);";
        let out = expand(src).unwrap();
        assert!(out.contains("Function('\\'use strict\\'; delete x;')"));
        assert!(out.contains("Function('delete x;')"));
        assert!(out.contains("(SyntaxError).prototype.isPrototypeOf(e)"));
        let strict_pos = out.find("use strict").unwrap();
        let lenient_pos = out.rfind("Function('delete x;')").unwrap();
        assert!(strict_pos < lenient_pos, "strict predicate must run first");
        assert!(!out.contains("testLenientAndStrict"));
    }

    #[test]
    fn returns_value_and_eval_shape() {
        let src = "testLenientAndStrict('delete Object();', returns(true), returns(true))";
        let out = expand(src).unwrap();
        assert!(out.contains("return eval('delete Object();') === (true)"));
    }

    #[test]
    fn standalone_and_curried_direct_calls() {
        let src = "parsesSuccessfully('var x;'); raisesException(TypeError)('null();');";
        let out = expand(src).unwrap();
        assert!(out.contains("Function('var x;')"));
        assert!(out.contains("e instanceof (TypeError)"));
    }

    #[test]
    fn double_quoted_literal_keeps_raw_escapes() {
        let src = r#"raisesException(SyntaxError)("Function('\"use strict\"; 010')")"#;
        let out = expand(src).unwrap();
        assert!(out.contains(r#"eval("Function('\"use strict\"; 010')")"#));
    }

    #[test]
    fn computed_code_string_stays_unported() {
        let src = "raisesException(TypeError)(in_strict_with('x = 2;'))";
        assert!(expand(src).is_none());
    }

    #[test]
    fn helper_as_value_stays_unported() {
        let src = "var p = parsesSuccessfully;";
        assert!(expand(src).is_none());
    }

    #[test]
    fn helper_name_in_comment_and_string_passes_through() {
        let src =
            "// testLenientAndStrict tries the strict side first\nvar s = 'parsesSuccessfully';";
        assert_eq!(expand(src).unwrap(), src);
    }
}
