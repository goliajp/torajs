//! ES §22.1.3.18.5 GetSubstitution — string-needle case.
//!
//! Replacement-template `$` substitution for `__torajs_str_replace`
//! and `__torajs_str_replace_all`. Split out of
//! `transform/replace.rs` to keep that file under the 500-LOC hard
//! limit (the per-module-local substitution machinery is ~135 LOC
//! self-contained; the replace wrappers only call into this on the
//! `template_has_dollar` branch).
//!
//! Patterns recognized (string needle, no captures):
//! - `$$` → `$`
//! - `$&` → matched substring
//! - `` $` `` → portion of `s` before the match
//! - `$'` → portion of `s` after the match
//! - `$N` (ASCII digit), `$<name>` — preserved literally. Per ES
//!   §22.1.3.18.5, with a string needle `captures` is an empty list
//!   so `$N` is out of range, and `namedCaptures` is `undefined` so
//!   `$<name>` is not interpreted. bun mirrors this.
//! - Trailing `$` or `$` followed by any other character — preserved
//!   literally.

use alloc::vec::Vec;

/// Does `template` contain any `$` at a code-unit boundary? `stride`
/// is 1 for Latin-1 / 2 for UTF-16 LE; UTF-16 LE encodes `$` as
/// `24 00` so we only need to check the leading byte of each code
/// unit. O(n) byte scan; called once per replace / replaceAll to
/// decide between the substitution-aware path and the zero-
/// allocation fast path.
#[inline]
pub fn template_has_dollar(template: &[u8], stride: usize) -> bool {
    let mut i = 0;
    while i + stride <= template.len() {
        if template[i] == b'$' {
            return true;
        }
        i += stride;
    }
    false
}

/// ES §22.1.3.18.5 GetSubstitution — string-needle case. Expand
/// `template` into `out`. All inputs (`template`, `matched`, `pre`,
/// `post`) share the same encoding (caller coerces beforehand);
/// `stride` is the byte width of one code unit (1 or 2). The
/// trigger characters (`$`, `&`, `` ` ``, `'`, `<`, ASCII digits)
/// are all ≤ 0x7F so their leading byte is identical under Latin-1
/// and UTF-16 LE; under UTF-16 LE the trailing 0x00 byte rides
/// along inside the emitted code unit.
pub fn expand_str_replacement(
    template: &[u8],
    matched: &[u8],
    pre: &[u8],
    post: &[u8],
    stride: usize,
    out: &mut Vec<u8>,
) {
    let mut i = 0;
    while i + stride <= template.len() {
        if template[i] == b'$' && i + 2 * stride <= template.len() {
            let next = template[i + stride];
            match next {
                b'$' => {
                    out.extend_from_slice(&template[i..i + stride]);
                    i += 2 * stride;
                    continue;
                }
                b'&' => {
                    out.extend_from_slice(matched);
                    i += 2 * stride;
                    continue;
                }
                b'`' => {
                    out.extend_from_slice(pre);
                    i += 2 * stride;
                    continue;
                }
                b'\'' => {
                    out.extend_from_slice(post);
                    i += 2 * stride;
                    continue;
                }
                // `$N` (ASCII digit) and `$<name>` are preserved
                // literally when the needle is a string — `captures`
                // is empty (so `$N` is out of range) and
                // `namedCaptures` is `undefined`. Fall through to
                // the default arm: emit the `$` code unit and let
                // the digit / `<...>` body get copied verbatim on
                // subsequent iterations. bun mirrors this.
                _ => {}
            }
        }
        out.extend_from_slice(&template[i..i + stride]);
        i += stride;
    }
}

/// Per-match substitution-aware variant of `replace_all_into`. Walks
/// `s`, expanding `template`'s `$` references against each match's
/// (`pre`, `matched`, `post`) window. Caller invokes only when
/// `template_has_dollar` returned true; the dollar-free fast path
/// in `replace.rs` stays zero-allocation.
pub fn replace_all_with_substitution(
    s: &[u8],
    needle: &[u8],
    template: &[u8],
    stride: usize,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let n_len = needle.len();
    let mut i = 0;
    if n_len > 0 && s.len() >= n_len {
        let limit = s.len() - n_len;
        while i <= limit {
            if &s[i..i + n_len] == needle {
                let pre = &s[..i];
                let post = &s[i + n_len..];
                expand_str_replacement(template, needle, pre, post, stride, &mut out);
                i += n_len;
            } else {
                out.extend_from_slice(&s[i..i + stride]);
                i += stride;
            }
        }
    }
    if i < s.len() {
        out.extend_from_slice(&s[i..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn has_dollar_latin1() {
        assert!(template_has_dollar(b"abc$&", 1));
        assert!(!template_has_dollar(b"plain", 1));
        assert!(template_has_dollar(b"$", 1));
        assert!(!template_has_dollar(b"", 1));
    }

    #[test]
    fn has_dollar_utf16_le() {
        // "$&" in UTF-16 LE: 24 00 26 00
        assert!(template_has_dollar(&[0x24, 0x00, 0x26, 0x00], 2));
        // "ab" in UTF-16 LE: 61 00 62 00 — no `$`
        assert!(!template_has_dollar(&[0x61, 0x00, 0x62, 0x00], 2));
    }

    #[test]
    fn expand_dollar_dollar_emits_literal() {
        let mut out = Vec::new();
        expand_str_replacement(b"$$x", b"M", b"P", b"Q", 1, &mut out);
        assert_eq!(&out, b"$x");
    }

    #[test]
    fn expand_dollar_amp_emits_matched() {
        let mut out = Vec::new();
        expand_str_replacement(b"[$&]", b"MM", b"P", b"Q", 1, &mut out);
        assert_eq!(&out, b"[MM]");
    }

    #[test]
    fn expand_dollar_backtick_emits_pre() {
        let mut out = Vec::new();
        expand_str_replacement(b"<$`>", b"M", b"PP", b"Q", 1, &mut out);
        assert_eq!(&out, b"<PP>");
    }

    #[test]
    fn expand_dollar_quote_emits_post() {
        let mut out = Vec::new();
        expand_str_replacement(b"<$'>", b"M", b"P", b"QQ", 1, &mut out);
        assert_eq!(&out, b"<QQ>");
    }

    #[test]
    fn expand_dollar_digit_preserved_literally() {
        // String-needle: `captures` is empty so `$N` is out of range
        // → preserved verbatim (ES §22.1.3.18.5).
        let mut out = Vec::new();
        expand_str_replacement(b"a$1b", b"M", b"P", b"Q", 1, &mut out);
        assert_eq!(&out, b"a$1b");
        out.clear();
        expand_str_replacement(b"a$12b", b"M", b"P", b"Q", 1, &mut out);
        assert_eq!(&out, b"a$12b");
    }

    #[test]
    fn expand_dollar_named_preserved_literally() {
        // String-needle: `namedCaptures` is undefined so `$<name>` is
        // preserved verbatim (ES §22.1.3.18.5).
        let mut out = Vec::new();
        expand_str_replacement(b"a$<x>b", b"M", b"P", b"Q", 1, &mut out);
        assert_eq!(&out, b"a$<x>b");
    }

    #[test]
    fn expand_dollar_unrecognized_preserves_literal() {
        let mut out = Vec::new();
        expand_str_replacement(b"a$Zb", b"M", b"P", b"Q", 1, &mut out);
        assert_eq!(&out, b"a$Zb");
    }

    #[test]
    fn expand_trailing_dollar_preserved() {
        let mut out = Vec::new();
        expand_str_replacement(b"end$", b"M", b"P", b"Q", 1, &mut out);
        assert_eq!(&out, b"end$");
    }

    #[test]
    fn replace_all_with_substitution_amp() {
        let out = replace_all_with_substitution(b"aaa", b"a", b"$&$&", 1);
        assert_eq!(&out, b"aaaaaa");
    }

    #[test]
    fn replace_all_with_substitution_pre_post() {
        // s = "axb", needle = "x", template = "[$`|$']" → "[a|b]" at the
        // single match position.
        let out = replace_all_with_substitution(b"axb", b"x", b"[$`|$']", 1);
        assert_eq!(&out, b"a[a|b]b");
    }
}
