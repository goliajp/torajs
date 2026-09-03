//! Numeric literal scanner — split out so the rest of the
//! `tokenize` match-arm dispatcher stays small. Handles every shape
//! ssa_lower accepts: decimal, leading-dot, scientific, BigInt,
//! binary (`0b`), octal (`0o`), hex (`0x`).
//!
//! Extracted from `lexer.rs` (2026-05-25, god-file decomp batch 23).

use super::util::{emit, peek};
use super::{Spanned, Token};

/// One digit of a base-2 / -8 / -16 literal body.
fn is_radix_digit(c: u8, radix: u32) -> bool {
    match radix {
        2 => c == b'0' || c == b'1',
        8 => (b'0'..=b'7').contains(&c),
        _ => c.is_ascii_hexdigit(),
    }
}

/// §12.9.3 NumericLiteralSeparator — a `_` is legal ONLY between two
/// digits of the same digit run. Trailing / leading / consecutive
/// separators and separators touching `.` / `e` / the radix prefix
/// are SyntaxError (rotation 264; the prior tolerant strip silently
/// accepted `1_` / `1__2` / `0x_1` — bun rejects all of them).
/// `digit_ok` picks the radix's digit set.
fn validate_separators(raw: &str, start: u32, digit_ok: impl Fn(u8) -> bool) -> Result<(), String> {
    let b = raw.as_bytes();
    for (k, &c) in b.iter().enumerate() {
        if c != b'_' {
            continue;
        }
        let prev_ok = k > 0 && digit_ok(b[k - 1]);
        let next_ok = k + 1 < b.len() && digit_ok(b[k + 1]);
        if !prev_ok || !next_ok {
            return Err(format!("invalid numeric separator at {start}"));
        }
    }
    Ok(())
}

/// Shared body of the `0b` / `0o` / `0x` arms (chunk 476 — the three
/// were byte-for-byte the same shape): skip the 2-byte prefix, scan
/// radix digits (`_` separators allowed between digits), strip
/// separators, handle the `n` BigInt suffix, else parse as u64 → f64
/// per JS spec §12.8.3.
///
/// BigInt suffix shape differs by radix (pre-split behaviour kept):
/// - binary / octal (P0.10) — pre-convert to decimal at lex time
///   (`u64` parse + `to_string`, `radix: 10`; ssa_lower's
///   `bigint_from_decimal` handles it).
/// - hex (T-25) — digits pass through verbatim with `radix: 16`.
fn scan_radix_literal(
    bytes: &[u8],
    i: &mut u32,
    out: &mut Vec<Spanned>,
    start: u32,
    len: u32,
    radix: u32,
    name: &str,
) -> Result<(), String> {
    *i += 2; // skip "0b" / "0o" / "0x"
    let dig_start = *i;
    while *i < len && (is_radix_digit(bytes[*i as usize], radix) || bytes[*i as usize] == b'_') {
        *i += 1;
    }
    if *i == dig_start {
        return Err(format!("invalid {name} literal at {start}"));
    }
    let raw = std::str::from_utf8(&bytes[dig_start as usize..*i as usize])
        .expect("ascii radix digits are valid utf-8");
    validate_separators(raw, start, |c| is_radix_digit(c, radix))?;
    let cleaned;
    let s: &str = if raw.contains('_') {
        cleaned = raw.replace('_', "");
        &cleaned
    } else {
        raw
    };
    if peek(bytes, *i) == Some(b'n') {
        if radix == 16 {
            let digits = s.to_string();
            *i += 1;
            emit(out, Token::BigInt { digits, radix: 16 }, start, *i);
        } else {
            let n: u64 = u64::from_str_radix(s, radix)
                .map_err(|_| format!("invalid {name} BigInt at {start}"))?;
            *i += 1;
            emit(
                out,
                Token::BigInt {
                    digits: n.to_string(),
                    radix: 10,
                },
                start,
                *i,
            );
        }
        return Ok(());
    }
    let n: u64 =
        u64::from_str_radix(s, radix).map_err(|_| format!("invalid {name} number at {start}"))?;
    emit(out, Token::Number(n as f64), start, *i);
    Ok(())
}

pub(super) fn scan_number(
    bytes: &[u8],
    i: &mut u32,
    out: &mut Vec<Spanned>,
    start: u32,
    len: u32,
    b: u8,
) -> Result<(), String> {
    // V3-18 m1.h.55 — `0b...` binary and `0o...` octal literals per
    // JS spec §12.8.3; 0x... hex is TS / JS standard (u64 parse then
    // f64 cast — values up to 2^53 round-trip exactly, covering every
    // realistic bitwise / mask use). All three share
    // `scan_radix_literal` incl. the `n` BigInt suffix.
    if b == b'0' && peek(bytes, *i + 1).is_some_and(|c| c == b'b' || c == b'B') {
        return scan_radix_literal(bytes, i, out, start, len, 2, "binary");
    }
    if b == b'0' && peek(bytes, *i + 1).is_some_and(|c| c == b'o' || c == b'O') {
        return scan_radix_literal(bytes, i, out, start, len, 8, "octal");
    }
    if b == b'0' && peek(bytes, *i + 1).is_some_and(|c| c == b'x' || c == b'X') {
        return scan_radix_literal(bytes, i, out, start, len, 16, "hex");
    }
    // V3-18 m1.h.55 — numeric separator `_` (per JS spec §12.8.3
    // NumericLiteralSeparator). Scanned into the run here, position-
    // validated by `validate_separators` below, stripped before
    // parsing.
    while *i < len && (bytes[*i as usize].is_ascii_digit() || bytes[*i as usize] == b'_') {
        *i += 1;
    }
    if peek(bytes, *i) == Some(b'.') && peek(bytes, *i + 1).is_some_and(|c| c.is_ascii_digit()) {
        *i += 1;
        while *i < len && (bytes[*i as usize].is_ascii_digit() || bytes[*i as usize] == b'_') {
            *i += 1;
        }
    } else if peek(bytes, *i) == Some(b'.')
        && peek(bytes, *i + 1).is_some_and(|c| c == b'e' || c == b'E')
    {
        // P0.10 — trailing-dot before exponent: `1.e5` / `1.E-3`
        // per ES spec §12.9.3 DecimalLiteral. Eat the dot here so
        // the exponent loop below picks up `e5`.
        *i += 1;
    } else if peek(bytes, *i) == Some(b'.')
        && peek(bytes, *i + 1).is_some_and(|c| {
            // P0.10 — trailing-dot DecimalLiteral followed by
            // anything that's NOT a member access continuation:
            // `8. !== 8`, `9.; foo()`, etc. Per ES spec §12.9.3
            // DecimalLiteral the trailing `.` is part of the
            // integer literal. Eat the dot when the lookahead
            // disqualifies member-access (Ident-start letter,
            // `_`, `$`).
            !c.is_ascii_alphanumeric() && c != b'_' && c != b'$' && c != b'.'
        })
    {
        *i += 1;
    } else if peek(bytes, *i) == Some(b'.') && peek(bytes, *i + 1) == Some(b'.') {
        // V3-18 m1.h.21 — `0..toString()` form. JS spec §12.8.3
        // allows DecimalLiteral to end with a trailing `.`; the
        // second `.` then begins a member access.
        *i += 1;
    }
    // Scientific notation: `e` / `E` optionally followed by `+` /
    // `-`, then one or more digits. Only consume when the suffix
    // is a real exponent — `1eFoo` parses as the number `1`
    // followed by the ident `eFoo`.
    if (peek(bytes, *i) == Some(b'e') || peek(bytes, *i) == Some(b'E')) && {
        let mut j = *i + 1;
        if peek(bytes, j) == Some(b'+') || peek(bytes, j) == Some(b'-') {
            j += 1;
        }
        peek(bytes, j).is_some_and(|c| c.is_ascii_digit())
    } {
        *i += 1;
        if peek(bytes, *i) == Some(b'+') || peek(bytes, *i) == Some(b'-') {
            *i += 1;
        }
        // P0.10 — accept `_` numeric separators inside exponent
        // digits per ES2021 (`1e1_0`).
        while *i < len && (bytes[*i as usize].is_ascii_digit() || bytes[*i as usize] == b'_') {
            *i += 1;
        }
    }
    let raw = std::str::from_utf8(&bytes[start as usize..*i as usize])
        .expect("ascii digits are valid utf-8");
    validate_separators(raw, start, |c| c.is_ascii_digit())?;
    // §12.9.3 — a DecimalIntegerLiteral starting `0<digit>` is the
    // LEGACY octal-like family, which admits no separators in any
    // grammar (`0_0` / `0_8` — bun rejects both; the bare `01` / `09`
    // strict-mode rejection is a recorded policy item, half-blade B).
    // Scoped to the INTEGER part: `0.5_5` / `0e1_0` keep their legal
    // fraction / exponent separators.
    let int_part = &raw[..raw.find(['.', 'e', 'E']).unwrap_or(raw.len())];
    if int_part.len() > 1 && int_part.starts_with('0') && int_part.contains('_') {
        return Err(format!(
            "numeric separator in a leading-zero literal at {start}"
        ));
    }
    // V3-18 m1.h.55 — strip numeric separators before parsing
    // into f64 / BigInt.
    let s_owned;
    let s: &str = if raw.contains('_') {
        s_owned = raw.replace('_', "");
        &s_owned
    } else {
        raw
    };
    /* T-25 BigInt: `<integer>n` literal. Only matches when the
     * lexeme has no `.` or `e/E` (decimal-only integer) and is
     * followed by `n`. JS rejects `1.5n` / `1e2n` at parse time —
     * same here. */
    if peek(bytes, *i) == Some(b'n') && !s.contains('.') && !s.contains('e') && !s.contains('E') {
        // §12.9.3 BigIntLiteralSuffix hangs off DecimalIntegerLiteral,
        // never the legacy octal-like family — `01n` / `08n` are
        // SyntaxError in every mode (bun rejects; the tolerant lex
        // minted BigInt(01) silently).
        if s.len() > 1 && s.starts_with('0') {
            return Err(format!("invalid BigInt literal at {start}"));
        }
        let digits = s.to_string();
        *i += 1;
        emit(out, Token::BigInt { digits, radix: 10 }, start, *i);
        return Ok(());
    }
    // annexB §B.1.1 — a `0`-prefixed integer run is the legacy family:
    // `010` is 8, not 10. `08` / `0778` are the NonOctalDecimal half and
    // keep their decimal value, which is what `parse` below already
    // gave; routing them through the same judge keeps one grammar.
    let n: f64 = match super::legacy_octal::legacy_octal_number_value(s) {
        Some(v) => v,
        None => s
            .parse()
            .map_err(|_| format!("invalid number at {start}"))?,
    };
    emit(out, Token::Number(n), start, *i);
    Ok(())
}

/// `.` dispatch — `...` spread, leading-dot numeric literal
/// (`.5` / `.123e2` per ES §12.9.3), or bare member-access `.`.
/// Extracted verbatim from `tokenize`'s `b'.'` match arm
/// (2026-07-03, fn-debt decomp; only change is `i` threading
/// through `&mut u32`).
pub(super) fn scan_dot(bytes: &[u8], i: &mut u32, out: &mut Vec<Spanned>, start: u32) {
    // `...` (spread/rest) emits a single DotDotDot token.
    // Bare `.` stays Dot for member access.
    if peek(bytes, *i + 1) == Some(b'.') && peek(bytes, *i + 2) == Some(b'.') {
        *i += 3;
        emit(out, Token::DotDotDot, start, *i);
    } else if peek(bytes, *i + 1).is_some_and(|c| c.is_ascii_digit()) {
        // P0.10 — leading-dot numeric literal: `.5`,
        // `.123`, `.5e2` per ES spec §12.9.3 NumericLiteral.
        // Pre-fix tora's lexer always emitted Token::Dot
        // here, leaving the parser to bail with 'expected
        // expression, got Dot'. Now consume the fractional
        // tail (and optional exponent) inline as part of
        // the numeric value, mirroring what the post-Int
        // path does.
        *i += 1; // consume `.`
        let mut digits = String::from("0.");
        while let Some(c) = peek(bytes, *i) {
            if c.is_ascii_digit() || c == b'_' {
                if c != b'_' {
                    digits.push(c as char);
                }
                *i += 1;
            } else {
                break;
            }
        }
        // Optional exponent: `[eE][+-]?DIGITS`
        if let Some(c) = peek(bytes, *i)
            && (c == b'e' || c == b'E')
        {
            digits.push(c as char);
            *i += 1;
            if let Some(s) = peek(bytes, *i)
                && (s == b'+' || s == b'-')
            {
                digits.push(s as char);
                *i += 1;
            }
            while let Some(c) = peek(bytes, *i) {
                if c.is_ascii_digit() {
                    digits.push(c as char);
                    *i += 1;
                } else {
                    break;
                }
            }
        }
        let n: f64 = digits.parse().unwrap_or(0.0);
        emit(out, Token::Number(n), start, *i);
    } else {
        emit(out, Token::Dot, start, super::util::advance(i));
    }
}
