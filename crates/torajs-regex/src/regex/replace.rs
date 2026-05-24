//! `__torajs_str_replace_regex` / `_all_regex` + `expand_repl` —
//! port of `runtime_regex.c` L2401-2600.
//!
//! Replacement-string substitution per ES spec: `$&` (whole match),
//! `$1`..`$99` (capture groups; two-digit form when the resulting
//! index is a valid group), `$$` (literal `$`), other `$X` left
//! literal.

use core::ffi::c_void;

use super::{RegExp, abort_unsupported, as_regex, str_from_bytes, str_slice};
use crate::node::{REGEX_MAX_CAPTURES, REGEX_SAVE_SLOTS};
use crate::parser::{RE_FLAG_G, RE_FLAG_Y};
use crate::vm::{Workspace, match_anchor, search_from_with_ws};

/// Expand `repl` into `out`, dereferencing `$N` against the
/// captured `saves[]` pairs. Unparticipating groups substitute the
/// empty string.
pub fn expand_repl(
    repl: &[u8],
    s: &[u8],
    st: i64,
    en: i64,
    saves: &[i64; REGEX_SAVE_SLOTS],
    n_captures: i32,
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
                let gs = saves[(2 * idx) as usize];
                let ge = saves[(2 * idx + 1) as usize];
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

fn replace_inner(re: &RegExp, s: &[u8], repl: &[u8], global: bool) -> Vec<u8> {
    let slen = s.len() as i64;
    let mut ws = Workspace::for_program(&re.prog);
    let mut out: Vec<u8> = Vec::with_capacity(s.len() + 16);
    let mut pos: i64 = 0;
    let sticky = re.flags & RE_FLAG_Y != 0;
    while pos <= slen {
        let m = if sticky {
            match_anchor(&re.prog, s, pos, re.flags)
        } else {
            search_from_with_ws(&re.prog, s, pos, re.flags, &mut ws)
        };
        let Some(m) = m else { break };
        out.extend_from_slice(&s[pos as usize..m.start as usize]);
        expand_repl(repl, s, m.start, m.end, &m.saves, re.n_captures, &mut out);
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
        return unsafe { str_from_bytes(s) as *mut c_void };
    }
    let re = unsafe { as_regex(re_ptr) };
    if re.rejected != 0 {
        abort_unsupported(re);
    }
    let s = unsafe { str_slice(str_ptr) };
    let repl = unsafe { str_slice(repl_ptr) };
    let global = re.flags & RE_FLAG_G != 0;
    let out = replace_inner(re, s, repl, global);
    unsafe { str_from_bytes(&out) as *mut c_void }
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
        return unsafe { str_from_bytes(s) as *mut c_void };
    }
    let re = unsafe { as_regex(re_ptr) };
    if re.rejected != 0 {
        abort_unsupported(re);
    }
    let s = unsafe { str_slice(str_ptr) };
    let repl = unsafe { str_slice(repl_ptr) };
    // replace_all == replace with implicit `g` (ignore the regex's
    // own g flag — JS spec actually throws TypeError if no g, but
    // tr deferred that to v0.2 #1.c per the C port comment).
    let out = replace_inner(re, s, repl, /* global */ true);
    unsafe { str_from_bytes(&out) as *mut c_void }
}
