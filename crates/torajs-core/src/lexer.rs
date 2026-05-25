//! Lexer — TS-shaped token stream. Subset for P0.2 (just enough for
//! `console.log("hello")`). The big match-arm tokenize loop lives in
//! this file; public types in `lexer/types.rs`, scanning primitives
//! (`advance` / `peek` / `regex_context` / `emit` / ...) in
//! `lexer/util.rs`.

mod scan;
mod scan_number;
mod types;
mod util;

pub use types::{Span, Spanned, TemplatePart, Token};

use util::{advance, emit, is_ident_cont, is_ident_start, peek, regex_context};

pub fn tokenize(src: &str) -> Result<Vec<Spanned>, String> {
    let bytes = src.as_bytes();
    let len = bytes.len() as u32;
    let mut out = Vec::new();
    let mut i: u32 = 0;

    while i < len {
        let start = i;
        let b = bytes[i as usize];
        match b {
            b' ' | b'\t' | b'\r' | b'\n' => {
                i += 1;
                continue;
            }
            b'.' => {
                // `...` (spread/rest) emits a single DotDotDot token.
                // Bare `.` stays Dot for member access.
                if peek(bytes, i + 1) == Some(b'.') && peek(bytes, i + 2) == Some(b'.') {
                    i += 3;
                    emit(&mut out, Token::DotDotDot, start, i);
                } else if peek(bytes, i + 1).is_some_and(|c| c.is_ascii_digit()) {
                    // P0.10 — leading-dot numeric literal: `.5`,
                    // `.123`, `.5e2` per ES spec §12.9.3 NumericLiteral.
                    // Pre-fix tora's lexer always emitted Token::Dot
                    // here, leaving the parser to bail with 'expected
                    // expression, got Dot'. Now consume the fractional
                    // tail (and optional exponent) inline as part of
                    // the numeric value, mirroring what the post-Int
                    // path does.
                    i += 1; // consume `.`
                    let mut digits = String::from("0.");
                    while let Some(c) = peek(bytes, i) {
                        if c.is_ascii_digit() || c == b'_' {
                            if c != b'_' {
                                digits.push(c as char);
                            }
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    // Optional exponent: `[eE][+-]?DIGITS`
                    if let Some(c) = peek(bytes, i)
                        && (c == b'e' || c == b'E')
                    {
                        digits.push(c as char);
                        i += 1;
                        if let Some(s) = peek(bytes, i)
                            && (s == b'+' || s == b'-')
                        {
                            digits.push(s as char);
                            i += 1;
                        }
                        while let Some(c) = peek(bytes, i) {
                            if c.is_ascii_digit() {
                                digits.push(c as char);
                                i += 1;
                            } else {
                                break;
                            }
                        }
                    }
                    let n: f64 = digits.parse().unwrap_or(0.0);
                    emit(&mut out, Token::Number(n), start, i);
                } else {
                    emit(&mut out, Token::Dot, start, advance(&mut i));
                }
            }
            b',' => emit(&mut out, Token::Comma, start, advance(&mut i)),
            b':' => emit(&mut out, Token::Colon, start, advance(&mut i)),
            b';' => emit(&mut out, Token::Semi, start, advance(&mut i)),
            b'(' => emit(&mut out, Token::LParen, start, advance(&mut i)),
            b')' => emit(&mut out, Token::RParen, start, advance(&mut i)),
            b'{' => emit(&mut out, Token::LBrace, start, advance(&mut i)),
            b'}' => emit(&mut out, Token::RBrace, start, advance(&mut i)),
            b'[' => emit(&mut out, Token::LBracket, start, advance(&mut i)),
            b']' => emit(&mut out, Token::RBracket, start, advance(&mut i)),
            b'+' => {
                i += 1;
                if peek(bytes, i) == Some(b'+') {
                    i += 1;
                    emit(&mut out, Token::PlusPlus, start, i);
                } else if peek(bytes, i) == Some(b'=') {
                    i += 1;
                    emit(&mut out, Token::PlusEq, start, i);
                } else {
                    emit(&mut out, Token::Plus, start, i);
                }
            }
            b'-' => {
                i += 1;
                if peek(bytes, i) == Some(b'-') {
                    i += 1;
                    emit(&mut out, Token::MinusMinus, start, i);
                } else if peek(bytes, i) == Some(b'=') {
                    i += 1;
                    emit(&mut out, Token::MinusEq, start, i);
                } else {
                    emit(&mut out, Token::Minus, start, i);
                }
            }
            b'*' => {
                i += 1;
                /* V3-01 — `**` exponent operator (and its compound
                 * assign `**=`). JS spec: right-associative,
                 * precedence higher than mul / div / mod. */
                if peek(bytes, i) == Some(b'*') {
                    i += 1;
                    if peek(bytes, i) == Some(b'=') {
                        i += 1;
                        emit(&mut out, Token::StarStarEq, start, i);
                    } else {
                        emit(&mut out, Token::StarStar, start, i);
                    }
                } else if peek(bytes, i) == Some(b'=') {
                    i += 1;
                    emit(&mut out, Token::StarEq, start, i);
                } else {
                    emit(&mut out, Token::Star, start, i);
                }
            }
            b'~' => emit(&mut out, Token::Tilde, start, advance(&mut i)),
            b'?' => {
                // `?` (ternary), `??` (nullish coalescing), `?.`
                // (optional chaining). Single-char emit becomes
                // multi-char when the suffix is `?` or `.`.
                if peek(bytes, i + 1) == Some(b'?') {
                    i += 2;
                    emit(&mut out, Token::QuestionQuestion, start, i);
                } else if peek(bytes, i + 1) == Some(b'.') {
                    i += 2;
                    emit(&mut out, Token::QuestionDot, start, i);
                } else {
                    emit(&mut out, Token::Question, start, advance(&mut i));
                }
            }
            b'/' => {
                // `//` line comment, `/* */` block comment, regex
                // literal, or division. Disambiguation between regex
                // and division uses the previous token: regex when prev
                // is None / a punctuator / a keyword that can start an
                // expression on its right.
                match peek(bytes, i + 1) {
                    Some(b'/') => {
                        // Line comment — consume to end-of-line / EOF.
                        i += 2;
                        while i < len && bytes[i as usize] != b'\n' {
                            i += 1;
                        }
                        // Don't consume the newline itself — outer loop's
                        // whitespace branch handles it (and any trailing
                        // \r\n line ending).
                    }
                    Some(b'*') => {
                        // Block comment — consume to first `*/`. Nested
                        // block comments are NOT supported (TS doesn't
                        // support them either; matches `tsc` / `bun`).
                        i += 2;
                        let comment_start = start;
                        loop {
                            if i + 1 >= len {
                                return Err(format!(
                                    "unterminated block comment starting at {comment_start}"
                                ));
                            }
                            if bytes[i as usize] == b'*' && bytes[(i + 1) as usize] == b'/' {
                                i += 2;
                                break;
                            }
                            i += 1;
                        }
                    }
                    _ if regex_context(out.last().map(|s| &s.token)) => {
                        // Scan a regex literal: `/pattern/flags`.
                        // Pattern body: read until an unescaped `/`,
                        // honoring `\\.` escapes and `[...]` character
                        // classes (where `/` is allowed bare).
                        let body_start = (i + 1) as usize;
                        let mut p = body_start;
                        let mut in_class = false;
                        loop {
                            if p >= len as usize {
                                return Err(format!(
                                    "unterminated regex literal starting at {start}"
                                ));
                            }
                            let c = bytes[p];
                            if c == b'\n' {
                                return Err(format!(
                                    "unterminated regex literal at {start} (line break before closing `/`)"
                                ));
                            }
                            if c == b'\\' {
                                // Skip the escape sequence's next byte.
                                p += 2;
                                continue;
                            }
                            if c == b'[' {
                                in_class = true;
                                p += 1;
                                continue;
                            }
                            if c == b']' && in_class {
                                in_class = false;
                                p += 1;
                                continue;
                            }
                            if c == b'/' && !in_class {
                                break;
                            }
                            p += 1;
                        }
                        let pattern = String::from_utf8_lossy(&bytes[body_start..p]).into_owned();
                        // Flags: any trailing ASCII letters.
                        let flags_start = p + 1;
                        let mut q = flags_start;
                        while q < len as usize && bytes[q].is_ascii_alphabetic() {
                            q += 1;
                        }
                        let flags = String::from_utf8_lossy(&bytes[flags_start..q]).into_owned();
                        i = q as u32;
                        emit(&mut out, Token::Regex { pattern, flags }, start, i);
                    }
                    Some(b'=') => {
                        i += 2;
                        emit(&mut out, Token::SlashEq, start, i);
                    }
                    _ => emit(&mut out, Token::Slash, start, advance(&mut i)),
                }
            }
            b'%' => {
                i += 1;
                if peek(bytes, i) == Some(b'=') {
                    i += 1;
                    emit(&mut out, Token::PercentEq, start, i);
                } else {
                    emit(&mut out, Token::Percent, start, i);
                }
            }
            b'&' => {
                i += 1;
                if peek(bytes, i) == Some(b'&') {
                    i += 1;
                    emit(&mut out, Token::AmpAmp, start, i);
                } else {
                    emit(&mut out, Token::Amp, start, i);
                }
            }
            b'|' => {
                i += 1;
                if peek(bytes, i) == Some(b'|') {
                    i += 1;
                    emit(&mut out, Token::PipePipe, start, i);
                } else {
                    emit(&mut out, Token::Pipe, start, i);
                }
            }
            b'^' => emit(&mut out, Token::Caret, start, advance(&mut i)),
            b'<' => {
                i += 1;
                if peek(bytes, i) == Some(b'=') {
                    i += 1;
                    emit(&mut out, Token::LtEq, start, i);
                } else if peek(bytes, i) == Some(b'<') {
                    i += 1;
                    emit(&mut out, Token::ShlShl, start, i);
                } else {
                    emit(&mut out, Token::Lt, start, i);
                }
            }
            b'>' => {
                i += 1;
                if peek(bytes, i) == Some(b'=') {
                    i += 1;
                    emit(&mut out, Token::GtEq, start, i);
                } else if peek(bytes, i) == Some(b'>') {
                    i += 1;
                    if peek(bytes, i) == Some(b'>') {
                        i += 1;
                        emit(&mut out, Token::ShrShrShr, start, i);
                    } else {
                        emit(&mut out, Token::ShrShr, start, i);
                    }
                } else {
                    emit(&mut out, Token::Gt, start, i);
                }
            }
            b'=' => {
                i += 1;
                if peek(bytes, i) == Some(b'=') {
                    i += 1;
                    if peek(bytes, i) == Some(b'=') {
                        i += 1;
                        emit(&mut out, Token::EqEqEq, start, i);
                    } else {
                        // V3-18 m3 — `==` IsLooselyEqual per §7.2.13.
                        // Restored from "out-of-scope" 2026-05-10
                        // (test262 100% bar). Emits a new
                        // Token::EqEq → BinOp::LooseEq.
                        emit(&mut out, Token::EqEq, start, i);
                    }
                } else if peek(bytes, i) == Some(b'>') {
                    i += 1;
                    emit(&mut out, Token::FatArrow, start, i);
                } else {
                    emit(&mut out, Token::Eq, start, i);
                }
            }
            b'!' => {
                i += 1;
                if peek(bytes, i) == Some(b'=') {
                    i += 1;
                    if peek(bytes, i) == Some(b'=') {
                        i += 1;
                        emit(&mut out, Token::BangEqEq, start, i);
                    } else {
                        // V3-18 m3 — `!=` is `!IsLooselyEqual`.
                        emit(&mut out, Token::BangEq, start, i);
                    }
                } else {
                    // Unary logical not — used as `!cond`. M1.5.
                    emit(&mut out, Token::Bang, start, i);
                }
            }
            b'"' | b'\'' => scan::scan_string(bytes, &mut i, &mut out, start, len)?,
            b'`' => {
                // Template literal. Read alternating literal segments
                // and `${...}` interpolations until the closing
                // backtick. Each interpolation's source slice is
                // recursively tokenized so the parser can drop a
                // sub-Parser into it without re-doing lex.
                //
                // Limitation: interpolations track only `{` `}` depth,
                // not strings or backticks inside the expression. So
                // `${ "}" }` (a literal `}` inside a string) and
                // nested templates `${\`...\`}` aren't supported. The
                // common arithmetic / member-access shapes work fine.
                i += 1; // consume opening backtick
                let mut parts: Vec<TemplatePart> = Vec::new();
                let mut buf: Vec<u8> = Vec::new();
                loop {
                    if i >= len {
                        return Err(format!("unterminated template literal starting at {start}"));
                    }
                    let b = bytes[i as usize];
                    if b == b'`' {
                        if !buf.is_empty() || parts.is_empty() {
                            let s = std::str::from_utf8(&buf)
                                .map_err(|_| format!("invalid utf-8 in template at {start}"))?
                                .to_string();
                            parts.push(TemplatePart::Lit(s));
                        }
                        i += 1; // consume closing backtick
                        break;
                    }
                    if b == b'$' && peek(bytes, i + 1) == Some(b'{') {
                        // Flush literal segment (even if empty — we
                        // need the alternation).
                        let s = std::str::from_utf8(&buf)
                            .map_err(|_| format!("invalid utf-8 in template at {start}"))?
                            .to_string();
                        parts.push(TemplatePart::Lit(s));
                        buf.clear();
                        i += 2; // consume `${`
                        let expr_start = i;
                        let mut depth: i32 = 1;
                        while i < len && depth > 0 {
                            match bytes[i as usize] {
                                b'{' => depth += 1,
                                b'}' => depth -= 1,
                                _ => {}
                            }
                            if depth == 0 {
                                break;
                            }
                            i += 1;
                        }
                        if i >= len {
                            return Err(format!(
                                "unterminated template `${{...}}` interpolation at {start}"
                            ));
                        }
                        let expr_end = i;
                        i += 1; // consume `}`
                        let expr_src =
                            std::str::from_utf8(&bytes[expr_start as usize..expr_end as usize])
                                .map_err(|_| {
                                    format!("invalid utf-8 in template interp at {start}")
                                })?;
                        let inner = tokenize(expr_src)?;
                        // Keep the trailing Eof so the sub-Parser's
                        // peek() never falls off the end (its expr
                        // parsers rely on the Eof guard).
                        parts.push(TemplatePart::Expr(inner));
                        continue;
                    }
                    buf.push(b);
                    i += 1;
                }
                emit(&mut out, Token::Template { parts }, start, i);
            }
            b'#' if peek(bytes, i + 1).is_some_and(is_ident_start) => {
                // P8.1 — `#name` PrivateIdentifier. Consume `#`, then
                // lex the identifier body like a normal `Ident` but
                // emit `Token::PrivateIdent(name)` carrying just the
                // name (no `#`). A bare `#` not followed by an ident
                // start falls through to the unexpected-byte error
                // below — narrow-surface (no use for `#` outside
                // PrivateIdentifier yet).
                i += 1;
                let ident_start = i;
                while i < len && is_ident_cont(bytes[i as usize]) {
                    i += 1;
                }
                let name = std::str::from_utf8(&bytes[ident_start as usize..i as usize])
                    .expect("ascii ident slice is valid utf-8");
                emit(&mut out, Token::PrivateIdent(name.to_string()), start, i);
            }
            b if is_ident_start(b) => {
                scan::scan_ident_or_keyword(bytes, &mut i, &mut out, start, len)
            }
            b if b.is_ascii_digit() => {
                scan_number::scan_number(bytes, &mut i, &mut out, start, len, b)?
            }
            _ => return Err(format!("unexpected byte {b:#x} at {start}")),
        }
    }
    emit(&mut out, Token::Eof, len, len);
    Ok(out)
}
