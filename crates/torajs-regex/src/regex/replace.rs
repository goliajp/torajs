//! `__torajs_str_replace_regex` / `_all_regex` + `expand_repl` —
//! port of `runtime_regex.c` L2401-2600.
//!
//! Replacement-string substitution per ES §22.1.3.18.5
//! GetSubstitution: `$&` (whole match), `` $` `` (portion before
//! the match), `$'` (portion after the match), `$1`..`$99` (capture
//! groups; two-digit form when the resulting index is a valid
//! group), `$<name>` (named capture; empty when the name is unknown
//! or the group did not participate — literal when the pattern has
//! no named groups at all), `$$` (literal `$`), other `$X` left
//! literal.

use alloc::vec::Vec;
use core::ffi::c_void;

use super::{
    __torajs_throw_type_error, RegExp, abort_unsupported, as_regex, str_from_bytes,
    str_from_bytes_ascii, str_slice, str_slice_ascii_view,
};
use crate::node::REGEX_MAX_CAPTURES;
use crate::parser::{RE_FLAG_G, RE_FLAG_Y};
use crate::vm::{match_anchor, save_slot, search_from_with_ws};

/// Expand `repl` into `out`, dereferencing `$N` against the
/// captured `saves[]` pairs. Unparticipating groups substitute the
/// empty string. `capture_names` is the pattern's `(?<name>...)`
/// table (index 0 unused; empty `Vec` at positional groups) and
/// `has_named` its non-empty-entry count gate — `$<name>` is only
/// interpreted when the pattern has at least one named group
/// (namedCaptures ≠ undefined per GetSubstitution).
pub fn expand_repl(
    repl: &[u8],
    s: &[u8],
    st: i64,
    en: i64,
    saves: &[i64],
    n_captures: i32,
    capture_names: &[Vec<u8>],
    has_named: bool,
    out: &mut Vec<u8>,
) {
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
                    if gi >= 1 && gi < REGEX_MAX_CAPTURES && gn.as_slice() == name {
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

fn replace_inner<'a>(
    re: &'a RegExp,
    s: &[u8],
    repl: &[u8],
    global: bool,
    haystack_is_ascii: bool,
) -> &'a [u8] {
    let slen = s.len() as i64;
    // V0.2 P14-S8 — reuse the per-RegExp cached Pike VM
    // workspace. The Pike VM's `step_id` counter increments
    // monotonically on every `vm_match_at` call, so stale
    // `visited[]` entries from prior runs auto-invalidate
    // (no clear pass needed). Only the cur/nxt thread lists
    // need explicit reset between invocations.
    // Round 5 attack #1 — pass the cache's `Option<Workspace>` down
    // as-is; the vm materialises it lazily at its `vm_match_at` call
    // sites (DFA-resident + no-save programs never touch it). No
    // explicit cur/nxt reset needed: `vm_match_at` clears `cur` and
    // resets the arena on entry, `nxt` per step.
    let ws_cell = re.workspace_cache.get();
    let ws = unsafe { &mut *ws_cell };
    // Round 5 attack str-replace #3 — reuse the per-RegExp output
    // buffer (alloc/free once per RegExp instead of per call). Same
    // single-threaded interior-mutability contract as
    // `workspace_cache` above; the borrow ends when the returned
    // slice is consumed by `str_from_bytes` in the extern wrapper.
    let out: &mut Vec<u8> = unsafe { &mut *re.replace_out_cache.get() };
    out.clear();
    out.reserve(s.len() + 16);
    let mut pos: i64 = 0;
    let sticky = re.flags & RE_FLAG_Y != 0;
    // Round 5 attack str-replace #1 (JSC `substituteBackreferences`
    // dollarless parity) — a replacement with no `$` provably never
    // reads captures, so the search can skip the 512-byte saves init
    // + the second-pass Pike VM capture extraction per hit.
    // It also decides how the replacement itself is written: with no
    // `$` it is a literal, so each hit appends it whole rather than
    // through `expand_repl`'s per-byte walk.
    let want_saves = repl.contains(&b'$');
    // Phase C-3 — bind the AOT-baked DFA view once outside the loop.
    // See match_all.rs for the rationale.
    // Round 3 Phase B sub-batch 7.2 — prefer the AOT-baked view, fall
    // back to the runtime-baked `RegExp.dfa_runtime` (eager-built at
    // `__torajs_regex_compile` ctor for DFA-eligible runtime literals).
    // Either way `dfa_ref: Option<&DfaProgram>` is the wire shape into
    // `vm::search_from_with_ws`; the vm's per-call `dfa_built_local`
    // path becomes dead in sub-batch 7.3.
    let dfa_view = re.baked_dfa_view();
    let dfa_ref = dfa_view.as_ref().or(re.dfa_runtime.as_ref());
    while pos <= slen {
        let m = if sticky {
            match_anchor(&re.prog, &s, pos, re.flags)
        } else {
            search_from_with_ws(
                &re.prog,
                &s,
                pos,
                re.flags,
                ws,
                dfa_ref,
                haystack_is_ascii,
                want_saves,
            )
        };
        let Some(m) = m else { break };
        out.extend_from_slice(&s[pos as usize..m.start as usize]);
        if want_saves {
            expand_repl(
                repl,
                s,
                m.start,
                m.end,
                m.saves(),
                re.n_captures,
                &re.capture_names,
                re.n_named_captures > 0,
                out,
            );
        } else {
            // Rotation 470 — `want_saves` IS `repl.contains(&b'$')`,
            // and with no `$` anywhere `expand_repl` walks the
            // replacement one `Vec::push` per byte and copies it
            // verbatim. The fact is already established above; act on
            // it and copy in one go. 23% of `str-replace-100k`'s
            // samples were that walk.
            out.extend_from_slice(repl);
        }
        if m.end == m.start {
            if m.start < slen {
                out.push(s[m.start as usize]);
            }
            pos = m.end + 1;
        } else {
            pos = m.end;
        }
        if !global {
            break;
        }
    }
    // pos may overshoot slen after an empty match at end-of-string
    // (pos = m.end + 1). Clamp before slicing — matches C's
    // `emit_bytes(s + pos, slen - pos)` which is a no-op when
    // slen - pos < 0 (n_bytes guard inside emit_bytes).
    let tail = (pos as usize).min(s.len());
    out.extend_from_slice(&s[tail..]);
    out
}

/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`; `str_ptr` / `repl_ptr`
/// are live `*Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_replace_regex(
    str_ptr: *const c_void,
    re_ptr: *const c_void,
    repl_ptr: *const c_void,
) -> *mut c_void {
    if re_ptr.is_null() {
        let s = unsafe { str_slice(str_ptr) };
        return unsafe { str_from_bytes(&s) as *mut c_void };
    }
    let re = unsafe { as_regex(re_ptr) };
    if re.rejected != 0 {
        abort_unsupported(re);
    }
    // Round 5 attacks str-replace #2/#4 — borrow the Str payload
    // directly when it's pure-ASCII Latin-1 (no transcode, no owned
    // Vec alloc/free per call); fall back to the owned transcode for
    // non-ASCII. The borrow is bounded to this call frame, same
    // contract as the match/exec paths using `str_slice_ascii_view`.
    let s_owned;
    let (s, haystack_is_ascii): (&[u8], bool) = match unsafe { str_slice_ascii_view(str_ptr) } {
        Some(v) => (v, true),
        None => {
            s_owned = unsafe { str_slice(str_ptr) };
            (&s_owned, false)
        }
    };
    let repl_owned;
    let (repl, repl_is_ascii): (&[u8], bool) = match unsafe { str_slice_ascii_view(repl_ptr) } {
        Some(v) => (v, true),
        None => {
            repl_owned = unsafe { str_slice(repl_ptr) };
            (&repl_owned, false)
        }
    };
    let global = re.flags & RE_FLAG_G != 0;
    let out = replace_inner(re, s, repl, global, haystack_is_ascii);
    // Round 5 attack str-replace #5 — the output is gap-copies of `s`
    // plus expansions of `repl`; when both are ASCII the result is
    // provably ASCII, so skip the encoding re-scan in the Str alloc.
    if haystack_is_ascii && repl_is_ascii {
        return unsafe { str_from_bytes_ascii(out) as *mut c_void };
    }
    unsafe { str_from_bytes(out) as *mut c_void }
}

/// # Safety
///
/// `re_ptr` is null or a live `*RegExp`; `str_ptr` / `repl_ptr`
/// are live `*Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_replace_all_regex(
    str_ptr: *const c_void,
    re_ptr: *const c_void,
    repl_ptr: *const c_void,
) -> *mut c_void {
    if re_ptr.is_null() {
        let s = unsafe { str_slice(str_ptr) };
        return unsafe { str_from_bytes(&s) as *mut c_void };
    }
    let re = unsafe { as_regex(re_ptr) };
    if re.rejected != 0 {
        abort_unsupported(re);
    }
    // §22.1.5 — String.prototype.replaceAll throws a TypeError when the
    // RegExp searchValue lacks the `g` flag (mirrors matchAll §22.1.3.13).
    // The kernel records the pending throw; the caller's post-call
    // emit_throw_check propagates it and discards this returned string.
    if re.flags & RE_FLAG_G == 0 {
        unsafe {
            __torajs_throw_type_error(
                b"String.prototype.replaceAll called with a non-global RegExp argument\0".as_ptr(),
            );
        }
        let s = unsafe { str_slice(str_ptr) };
        return unsafe { str_from_bytes(&s) as *mut c_void };
    }
    // Round 5 attacks str-replace #2/#4 — same ASCII borrow shape as
    // `__torajs_str_replace_regex` above.
    let s_owned;
    let (s, haystack_is_ascii): (&[u8], bool) = match unsafe { str_slice_ascii_view(str_ptr) } {
        Some(v) => (v, true),
        None => {
            s_owned = unsafe { str_slice(str_ptr) };
            (&s_owned, false)
        }
    };
    let repl_owned;
    let (repl, repl_is_ascii): (&[u8], bool) = match unsafe { str_slice_ascii_view(repl_ptr) } {
        Some(v) => (v, true),
        None => {
            repl_owned = unsafe { str_slice(repl_ptr) };
            (&repl_owned, false)
        }
    };
    // replace_all == replace with implicit `g` (ignore the regex's
    // own g flag — JS spec actually throws TypeError if no g, but
    // tr deferred that to v0.2 #1.c per the C port comment).
    let out = replace_inner(re, s, repl, /* global */ true, haystack_is_ascii);
    if haystack_is_ascii && repl_is_ascii {
        return unsafe { str_from_bytes_ascii(out) as *mut c_void };
    }
    unsafe { str_from_bytes(out) as *mut c_void }
}

#[cfg(test)]
mod tests {
    use super::expand_repl;
    use alloc::vec;
    use alloc::vec::Vec;

    // s = "abcd", match = "bc" (st 1, en 3), no captures.
    fn expand_plain(repl: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        expand_repl(repl, b"abcd", 1, 3, &[1, 3], 0, &[], false, &mut out);
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
        expand_repl(
            repl,
            b"abcd",
            0,
            2,
            saves,
            2,
            &named_names(),
            true,
            &mut out,
        );
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
        expand_repl(
            b"[$<x>]",
            b"ba",
            0,
            1,
            &[0, 1, -1, -1, 0, 1],
            2,
            &names,
            true,
            &mut out,
        );
        assert_eq!(out, b"[b]");
    }
}
