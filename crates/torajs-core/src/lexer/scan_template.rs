//! Template literal scanner — alternating literal segments and
//! `${...}` interpolations. Extracted verbatim from `tokenize`'s
//! `b'\u{60}'` match arm (2026-07-03, fn-debt decomp; only change is
//! `i` threading through `&mut u32` and the recursive tokenize call
//! becoming `super::tokenize`).

use super::util::{emit, peek};
use super::{Spanned, TemplatePart, Token};

pub(super) fn scan_template(
    bytes: &[u8],
    i: &mut u32,
    out: &mut Vec<Spanned>,
    start: u32,
    len: u32,
) -> Result<(), String> {
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
    *i += 1; // consume opening backtick
    let mut parts: Vec<TemplatePart> = Vec::new();
    let mut buf: Vec<u8> = Vec::new();
    // §12.9.6 TRV — the raw spelling runs in parallel: escapes stay
    // verbatim, line-terminator sequences normalize to `\n`.
    let mut raw: Vec<u8> = Vec::new();
    loop {
        if *i >= len {
            return Err(format!("unterminated template literal starting at {start}"));
        }
        let b = bytes[*i as usize];
        if b == b'`' {
            if !buf.is_empty() || !raw.is_empty() || parts.is_empty() {
                let s = torajs_wtf8::Wtf8Buf::from_bytes(std::mem::take(&mut buf))
                    .map_err(|_| format!("invalid utf-8 in template at {start}"))?;
                let r = std::str::from_utf8(&raw)
                    .map_err(|_| format!("invalid utf-8 in template at {start}"))?
                    .to_string();
                parts.push(TemplatePart::Lit { cooked: s, raw: r });
            }
            *i += 1; // consume closing backtick
            break;
        }
        // S156 — interpret escape sequences in literal
        // segments per ES §12.8.6.2 (TV(TemplateCharacter)).
        // Same set as `"..."` / `'...'` string literals
        // handles: n/t/r/0/v/f/b → control bytes, `/$/{ →
        // self, \\ → backslash, \" \' → quote, \` → ` (the
        // backtick wouldn't otherwise be reachable inside
        // the template). Unknown escapes pass through as
        // the escaped char (lenient — mirrors v8/bun).
        if b == b'\\' && *i + 1 < len {
            let esc = bytes[(*i + 1) as usize];
            // §12.9.4.3 LineContinuation — `\` + LineTerminator
            // Sequence contributes nothing (TV is the empty
            // sequence; TRV keeps `\` + the normalized `\n`).
            // `\r\n` is one sequence; U+2028 / U+2029 are
            // E2 80 A8 / A9 in UTF-8.
            if esc == b'\n' {
                raw.extend_from_slice(b"\\\n");
                *i += 2;
                continue;
            }
            if esc == b'\r' {
                raw.extend_from_slice(b"\\\n");
                *i += 2;
                if *i < len && bytes[*i as usize] == b'\n' {
                    *i += 1;
                }
                continue;
            }
            if esc == 0xE2
                && *i + 3 < len
                && bytes[(*i + 2) as usize] == 0x80
                && (bytes[(*i + 3) as usize] == 0xA8 || bytes[(*i + 3) as usize] == 0xA9)
            {
                raw.push(b'\\');
                raw.extend_from_slice(&bytes[(*i + 1) as usize..(*i + 4) as usize]);
                *i += 4;
                continue;
            }
            // `\xNN` / `\uNNNN` / `\u{N…N}` cook the same as in a string
            // literal (§12.9.6 TV); TRV keeps the spelling verbatim.
            if matches!(esc, b'x' | b'u')
                && let Some((cp, n)) = super::util::scan_hex_escape(bytes, *i)
            {
                super::util::push_codepoint(&mut buf, cp);
                raw.extend_from_slice(&bytes[*i as usize..(*i + n) as usize]);
                *i += n;
                continue;
            }
            let mapped: u8 = match esc {
                b'n' => b'\n',
                b't' => b'\t',
                b'r' => b'\r',
                b'0' => 0,
                b'v' => 0x0B,
                b'f' => 0x0C,
                b'b' => 0x08,
                b'\\' => b'\\',
                b'`' => b'`',
                b'$' => b'$',
                b'\'' => b'\'',
                b'"' => b'"',
                other => other,
            };
            buf.push(mapped);
            raw.push(b'\\');
            raw.push(esc);
            *i += 2;
            continue;
        }
        if b == b'$' && peek(bytes, *i + 1) == Some(b'{') {
            // Flush literal segment (even if empty — we
            // need the alternation).
            let s = torajs_wtf8::Wtf8Buf::from_bytes(std::mem::take(&mut buf))
                .map_err(|_| format!("invalid utf-8 in template at {start}"))?;
            let r = std::str::from_utf8(&raw)
                .map_err(|_| format!("invalid utf-8 in template at {start}"))?
                .to_string();
            parts.push(TemplatePart::Lit { cooked: s, raw: r });
            buf.clear();
            raw.clear();
            *i += 2; // consume `${`
            let expr_start = *i;
            let mut depth: i32 = 1;
            while *i < len && depth > 0 {
                match bytes[*i as usize] {
                    b'{' => depth += 1,
                    b'}' => depth -= 1,
                    _ => {}
                }
                if depth == 0 {
                    break;
                }
                *i += 1;
            }
            if *i >= len {
                return Err(format!(
                    "unterminated template `${{...}}` interpolation at {start}"
                ));
            }
            let expr_end = *i;
            *i += 1; // consume `}`
            let expr_src = std::str::from_utf8(&bytes[expr_start as usize..expr_end as usize])
                .map_err(|_| format!("invalid utf-8 in template interp at {start}"))?;
            let inner = super::tokenize(expr_src)?;
            // Keep the trailing Eof so the sub-Parser's
            // peek() never falls off the end (its expr
            // parsers rely on the Eof guard).
            parts.push(TemplatePart::Expr(inner));
            continue;
        }
        // §12.9.6 — a bare `\r` / `\r\n` normalizes to `\n` in BOTH
        // spellings (TV and TRV of LineTerminatorSequence).
        if b == b'\r' {
            buf.push(b'\n');
            raw.push(b'\n');
            *i += 1;
            if *i < len && bytes[*i as usize] == b'\n' {
                *i += 1;
            }
            continue;
        }
        buf.push(b);
        raw.push(b);
        *i += 1;
    }
    emit(out, Token::Template { parts }, start, *i);
    Ok(())
}
