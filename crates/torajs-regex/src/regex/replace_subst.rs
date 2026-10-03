//! GetSubstitution (ES §22.1.3.19.1) over the engine's byte haystack:
//! `$&` (whole match), `` $` `` (portion before the match), `$'`
//! (portion after the match), `$1`..`$99` (capture groups; two-digit
//! form when the resulting index is a valid group), `$<name>` (named
//! capture; empty when the name is unknown or the group did not
//! participate — literal when the pattern has no named groups at all),
//! `$$` (literal `$`), other `$X` left literal.

use alloc::vec::Vec;

use crate::node::REGEX_MAX_CAPTURES;
use crate::vm::save_slot;

/// The match a replacement pattern expands against: the subject `s`,
/// the matched span `st..en`, the capture `saves` pairs, and the
/// pattern's capture table. `capture_names` is the `(?<name>...)` table
/// (index 0 unused; empty `Vec` at positional groups) and `has_named`
/// its non-empty-entry count gate — `$<name>` is only interpreted when
/// the pattern has at least one named group (namedCaptures ≠ undefined
/// per GetSubstitution).
#[derive(Clone, Copy)]
pub struct ReplMatch<'a> {
    pub s: &'a [u8],
    pub st: i64,
    pub en: i64,
    pub saves: &'a [i64],
    pub n_captures: i32,
    pub capture_names: &'a [Vec<u8>],
    pub has_named: bool,
}

/// Expand `repl` into `out`, dereferencing `$N` against the match's
/// captured `saves[]` pairs. Unparticipating groups substitute the
/// empty string.
pub fn expand_repl(repl: &[u8], m: ReplMatch<'_>, out: &mut Vec<u8>) {
    let ReplMatch {
        s,
        st,
        en,
        saves,
        n_captures,
        capture_names,
        has_named,
    } = m;
    let mut i = 0;
    while i < repl.len() {
        let c = repl[i];
        if c != b'$' || i + 1 >= repl.len() {
            out.push(c);
            i += 1;
            continue;
        }
        let nxt = repl[i + 1];
        if nxt == b'$' {
            out.push(b'$');
            i += 2;
            continue;
        }
        if nxt == b'&' {
            out.extend_from_slice(&s[st as usize..en as usize]);
            i += 2;
            continue;
        }
        if nxt == b'`' {
            out.extend_from_slice(&s[..st as usize]);
            i += 2;
            continue;
        }
        if nxt == b'\'' {
            out.extend_from_slice(&s[en as usize..]);
            i += 2;
            continue;
        }
        if nxt == b'<' && has_named {
            // `$<name>` — scan for the closing `>`. Without one the
            // whole thing is literal (fall through to the `$` arm).
            if let Some(close) = repl[i + 2..].iter().position(|&b| b == b'>') {
                let name = &repl[i + 2..i + 2 + close];
                // Duplicate named groups (ES2025): the name maps to
                // whichever same-named group participated; scan all
                // indices carrying the name and take the first with
                // written slots. Unknown name / no participant → empty.
                for (gi, gn) in capture_names.iter().enumerate() {
                    if (1..REGEX_MAX_CAPTURES).contains(&gi) && gn.as_slice() == name {
                        let gs = save_slot(saves, 2 * gi);
                        let ge = save_slot(saves, 2 * gi + 1);
                        if gs >= 0 && ge >= 0 {
                            out.extend_from_slice(&s[gs as usize..ge as usize]);
                            break;
                        }
                    }
                }
                i += 2 + close + 1;
                continue;
            }
        }
        if nxt.is_ascii_digit() {
            let d1 = (nxt - b'0') as i32;
            let mut idx = d1;
            let mut extra_consumed = 0;
            // Try two-digit `$NN` (incl. `$01` → group 1) when the
            // resulting idx is a valid group and fits in saves.
            if i + 2 < repl.len() && repl[i + 2].is_ascii_digit() {
                let two = d1 * 10 + (repl[i + 2] - b'0') as i32;
                if two >= 1 && two <= n_captures && (two as usize) < REGEX_MAX_CAPTURES {
                    idx = two;
                    extra_consumed = 1;
                }
            }
            if idx >= 1 && idx <= n_captures && (idx as usize) < REGEX_MAX_CAPTURES {
                let gs = save_slot(saves, (2 * idx) as usize);
                let ge = save_slot(saves, (2 * idx + 1) as usize);
                if gs >= 0 && ge >= 0 {
                    out.extend_from_slice(&s[gs as usize..ge as usize]);
                }
                i += 2 + extra_consumed;
                continue;
            }
            // `$0` standalone or `$N` for N > n_captures — emit `$`
            // literally; the next iteration will consume the digit.
            out.push(b'$');
            i += 1;
            continue;
        }
        // Unknown `$X` — emit `$` literally; X stays for next iter.
        out.push(b'$');
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::{ReplMatch, expand_repl};
    use alloc::vec;
    use alloc::vec::Vec;

    // s = "abcd", match = "bc" (st 1, en 3), no captures.
    fn expand_plain(repl: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let m = ReplMatch {
            s: b"abcd",
            st: 1,
            en: 3,
            saves: &[1, 3],
            n_captures: 0,
            capture_names: &[],
            has_named: false,
        };
        expand_repl(repl, m, &mut out);
        out
    }

    #[test]
    fn dollar_backtick_emits_pre() {
        assert_eq!(expand_plain(b"[$`]"), b"[a]");
    }

    #[test]
    fn dollar_quote_emits_post() {
        assert_eq!(expand_plain(b"[$']"), b"[d]");
    }

    #[test]
    fn dollar_amp_pre_post_combined() {
        assert_eq!(expand_plain(b"$`|$&|$'"), b"a|bc|d");
    }

    #[test]
    fn named_ref_literal_without_named_groups() {
        // namedCaptures is undefined → `$<x>` stays literal.
        assert_eq!(expand_plain(b"[$<x>]"), b"[$<x>]");
    }

    // s = "abcd", pattern shape `(?<fst>.)(?<snd>.)` matched at 0..2:
    // group 1 = "a" (0,1), group 2 = "b" (1,2).
    fn named_names() -> Vec<Vec<u8>> {
        vec![Vec::new(), b"fst".to_vec(), b"snd".to_vec()]
    }

    fn expand_named(repl: &[u8], saves: &[i64]) -> Vec<u8> {
        let mut out = Vec::new();
        let m = ReplMatch {
            s: b"abcd",
            st: 0,
            en: 2,
            saves,
            n_captures: 2,
            capture_names: &named_names(),
            has_named: true,
        };
        expand_repl(repl, m, &mut out);
        out
    }

    #[test]
    fn named_ref_participating_group() {
        let out = expand_named(b"[$<snd>]", &[0, 2, 0, 1, 1, 2]);
        assert_eq!(out, b"[b]");
    }

    #[test]
    fn named_ref_unknown_name_empty() {
        let out = expand_named(b"[$<fth>]", &[0, 2, 0, 1, 1, 2]);
        assert_eq!(out, b"[]");
    }

    #[test]
    fn named_ref_unparticipating_group_empty() {
        // group 2 slots unwritten (-1) → empty.
        let out = expand_named(b"[$<snd>]", &[0, 2, 0, 1, -1, -1]);
        assert_eq!(out, b"[]");
    }

    #[test]
    fn named_ref_unterminated_stays_literal() {
        let out = expand_named(b"[$<snd]", &[0, 2, 0, 1, 1, 2]);
        assert_eq!(out, b"[$<snd]");
    }

    #[test]
    fn named_ref_duplicate_takes_participant() {
        // `(?<x>a)|(?<x>b)` alternation: same name at 1 and 2, only
        // group 2 participated.
        let names = vec![Vec::new(), b"x".to_vec(), b"x".to_vec()];
        let mut out = Vec::new();
        let m = ReplMatch {
            s: b"ba",
            st: 0,
            en: 1,
            saves: &[0, 1, -1, -1, 0, 1],
            n_captures: 2,
            capture_names: &names,
            has_named: true,
        };
        expand_repl(b"[$<x>]", m, &mut out);
        assert_eq!(out, b"[b]");
    }
}
