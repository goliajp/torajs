//! Numeric literal scanner — split out so the rest of the
//! `tokenize` match-arm dispatcher stays small. Handles every shape
//! ssa_lower accepts: decimal, leading-dot, scientific, BigInt,
//! binary (`0b`), octal (`0o`), hex (`0x`).
//!
//! Extracted from `lexer.rs` (2026-05-25, god-file decomp batch 23).

use super::util::{emit, peek};
use super::{Spanned, Token};

pub(super) fn scan_number(
    bytes: &[u8],
    i: &mut u32,
    out: &mut Vec<Spanned>,
    start: u32,
    len: u32,
    b: u8,
) -> Result<(), String> {
    // V3-18 m1.h.55 — `0b...` binary and `0o...` octal
    // literals per JS spec §12.8.3. Both lex as base-2 / -8
    // u64, then cast to f64 (matching the existing 0x...
    // path). Same `n` BigInt suffix support.
    if b == b'0' && peek(bytes, *i + 1).is_some_and(|c| c == b'b' || c == b'B') {
        *i += 2;
        let dig_start = *i;
        while *i < len
            && (bytes[*i as usize] == b'0'
                || bytes[*i as usize] == b'1'
                || bytes[*i as usize] == b'_')
        {
            *i += 1;
        }
        if *i == dig_start {
            return Err(format!("invalid binary literal at {start}"));
        }
        let raw = std::str::from_utf8(&bytes[dig_start as usize..*i as usize])
            .expect("ascii bin digits are valid utf-8");
        let cleaned;
        let s: &str = if raw.contains('_') {
            cleaned = raw.replace('_', "");
            &cleaned
        } else {
            raw
        };
        // P0.10 — binary BigInt `0b...n`. Pre-convert to decimal at
        // lex time (ssa_lower's bigint_from_decimal handles it).
        if peek(bytes, *i) == Some(b'n') {
            let n: u64 = u64::from_str_radix(s, 2)
                .map_err(|_| format!("invalid binary BigInt at {start}"))?;
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
            return Ok(());
        }
        let n: u64 =
            u64::from_str_radix(s, 2).map_err(|_| format!("invalid binary number at {start}"))?;
        emit(out, Token::Number(n as f64), start, *i);
        return Ok(());
    }
    if b == b'0' && peek(bytes, *i + 1).is_some_and(|c| c == b'o' || c == b'O') {
        *i += 2;
        let dig_start = *i;
        while *i < len
            && ((bytes[*i as usize] >= b'0' && bytes[*i as usize] <= b'7')
                || bytes[*i as usize] == b'_')
        {
            *i += 1;
        }
        if *i == dig_start {
            return Err(format!("invalid octal literal at {start}"));
        }
        let raw = std::str::from_utf8(&bytes[dig_start as usize..*i as usize])
            .expect("ascii oct digits are valid utf-8");
        let cleaned;
        let s: &str = if raw.contains('_') {
            cleaned = raw.replace('_', "");
            &cleaned
        } else {
            raw
        };
        // P0.10 — octal BigInt `0o...n`. Same shape as binary BigInt.
        if peek(bytes, *i) == Some(b'n') {
            let n: u64 = u64::from_str_radix(s, 8)
                .map_err(|_| format!("invalid octal BigInt at {start}"))?;
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
            return Ok(());
        }
        let n: u64 =
            u64::from_str_radix(s, 8).map_err(|_| format!("invalid octal number at {start}"))?;
        emit(out, Token::Number(n as f64), start, *i);
        return Ok(());
    }
    // 0x... hex literal — TS / JS standard. Parse as u64 and
    // cast to f64; values up to 2^53 round-trip exactly, which
    // covers every realistic bitwise / mask use.
    if b == b'0' && peek(bytes, *i + 1).is_some_and(|c| c == b'x' || c == b'X') {
        *i += 2; // skip "0x"
        let hex_start = *i;
        while *i < len && (bytes[*i as usize].is_ascii_hexdigit() || bytes[*i as usize] == b'_') {
            *i += 1;
        }
        if *i == hex_start {
            return Err(format!("invalid hex literal at {start}"));
        }
        let raw = std::str::from_utf8(&bytes[hex_start as usize..*i as usize])
            .expect("ascii hex digits are valid utf-8");
        let cleaned;
        let s: &str = if raw.contains('_') {
            cleaned = raw.replace('_', "");
            &cleaned
        } else {
            raw
        };
        /* T-25 BigInt: `0x...n`. Hex-radix BigInt literal. */
        if peek(bytes, *i) == Some(b'n') {
            let digits = s.to_string();
            *i += 1;
            emit(out, Token::BigInt { digits, radix: 16 }, start, *i);
            return Ok(());
        }
        let n: u64 =
            u64::from_str_radix(s, 16).map_err(|_| format!("invalid hex number at {start}"))?;
        emit(out, Token::Number(n as f64), start, *i);
        return Ok(());
    }
    // V3-18 m1.h.55 — numeric separator `_` (per JS spec §12.8.3
    // NumericLiteralSeparator). Stripped before parsing. Allowed
    // between digits only; consecutive `_` or leading/trailing `_`
    // aren't valid but our tolerant parse silently allows them —
    // strict spec rejection is a polish item.
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
        let digits = s.to_string();
        *i += 1;
        emit(out, Token::BigInt { digits, radix: 10 }, start, *i);
        return Ok(());
    }
    let n: f64 = s
        .parse()
        .map_err(|_| format!("invalid number at {start}"))?;
    emit(out, Token::Number(n), start, *i);
    Ok(())
}
