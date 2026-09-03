//! Annex B §B.1.1 / §B.1.2 legacy octal — the one place that decides
//! what `\101` and `010` mean, shared by everyone who has to ask.
//!
//! Two consumers, deliberately the same code: the scanners
//! ([`super::scan::scan_string`], [`super::scan_number::scan_number`],
//! [`super::scan_template`]) call it for the VALUE, and the parse-time
//! recorder ([`crate::ast::legacy_octal_sites`]) calls it over the same
//! spans for the POSITIONS the strict goal rejects. Two hand-written
//! copies of this grammar would drift, and the drift would be a value
//! that is legal under one of them and an error under the other.
//!
//! The value half is unconditional — sloppy script code really does
//! evaluate `"\101"` to `"A"` and `010` to `8`, and tr answered `"101"`
//! and `10`. The rejection half is the goal's, not the lexer's, and
//! lives in the recorder.

/// `\` + LegacyOctalEscapeSequence (§B.1.2) at `i` (which points at the
/// backslash). `Some((code_unit, width_including_backslash))`.
///
/// The productions, spelled out because the lookaheads carry them:
/// `0` [lookahead ∈ {8,9}] · NonZeroOctalDigit [lookahead ∉ OctalDigit]
/// · ZeroToThree OctalDigit [lookahead ∉ OctalDigit] · FourToSeven
/// OctalDigit · ZeroToThree OctalDigit OctalDigit.
///
/// A bare `\0` NOT followed by a decimal digit is the ordinary NUL
/// escape of §12.9.4.1 and is not legacy at all — this returns `None`
/// for it, and the caller's `\0` arm keeps handling it.
pub(crate) fn scan_legacy_octal_escape(bytes: &[u8], i: u32) -> Option<(u32, u32)> {
    let oct = |k: u32| {
        bytes
            .get(k as usize)
            .copied()
            .filter(|c| (b'0'..=b'7').contains(c))
    };
    let d0 = oct(i + 1)?;
    // `\0` is legacy only when a decimal digit follows: `\08` is
    // NUL-then-`8` under the legacy production, `\0` alone is not.
    if d0 == b'0' && !bytes.get((i + 2) as usize).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let v0 = u32::from(d0 - b'0');
    let Some(d1) = oct(i + 2) else {
        return Some((v0, 2));
    };
    let v1 = v0 * 8 + u32::from(d1 - b'0');
    // Only ZeroToThree admits a third digit — `\401` is `\40` then the
    // character `1`, which is what keeps the value inside one code unit.
    if v0 <= 3
        && let Some(d2) = oct(i + 3)
    {
        return Some((v1 * 8 + u32::from(d2 - b'0'), 4));
    }
    Some((v1, 3))
}

/// Does `\` at `i` begin a NonOctalDecimalEscapeSequence (`\8` / `\9`,
/// §B.1.2)? Its VALUE needs no help — the passthrough arm yields the
/// digit itself, which is what the production says — so this answers
/// only the question the strict goal asks.
pub(crate) fn is_non_octal_decimal_escape(bytes: &[u8], i: u32) -> bool {
    matches!(bytes.get((i + 1) as usize), Some(b'8' | b'9'))
}

/// Offset of the first legacy escape in `bytes[from..to]`, or `None`.
///
/// Steps two bytes per backslash rather than decoding, which is safe
/// because no longer escape (`\xNN`, `\uNNNN`, `\u{…}`) contains a
/// backslash for the walk to land inside of, and an escaped backslash
/// (`\\101`) is stepped over as the pair it is. Callers pass a whole
/// literal's span, quotes and all — a quote is never a backslash.
pub(crate) fn first_legacy_escape(bytes: &[u8], from: u32, to: u32) -> Option<u32> {
    let mut i = from;
    while i + 1 < to {
        if bytes[i as usize] == b'\\' {
            if scan_legacy_octal_escape(bytes, i).is_some() || is_non_octal_decimal_escape(bytes, i)
            {
                return Some(i);
            }
            i += 2;
            continue;
        }
        i += 1;
    }
    None
}

/// The value of a `0`-prefixed integer literal under Annex B §B.1.1,
/// given the literal's raw spelling.
///
/// `None` = not the legacy family at all (`0`, `0.5`, `0x10`, `1`).
/// `Some(v)` covers both halves: LegacyOctalIntegerLiteral (`010` → 8)
/// and NonOctalDecimalIntegerLiteral (`08` → 8, `0778` → 778), which
/// share the shape and differ only in whether an `8` or `9` appears —
/// one digit anywhere in the run turns the whole thing decimal.
pub(crate) fn legacy_octal_number_value(raw: &str) -> Option<f64> {
    let rest = raw.strip_prefix('0').filter(|r| !r.is_empty())?;
    if !rest.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if rest.bytes().any(|c| c == b'8' || c == b'9') {
        return raw.parse().ok();
    }
    u64::from_str_radix(rest, 8).ok().map(|v| v as f64)
}
